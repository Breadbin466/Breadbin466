// =======================================================
// src/emulator/application.rs — Winit Event Loop Handler
// =======================================================

use crate::ui::MenuManager;
#[cfg(any(target_os = "windows", target_os = "linux"))]
use crate::ui::menu::{MenuKey, MenuModifier};
#[cfg(target_os = "linux")]
use crate::ui::shell::Shell;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
#[cfg(any(target_os = "windows", target_os = "linux"))]
use winit::keyboard::ModifiersState;
use winit::keyboard::{KeyCode, PhysicalKey};
#[cfg(target_os = "linux")]
use winit::platform::x11::EventLoopBuilderExtX11;
use winit::window::WindowId;

use super::Result;
use super::orchestrator::Orchestrator;
use crate::emulator::command_line::CommandLine;

/* Breadbin owns the native event-loop state. The emulation stack is created only after the platform reports that the application has resumed, which keeps window and GPU construction on the thread and lifecycle boundary required by the desktop backend. */
pub struct Breadbin {
	command_line: CommandLine,
	orchestrator: Option<Orchestrator>,
	#[cfg(any(target_os = "windows", target_os = "linux"))]
	modifiers: ModifiersState,
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
		#[cfg(any(target_os = "windows", target_os = "linux"))]
		{ self.modifiers = ModifiersState::default(); }
		if let Some(o) = self.orchestrator.as_mut() { o.resynchronise_host(); }
		if self.orchestrator.is_none() {
			match Orchestrator::new(application, &self.command_line) {
				Ok(driver) => self.orchestrator = Some(driver),
				Err(_) => {
					application.exit();
				}
			}
		}
	}

	fn suspended(&mut self, _: &ActiveEventLoop) {
		#[cfg(any(target_os = "windows", target_os = "linux"))]
		{ self.modifiers = ModifiersState::default(); }
		if let Some(o) = self.orchestrator.as_mut() { o.resynchronise_host(); }
	}

	/* Native window events are routed by window ownership before they reach emulator input. Native Inspector controls own their events; only main-window events may mutate input state, media state or presentation state. */
	fn window_event(
		&mut self,
		application: &ActiveEventLoop,
		window_id: WindowId,
		event: WindowEvent,
	) {
		let o = match self.orchestrator.as_mut() {
			Some(driver) => driver,
			None => return,
		};

		if let WindowEvent::ThemeChanged(theme) = &event {
			o.is_dark_mode = *theme == winit::window::Theme::Dark;
		}

		if window_id != o.context.window.id() { return; }

		match event {
			WindowEvent::Focused(false) => {
				o.handle_focus_lost();
				#[cfg(any(target_os = "windows", target_os = "linux"))]
				{ self.modifiers = ModifiersState::default(); }
			}
			WindowEvent::CloseRequested => {
				if o.context.prepare_shutdown() {
					application.exit();
				}
			}
			WindowEvent::Occluded(hidden) => {
				if !hidden {
					o.context.window.request_redraw();
				}
			}
			WindowEvent::RedrawRequested => {
				if o.draw_frame().is_err() {
					application.exit();
					return;
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
			WindowEvent::KeyboardInput {
				event: key_event, ..
			} => {
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
				o.handle_mouse_button(button, pressed);
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
			let can_exit = self
				.orchestrator
				.as_mut()
				.map(|orchestrator| orchestrator.context.prepare_shutdown())
				.unwrap_or(true);
			if can_exit {
				application.exit();
				return;
			}
			Shell::clear_close_requested();
		}

		#[cfg(target_os = "linux")]
		if Shell::take_focus_lost() {
			self.modifiers = ModifiersState::default();
			if let Some(o) = self.orchestrator.as_mut() { o.handle_focus_lost(); }
		}

		#[cfg(target_os = "linux")]
		while let Some((key, state)) = Shell::poll_key_event() {
			if let Some(o) = self.orchestrator.as_mut() {
				o.handle_input_event(key, state);
			}
		}

		while let Some(id) = MenuManager::poll_event() {
			if let Some(o) = self.orchestrator.as_mut() {
				o.handle_menu_event(&id);
			}
		}

		if let Some(o) = self.orchestrator.as_mut() {
			o.context.menu.pump();
			if o.update().is_err() {
				application.exit();
				return;
			}

			if o.quit_time_reached() {
				if o.context.prepare_shutdown() {
					application.exit();
					return;
				}
			}

			if o.inspector_requested {
				o.inspector_requested = false;
				o.open_inspector(application);
			}

			if o.context.input.close_requested {
				if o.context.prepare_shutdown() {
					application.exit();
				} else {
					o.context.input.close_requested = false;
				}
			}
		}

		let paused = self
			.orchestrator
			.as_ref()
			.map(|o| o.paused)
			.unwrap_or(false);
		application.set_control_flow(if paused {
			self.orchestrator.as_ref().and_then(|o| o.inspector.as_ref())
				.and_then(|inspector| inspector.next_refresh_time())
				.map(ControlFlow::WaitUntil).unwrap_or_else(|| ControlFlow::WaitUntil(std::time::Instant::now() + std::time::Duration::from_secs(1)))
		} else if let Some(o) = self.orchestrator.as_ref() {
			if o.is_warping() {
				ControlFlow::Poll
			} else {
				/* Poll through the short wake margin instead of blocking inside
				 * emulation, so newly arrived keys reach the imminent PAL frame. */
				let wake = o.get_next_frame_time() - super::timing::HOST_WAKE_MARGIN;
				if std::time::Instant::now() >= wake {
					ControlFlow::Poll
				} else {
					ControlFlow::WaitUntil(wake)
				}
			}
		} else {
			ControlFlow::Wait
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