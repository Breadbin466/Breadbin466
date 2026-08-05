// =======================================================
// src/ui/shell_macos.rs — macOS application shell
// =======================================================

use std::sync::Arc;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowAttributes};

use crate::emulator::Result;

/* AppKit window ownership and decorations are already supplied by winit, so the macOS shell only wraps creation in the shared Arc-based contract. */
pub struct Shell;

impl Shell {
	pub fn create_window(application: &ActiveEventLoop, attributes: WindowAttributes) -> Result<Arc<Window>> {
		Ok(Arc::new(application.create_window(attributes)?))
	}
}