// =======================================================
// src/emulator/application.rs — Winit Event Loop Handler
// =======================================================

use winit::application::ApplicationHandler;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::WindowId;
use winit::event::WindowEvent;
use winit::keyboard::{KeyCode, PhysicalKey};
#[cfg(any(target_os = "windows", target_os = "linux"))]
use winit::keyboard::ModifiersState;
use winit::event::MouseButton;
#[cfg(target_os = "linux")]
use winit::platform::x11::EventLoopBuilderExtX11;
use crate::ui::MenuManager;
#[cfg(any(target_os = "windows", target_os = "linux"))]
use crate::ui::menu::{MenuKey, MenuModifier};
#[cfg(target_os = "linux")]
use crate::ui::shell::Shell;

use super::Result;
use crate::emulator::command_line::CommandLine;
use super::orchestrator::Orchestrator;

/* Breadbin owns the native event-loop state. The emulation stack is created only after the platform reports that the application has resumed, which keeps window and GPU construction on the thread and lifecycle boundary required by the desktop backend. */
pub struct Breadbin {
	command_line: CommandLine,
	orchestrator: Option<Orchestrator>,
	#[cfg(any(target_os = "windows", target_os = "linux"))]
	modifiers:    ModifiersState,
}

impl Breadbin {
	/* run transfers control to winit for the lifetime of the process. After this call, all emulator work is driven by ApplicationHandler callbacks rather than by a separate polling loop in main(). */
	pub fn run(command_line: CommandLine) -> Result<()> {
		#[cfg(target_os = "linux")]
		let application = {
			let mut builder = EventLoop::<()>::builder();
			builder.with_x11();
			builder.build()?
		};
		#[cfg(not(target_os = "linux"))]
		let application = EventLoop::new()?;
		let mut app = Self {
			command_line,
			orchestrator: None,
			#[cfg(any(target_os = "windows", target_os = "linux"))]
			modifiers: ModifiersState::default(),
		};
		application.run_app(&mut app)?;
		Ok(())
	}
}

impl ApplicationHandler for Breadbin {
	/* resumed is the single construction gate for Orchestrator. Repeated resume notifications reuse the existing machine instead of rebuilding mutable emulator state. */
	fn resumed(&mut self, application: &ActiveEventLoop) {
		if self.orchestrator.is_none() {
			match Orchestrator::new(application, &self.command_line) {
				Ok(driver) => self.orchestrator = Some(driver),
				Err(_) => {
					application.exit();
				}
			}
		}
	}

