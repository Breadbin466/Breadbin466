// =======================================================
// src/vic/graphics_mask.rs — Foreground collision mask write helpers - Updates the mask buffer that determines sprite/foreground priority
// =======================================================

use crate::vic::screen::Screen;

/*
The graphics mask records foreground occupancy independently of palette values. Standard modes contribute one mask bit per source bit; multicolour modes widen each non-zero two-bit symbol to a two-pixel foreground pair.
*/

#[inline(always)]
/* Convert the high bit of each two-bit multicolour symbol into two adjacent foreground pixels. Symbol zero remains transparent to sprite priority and collision logic. */
fn graphics_mask_mcm(i: u8) -> u8 {
	let masked = i & 0xAA;
	masked | (masked >> 1)
}

/*
Coverage bits limit an update to the portion of a cell affected by a transition, while pixel bits carry the actual foreground result. Combining both into the packed mask avoids disturbing neighbouring pixels already rendered.
*/
#[inline(always)]
fn write_mask_pair(screen: &mut Screen, pixel_bits: u16, coverage_bits: u16, cycle: u16, byte_offset: isize) {
	let hi  = (pixel_bits >> 8) as u8;
	let lo  = (pixel_bits & 0xFF) as u8;
	let mhi = (coverage_bits >> 8) as u8;
	let mlo = (coverage_bits & 0xFF) as u8;
	let idx1 = cycle as isize - 1 + byte_offset;
	let idx2 = cycle as isize + byte_offset;
	if idx1 >= 0 {
		let index = idx1 as usize;
		if index < screen.mask_buf.len() {
			let value = screen.mask_buf[index];
			screen.mask_buf[index] = (value & !mhi) | hi;
		}
	}
	if idx2 >= 0 {
		let index = idx2 as usize;
		if index < screen.mask_buf.len() {
			let value = screen.mask_buf[index];
			screen.mask_buf[index] = (value & !mlo) | lo;
		}
	}
}

#[inline(always)]
/* Write a complete standard-width cell into the packed foreground mask at its scrolled raster position. */
pub fn write_fore_mask_std(screen: &mut Screen, gfx_data: u8, horizontal_scroll: i8, cycle: u16) {
	let byte_offset = (horizontal_scroll / 8) as isize;
	let shift       = (horizontal_scroll & 7) as u16;
	let pixel_bits       = ((gfx_data as u16) << 8) >> shift;
	let coverage_bits       = (0xFFu16 << 8) >> shift;
	write_mask_pair(screen, pixel_bits, coverage_bits, cycle, byte_offset);
}

#[inline(always)]
/* Update only the surviving fragment of a standard-width cell after a mode, border or scroll transition. */
pub fn write_fore_mask_std_ex(screen: &mut Screen, mut gfx_data: u8, horizontal_scroll: i8, pixel_start: u8, count: u8, cycle: u16) {
	let byte_offset = (horizontal_scroll / 8) as isize;
	gfx_data      <<= pixel_start;
	let mask: u8    = 0xFFu8 << (8 - count);
	gfx_data       &= mask;
	let shift       = (horizontal_scroll & 7) as u16;
	let pixel_bits       = ((gfx_data as u16) << 8) >> shift;
	let coverage_bits       = ((mask as u16) << 8) >> shift;
	write_mask_pair(screen, pixel_bits, coverage_bits, cycle, byte_offset);
}

#[inline(always)]
/* Expand each occupied multicolour symbol to its two physical pixels before writing foreground coverage. */
pub fn write_fore_mask_mcm(screen: &mut Screen, gfx_data: u8, horizontal_scroll: i8, cycle: u16) {
	let byte_offset = (horizontal_scroll / 8) as isize;
	let masked      = graphics_mask_mcm(gfx_data);
	let shift       = (horizontal_scroll & 7) as u16;
	let pixel_bits       = ((masked as u16) << 8) >> shift;
	let coverage_bits       = (0xFFu16 << 8) >> shift;
	write_mask_pair(screen, pixel_bits, coverage_bits, cycle, byte_offset);
}

#[inline(always)]
/* Apply fragment coverage to a multicolour cell while preserving neighbouring mask bits already emitted by the previous pipeline state. */
pub fn write_fore_mask_mcm_ex(screen: &mut Screen, gfx_data: u8, horizontal_scroll: i8, pixel_start: u8, count: u8, cycle: u16) {
	let byte_offset = (horizontal_scroll / 8) as isize;
	let mut g       = graphics_mask_mcm(gfx_data);
	g              <<= pixel_start;
	let mask: u8    = 0xFFu8 << (8 - count);
	g              &= mask;
	let shift       = (horizontal_scroll & 7) as u16;
	let pixel_bits       = ((g as u16) << 8) >> shift;
	let coverage_bits       = ((mask as u16) << 8) >> shift;
	write_mask_pair(screen, pixel_bits, coverage_bits, cycle, byte_offset);
}