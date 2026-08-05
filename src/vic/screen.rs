// =======================================================
// src/vic/screen.rs — Video Output & Pixel Buffer
// =======================================================

use crate::vic::constants::{LINE_BUF_LEN, MASK_BUF_LEN, BORDER_BITS_LEN, FRAMEBUFFER_SIZE};
use crate::vic::constants::{
	TOTAL_WIDTH, TOTAL_HEIGHT, VIC_PALETTE,
	BORDERCOLORINDEX, DISPLAY_START,
};

/*
Screen is the raster-domain output stage. It stores colour indices, foreground masks, collision masks and the final RGB framebuffer separately so border, foreground and sprite units can contribute in hardware order before the line is converted to host-visible pixels.
*/
pub struct Screen {
	pub framebuffer:     Box<[u8; FRAMEBUFFER_SIZE]>,
	pub line_buf:       [u8; LINE_BUF_LEN],
	pub mask_buf:       [u8; MASK_BUF_LEN],
	pub collision_line: [u8; LINE_BUF_LEN],
	pub sprite_line:    [u8; LINE_BUF_LEN],
	pub sprite_behind:  [bool; LINE_BUF_LEN],
	border_bits:        [u8; BORDER_BITS_LEN],
	pub bg_color:       [u8; 16],
	resolved_bg:       [u8; 16],
	resolved_palette:  [u32; 32],
	has_visible_sprite: bool,
	sprite_buffers_dirty: bool,
	compose_video: bool,
}

/* Pack one RGB palette entry into the 24-bit lane format used by the two-pixel fast path. The unused high byte remains zero so adjacent pixels can be concatenated without colour bleed. */
#[inline(always)]
fn palette_u32(index: usize) -> u32 {
	let rgb = VIC_PALETTE[index];
	u32::from_le_bytes([rgb[0], rgb[1], rgb[2], 0])
}

/* Build parallel palette banks for direct colour indices and deferred background slots. Entries 16 through 31 begin as black and are replaced when the corresponding background register is written. */
fn build_resolved_palette() -> [u32; 32] {
	let mut palette = [0u32; 32];
	let mut index = 0;
	while index < 16 {
		let colour = palette_u32(index);
		palette[index] = colour;
		palette[index + 16] = palette_u32(0);
		index += 1;
	}
	palette
}

impl Screen {
	/*
	Construction allocates independent foreground, border, sprite and collision planes. Keeping them separate until flush_line preserves VIC-II priority rules and permits collision processing even when host video composition is disabled.
	*/
	pub fn new() -> Self {
		Self {
			framebuffer:     Box::new([0u8; FRAMEBUFFER_SIZE]),
			line_buf:       [0u8; LINE_BUF_LEN],
			mask_buf:       [0u8; MASK_BUF_LEN],
			collision_line: [0u8; LINE_BUF_LEN],
			sprite_line:    [0u8; LINE_BUF_LEN],
			sprite_behind:  [false; LINE_BUF_LEN],
			border_bits:    [0u8; BORDER_BITS_LEN],
			bg_color:       [0u8; 16],
			resolved_bg:       [0u8; 16],
			resolved_palette:  build_resolved_palette(),
			has_visible_sprite: false,
			sprite_buffers_dirty: false,
			compose_video: true,
		}
	}

	/* Reset clears every transient plane and paints the host framebuffer black. This prevents stale sprite, border or deferred-background data from surviving a machine reset. */
	pub fn reset(&mut self) {
		self.line_buf.fill(0);
		self.mask_buf.fill(0);
		self.collision_line.fill(0);
		self.sprite_line.fill(0);
		self.sprite_behind.fill(false);
		self.border_bits.fill(0);
		self.bg_color.fill(0);
		self.resolved_bg.fill(0);
		self.resolved_palette = build_resolved_palette();
		self.has_visible_sprite = false;
		self.sprite_buffers_dirty = false;
		let rgb = VIC_PALETTE[0];
		for chunk in self.framebuffer.chunks_exact_mut(3) {
			chunk[0] = rgb[0];
			chunk[1] = rgb[1];
			chunk[2] = rgb[2];
		}
	}

	#[inline(always)]
	/* Disable only host-visible colour composition. Collision and priority masks continue to advance so headless or warp execution preserves machine-visible behaviour. */
	pub fn set_compose_video(&mut self, enabled: bool) {
		self.compose_video = enabled;
	}

