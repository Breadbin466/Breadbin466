// =======================================================
// src/ui/shell_linux.rs — Native GTK/X11 application shell
// =======================================================

use crate::ui::constants::WINDOW_TITLE;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use gtk::prelude::*;
use winit::event::ElementState;
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::KeyCode;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::{Window, WindowAttributes};

use crate::emulator::Result;
use crate::ui::window_icon::{load_gtk_icon, load_window_icon};

static CLOSE_REQUESTED: AtomicBool = AtomicBool::new(false);

/* Linux embeds the undecorated X11 winit child inside a GTK toplevel. GTK owns menus, accelerators, decorations and fullscreen state, while winit remains the rendering/input surface. */
struct LinuxShell {
	window: gtk::Window,
	root: gtk::Box,
	menu_bar: Option<gtk::MenuBar>,
	accel_group: Option<gtk::AccelGroup>,
	content_width: u32,
	content_height: u32,
	fullscreen: bool,
}

thread_local! {
	static LINUX_SHELL: RefCell<Option<LinuxShell>> = const { RefCell::new(None) };
	static KEY_EVENTS: RefCell<VecDeque<(KeyCode, ElementState)>> = const { RefCell::new(VecDeque::new()) };
}

/* Shell exposes the same host-window contract as macOS and Windows while hiding the additional GTK/XEmbed integration required on Linux. */
pub struct Shell;

impl Shell {
	/* The X11 child is created hidden, embedded into a GtkSocket, then revealed only after both toolkits agree on ownership and physical size. */
	pub fn create_window(
		application: &ActiveEventLoop,
		attributes: WindowAttributes,
	) -> Result<Arc<Window>> {
		gtk::gdk::set_allowed_backends("x11");
		gtk::init()?;
		CLOSE_REQUESTED.store(false, Ordering::Release);

		let attributes = attributes
			.with_visible(false)
			.with_decorations(false)
			.with_window_icon(Some(load_window_icon()?));
		let embedded = Arc::new(application.create_window(attributes)?);
		let handle = embedded.window_handle()?;
		let xid = match handle.as_raw() {
			RawWindowHandle::Xlib(handle) => handle.window,
			_ => return Err("The Linux window does not expose an X11 handle".into()),
		};

		let physical = embedded.inner_size();
		let window = gtk::Window::new(gtk::WindowType::Toplevel);
		window.set_title(WINDOW_TITLE);
		window.set_icon(Some(&load_gtk_icon()?));
		window.set_resizable(true);
		let scale = window.scale_factor().max(1) as f64;
		let logical_width = physical_to_logical(physical.width, scale);
		let logical_height = physical_to_logical(physical.height, scale);
		window.set_default_size(logical_width, logical_height);

		let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
		let socket = gtk::Socket::new();
		socket.set_hexpand(true);
		socket.set_vexpand(true);
		socket.set_can_focus(true);
		root.pack_start(&socket, true, true, 0);
		window.add(&root);
		window.connect_delete_event(|_, _| {
			CLOSE_REQUESTED.store(true, Ordering::Release);
			glib::Propagation::Stop
		});
		/* The XEmbed child does not reliably forward keyboard events through winit on
		 * every GTK/X11 combination. Capturing the GTK toplevel events preserves the
		 * physical C64 keyboard path while menu accelerators continue to be handled by
		 * GTK itself. Events are queued and consumed on the winit application thread. */
		window.connect_key_press_event(|_, event| {
			queue_key_event(event, ElementState::Pressed);
			glib::Propagation::Proceed
		});
		window.connect_key_release_event(|_, event| {
			queue_key_event(event, ElementState::Released);
			glib::Propagation::Proceed
		});
		window.show_all();
		socket.add_id(xid as gtk::xlib::Window);
		embedded.set_visible(true);
		socket.grab_focus();
		embedded.focus_window();

		LINUX_SHELL.with(|slot| {
			*slot.borrow_mut() = Some(LinuxShell {
				window,
				root,
				menu_bar: None,
				accel_group: None,
				content_width: physical.width,
				content_height: physical.height,
				fullscreen: false,
			});
		});

		Ok(embedded)
	}

