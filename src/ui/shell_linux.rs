// =======================================================
// src/ui/shell_linux.rs — Native GTK/X11 application shell
// =======================================================

use crate::ui::constants::{WINDOW_TITLE};
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use gtk::prelude::*;
use winit::event_loop::ActiveEventLoop;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::{Window, WindowAttributes};

use crate::emulator::Result;

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
}

/* Shell exposes the same host-window contract as macOS and Windows while hiding the additional GTK/XEmbed integration required on Linux. */
pub struct Shell;

impl Shell {
	/* The X11 child is created hidden, embedded into a GtkSocket, then revealed only after both toolkits agree on ownership and physical size. */
	pub fn create_window(application: &ActiveEventLoop, attributes: WindowAttributes) -> Result<Arc<Window>> {
		gtk::gdk::set_allowed_backends("x11");
		gtk::init()?;
		CLOSE_REQUESTED.store(false, Ordering::Release);

		let attributes = attributes.with_visible(false).with_decorations(false);
		let embedded = Arc::new(application.create_window(attributes)?);
		let handle = embedded.window_handle()?;
		let xid = match handle.as_raw() {
			RawWindowHandle::Xlib(handle) => handle.window,
			_ => return Err("The Linux window does not expose an X11 handle".into()),
		};

		let physical = embedded.inner_size();
		let window = gtk::Window::new(gtk::WindowType::Toplevel);
		window.set_title(WINDOW_TITLE);
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
		window.show_all();
		socket.add_id(xid as gtk::xlib::Window);
		embedded.set_visible(true);
		socket.grab_focus();

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

	pub fn fullscreen() -> bool {
		LINUX_SHELL.with(|slot| slot.borrow().as_ref().map(|shell| shell.fullscreen).unwrap_or(false))
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
	let menu_height = shell.menu_bar.as_ref().map(|menu| menu.preferred_height().1).unwrap_or(0);
	shell.window.resize(width, content_height + menu_height);
}

fn physical_to_logical(value: u32, scale: f64) -> i32 {
	(value as f64 / scale).round().max(1.0) as i32
}