	/* Native window events are routed by window ownership before they reach emulator input. Inspector events are consumed locally, while main-window events may mutate input state, media state or presentation state. */
	fn window_event(&mut self, application: &ActiveEventLoop, window_id: WindowId, event: WindowEvent) {
		let o = match self.orchestrator.as_mut() {
			Some(driver) => driver,
			None => return,
		};

		if let WindowEvent::ThemeChanged(theme) = &event {
			o.is_dark_mode = *theme == winit::window::Theme::Dark;
		}

		if o.is_inspector_window(window_id) {
			match event {
				WindowEvent::CloseRequested => {
					o.close_inspector();
				}
				WindowEvent::RedrawRequested => {
					o.redraw_inspector();
				}
				WindowEvent::Resized(size) => {
					if let Some(inspector) = o.inspector.as_mut() {
						inspector.handle_resize(size.width, size.height);
					}
				}
				_ => {}
			}
			return;
		}

		match event {
			WindowEvent::CloseRequested => {
				application.exit();
			}
			WindowEvent::RedrawRequested => {
				if let Err(_) = o.draw_frame() {
					application.exit();
				}
			}
			/* Resize events are normalised by Orchestrator before renderer reconfiguration,
			 * so every platform receives the same locked presentation ratio. */
			WindowEvent::Resized(size) => {
				o.handle_resize(size.width, size.height);
			}
			WindowEvent::ScaleFactorChanged { .. } => {
				let size = o.context.window.inner_size();
				o.handle_resize(size.width, size.height);
			}
			#[cfg(any(target_os = "windows", target_os = "linux"))]
			WindowEvent::ModifiersChanged(modifiers) => {
				self.modifiers = modifiers.state();
			}
			WindowEvent::KeyboardInput { event: key_event, .. } => {
				if let PhysicalKey::Code(key_code) = key_event.physical_key {
					if key_event.state == winit::event::ElementState::Pressed
						&& !key_event.repeat
						&& !is_modifier_key(key_code)
						&& o.resume_from_pause()
					{
						return;
					}
					#[cfg(any(target_os = "windows", target_os = "linux"))]
					if key_event.state == winit::event::ElementState::Pressed && !key_event.repeat {
						if let Some(menu_key) = desktop_menu_key(key_code) {
							let modifier = if self.modifiers.control_key()
								&& !self.modifiers.alt_key()
								&& !self.modifiers.super_key()
							{
								Some(MenuModifier::Primary)
							} else if self.modifiers.alt_key()
								&& !self.modifiers.control_key()
								&& !self.modifiers.super_key()
							{
								Some(MenuModifier::Alt)
							} else if !self.modifiers.control_key()
								&& !self.modifiers.alt_key()
								&& !self.modifiers.super_key()
							{
								Some(MenuModifier::Unmodified)
							} else {
								None
							};
							if let Some(modifier) = modifier {
								if let Some(id) = o.context.menu.shortcut_command(
									menu_key,
									modifier,
									self.modifiers.shift_key(),
								) {
									o.context.input.clear_all();
									o.handle_menu_event(&id);
									return;
								}
							}
						}
					}
					o.handle_input_event(key_code, key_event.state);
				}
			}
			WindowEvent::CursorMoved { position, .. } => {
				o.handle_cursor_moved(position.x, position.y);
			}
			WindowEvent::CursorLeft { .. } => {
				o.handle_cursor_left();
			}
			WindowEvent::MouseInput { state, button, .. } => {
				let pressed = state == winit::event::ElementState::Pressed;
				if pressed && button == MouseButton::Left {
					o.handle_mouse_click();
				}
			}
			WindowEvent::DroppedFile(path) => {
				o.handle_drop(path);
			}
			_ => {}
		}
	}

	/* about_to_wait is the host scheduling boundary. It drains desktop menu events, advances emulation, applies deferred window requests and then chooses Poll or Wait according to pause state. */
	fn about_to_wait(&mut self, application: &ActiveEventLoop) {
		#[cfg(target_os = "linux")]
		if Shell::close_requested() {
			application.exit();
			return;
		}

		while let Some(id) = MenuManager::poll_event() {
			if let Some(o) = self.orchestrator.as_mut() {
				o.handle_menu_event(&id);
			}
		}

		if let Some(o) = self.orchestrator.as_mut() {
			o.context.menu.pump();
			o.update();

			if o.inspector_requested {
				o.inspector_requested = false;
				o.open_inspector(application);
			}

			if o.context.input.close_requested {
				application.exit();
			}
		}

		let paused = self.orchestrator.as_ref().map(|o| o.paused).unwrap_or(false);
		application.set_control_flow(if paused {
			ControlFlow::Wait
		} else {
			ControlFlow::Poll
		});
	}
}

fn is_modifier_key(key: KeyCode) -> bool {
	matches!(
		key,
		KeyCode::ShiftLeft
			| KeyCode::ShiftRight
			| KeyCode::ControlLeft
			| KeyCode::ControlRight
			| KeyCode::AltLeft
			| KeyCode::AltRight
			| KeyCode::SuperLeft
			| KeyCode::SuperRight
	)
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn desktop_menu_key(key: KeyCode) -> Option<MenuKey> {
	match key {
		KeyCode::KeyC => Some(MenuKey::C),
		KeyCode::KeyD => Some(MenuKey::D),
		KeyCode::Enter | KeyCode::NumpadEnter => Some(MenuKey::Enter),
		KeyCode::KeyF => Some(MenuKey::F),
		KeyCode::F4 => Some(MenuKey::F4),
		KeyCode::KeyI => Some(MenuKey::I),
		KeyCode::KeyJ => Some(MenuKey::J),
		KeyCode::KeyM => Some(MenuKey::M),
		KeyCode::KeyP => Some(MenuKey::P),
		KeyCode::Pause => Some(MenuKey::Pause),
		KeyCode::KeyQ => Some(MenuKey::Q),
		KeyCode::KeyR => Some(MenuKey::R),
		KeyCode::KeyT => Some(MenuKey::T),
		KeyCode::KeyW => Some(MenuKey::W),
		_ => None,
	}
}