	/* Replacing the menu also replaces its accelerator group so stale shortcuts cannot remain attached to the GTK toplevel. */
	pub fn install_menu_bar(menu_bar: gtk::MenuBar, accel_group: gtk::AccelGroup) -> Result<()> {
		LINUX_SHELL.with(|slot| -> Result<()> {
			let mut slot = slot.borrow_mut();
			let shell = slot.as_mut().ok_or("Linux GTK shell is not initialised")?;
			if let Some(previous) = shell.menu_bar.take() {
				shell.root.remove(&previous);
			}
			if let Some(previous) = shell.accel_group.take() {
				shell.window.remove_accel_group(&previous);
			}
			shell.root.pack_start(&menu_bar, false, false, 0);
			shell.root.reorder_child(&menu_bar, 0);
			shell.window.add_accel_group(&accel_group);
			menu_bar.show_all();
			shell.menu_bar = Some(menu_bar);
			shell.accel_group = Some(accel_group);
			resize_shell(shell);
			Ok(())
		})
	}

	pub fn resize_content(width: u32, height: u32) {
		LINUX_SHELL.with(|slot| {
			if let Some(shell) = slot.borrow_mut().as_mut() {
				shell.content_width = width;
				shell.content_height = height;
				resize_shell(shell);
			}
		});
	}

	pub fn close_requested() -> bool {
		CLOSE_REQUESTED.load(Ordering::Acquire)
	}

	pub fn clear_close_requested() {
		CLOSE_REQUESTED.store(false, Ordering::Release);
	}

	pub fn poll_key_event() -> Option<(KeyCode, ElementState)> {
		KEY_EVENTS.with(|events| events.borrow_mut().pop_front())
	}

	pub fn fullscreen() -> bool {
		LINUX_SHELL.with(|slot| {
			slot.borrow()
				.as_ref()
				.map(|shell| shell.fullscreen)
				.unwrap_or(false)
		})
	}

	pub fn set_fullscreen(fullscreen: bool) {
		LINUX_SHELL.with(|slot| {
			if let Some(shell) = slot.borrow_mut().as_mut() {
				if fullscreen {
					shell.window.fullscreen();
				} else {
					shell.window.unfullscreen();
				}
				shell.fullscreen = fullscreen;
				if !fullscreen {
					resize_shell(shell);
				}
			}
		});
	}
}

/* GTK sizes are logical pixels, whereas winit reports physical content pixels; the shell adds the current menu height after scale conversion. */
fn resize_shell(shell: &LinuxShell) {
	if shell.fullscreen {
		return;
	}
	let scale = shell.window.scale_factor().max(1) as f64;
	let width = physical_to_logical(shell.content_width, scale);
	let content_height = physical_to_logical(shell.content_height, scale);
	let menu_height = shell
		.menu_bar
		.as_ref()
		.map(|menu| menu.preferred_height().1)
		.unwrap_or(0);
	shell.window.resize(width, content_height + menu_height);
}

fn physical_to_logical(value: u32, scale: f64) -> i32 {
	(value as f64 / scale).round().max(1.0) as i32
}

fn queue_key_event(event: &gtk::gdk::EventKey, state: ElementState) {
	if let Some(key) = linux_key_code(event) {
		KEY_EVENTS.with(|events| events.borrow_mut().push_back((key, state)));
	}
}

/* GDK reports logical key symbols for the GTK-owned toplevel. Breadbin maps the
 * complete host key set used by the C64 matrix and its desktop shortcuts; unknown
 * multimedia or compositor keys are deliberately ignored. */
