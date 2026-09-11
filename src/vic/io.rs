// =======================================================
// src/vic/io.rs — VIC-II register read/write dispatch
// =======================================================

use crate::memory::Memory;
use super::state::VicII;
use super::constants::IRQ_RASTER;

/*
The I/O layer joins the programmable register file to live VIC-II state. Writes can alter raster comparison, display mode and border behaviour immediately, while reads merge stored values with raster, light-pen and collision latches.
*/
impl VicII {
	#[inline(always)]
	/* Register reads first resolve collision or live-status latches, then complete any sprite access that the CPU displaced while AEC remained high. The returned byte is therefore both the CPU-visible value and the replacement bus value seen by that interrupted slot. */
	pub fn read_register(&mut self, addr: u16) -> u8 {
		let reg   = addr & 0x3F;
		let cycle = self.timing.cycle;
		let data  = if reg == 0x1E {
			self.sprites.read_sprite_sprite_collision()
		} else if reg == 0x1F {
			self.sprites.read_sprite_data_collision()
		} else {
			self.regs.read(addr, &self.irq, self.timing.raster_line, self.light_pen_x, self.light_pen_y)
		};

		if !self.aec_low {
			self.sprites.complete_interrupted_sprite_access(cycle, data);
		}

		data
	}

	#[inline]
	/* Writes are split between cycle-sensitive side effects and ordinary latch storage. Position, expansion, priority, raster and scroll changes are observed at their hardware timing points before the common register file is updated. */
	pub fn write_register(&mut self, addr: u16, val: u8, _memory: &mut Memory) {
		let was_badline = self.timing.is_badline;
		let reg         = addr & 0x3F;
		let old_rsel    = self.regs.rsel();
		let cycle       = self.timing.cycle;

		if !self.aec_low {
			self.sprites.complete_interrupted_sprite_access(cycle, val);
		}

		/* Several sprite registers affect an already-running display pipeline, so their previous values and effective change clocks are retained explicitly. */
		match reg {
			0x00 | 0x02 | 0x04 | 0x06 | 0x08 | 0x0A | 0x0C | 0x0E => {
				let index = (reg >> 1) as usize;
				let bit = 1u8 << index;
				let x = val as u16 | if self.regs.msb_x & bit != 0 { 0x100 } else { 0 };
				self.sprites.set_horizontal_pos(index, x, cycle as u8);
			}
			0x10 => {
				let changed = self.regs.msb_x ^ val;
				let mut index = 0;
				while index < 8 {
					let bit = 1u8 << index;
					if changed & bit != 0 {
						let x = self.regs.mx[index] as u16 | if val & bit != 0 { 0x100 } else { 0 };
						self.sprites.set_horizontal_pos(index, x, cycle as u8);
					}
					index += 1;
				}
			}
			0x1B => {
				self.sprites.prio_prev          = self.regs.sprite_prio;
				self.sprites.prio_changed_cycle = self.current_clock + 2;
			}
			0x1C => {
				self.sprites.mc_prev       = self.regs.sprite_mc;
				self.sprites.mc_changed_at = self.current_clock + 2;
			}
			0x1D => {
				self.sprites.horizontal_expand_prev       = self.regs.sprite_x_exp;
				self.sprites.horizontal_expand_changed_at = self.current_clock + 2;
			}
			0x17 => {
				self.sprites.write_y_expand(val, cycle, &mut self.regs);
			}
			0x12 => {
				let old_raster_compare = self.regs.raster_irq;
				self.regs.raster_irq   = (self.regs.raster_irq & 0x0100) | (val as u16);
				if self.regs.raster_irq != old_raster_compare {
					self.check_raster_compare(cycle);
				}
			}
			_ => {}
		}

		if reg == 0x11 {
			self.apply_vertical_scroll_write(val, cycle, old_rsel, was_badline);
			return;
		}

		if reg == 0x16 {
			self.apply_horizontal_scroll_write(val, cycle);
			return;
		}

		self.regs.write(addr, val, &mut self.irq);
		/* Colour writes retain the previous output value alongside the new register value so the screen-side colour stage can apply the 6569 one-dot propagation delay. */
		match reg {
			0x20 => self.screen.set_background_colour(14, self.regs.border_col, cycle),
			0x21..=0x24 => self.screen.set_background_colour((reg - 0x21) as usize, self.regs.bg_cols[(reg - 0x21) as usize], cycle),
			0x25 => self.screen.set_background_colour(12, self.regs.sprite_mc_0, cycle),
			0x26 => self.screen.set_background_colour(13, self.regs.sprite_mc_1, cycle),
			0x27..=0x2E => self.screen.set_background_colour(4 + (reg - 0x27) as usize, self.regs.sprite_cols[(reg - 0x27) as usize], cycle),
			_ => {}
		}
	}

	/* Raster IRQ is edge-like at the comparator level: a source is triggered only when equality becomes true. Writes around the frame boundary use a separate cycle-2 path because line 311 and line 0 share the comparator transition across cycles 1 and 2. */
	pub(super) fn check_raster_compare(&mut self, cycle: u16) {
		if self.check_irq_in_cycle2 {
			if cycle != 1 && self.timing.raster_line == crate::vic::constants::PAL_LINES - 1 {
				if !self.raster_match {
					self.raster_match = true;
					self.irq.trigger(IRQ_RASTER);
				}
			} else {
				self.raster_match = false;
			}
		} else if cycle != 63 {
			if self.timing.raster_line == self.regs.raster_irq {
				if !self.raster_match {
					self.raster_match = true;
					self.irq.trigger(IRQ_RASTER);
				}
			} else {
				self.raster_match = false;
			}
		}
	}
}