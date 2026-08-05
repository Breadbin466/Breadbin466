// =======================================================
// src/vic/foreground_renderer.rs — Foreground pixel generation: background fill, standard/multicolour/ECM mode dispatch and partial-pixel extended variant
// =======================================================

use crate::vic::screen::Screen;
use crate::vic::constants::{BACKCOLORINDEX0, BACKCOLORINDEX1, BACKCOLORINDEX2, BACKCOLORINDEX3, VIC_BLACK};
use super::graphics_pixels::*;
use super::graphics_mask::*;
use super::foreground::{ForegroundCell, PixelSpan};
use super::foreground_transition::render_foreground_span;

/*
The foreground renderer converts a latched graphics cell into colour indices and a separate foreground-coverage mask. Colour generation and mask generation remain distinct because sprite priority and collision logic depend on whether a graphics pixel is foreground, not merely on its final colour.
*/

/*
An idle or blank cell still emits the mode-dependent background colour. Extended-colour mode selects that background from the upper character-code bits, while illegal modes resolve to black without inventing foreground coverage.
*/
#[inline]
pub fn render_foreground_cell(screen: &mut Screen, cell: ForegroundCell, pixels_to_skip: &mut u8) {
	let horizontal_scroll = cell.horizontal_scroll as u8;
	let char_data = cell.char_data;
	let graphics_mode = cell.mode;
	let cycle = cell.cycle;
	let skip = *pixels_to_skip;
	if skip > 0 {
		*pixels_to_skip = 0;
		let adjusted = cell.with_scroll(horizontal_scroll.wrapping_add(skip) as i8);
		render_foreground_span(screen, adjusted, PixelSpan::new(skip, 8 - skip), pixels_to_skip);
		return;
	}

	let color: u8 = match graphics_mode.code() {
		0 | 1 | 3 => BACKCOLORINDEX0,
		2          => (char_data & 0x0F) as u8,
		4          => match (char_data & 0xC0) >> 6 {
			0 => BACKCOLORINDEX0,
			1 => BACKCOLORINDEX1,
			2 => BACKCOLORINDEX2,
			_ => BACKCOLORINDEX3,
		},
		_          => VIC_BLACK,
	};
	write_color_byte8(screen, color, horizontal_scroll as i8, cycle);
}

/*
A fetched cell is dispatched through the standard, multicolour, bitmap or extended-colour path. When video composition is disabled, the colour writes are skipped but the foreground mask is still produced so collision-visible behaviour remains clocked.
*/
#[inline]
pub fn render_foreground(screen: &mut Screen, mut cell: ForegroundCell, pixels_to_skip: &mut u8) {
	if cell.vertical_border { cell.gfx_data = 0; }
	let gfx_data = cell.gfx_data;
	let horizontal_scroll = cell.horizontal_scroll;
	let char_data = cell.char_data;
	let graphics_mode = cell.mode;
	let cycle = cell.cycle;
	let skip = *pixels_to_skip;
	if skip > 0 {
		*pixels_to_skip = 0;
		render_foreground_span(screen, cell.with_scroll(horizontal_scroll.wrapping_add(skip as i8)), PixelSpan::new(skip, 8 - skip), pixels_to_skip);
		return;
	}

	if !screen.compose_video() {
		match graphics_mode.code() {
			1 | 5 if ((char_data >> 8) as u8 & 8) != 0 => {
				write_fore_mask_mcm(screen, gfx_data, horizontal_scroll, cycle);
			}
			3 | 7 => {
				write_fore_mask_mcm(screen, gfx_data, horizontal_scroll, cycle);
			}
			_ => {
				write_fore_mask_std(screen, gfx_data, horizontal_scroll, cycle);
			}
		}
		return;
	}

	match graphics_mode.code() {
		0 => {
			let a = [BACKCOLORINDEX0, ((char_data >> 8) & 0x0F) as u8];
			write_std2_byte(screen, gfx_data, horizontal_scroll, a, cycle);
			write_fore_mask_std(screen, gfx_data, horizontal_scroll, cycle);
		}
		1 => {
			let colour = (char_data >> 8) as u8;
			if (colour & 8) != 0 {
				let a = [BACKCOLORINDEX0, BACKCOLORINDEX1, BACKCOLORINDEX2, colour & 7];
				write_mcm4_byte(screen, gfx_data, horizontal_scroll, a, cycle);
				write_fore_mask_mcm(screen, gfx_data, horizontal_scroll, cycle);
			} else {
				let a = [BACKCOLORINDEX0, colour & 7];
				write_std2_byte(screen, gfx_data, horizontal_scroll, a, cycle);
				write_fore_mask_std(screen, gfx_data, horizontal_scroll, cycle);
			}
		}
		2 => {
			let a = [(char_data & 0x0F) as u8, ((char_data >> 4) & 0x0F) as u8];
			write_std2_byte(screen, gfx_data, horizontal_scroll, a, cycle);
			write_fore_mask_std(screen, gfx_data, horizontal_scroll, cycle);
		}
		3 => {
			let a = [BACKCOLORINDEX0, ((char_data >> 4) & 0x0F) as u8, (char_data & 0x0F) as u8, ((char_data >> 8) & 0x0F) as u8];
			write_mcm4_byte(screen, gfx_data, horizontal_scroll, a, cycle);
			write_fore_mask_mcm(screen, gfx_data, horizontal_scroll, cycle);
		}
		4 => {
			let b0 = match (char_data & 0xC0) >> 6 { 0 => BACKCOLORINDEX0, 1 => BACKCOLORINDEX1, 2 => BACKCOLORINDEX2, _ => BACKCOLORINDEX3 };
			let a  = [b0, (char_data >> 8) as u8];
			write_std2_byte(screen, gfx_data, horizontal_scroll, a, cycle);
			write_fore_mask_std(screen, gfx_data, horizontal_scroll, cycle);
		}
		5 => {
			let colour = (char_data >> 8) as u8;
			write_color_byte8(screen, VIC_BLACK, horizontal_scroll, cycle);
			if (colour & 8) != 0 {
				write_fore_mask_mcm(screen, gfx_data, horizontal_scroll, cycle);
			} else {
				write_fore_mask_std(screen, gfx_data, horizontal_scroll, cycle);
			}
		}
		6 => {
			write_color_byte8(screen, VIC_BLACK, horizontal_scroll, cycle);
			write_fore_mask_std(screen, gfx_data, horizontal_scroll, cycle);
		}
		_ => {
			write_color_byte8(screen, VIC_BLACK, horizontal_scroll, cycle);
			write_fore_mask_mcm(screen, gfx_data, horizontal_scroll, cycle);
		}
	}
}

impl super::state::VicII {
	#[inline(always)]
	/* Snapshot the currently latched graphics cell before dispatch. This prevents later register or fetch changes in the same host call from leaking into the cell already entering the pixel pipeline. */
	pub(super) fn draw_visible_foreground(&mut self, horizontal_scroll: i8, cycle: u16) {
		let cell = ForegroundCell::new(
			self.graphics_data_fetched,
			horizontal_scroll,
			self.char_data_fetched,
			self.graphics_mode,
			0,
			self.border.char_data_output_disabled,
			cycle,
		);
		render_foreground(&mut self.screen, cell, &mut self.pixels_to_skip);
	}
}