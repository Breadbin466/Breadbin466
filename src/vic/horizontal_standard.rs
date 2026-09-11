// =======================================================
// src/vic/horizontal_standard.rs — VIC-II standard-colour horizontal transition paths
// =======================================================

use super::state::VicII;
use super::foreground::{ForegroundCell, PixelSpan};
use super::foreground_transition::render_foreground_span;
use super::horizontal_transition::ScrollTransition;

/*
Standard-width transitions operate on single-pixel source bits. The cases below select the exact old/new cell fragments needed when XSCROLL moves forward or backward during active display.
*/
/* Standard-width rendering consumes one graphics bit per output pixel. Foreground occupancy is tracked independently from colour so sprite priority and collision logic remain correct in every graphics mode. */
impl VicII {
/*
Each XSCROLL value selects a different splice between the older and current eight-bit cells. Values five through seven visibly pull one to three pixels from the previous cell, while bitmap and ECM transitions also replace the colour metadata that travels beside those pixels.
*/
pub(super) fn apply_non_mcm_scroll_transition(&mut self, transition: ScrollTransition) {
		let ScrollTransition {
			mode_old,
			mode_new,
			old_scroll: old_horizontal_scroll,
			decrement: horizontal_scroll_decrement,
			bitmap_mode_old: bmm_old,
			character_output_disabled: cdod,
			graphics_previous: gfx_data1,
			graphics_older: gfx_data0,
			character_older: char_data0,
			character_previous: char_data1,
			character_current: char_data2,
			cycle,
		} = transition;
		let mut gfx_data1_t = gfx_data1;
		let mut gfx_data0_t = gfx_data0;
		let mut char_data1_merged = char_data1;

		if bmm_old {
			if horizontal_scroll_decrement >= 4 {
				char_data1_merged = char_data2 & 0xFFF;
			}
		} else {
			if horizontal_scroll_decrement >= 4 {
				char_data1_merged = (char_data1 & 0x0FF) | (char_data2 & 0xF00);
			}
			let ecm_old = mode_old.extended_colour_bit();
			if ecm_old && horizontal_scroll_decrement > 4 {
				char_data1_merged = (char_data1_merged & 0xF3F) | (char_data2 & 0x0C0);
			}
		}

		if mode_new != mode_old {
			if (char_data1 & 0x800) != 0 || bmm_old {
				let t       = gfx_data1 & 0xAA;
				gfx_data1_t = t | (t >> 1);
			}
			if (char_data0 & 0x800) != 0 || bmm_old {
				let t       = gfx_data0 & 0xAA;
				gfx_data0_t = t | (t >> 1);
			}
		}

		let skip_ref = &mut self.pixels_to_skip;
		let screen   = &mut self.screen;

		match old_horizontal_scroll {
			0 => {
				if mode_old.extended_colour_bit() {
					render_foreground_span(screen, ForegroundCell::new(gfx_data1, 4, char_data1, mode_old, 0, cdod, cycle), PixelSpan::new(4, 1), skip_ref);
					render_foreground_span(screen, ForegroundCell::new(gfx_data1_t, 5, char_data1, mode_new, 0, cdod, cycle), PixelSpan::new(5, 3), skip_ref);
				} else {
					render_foreground_span(screen, ForegroundCell::new(gfx_data1_t, 4, char_data1, mode_new, 0, cdod, cycle), PixelSpan::new(4, 3), skip_ref);
					render_foreground_span(screen, ForegroundCell::new(gfx_data1, 7, char_data1, mode_new, 0, cdod, cycle), PixelSpan::new(7, 1), skip_ref);
				}
			}
			1 => {
				render_foreground_span(screen, ForegroundCell::new(gfx_data1_t, 4, char_data1, mode_new, 0, cdod, cycle), PixelSpan::new(4 - old_horizontal_scroll, 3), skip_ref);
				render_foreground_span(screen, ForegroundCell::new(gfx_data1, 7, char_data1, mode_new, 0, cdod, cycle), PixelSpan::new(7 - old_horizontal_scroll, 2), skip_ref);
			}
			2 => {
				render_foreground_span(screen, ForegroundCell::new(gfx_data1_t, 4, char_data1, mode_new, 0, cdod, cycle), PixelSpan::new(4 - old_horizontal_scroll, 3), skip_ref);
				render_foreground_span(screen, ForegroundCell::new(gfx_data1, 7, char_data1, mode_new, 0, cdod, cycle), PixelSpan::new(7 - old_horizontal_scroll, 3), skip_ref);
			}
			3 => {
				render_foreground_span(screen, ForegroundCell::new(gfx_data1_t, 4, char_data1, mode_new, 0, cdod, cycle), PixelSpan::new(4 - old_horizontal_scroll, 3), skip_ref);
				render_foreground_span(screen, ForegroundCell::new(gfx_data1, 7, char_data1, mode_new, 0, cdod, cycle), PixelSpan::new(7 - old_horizontal_scroll, 4), skip_ref);
			}
			4 => {
				render_foreground_span(screen, ForegroundCell::new(gfx_data1_t, 4, char_data1_merged, mode_new, 0, cdod, cycle), PixelSpan::new(4 - old_horizontal_scroll, 3), skip_ref);
				render_foreground_span(screen, ForegroundCell::new(gfx_data1, 7, char_data1_merged, mode_new, 0, cdod, cycle), PixelSpan::new(7 - old_horizontal_scroll, 5), skip_ref);
			}
			5 => {
				render_foreground_span(screen, ForegroundCell::new(gfx_data0_t, 4, char_data0, mode_new, 0, cdod, cycle), PixelSpan::new(12 - old_horizontal_scroll, 1), skip_ref);
				render_foreground_span(screen, ForegroundCell::new(gfx_data1_t, 5, char_data1_merged, mode_new, 0, cdod, cycle), PixelSpan::new(0, 2), skip_ref);
				render_foreground_span(screen, ForegroundCell::new(gfx_data1, 7, char_data1_merged, mode_new, 0, cdod, cycle), PixelSpan::new(2, 6), skip_ref);
			}
			6 => {
				render_foreground_span(screen, ForegroundCell::new(gfx_data0_t, 4, char_data0, mode_new, 0, cdod, cycle), PixelSpan::new(12 - old_horizontal_scroll, 2), skip_ref);
				render_foreground_span(screen, ForegroundCell::new(gfx_data1_t, 6, char_data1_merged, mode_new, 0, cdod, cycle), PixelSpan::new(0, 1), skip_ref);
				render_foreground_span(screen, ForegroundCell::new(gfx_data1, 7, char_data1_merged, mode_new, 0, cdod, cycle), PixelSpan::new(1, 7), skip_ref);
			}
			_ => {
				render_foreground_span(screen, ForegroundCell::new(gfx_data0_t, 4, char_data0, mode_new, 0, cdod, cycle), PixelSpan::new(12 - old_horizontal_scroll, 3), skip_ref);
				render_foreground_span(screen, ForegroundCell::new(gfx_data1, 7, char_data1_merged, mode_new, 0, cdod, cycle), PixelSpan::new(0, 8), skip_ref);
			}
		}
	}
}