	#[inline(always)]
	/* Report whether host-visible colour composition is enabled; collision and priority processing are intentionally independent. */
	pub fn compose_video(&self) -> bool {
		self.compose_video
	}

	#[inline(always)]
	/* Mark an arbitrary pixel span in the packed border plane. Partial first and last bytes preserve neighbouring pixels emitted by earlier border transitions. */
	pub fn set_border_span(&mut self, start: usize, count: usize) {
		if count == 0 {
			return;
		}
		let end = start + count;
		if end > LINE_BUF_LEN {
			return;
		}
		let first_byte = start >> 3;
		let last_byte = (end - 1) >> 3;
		let leading = 0xFFu8 >> (start & 7);
		let trailing = !(0x7Fu8 >> ((end - 1) & 7));
		if first_byte == last_byte {
			self.border_bits[first_byte] |= leading & trailing;
			return;
		}
		self.border_bits[first_byte] |= leading;
		let mut index = first_byte + 1;
		while index < last_byte {
			self.border_bits[index] = 0xFF;
			index += 1;
		}
		self.border_bits[last_byte] |= trailing;
	}

	/*
	A new raster line clears transient masks and lazily clears sprite planes only when the previous line wrote them. The colour plane starts as border colour because foreground and sprite stages overwrite selected pixels later in the cycle sequence.
	*/
	#[inline(always)]
	pub fn begin_line(&mut self) {
		self.mask_buf.fill(0);
		if self.compose_video {
			self.line_buf.fill(BORDERCOLORINDEX);
			self.border_bits.fill(0);
		}
		if self.sprite_buffers_dirty {
			self.collision_line.fill(0);
			if self.compose_video {
				self.sprite_line.fill(0);
				self.sprite_behind.fill(false);
			}
			self.sprite_buffers_dirty = false;
		}
		self.has_visible_sprite = false;
	}

	#[inline(always)]
	/* Update both the raw VIC colour register and the pre-resolved palette slot used by deferred foreground pixels. */
	pub fn set_background_colour(&mut self, index: usize, value: u8) {
		let colour = value & 0x0F;
		self.bg_color[index] = value;
		self.resolved_bg[index] = colour;
		self.resolved_palette[index + 16] = palette_u32(colour as usize);
	}

	/*
	Foreground pixels initially store symbolic background-register indices. Resolving them after the cell has been generated lets colour-register writes affect the same timing window as the hardware multiplexers.
	*/
	#[inline(always)]
	pub fn color_foreground(&mut self, cycle: u16) {
		if !self.compose_video || cycle < 10 || cycle > 62 {
			return;
		}
		let base = cycle as usize * 8 - 20;
		let pixels = &mut self.line_buf[base..base + 8];
		let colours = self.resolved_bg;
		let mut index = 0;
		while index < 8 {
			let value = pixels[index];
			if value & 0x80 != 0 {
				pixels[index] = colours[(value & 0x0F) as usize];
			}
			index += 1;
		}
	}

	#[inline(always)]
	/* The first opaque sprite written at a pixel wins, matching sprite-number priority. Later sprites may still contribute to collision state through the separate collision plane. */
	pub fn write_sprite_pixel(&mut self, x: usize, colour: u8, behind_foreground: bool) {
		if x >= self.sprite_line.len() {
			return;
		}
		self.sprite_buffers_dirty = true;
		if !self.compose_video || self.sprite_line[x] != 0 {
			return;
		}
		self.sprite_line[x] = colour;
		self.sprite_behind[x] = behind_foreground;
		if colour != 0 && x >= DISPLAY_START && x < DISPLAY_START + TOTAL_WIDTH {
			self.has_visible_sprite = true;
		}
	}

	/*
	Final line composition applies sprite priority, border coverage and palette resolution before copying RGB bytes to the host framebuffer. Lines without visible sprites take a shorter path but produce the same foreground and border result.
	*/
	#[inline]
	pub fn flush_line(&mut self, y: usize) {
		if !self.compose_video {
			return;
		}
		if self.has_visible_sprite {
			self.flush_line_with_sprites(y);
		} else {
			self.flush_line_without_sprites(y);
		}
	}