fn linux_key_code(event: &gtk::gdk::EventKey) -> Option<KeyCode> {
	let name = event.keyval().name()?;
	Some(match name.as_str() {
		"a" | "A" => KeyCode::KeyA,
		"b" | "B" => KeyCode::KeyB,
		"c" | "C" => KeyCode::KeyC,
		"d" | "D" => KeyCode::KeyD,
		"e" | "E" => KeyCode::KeyE,
		"f" | "F" => KeyCode::KeyF,
		"g" | "G" => KeyCode::KeyG,
		"h" | "H" => KeyCode::KeyH,
		"i" | "I" => KeyCode::KeyI,
		"j" | "J" => KeyCode::KeyJ,
		"k" | "K" => KeyCode::KeyK,
		"l" | "L" => KeyCode::KeyL,
		"m" | "M" => KeyCode::KeyM,
		"n" | "N" => KeyCode::KeyN,
		"o" | "O" => KeyCode::KeyO,
		"p" | "P" => KeyCode::KeyP,
		"q" | "Q" => KeyCode::KeyQ,
		"r" | "R" => KeyCode::KeyR,
		"s" | "S" => KeyCode::KeyS,
		"t" | "T" => KeyCode::KeyT,
		"u" | "U" => KeyCode::KeyU,
		"v" | "V" => KeyCode::KeyV,
		"w" | "W" => KeyCode::KeyW,
		"x" | "X" => KeyCode::KeyX,
		"y" | "Y" => KeyCode::KeyY,
		"z" | "Z" => KeyCode::KeyZ,
		"0" | "parenright" => KeyCode::Digit0,
		"1" | "exclam" => KeyCode::Digit1,
		"2" | "at" => KeyCode::Digit2,
		"3" | "numbersign" => KeyCode::Digit3,
		"4" | "dollar" => KeyCode::Digit4,
		"5" | "percent" => KeyCode::Digit5,
		"6" | "asciicircum" => KeyCode::Digit6,
		"7" | "ampersand" => KeyCode::Digit7,
		"8" | "asterisk" => KeyCode::Digit8,
		"9" | "parenleft" => KeyCode::Digit9,
		"space" => KeyCode::Space,
		"Return" | "KP_Enter" => KeyCode::Enter,
		"BackSpace" => KeyCode::Backspace,
		"Delete" | "KP_Delete" => KeyCode::Delete,
		"Escape" => KeyCode::Escape,
		"Tab" | "ISO_Left_Tab" => KeyCode::Tab,
		"Shift_L" => KeyCode::ShiftLeft,
		"Shift_R" => KeyCode::ShiftRight,
		"Control_L" => KeyCode::ControlLeft,
		"Control_R" => KeyCode::ControlRight,
		"Alt_L" | "Meta_L" => KeyCode::AltLeft,
		"Alt_R" | "Meta_R" | "ISO_Level3_Shift" => KeyCode::AltRight,
		"Super_L" => KeyCode::SuperLeft,
		"Super_R" => KeyCode::SuperRight,
		"Left" | "KP_Left" => KeyCode::ArrowLeft,
		"Right" | "KP_Right" => KeyCode::ArrowRight,
		"Up" | "KP_Up" => KeyCode::ArrowUp,
		"Down" | "KP_Down" => KeyCode::ArrowDown,
		"Home" | "KP_Home" => KeyCode::Home,
		"minus" | "underscore" | "KP_Subtract" => KeyCode::Minus,
		"equal" | "plus" | "KP_Add" => KeyCode::Equal,
		"bracketleft" | "braceleft" => KeyCode::BracketLeft,
		"bracketright" | "braceright" => KeyCode::BracketRight,
		"backslash" | "bar" => KeyCode::Backslash,
		"semicolon" | "colon" => KeyCode::Semicolon,
		"apostrophe" | "quotedbl" => KeyCode::Quote,
		"grave" | "asciitilde" => KeyCode::Backquote,
		"comma" | "less" => KeyCode::Comma,
		"period" | "greater" | "KP_Decimal" => KeyCode::Period,
		"slash" | "question" | "KP_Divide" => KeyCode::Slash,
		"F1" => KeyCode::F1,
		"F2" => KeyCode::F2,
		"F3" => KeyCode::F3,
		"F4" => KeyCode::F4,
		"F5" => KeyCode::F5,
		"F6" => KeyCode::F6,
		"F7" => KeyCode::F7,
		"F8" => KeyCode::F8,
		"F9" => KeyCode::F9,
		"F10" => KeyCode::F10,
		"F11" => KeyCode::F11,
		"F12" => KeyCode::F12,
		"Pause" => KeyCode::Pause,
		_ => return None,
	})
}