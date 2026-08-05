// =======================================================
// src/vic/horizontal_transition.rs — mid-line horizontal-scroll pixel pipeline transitions
// =======================================================

use super::state::VicII;
use super::fsm::GraphicsMode;
use super::foreground::{ForegroundCell, PixelSpan};
use super::foreground_transition::render_foreground_span;

/*
ScrollTransition captures the old and new XSCROLL values together with the active graphics mode and cycle. The renderer uses this snapshot to reproduce partial-cell effects from a $D016 write without consulting registers that may change again later in the line.
*/
#[derive(Clone, Copy)]
pub(super) struct ScrollTransition {
	/* Graphics route that produced the pixels already present in the shifter. */
	pub(super) mode_old: GraphicsMode,
	/* Graphics route selected by the new control-register value. */
	pub(super) mode_new: GraphicsMode,
	/* XSCROLL alignment under which the current cell began. */
	pub(super) old_scroll: u8,
	/* Number of pixels newly exposed from older pipeline stages. */
	pub(super) decrement: i32,
	/* Bitmap mode changes which colour metadata travels with the fetched byte. */
	pub(super) bitmap_mode_old: bool,
	/* Border logic may suppress character colour while foreground occupancy remains live. */
	pub(super) character_output_disabled: bool,
	/* Graphics byte in the immediately preceding pipeline stage. */
	pub(super) graphics_previous: u8,
	/* Graphics byte required when a large decrement reaches into an older cell. */
	pub(super) graphics_older: u8,
	/* Character metadata paired with graphics_older. */
	pub(super) character_older: u16,
	/* Character metadata paired with graphics_previous. */
	pub(super) character_previous: u16,
	/* Newly fetched character metadata available to the replacement route. */
	pub(super) character_current: u16,
	/* Raster cycle whose output span is being reconstructed. */
	pub(super) cycle: u16,
}

/*
Horizontal-scroll transitions are split between standard and multicolour paths because a one-pixel change in XSCROLL can cross a two-pixel multicolour symbol boundary. Large decrements additionally expose pixels from the preceding cell.
*/
impl VicII {

	/*
	A live XSCROLL write can either hide pixels that have not yet emerged or reveal pixels from an older pipeline cell. The method first emits any newly exposed gap, then snapshots all three relevant graphics/character stages before dispatching to the standard or multicolour reconstruction path.
	*/
	pub(super) fn apply_scroll_decrement_pixel_transition(&mut self, mode_old: GraphicsMode, mode_new: GraphicsMode, old_horizontal_scroll: u8, new_horizontal_scroll: u8, cycle: u16) {
		let char_data1 = self.char_data_pipeline_1;
		let char_data2 = self.char_data_fetched;

		if new_horizontal_scroll > old_horizontal_scroll {
			let skip_ref = &mut self.pixels_to_skip;
			render_foreground_span(&mut self.screen, ForegroundCell::new(0, old_horizontal_scroll as i8, char_data1, mode_new, 0, false, cycle + 1), PixelSpan::new(0, new_horizontal_scroll - old_horizontal_scroll), skip_ref);
		}

		let horizontal_scroll_decrement: i32 = if old_horizontal_scroll > new_horizontal_scroll {
			(old_horizontal_scroll - new_horizontal_scroll) as i32
		} else {
			0
		};

		/*
		Small decrements within an unchanged single-width route are already represented by the normal shifter position. Larger moves cross a half-cell boundary and therefore require data from the older pipeline stage.
		*/
		if mode_new == mode_old && horizontal_scroll_decrement < 4 {
			return;
		}

		let gfx_data1  = self.graphics_data_pipeline_1;
		let gfx_data0  = self.graphics_data_pipeline_2;
		let char_data0 = self.char_data_pipeline_2;
		let bmm_old = mode_old.bitmap_bit();
		let cdod       = self.border.char_data_output_disabled;

		{
			let skip_ref = &mut self.pixels_to_skip;
			render_foreground_span(&mut self.screen, ForegroundCell::new(gfx_data1, 0, char_data1, mode_new, 0, cdod, cycle + 1), PixelSpan::new(8 - old_horizontal_scroll, old_horizontal_scroll), skip_ref);
		}

		let transition = ScrollTransition {
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
		};

		if !mode_new.multicolour_bit() {
			self.apply_non_mcm_scroll_transition(transition);
		} else if !mode_old.multicolour_bit() {
			self.apply_to_mcm_scroll_transition(transition);
		} else if horizontal_scroll_decrement >= 4 {
			self.apply_mcm_large_decrement_transition(transition);
		}
	}

}