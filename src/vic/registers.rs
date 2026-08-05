// =======================================================
// src/vic/registers.rs — VIC-II register file & interrupt state
// =======================================================

use crate::vic::constants::*;

/* IrqState separates source latches from the enable mask. Bit 7 is derived from their intersection and mirrors the active-low IRQ output rather than acting as an independent source. */
#[derive(Debug, Clone, Copy)]
pub struct IrqState {
	pub flags: u8,
	pub enable: u8,
	pub line_active: bool,
}

impl IrqState {
	/* Construction leaves every source latch and enable bit clear, so the external IRQ output is released until software enables and a source raises a request. */
	pub fn new() -> Self {
		Self {
			flags: 0,
			enable: 0,
			line_active: false,
		}
	}

	/* Reset clears both pending sources and their mask; it does not preserve a latched event across a machine reset. */
	/* Reset restores the programmable register latches without manufacturing reads of live status registers. */
	pub fn reset(&mut self) {
		*self = Self::new();
	}

	#[inline(always)]
	/* A hardware source remains latched until software acknowledges it through $D019, even when that source is currently masked. */
	pub fn trigger(&mut self, source: u8) {
		self.flags |= source;
		self.update_line();
	}

	#[inline(always)]
	/* Writing a one to a source bit in $D019 acknowledges that source; zero bits leave their latches unchanged. */
	pub fn clear(&mut self, mask: u8) {
		self.flags &= !(mask & 0x0F);
		self.update_line();
	}

	#[inline(always)]
	/* $D01A replaces the four-bit source mask. Enabling an already-pending source can therefore assert IRQ immediately. */
	pub fn write_enable(&mut self, val: u8) {
		self.enable = val & 0x0F;
		self.update_line();
	}

	#[inline(always)]
	/* Bit 7 and the motherboard-facing line are recomputed from the intersection of pending and enabled sources after every latch or mask change. */
	fn update_line(&mut self) {
		if (self.flags & self.enable & 0x0F) != 0 {
			self.flags |= IRQ_STATUS;
			self.line_active = true;
		} else {
			self.flags &= !IRQ_STATUS;
			self.line_active = false;
		}
	}
}

/* Registers contains the programmable VIC-II register file only. Raster counters, collision latches and light-pen values live in their owning units and are merged on reads, matching the chip's mixture of writable latches and live status. (C64-PRG-1982, VIC-II register map) */
#[derive(Debug, Clone)]
pub struct Registers {
	pub mx: [u8; 8],
	pub my: [u8; 8],
	pub msb_x: u8,
	pub sprite_en: u8,
	pub sprite_y_exp: u8,
	pub sprite_x_exp: u8,
	pub sprite_prio: u8,
	pub sprite_mc: u8,
	pub sprite_cols: [u8; 8],
	pub sprite_mc_0: u8,
	pub sprite_mc_1: u8,

	pub ctrl1: u8,
	pub ctrl2: u8,
	pub raster_irq: u16,
	pub mem_ptrs: u8,

	pub border_col: u8,
	pub bg_cols: [u8; 4],
}

impl Registers {
	/* Construction establishes the power-on values of writable latches only; live raster, collision and light-pen state are owned elsewhere. */
	pub fn new() -> Self {
		Self {
			mx: [0; 8],
			my: [0; 8],
			msb_x: 0,
			sprite_en: 0,
			sprite_y_exp: 0,
			sprite_x_exp: 0,
			sprite_prio: 0,
			sprite_mc: 0,
			sprite_cols: [0; 8],
			sprite_mc_0: 0,
			sprite_mc_1: 0,
			ctrl1: 0x00,
			ctrl2: 0x00,
			raster_irq: 0,
			mem_ptrs: 0x00,
			border_col: 0x00,
			bg_cols: [0x00; 4],
		}
	}

	/* Reset restores the programmable register latches without manufacturing reads of live status registers. */
	pub fn reset(&mut self) {
		*self = Self::new();
	}

	#[inline(always)] pub fn den(&self) -> bool { (self.ctrl1 & 0x10) != 0 }
	/* RSEL selects the 24-row or 25-row vertical border comparator positions; it does not itself decide badline eligibility. */
	#[inline(always)] pub fn rsel(&self) -> bool { (self.ctrl1 & 0x08) != 0 }
	/* The control-register helpers expose only the programmed latch bits. Live raster state is deliberately merged later by read(), where the shared bit positions can be resolved correctly. */
	#[inline(always)] pub fn y_scroll(&self) -> u8 { self.ctrl1 & 0x07 }
	#[inline(always)] pub fn bmm(&self) -> bool { (self.ctrl1 & 0x20) != 0 }
	#[inline(always)] pub fn ecm(&self) -> bool { (self.ctrl1 & 0x40) != 0 }
	#[inline(always)] pub fn csel(&self) -> bool { (self.ctrl2 & 0x08) != 0 }
	#[inline(always)] pub fn x_scroll(&self) -> u8 { self.ctrl2 & 0x07 }
	/* MCM selects multicolour decoding but may still be overridden per character in text mode by the colour-RAM high bit. */
	#[inline(always)] pub fn mcm(&self) -> bool { (self.ctrl2 & 0x10) != 0 }

