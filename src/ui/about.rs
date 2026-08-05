// =======================================================
// src/ui/about.rs — About dialog façade and shared content
// =======================================================

pub use crate::ui::constants::{APP_NAME, VERSION, COPYRIGHT, DESCRIPTION_PARAGRAPHS};
use winit::window::Window;

/* The shared About façade owns product text and error policy, while each platform backend presents that same content through its native dialog conventions. */

/* The shared description joins the same paragraph set with a backend-selected separator, preserving identical wording across native dialogs. */
pub fn description(separator: &str) -> String {
	DESCRIPTION_PARAGRAPHS.join(separator)
}

/* Presentation errors are reported without terminating emulation because the About dialog is an optional host feature. */
pub fn show(window: &Window) {
	if let Err(error) = platform_show(window) {
		eprintln!("[ABOUT] {error}");
	}
}

#[cfg(target_os = "macos")]
fn platform_show(window: &Window) -> crate::emulator::Result<()> {
	super::about_macos::show(window)
}

#[cfg(target_os = "windows")]
fn platform_show(window: &Window) -> crate::emulator::Result<()> {
	super::about_windows::show(window)
}

#[cfg(target_os = "linux")]
fn platform_show(window: &Window) -> crate::emulator::Result<()> {
	super::about_linux::show(window)
}