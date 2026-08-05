// =======================================================
// src/vic/graphics_pixels.rs — Low-level pixel write helpers: standard 2-colour, multi-colour 4-colour and solid colour fill into the line buffer
// =======================================================

use crate::vic::constants::{STD_PIXEL_MASKS};
use crate::vic::screen::Screen;

/*
These primitives write encoded VIC-II colour slots into the raster line buffer. The high bit marks values that still require background-register resolution, allowing colour-register writes to remain visible until the cell is finalised.
*/

/*
Horizontal scroll shifts the cell relative to the cycle-derived base. Positions before the retained line buffer are rejected rather than wrapped into the visible raster.
*/
#[inline(always)]
fn line_base(cycle: u16, horizontal_scroll: i8) -> Option<usize> {
	let base = cycle as isize * 8 - 20 + horizontal_scroll as isize + 8;
	(base >= 0).then_some(base as usize)
}

#[inline]
/* Fill a complete eight-pixel cell with one already-encoded colour slot. */
pub fn write_color_byte8(screen: &mut Screen, color: u8, horizontal_scroll: i8, cycle: u16) {
	if !screen.compose_video() { return; }
	let Some(base) = line_base(cycle, horizontal_scroll) else { return; };
	let Some(target) = screen.line_buf.get_mut(base..base + 8) else { return; };
	target.fill(color);
}

#[inline]
/* Fill only the visible fragment retained after a mode, scroll or border transition. */
pub fn write_color_span(
	screen: &mut Screen,
	color: u8,
	horizontal_scroll: i8,
	cycle: u16,
	pixel_start: u8,
	count: u8,
) {
	if !screen.compose_video() || count == 0 || pixel_start >= 8 {
		return;
	}
	let Some(base) = line_base(cycle, horizontal_scroll) else { return; };
	let visible = usize::from(count.min(8 - pixel_start));
	let Some(target) = screen.line_buf.get_mut(base..base + visible) else { return; };
	target.fill(color);
}

/*
Standard graphics expands each source bit to one pixel and selects between two encoded colour slots. The extended form below writes only the surviving portion of a cell after a control transition.
*/
#[inline]
pub fn write_std2_byte(screen: &mut Screen, gfx_data: u8, horizontal_scroll: i8, colour2: [u8; 2], cycle: u16) {
	if !screen.compose_video() { return; }
	let Some(base) = line_base(cycle, horizontal_scroll) else { return; };
	let Some(target) = screen.line_buf.get_mut(base..base + 8) else { return; };
	let select = STD_PIXEL_MASKS[gfx_data as usize];
	let colour0 = u64::from_ne_bytes([colour2[0]; 8]);
	let colour1 = u64::from_ne_bytes([colour2[1]; 8]);
	let pixels = (colour0 & !select) | (colour1 & select);
	target.copy_from_slice(&pixels.to_ne_bytes());
}

#[inline]
/* Decode a standard-width fragment directly so skipped leading pixels do not overwrite data from the previous pipeline state. */
pub fn write_std2_byte_ex(
	screen: &mut Screen,
	gfx_data: u8,
	horizontal_scroll: i8,
	colour2: [u8; 2],
	cycle: u16,
	pixel_start: u8,
	count: u8,
) {
	if !screen.compose_video() || count == 0 || pixel_start >= 8 {
		return;
	}
	let Some(base) = line_base(cycle, horizontal_scroll) else { return; };
	let visible = usize::from(count.min(8 - pixel_start));
	let Some(target) = screen.line_buf.get_mut(base..base + visible) else { return; };
	let mut offset = 0usize;
	let mut shift = 7 - pixel_start;
	while offset < visible {
		target[offset] = colour2[((gfx_data >> shift) & 1) as usize];
		offset += 1;
		shift = shift.wrapping_sub(1);
	}
}

/*
Multicolour graphics decodes four two-bit symbols and doubles each symbol horizontally. This is deliberately separate from sprite multicolour expansion because foreground masks and colour sources differ.
*/
#[inline]
pub fn write_mcm4_byte(screen: &mut Screen, gfx_data: u8, horizontal_scroll: i8, colour4: [u8; 4], cycle: u16) {
	if !screen.compose_video() { return; }
	let Some(base) = line_base(cycle, horizontal_scroll) else { return; };
	let Some(target) = screen.line_buf.get_mut(base..base + 8) else { return; };
	let colour0 = colour4[((gfx_data >> 6) & 3) as usize];
	let colour1 = colour4[((gfx_data >> 4) & 3) as usize];
	let colour2 = colour4[((gfx_data >> 2) & 3) as usize];
	let colour3 = colour4[(gfx_data & 3) as usize];
	let pixels = u64::from_le_bytes([
		colour0, colour0, colour1, colour1, colour2, colour2, colour3, colour3,
	]);
	target.copy_from_slice(&pixels.to_le_bytes());
}

#[inline]
/* Decode a multicolour fragment while preserving two-pixel symbol pairing even when the fragment begins on the second pixel of a symbol. */
pub fn write_mcm4_byte_ex(
	screen: &mut Screen,
	gfx_data: u8,
	horizontal_scroll: i8,
	colour4: [u8; 4],
	cycle: u16,
	pixel_start: u8,
	count: u8,
) {
	if !screen.compose_video() || count == 0 || pixel_start >= 8 {
		return;
	}
	let Some(base) = line_base(cycle, horizontal_scroll) else { return; };
	let visible = usize::from(count.min(8 - pixel_start));
	let Some(target) = screen.line_buf.get_mut(base..base + visible) else { return; };
	let mut offset = 0usize;
	let mut source_pixel = pixel_start as usize;
	while offset < visible {
		let shift = 6 - ((source_pixel & !1) as u8);
		target[offset] = colour4[((gfx_data >> shift) & 3) as usize];
		offset += 1;
		source_pixel += 1;
	}
}