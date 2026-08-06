// =======================================================
// src/ui/routing.rs — Drag & Drop and Graphical Interers Routing
// =======================================================

use crate::ui::constants::{OSD_GUI_HEIGHT_CHARS, OSD_BUFFER_SCALE, OSD_TRANSPORT_COUNT, OSD_SAFE_PADDING_X};
use std::path::PathBuf;
use crate::emulator::context::AppContext;

/* InputRouter converts spatial or file-system gestures into the same AppContext operations used elsewhere. It contains no machine state and therefore cannot bypass media history or transport invariants. */
pub struct InputRouter;

impl InputRouter {
	/* File type alone selects the mount action; unsupported drops are ignored without altering current media state. */
	pub fn handle_drop(path: PathBuf, context: &mut AppContext) {
		let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();

		match ext.as_str() {
			"crt" => context.mount_cartridge(path),
			"prg" => context.open_prg(path, true),
			"d64" | "g64" | "nib" | "nbz" => {
				context.mount_disk(path);
			}
			"tap" => context.mount_tape(path),
			_ => {}
		}
	}

	/* Mouse coordinates are mapped back into the renderer source buffer before hit-testing the OSD transport, so controls remain stable under arbitrary window scaling. */
	pub fn handle_mouse_click(cursor_pos: (f64, f64), context: &mut AppContext) {
		let (mx, my) = cursor_pos;
		let win_size = context.window.inner_size();
		let win_w = win_size.width as f64;
		let win_h = win_size.height as f64;
		if win_w == 0.0 || win_h == 0.0 { return; }

		let buf_w = context.renderer.src_width;
		let buf_h = context.renderer.src_height;

		let bx = ((mx / win_w) * buf_w as f64) as usize;
		let by = ((my / win_h) * buf_h as f64) as usize;

		let gui_px = OSD_GUI_HEIGHT_CHARS * OSD_BUFFER_SCALE;
		let bar_start_y = buf_h.saturating_sub(gui_px);
		if by < bar_start_y { return; }

		let transport_x = buf_w.saturating_sub(56 + OSD_TRANSPORT_COUNT * 8 + 8 + OSD_SAFE_PADDING_X);
		if bx < transport_x { return; }

		let slot = (bx - transport_x) / 8;
		if slot >= OSD_TRANSPORT_COUNT { return; }

		match slot {
			0 => context.toggle_tape_record(),
			1 => context.toggle_tape_playback(),
			2 => context.datassette.rewind(),
			3 => {}
			4 => context.stop_tape(),
			5 => context.eject_tape(),
			_ => {}
		}
	}
}