	#[inline(always)]
	/* The upper nibble of $D018 selects the 1 KiB video-matrix page inside the current 16 KiB VIC bank. */
	pub fn vm_base(&self) -> u16 {
		((self.mem_ptrs as u16) & 0xF0) << 6
	}

	#[inline(always)]
	/* Bits 1 through 3 of $D018 select the 2 KiB character-generator or bitmap base used by graphics accesses. */
	pub fn cb_base(&self) -> u16 {
		((self.mem_ptrs as u16) & 0x0E) << 10
	}

	/* Register writes decode only the low six address bits because the VIC-II register block is mirrored throughout the I/O page. */
	pub fn write(&mut self, addr: u16, val: u8, irq: &mut IrqState) {
		let reg = addr & 0x003F;
		match reg {
			0x00..=0x0F => {
				let i = (reg as usize) >> 1;
				if (reg & 1) == 0 {
					self.mx[i] = val;
				} else {
					self.my[i] = val;
				}
			}
			0x10 => self.msb_x = val,
			0x12 => {
				self.raster_irq = (self.raster_irq & 0x0100) | (val as u16);
			}
			0x15 => self.sprite_en = val,
			0x18 => self.mem_ptrs = val,
			/* $D019 is write-one-to-clear; $D01A replaces the source-enable mask. */
			0x19 => irq.clear(val & 0x0F),
			0x1A => irq.write_enable(val),
			0x1B => self.sprite_prio = val,
			0x1C => self.sprite_mc = val,
			0x1D => self.sprite_x_exp = val,
			0x20 => self.border_col = val & 0x0F,
			0x21 => self.bg_cols[0] = val & 0x0F,
			0x22 => self.bg_cols[1] = val & 0x0F,
			0x23 => self.bg_cols[2] = val & 0x0F,
			0x24 => self.bg_cols[3] = val & 0x0F,
			0x25 => self.sprite_mc_0 = val & 0x0F,
			0x26 => self.sprite_mc_1 = val & 0x0F,
			0x27..=0x2E => {
				self.sprite_cols[(reg as usize) - 0x27] = val & 0x0F;
			}
			_ => {}
		}
	}

	/* Reads combine stored fields with live raster, IRQ and light-pen state. Unimplemented registers return all ones, while colour and control registers expose the fixed high bits present on the external data bus. */
	pub fn read(&self, addr: u16, irq: &IrqState, current_raster: u16, light_pen_x: u8, light_pen_y: u8) -> u8 {
		let reg = addr & 0x003F;
		match reg {
			0x00..=0x0F => {
				let i = (reg as usize) >> 1;
				if (reg & 1) == 0 {
					self.mx[i]
				} else {
					self.my[i]
				}
			}
			0x10 => self.msb_x,
			/* $D011 reads the live ninth raster bit rather than the programmed compare bit stored in the same register position. */
			0x11 => {
				let rst8 = if (current_raster & 0x0100) != 0 { 0x80 } else { 0x00 };
				(self.ctrl1 & 0x7F) | rst8
			}
			0x12 => (current_raster & 0x00FF) as u8,
			/* Light-pen coordinates are latched state, not writable register storage. */
			0x13 => light_pen_x,
			0x14 => light_pen_y,
			0x15 => self.sprite_en,
			/* Bits 6 and 7 are not backed by writable latches and therefore read high; the lower control bits remain the programmed horizontal mode state. */
			0x16 => self.x_scroll() | ((self.csel() as u8) << 3) | (((self.ctrl2 >> 4) & 1) << 4) | (self.ctrl2 & 0x20) | 0xC0,
			0x17 => self.sprite_y_exp,
			/* Bit 0 of $D018 is not implemented by the VIC-II address decode and consequently reads high. */
			0x18 => self.mem_ptrs | 0x01,
			/* Unused bits read high; bit 7 reflects the resolved IRQ output, while bits 0 through 3 expose source latches. */
			0x19 => (irq.flags & 0x0F) | 0x70 | (if irq.line_active { 0x80 } else { 0x00 }),
			0x1A => irq.enable | 0xF0,
			0x1B => self.sprite_prio,
			0x1C => self.sprite_mc,
			0x1D => self.sprite_x_exp,
			0x20 => self.border_col | 0xF0,
			0x21 => self.bg_cols[0] | 0xF0,
			0x22 => self.bg_cols[1] | 0xF0,
			0x23 => self.bg_cols[2] | 0xF0,
			0x24 => self.bg_cols[3] | 0xF0,
			0x25 => self.sprite_mc_0 | 0xF0,
			0x26 => self.sprite_mc_1 | 0xF0,
			0x27..=0x2E => self.sprite_cols[(reg as usize) - 0x27] | 0xF0,
			_ => 0xFF,
		}
	}
}

impl Default for Registers {
	fn default() -> Self {
		Self::new()
	}
}