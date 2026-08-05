// =======================================================
// src/vic/foreground.rs — VIC-II foreground cell and pixel-span state
// =======================================================

use super::fsm::GraphicsMode;

#[derive(Clone, Copy)]
/*
ForegroundCell is the immutable input for one eight-pixel graphics cell. It captures fetched graphics data together with the control-register state that was effective for that cell, allowing later register writes to affect only the appropriate part of the pipeline.
*/
pub struct ForegroundCell {
	pub gfx_data: u8,
	pub horizontal_scroll: i8,
	pub char_data: u16,
	pub mode: GraphicsMode,
	pub single_width_override: u8,
	pub vertical_border: bool,
	pub cycle: u16,
}

impl ForegroundCell {
	#[inline(always)]
	/* Construction freezes the fetch result and the control state that applied to it. Later writes create transition spans instead of mutating this already-scheduled cell. */
	pub const fn new(
		gfx_data: u8,
		horizontal_scroll: i8,
		char_data: u16,
		mode: GraphicsMode,
		single_width_override: u8,
		vertical_border: bool,
		cycle: u16,
	) -> Self {
		Self {
			gfx_data,
			horizontal_scroll,
			char_data,
			mode,
			single_width_override,
			vertical_border,
			cycle,
		}
	}

	#[inline(always)]
	/* A blank cell still retains scroll, matrix data and mode because border openings or mid-cell changes may expose part of it later in the pipeline. */
	pub const fn blank(horizontal_scroll: u8, char_data: u16, mode: GraphicsMode, cycle: u16) -> Self {
		Self::new(0, horizontal_scroll as i8, char_data, mode, 0, false, cycle)
	}

	#[inline(always)]
	/* Derive a transition cell with a new alignment while preserving the fetch data and mode already committed to the pipeline. */
	pub(super) fn with_scroll(self, horizontal_scroll: i8) -> Self {
		Self { horizontal_scroll, ..self }
	}
}

#[derive(Clone, Copy)]
/*
PixelSpan identifies the subset of a graphics cell that remains valid across a mid-cell mode or scroll transition. Splitting a cell instead of redrawing it preserves writes that take effect on a particular pixel boundary.
*/
pub struct PixelSpan {
	pub start: u8,
	pub count: u8,
}

impl PixelSpan {
	#[inline(always)]
	/* Spans are expressed in source-pixel order and are clipped by the caller before colour and coverage are written to the raster buffers. */
	pub const fn new(start: u8, count: u8) -> Self {
		Self { start, count }
	}
}