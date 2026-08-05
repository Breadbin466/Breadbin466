// =======================================================
// src/reu/registers.rs — MOS 8726 register interface
// =======================================================

use super::reu::Reu;

impl Reu {
	/* The REC decodes five low address bits. Registers $0B-$1F are unused and read as an undriven $FF value. */
	pub fn read(&mut self, addr: u16) -> u8 {
		let index = (addr & 0x1F) as usize;
		if index >= self.regs.len() {
			return 0xFF;
		}
		let value = match index {
			6 => self.regs[6] | 0xF8,
			9 => self.regs[9] | 0x1F,
			10 => self.regs[10] | 0x3F,
			_ => self.regs[index],
		};
		if index == 0 {
			self.regs[0] &= 0x1F;
			self.irq_pending = false;
		}
		value
	}

	/* Address and length writes update the half-autoload shadows. COMMAND bit 4 disables the $FF00 trigger, so an armed command starts immediately when that bit is set. */
	pub fn write(&mut self, addr: u16, value: u8) {
		let index = (addr & 0x1F) as usize;
		if index >= self.regs.len() {
			return;
		}
		match index {
			0 => {}
			1 => {
				self.regs[1] = value;
				self.waiting_ff00 = false;
				if value & 0x80 != 0 {
					if value & 0x10 != 0 { self.start_dma(); } else { self.waiting_ff00 = true; }
				}
			}
			2 | 3 => {
				self.regs[index] = value;
				self.shadow_c64_addr = u16::from(self.regs[2]) | (u16::from(self.regs[3]) << 8);
			}
			4 | 5 => {
				self.regs[index] = value;
				self.shadow_reu_addr = usize::from(self.regs[4]) | (usize::from(self.regs[5]) << 8) | (usize::from(self.regs[6] & 0x07) << 16);
			}
			6 => {
				self.regs[6] = (value & 0x07) | 0xF8;
				self.shadow_reu_addr = usize::from(self.regs[4]) | (usize::from(self.regs[5]) << 8) | (usize::from(self.regs[6] & 0x07) << 16);
			}
			7 | 8 => {
				self.regs[index] = value;
				self.shadow_len = usize::from(self.regs[7]) | (usize::from(self.regs[8]) << 8);
			}
			9 => self.regs[9] = value | 0x1F,
			10 => self.regs[10] = value | 0x3F,
			_ => {}
		}
	}
}
