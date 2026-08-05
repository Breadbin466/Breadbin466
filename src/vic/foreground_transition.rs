// =======================================================
// src/vic/foreground_transition.rs — partial foreground output during live mode and scroll changes
// =======================================================

use crate::vic::screen::Screen;
use crate::vic::constants::{BACKCOLORINDEX0, BACKCOLORINDEX1, BACKCOLORINDEX2, BACKCOLORINDEX3, VIC_BLACK};
use super::foreground::{ForegroundCell, PixelSpan};
use super::graphics_pixels::*;
use super::graphics_mask::*;

/*
Mid-cell control-register writes are rendered as explicit spans. The first span uses the old mode or scroll state and the remainder uses the new state, matching the pixel boundary at which the VIC-II pipeline observes the write.
*/

/*
Only the requested pixel interval is emitted. The helper applies the same colour and foreground-mask rules as a complete cell while preserving any pixels already produced by the preceding state.
*/
pub fn render_foreground_span(screen: &mut Screen, mut cell: ForegroundCell, span: PixelSpan, pixels_to_skip: &mut u8) {
	let mut pixel_start = span.start;
	let mut count = span.count;
	if count == 0 || count > 8 || pixel_start > 8 { return; }
	let skip = *pixels_to_skip;
	if skip > 0 {
		*pixels_to_skip = 0;
		cell.horizontal_scroll = cell.horizontal_scroll.wrapping_add(skip as i8);
		pixel_start = pixel_start.wrapping_add(skip);
		count = count.saturating_sub(skip);
		if count == 0 || pixel_start > 8 { return; }
	}
	if cell.vertical_border { cell.gfx_data = 0; }
	let gfx_data = cell.gfx_data;
	let horizontal_scroll = cell.horizontal_scroll;
	let char_data = cell.char_data;
	let graphics_mode = cell.mode;
	let single_width_override = cell.single_width_override;
	let cycle = cell.cycle;

	/*
	When colour composition is disabled, transition rendering still writes the foreground-occupancy mask. Collision and sprite-priority behaviour therefore remains cycle-accurate even when the host skips visible pixel generation.
	*/
	if !screen.compose_video() {
		let multicolour_mask = match graphics_mode.code() {
			1 | 5 => ((char_data >> 8) as u8 & 8) != 0 && single_width_override == 0,
			3 | 7 => single_width_override == 0,
			_ => false,
		};
		if multicolour_mask {
			write_fore_mask_mcm_ex(screen, gfx_data, horizontal_scroll, pixel_start, count, cycle);
		} else {
			write_fore_mask_std_ex(screen, gfx_data, horizontal_scroll, pixel_start, count, cycle);
		}
		return;
	}

	/* The eight branches mirror the three hardware control bits directly. Illegal modes still update occupancy masks even when their visible colour output collapses to black. */
	match graphics_mode.code() {
		0 => {
			let a = [BACKCOLORINDEX0, ((char_data >> 8) & 0x0F) as u8];
			write_std2_byte_ex(screen, gfx_data, horizontal_scroll, a, cycle, pixel_start, count);
			write_fore_mask_std_ex(screen, gfx_data, horizontal_scroll, pixel_start, count, cycle);
		}
		1 => {
			let colour = (char_data >> 8) as u8;
			if (colour & 8) != 0 {
				if single_width_override == 0 {
					let a = [BACKCOLORINDEX0, BACKCOLORINDEX1, BACKCOLORINDEX2, colour & 7];
					write_mcm4_byte_ex(screen, gfx_data, horizontal_scroll, a, cycle, pixel_start, count);
					write_fore_mask_mcm_ex(screen, gfx_data, horizontal_scroll, pixel_start, count, cycle);
				} else {
					write_std2_byte_ex(screen, gfx_data, horizontal_scroll, [BACKCOLORINDEX0, BACKCOLORINDEX2], cycle, pixel_start, count);
					write_fore_mask_std_ex(screen, gfx_data, horizontal_scroll, pixel_start, count, cycle);
				}
			} else {
				let a = [BACKCOLORINDEX0, colour & 7];
				write_std2_byte_ex(screen, gfx_data, horizontal_scroll, a, cycle, pixel_start, count);
				write_fore_mask_std_ex(screen, gfx_data, horizontal_scroll, pixel_start, count, cycle);
			}
		}
		2 => {
			let a = [(char_data & 0x0F) as u8, ((char_data >> 4) & 0x0F) as u8];
			write_std2_byte_ex(screen, gfx_data, horizontal_scroll, a, cycle, pixel_start, count);
			write_fore_mask_std_ex(screen, gfx_data, horizontal_scroll, pixel_start, count, cycle);
		}
		3 => {
			if single_width_override == 0 {
				let a = [BACKCOLORINDEX0, ((char_data >> 4) & 0x0F) as u8, (char_data & 0x0F) as u8, ((char_data >> 8) & 0x0F) as u8];
				write_mcm4_byte_ex(screen, gfx_data, horizontal_scroll, a, cycle, pixel_start, count);
				write_fore_mask_mcm_ex(screen, gfx_data, horizontal_scroll, pixel_start, count, cycle);
			} else {
				let a = [BACKCOLORINDEX0, (char_data & 0x0F) as u8];
				write_std2_byte_ex(screen, gfx_data, horizontal_scroll, a, cycle, pixel_start, count);
				write_fore_mask_std_ex(screen, gfx_data, horizontal_scroll, pixel_start, count, cycle);
			}
		}
		4 => {
			let b0 = match (char_data & 0xC0) >> 6 { 0 => BACKCOLORINDEX0, 1 => BACKCOLORINDEX1, 2 => BACKCOLORINDEX2, _ => BACKCOLORINDEX3 };
			let a  = [b0, (char_data >> 8) as u8];
			write_std2_byte_ex(screen, gfx_data, horizontal_scroll, a, cycle, pixel_start, count);
			write_fore_mask_std_ex(screen, gfx_data, horizontal_scroll, pixel_start, count, cycle);
		}
		5 => {
			let colour = (char_data >> 8) as u8;
			write_color_span(screen, VIC_BLACK, horizontal_scroll, cycle, pixel_start, count);
			if (colour & 8) != 0 && single_width_override == 0 {
				write_fore_mask_mcm_ex(screen, gfx_data, horizontal_scroll, pixel_start, count, cycle);
			} else {
				write_fore_mask_std_ex(screen, gfx_data, horizontal_scroll, pixel_start, count, cycle);
			}
		}
		6 => {
			write_color_span(screen, VIC_BLACK, horizontal_scroll, cycle, pixel_start, count);
			write_fore_mask_std_ex(screen, gfx_data, horizontal_scroll, pixel_start, count, cycle);
		}
		_ => {
			write_color_span(screen, VIC_BLACK, horizontal_scroll, cycle, pixel_start, count);
			if single_width_override == 0 {
				write_fore_mask_mcm_ex(screen, gfx_data, horizontal_scroll, pixel_start, count, cycle);
			} else {
				write_fore_mask_std_ex(screen, gfx_data, horizontal_scroll, pixel_start, count, cycle);
			}
		}
	}
}