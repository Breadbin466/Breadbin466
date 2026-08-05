// =======================================================
// src/ui/about_linux.rs — Native GTK About dialog backend
// =======================================================

use crate::ui::constants::{ICON_CANDIDATES};
use gtk::prelude::*;
use winit::window::Window;

use crate::emulator::Result;
use super::about::{description, APP_NAME, COPYRIGHT, VERSION};

/* The GTK backend creates a non-blocking native About dialog and resolves the first icon name available in the current desktop theme. */

/* GTK uses the platform About dialog and resolves the application icon independently from the winit rendering surface. */
pub fn show(_window: &Window) -> Result<()> {
	let dialog = gtk::AboutDialog::new();
	dialog.set_program_name(APP_NAME);
	dialog.set_version(Some(VERSION));
	dialog.set_comments(Some(&description("\n\n")));
	dialog.set_copyright(Some(COPYRIGHT));
	dialog.set_modal(false);
	dialog.set_resizable(false);
	dialog.set_default_width(560);

	if let Some(name) = resolve_icon_name() {
		dialog.set_logo_icon_name(Some(name));
		dialog.set_icon_name(Some(name));
	}

	dialog.connect_response(|dialog, _| dialog.close());
	dialog.show_all();
	Ok(())
}

fn resolve_icon_name() -> Option<&'static str> {
	let theme = gtk::IconTheme::default()?;
	ICON_CANDIDATES.into_iter().find(|name| theme.has_icon(name))
}