	#[inline]
	/* The no-sprite path resolves two foreground pixels at a time into packed RGB bytes. It is an optimisation only; symbolic background slots and border-black slots retain the same meaning as in the full compositor. */
	fn flush_line_without_sprites(&mut self, y: usize) {
		if y >= TOTAL_HEIGHT {
			return;
		}
		let buffered_width = (LINE_BUF_LEN - DISPLAY_START).min(TOTAL_WIDTH);
		let visible_end = DISPLAY_START + buffered_width;
		let foreground = &self.line_buf[DISPLAY_START..visible_end];
		let row_start = y * TOTAL_WIDTH * 3;
		let row_end = row_start + TOTAL_WIDTH * 3;
		let framebuffer = &mut self.framebuffer[row_start..row_end];

		let resolved_palette = self.resolved_palette;

		let mut pixel = 0;
		while pixel + 2 <= buffered_width {
			let value0 = foreground[pixel];
			let value1 = foreground[pixel + 1];
			let slot0 = ((value0 & 0x0F) | ((value0 >> 3) & 0x10)) as usize;
			let slot1 = ((value1 & 0x0F) | ((value1 >> 3) & 0x10)) as usize;
			let rgb0 = resolved_palette[slot0];
			let rgb1 = resolved_palette[slot1];
			let pair = u64::from(rgb0) | (u64::from(rgb1) << 24);
			let output = pixel * 3;
			framebuffer[output..output + 8].copy_from_slice(&pair.to_le_bytes());
			pixel += 2;
		}
		while pixel < buffered_width {
			let value = foreground[pixel];
			let slot = ((value & 0x0F) | ((value >> 3) & 0x10)) as usize;
			let rgb = resolved_palette[slot].to_le_bytes();
			let output = pixel * 3;
			framebuffer[output] = rgb[0];
			framebuffer[output + 1] = rgb[1];
			framebuffer[output + 2] = rgb[2];
			pixel += 1;
		}

		framebuffer[buffered_width * 3..].fill(0);
	}

	#[inline]
	/* The sprite path resolves foreground opacity, border coverage and behind-foreground priority per pixel before palette lookup. Border pixels suppress sprites regardless of sprite priority. */
	fn flush_line_with_sprites(&mut self, y: usize) {
		if y >= TOTAL_HEIGHT {
			return;
		}
		let buffered_width = (LINE_BUF_LEN - DISPLAY_START).min(TOTAL_WIDTH);
		let visible_end = DISPLAY_START + buffered_width;
		let foreground = &self.line_buf[DISPLAY_START..visible_end];
		let sprites = &self.sprite_line[DISPLAY_START..visible_end];
		let priorities = &self.sprite_behind[DISPLAY_START..visible_end];
		let row_start = y * TOTAL_WIDTH * 3;
		let row_end = row_start + TOTAL_WIDTH * 3;
		let framebuffer = &mut self.framebuffer[row_start..row_end];
		let background_colours = self.bg_color;
		let mut mask_byte_index = (DISPLAY_START + 4) >> 3;
		let mut mask = 1u8 << (7 - ((DISPLAY_START + 4) & 7));
		let mut border_byte_index = DISPLAY_START >> 3;
		let mut border_mask = 1u8 << (7 - (DISPLAY_START & 7));
		let mut pixel = 0;

		while pixel < buffered_width {
			let mut packed = [0u8; 8];
			let mut lane = 0;
			while lane < 2 && pixel < buffered_width {
				let foreground_opaque = self.mask_buf[mask_byte_index] & mask != 0;
				let border_here = self.border_bits[border_byte_index] & border_mask != 0;
				let sprite = sprites[pixel];
				let sprite_visible = sprite != 0
					&& !border_here
					&& (!priorities[pixel] || !foreground_opaque);
				let value = if sprite_visible { sprite } else { foreground[pixel] };
				let colour = if value & 0x80 != 0 {
					background_colours[(value & 0x0F) as usize] & 0x0F
				} else {
					value & 0x0F
				};
				let rgb = VIC_PALETTE[colour as usize];
				packed[lane * 3] = rgb[0];
				packed[lane * 3 + 1] = rgb[1];
				packed[lane * 3 + 2] = rgb[2];

				mask >>= 1;
				if mask == 0 {
					mask = 0x80;
					mask_byte_index += 1;
				}
				border_mask >>= 1;
				if border_mask == 0 {
					border_mask = 0x80;
					border_byte_index += 1;
				}
				pixel += 1;
				lane += 1;
			}
			let output = (pixel - lane) * 3;
			framebuffer[output..output + 8].copy_from_slice(&packed);
		}

		framebuffer[buffered_width * 3..].fill(0);
	}

}

/* Default construction is identical to the documented hardware reset baseline exposed by new(). */
impl Default for Screen {
	fn default() -> Self {
		Self::new()
	}
}