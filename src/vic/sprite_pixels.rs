// =======================================================
// src/vic/sprite_pixels.rs — Per-sprite pixel output, collision detection and shifter advance
// =======================================================

use crate::vic::constants::{SPRITE_PIXEL_OFFSET};
use crate::vic::registers::Registers;
use crate::vic::screen::Screen;
use super::sprite::{Sprite, SpriteShifterState, VIDEOWIDTH};
use super::sprite_unit::{SpriteDrawFlags, SpriteCollisionOutputs};

impl Sprite {
	/*
	The sprite shifter produces one host-visible pixel at a time while preserving the VIC-II's single-bit and multicolour phase rules, horizontal expansion, mid-cycle register changes, foreground priority and both collision classes. Pixel output and collision detection remain separate: a sprite hidden behind foreground graphics still collides with that foreground. (BAUER-VIC-II-1996, sprite display and collision logic)
	*/
	#[inline]
	pub fn draw_sprite(
		&mut self,
		mut pixels_left: i32,
		regs: &Registers,
		screen: &mut Screen,
		flags: &SpriteDrawFlags,
		coll: &mut SpriteCollisionOutputs,
	) {
		let data_priority     = (regs.sprite_prio  & self.spr_bit) != 0;
		let multicolour       = (regs.sprite_mc     & self.spr_bit) != 0;
		let horizontal_expand = (regs.sprite_x_exp  & self.spr_bit) != 0;

		/* Sprite colour indices carry a private high bit so the compositor can distinguish sprite pixels from background colours before the final palette lookup. */
		let colour1: u8 = 12 | 128;
		let colour2: u8 = (4 + self.index as u8) | 128;
		let colour3: u8 = 13 | 128;

		let mask_shift = ((self.horizontal_pixel_cursor + 4) & 7) as u32;
		let mask_byte  = ((self.horizontal_pixel_cursor + 4) / 8) as i32;

		/* Two adjacent foreground-mask bytes are aligned to the sprite pixel cursor so background collision is tested at the same dot that receives the sprite pixel. */
		let mut mask: u16 = if mask_byte < 14 || mask_byte > 57 {
			0
		} else {
			let mb = mask_byte as usize;
			let hi = screen.mask_buf.get(mb).copied().unwrap_or(0) as u16;
			let lo = screen.mask_buf.get(mb + 1).copied().unwrap_or(0) as u16;
			((hi << 8) | lo) << mask_shift
		};

		/* Each loop iteration resolves one raster pixel from the current shifter phase. Register changes that occur within the six-pixel host slice use the previous value until the exact transition boundary is crossed. */
		while pixels_left > 0 {
			if !self.data_reload_stall {
				if !self.horizontal_expand_flip {
					if self.bits_remaining <= 0 && !self.multicolour_flip {
						self.shifter_state = if flags.sprite_display_bit { SpriteShifterState::Armed } else { SpriteShifterState::Idle };
						break;
					}

					let mut mc_this_pixel = multicolour;
					if pixels_left > 6 && flags.mc_changed_now { mc_this_pixel = flags.mc_prev; }

					if mc_this_pixel || self.multicolour_flip {
						self.multicolour_flip = !self.multicolour_flip;
						if self.multicolour_flip {
							let bits = ((self.data_buffer & 0xC00000) >> 22) as u8;
							self.pending_pixel_colour = match bits {
								0 => 0x00, 1 => colour1, 2 => colour2, 3 => colour3, _ => 0x00,
							};
						}
					} else if (self.data_buffer & 0x800000) != 0 {
						self.pending_pixel_colour = colour2;
					} else {
						self.pending_pixel_colour = 0x00;
					}
				}

				if pixels_left == 6 && self.horizontal_expand_flip && flags.mc_changed_now {
					if !flags.mc_prev && multicolour {
						let bits = ((self.data_buffer & 0x800000) >> 22) as u8;
						self.pending_pixel_colour = match bits {
							0 => 0x00, 1 => colour1, 2 => colour2, 3 => colour3, _ => 0x00,
						};
						self.multicolour_flip = true;
					} else if flags.mc_prev && !multicolour {
						self.multicolour_flip = false;
					}
				}
			}

			/* Crossing the fixed data-reload position stalls the shifter for the remainder of the reload window; this is observable when a sprite begins unusually close to its own fetch slots. */
			if self.horizontal_pixel_cursor == self.reload_trigger_index as i32 && self.bits_remaining > 0 {
				self.data_reload_stall = true;
				self.bits_remaining    = 7;

				if self.multicolour_flip && !self.horizontal_expand_flip && self.pointer < 0x80 {
					self.pending_pixel_colour = if (self.data_buffer & 0x800000) == 0 { 0x00 } else { colour2 };
				}

				self.horizontal_expand_flip = false;
				self.multicolour_flip       = false;
			}

			/* Transparent sprite pixels neither write colour nor participate in collision latches. Opaque pixels always update collision state even when foreground priority suppresses their colour. */
			if self.pending_pixel_colour != 0 {
				let mut s_sprite_coll: u8 = 0;
				let mut s_data_coll:   u8 = 0;
				let xp = self.horizontal_pixel_cursor as usize;

				if xp < screen.collision_line.len() {
					if screen.collision_line[xp] != 0 {
						s_sprite_coll = screen.collision_line[xp] | self.spr_bit;
					} else {
						let behind_foreground = if pixels_left > 6 && flags.prio_changed_now {
							flags.prio_prev
						} else {
							data_priority
						};
						screen.write_sprite_pixel(xp, self.pending_pixel_colour, behind_foreground);
					}
					screen.collision_line[xp] = self.spr_bit;
				}

				if (mask as i16) < 0 { s_data_coll = self.spr_bit; }

				if pixels_left > SPRITE_PIXEL_OFFSET {
					coll.curr_sprite_sprite_coll |= s_sprite_coll;
					coll.curr_sprite_data_coll   |= s_data_coll;
				} else {
					coll.next_sprite_sprite_coll |= s_sprite_coll;
					coll.next_sprite_data_coll   |= s_data_coll;
				}

				if s_sprite_coll != 0 {
					if pixels_left > SPRITE_PIXEL_OFFSET { coll.sprite_sprite_int |= 1; } else { coll.sprite_sprite_int |= 2; }
				}
				if s_data_coll != 0 {
					if pixels_left > SPRITE_PIXEL_OFFSET { coll.sprite_data_int |= 1; } else { coll.sprite_data_int |= 2; }
				}
			}

			/* Horizontal expansion repeats a source bit by toggling a flip-flop; multicolour mode independently repeats each two-bit symbol. The shifter advances only when both active repeat phases permit it. */
			if self.data_reload_stall {
				if self.bits_remaining <= 0 {
					self.horizontal_expand_flip = false;
					self.multicolour_flip       = false;
					break;
				}
				self.bits_remaining -= 1;
			} else {
				let mut expand_this_pixel = horizontal_expand;
				if pixels_left > 6 && flags.horizontal_expand_changed_now { expand_this_pixel = flags.horizontal_expand_prev; }
				if expand_this_pixel || self.horizontal_expand_flip {
					self.horizontal_expand_flip = !self.horizontal_expand_flip;
				}
				if !self.horizontal_expand_flip {
					self.data_buffer    <<= 1;
					self.bits_remaining  -= 1;
				}
			}

			mask <<= 1;

			self.horizontal_pixel_cursor += 1;
			if self.horizontal_pixel_cursor >= VIDEOWIDTH {
				self.horizontal_pixel_cursor -= VIDEOWIDTH;
			}
			pixels_left -= 1;
		}

		if self.bits_remaining <= 0 && !self.horizontal_expand_flip && !self.multicolour_flip {
			self.shifter_state = if flags.sprite_display_bit { SpriteShifterState::Armed } else { SpriteShifterState::Idle };
		}
	}
}