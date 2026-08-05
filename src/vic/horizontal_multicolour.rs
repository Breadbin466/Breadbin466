// =======================================================
// src/vic/horizontal_multicolour.rs — VIC-II multicolour horizontal transition paths
// =======================================================

use super::state::VicII;
use super::foreground::{ForegroundCell, PixelSpan};
use super::foreground_transition::render_foreground_span;
use super::horizontal_transition::ScrollTransition;

/*
Multicolour transitions preserve two-pixel symbol alignment. Odd XSCROLL changes may expose half of a logical symbol, so the transition keeps explicit single-width overrides instead of rounding to a complete pair.
*/
/* Multicolour rendering consumes graphics bits in pairs and duplicates each decoded symbol horizontally. Alignment state is retained across cell boundaries because an XSCROLL write can split a two-pixel symbol in the middle of a raster cycle. */
impl VicII {
/*
Entering a multicolour route mid-cell may begin on either half of a two-pixel symbol. The adjusted and spill copies retain both possible alignments, and single-width override marks the one-pixel fragments that cannot yet be duplicated as a complete symbol.
*/
pub(super) fn apply_to_mcm_scroll_transition(&mut self, transition: ScrollTransition) {
		let ScrollTransition {
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
			..
		} = transition;
		let mut gfx_data1_adjusted;
		let mut gfx_data1_spill;
		let char_data1_merged;
		let mut decremented_shift_mode: u8 = 0;

		if old_horizontal_scroll != 7 {
			if (old_horizontal_scroll & 1) != 0 {
				gfx_data1_adjusted = gfx_data1 << 1;
				gfx_data1_spill    = gfx_data1 << 1;
			} else {
				gfx_data1_adjusted = gfx_data1;
				gfx_data1_spill    = gfx_data1;
			}
			if (char_data1 & 0x800) != 0 || bmm_old {
				gfx_data1_adjusted &= 0xAA;
			}
		} else {
			gfx_data1_adjusted = gfx_data1;
			gfx_data1_spill    = gfx_data1;
		}

		if bmm_old {
			char_data1_merged = if horizontal_scroll_decrement >= 4 { char_data2 & 0xFFF } else { char_data1 };
		} else if horizontal_scroll_decrement >= 4 {
			if horizontal_scroll_decrement == 4 {
				char_data1_merged = (char_data1 & 0x8FF) | (char_data2 & 0x700);
			} else {
				if (char_data1 & 0x800) == 0 {
					decremented_shift_mode = 1;
				} else if (char_data2 & 0x800) == 0 {
					let t              = gfx_data1_adjusted & 0xAA;
					gfx_data1_adjusted = t | (t >> 1);
					let t              = gfx_data1_spill & 0xAA;
					gfx_data1_spill    = t | (t >> 1);
				}
				char_data1_merged = (char_data1 & 0x0FF) | (char_data2 & 0xF00);
			}
		} else {
			char_data1_merged = char_data1;
		}

		let skip_ref = &mut self.pixels_to_skip;
		let screen   = &mut self.screen;

		match old_horizontal_scroll {
			0 => {
				render_foreground_span(screen, ForegroundCell::new(gfx_data1, 4, char_data1, mode_new, 1, cdod, cycle), PixelSpan::new(4, 2), skip_ref);
				render_foreground_span(screen, ForegroundCell::new(gfx_data1_adjusted, 6, char_data1, mode_new, 0, cdod, cycle), PixelSpan::new(6, 2), skip_ref);
			}
			1 => {
				render_foreground_span(screen, ForegroundCell::new(gfx_data1, 4, char_data1, mode_new, 1, cdod, cycle), PixelSpan::new(3, 2), skip_ref);
				render_foreground_span(screen, ForegroundCell::new(gfx_data1_adjusted, 6, char_data1, mode_new, 0, cdod, cycle), PixelSpan::new(4, 2), skip_ref);
				render_foreground_span(screen, ForegroundCell::new(gfx_data1_spill, 8, char_data1, mode_new, 0, cdod, cycle), PixelSpan::new(6, 2), skip_ref);
			}
			2 => {
				render_foreground_span(screen, ForegroundCell::new(gfx_data1, 4, char_data1, mode_new, 1, cdod, cycle), PixelSpan::new(2, 2), skip_ref);
				render_foreground_span(screen, ForegroundCell::new(gfx_data1_adjusted, 6, char_data1, mode_new, 0, cdod, cycle), PixelSpan::new(4, 2), skip_ref);
				render_foreground_span(screen, ForegroundCell::new(gfx_data1_spill, 8, char_data1, mode_new, 0, cdod, cycle), PixelSpan::new(6, 2), skip_ref);
			}
			3 => {
				render_foreground_span(screen, ForegroundCell::new(gfx_data1, 4, char_data1, mode_new, 1, cdod, cycle), PixelSpan::new(1, 2), skip_ref);
				render_foreground_span(screen, ForegroundCell::new(gfx_data1_adjusted, 6, char_data1, mode_new, 0, cdod, cycle), PixelSpan::new(2, 2), skip_ref);
				render_foreground_span(screen, ForegroundCell::new(gfx_data1_spill, 8, char_data1, mode_new, 0, cdod, cycle), PixelSpan::new(4, 4), skip_ref);
			}
			4 => {
				render_foreground_span(screen, ForegroundCell::new(gfx_data1, 4, char_data1_merged, mode_new, 1, cdod, cycle), PixelSpan::new(0, 2), skip_ref);
				render_foreground_span(screen, ForegroundCell::new(gfx_data1_adjusted, 6, char_data1_merged, mode_new, decremented_shift_mode, cdod, cycle), PixelSpan::new(2, 2), skip_ref);
				render_foreground_span(screen, ForegroundCell::new(gfx_data1_spill, 8, char_data1_merged, mode_new, decremented_shift_mode, cdod, cycle), PixelSpan::new(4, 4), skip_ref);
			}
			5 => {
				render_foreground_span(screen, ForegroundCell::new(gfx_data0, 4, char_data0, mode_new, 1, cdod, cycle), PixelSpan::new(7, 1), skip_ref);
				render_foreground_span(screen, ForegroundCell::new(gfx_data1, 5, char_data1_merged, mode_new, 1, cdod, cycle), PixelSpan::new(0, 1), skip_ref);
				render_foreground_span(screen, ForegroundCell::new(gfx_data1_adjusted, 6, char_data1_merged, mode_new, decremented_shift_mode, cdod, cycle), PixelSpan::new(0, 2), skip_ref);
				render_foreground_span(screen, ForegroundCell::new(gfx_data1_spill, 8, char_data1_merged, mode_new, decremented_shift_mode, cdod, cycle), PixelSpan::new(2, 6), skip_ref);
			}
			6 => {
				render_foreground_span(screen, ForegroundCell::new(gfx_data0, 4, char_data0, mode_new, 1, cdod, cycle), PixelSpan::new(6, 2), skip_ref);
				render_foreground_span(screen, ForegroundCell::new(gfx_data1_adjusted, 6, char_data1_merged, mode_new, decremented_shift_mode, cdod, cycle), PixelSpan::new(0, 2), skip_ref);
				render_foreground_span(screen, ForegroundCell::new(gfx_data1_spill, 8, char_data1_merged, mode_new, decremented_shift_mode, cdod, cycle), PixelSpan::new(2, 6), skip_ref);
			}
			_ => {
				render_foreground_span(screen, ForegroundCell::new(gfx_data0, 4, char_data0, mode_new, 1, cdod, cycle), PixelSpan::new(5, 2), skip_ref);
				render_foreground_span(screen, ForegroundCell::new((gfx_data0 & 0x55) << 1, 6, char_data0, mode_new, decremented_shift_mode, cdod, cycle), PixelSpan::new(6, 1), skip_ref);
				render_foreground_span(screen, ForegroundCell::new(gfx_data1_spill, 7, char_data1_merged, mode_new, decremented_shift_mode, cdod, cycle), PixelSpan::new(0, 8), skip_ref);
			}
		}
	}

/*
A decrement of four or more pixels moves the visible origin into the preceding half-cell. Only the first four reconstructed pixels belong to the transition cycle; the ordinary pipeline resumes after that boundary with the newly selected character metadata.
*/
pub(super) fn apply_mcm_large_decrement_transition(&mut self, transition: ScrollTransition) {
		let ScrollTransition {
			mode_new,
			old_scroll: old_horizontal_scroll,
			decrement: horizontal_scroll_decrement,
			bitmap_mode_old: bmm_old,
			character_output_disabled: cdod,
			graphics_previous: gfx_data1,
			character_previous: char_data1,
			character_current: char_data2,
			cycle,
			..
		} = transition;
		let mut gfx_data1_adjusted = gfx_data1;
		let char_data1_merged;
		let mut decremented_shift_mode: u8 = 0;

		if bmm_old {
			char_data1_merged = char_data2 & 0xFFF;
		} else if horizontal_scroll_decrement == 4 {
			char_data1_merged = (char_data1 & 0x8FF) | (char_data2 & 0x700);
		} else {
			if (char_data1 & 0x800) == 0 {
				decremented_shift_mode = 1;
			} else if (char_data2 & 0x800) == 0 {
				let t              = gfx_data1_adjusted & 0xAA;
				gfx_data1_adjusted = t | (t >> 1);
			}
			char_data1_merged = (char_data1 & 0x0FF) | (char_data2 & 0xF00);
		}

		let skip_ref = &mut self.pixels_to_skip;
		let screen   = &mut self.screen;

		match old_horizontal_scroll {
			4 => render_foreground_span(screen, ForegroundCell::new(gfx_data1_adjusted, 4, char_data1_merged, mode_new, decremented_shift_mode, cdod, cycle), PixelSpan::new(0, 4), skip_ref),
			5 => render_foreground_span(screen, ForegroundCell::new(gfx_data1_adjusted, 5, char_data1_merged, mode_new, decremented_shift_mode, cdod, cycle), PixelSpan::new(0, 4), skip_ref),
			6 => render_foreground_span(screen, ForegroundCell::new(gfx_data1_adjusted, 6, char_data1_merged, mode_new, decremented_shift_mode, cdod, cycle), PixelSpan::new(0, 4), skip_ref),
			7 => render_foreground_span(screen, ForegroundCell::new(gfx_data1_adjusted, 7, char_data1_merged, mode_new, decremented_shift_mode, cdod, cycle), PixelSpan::new(0, 4), skip_ref),
			_ => {}
		}
	}
}