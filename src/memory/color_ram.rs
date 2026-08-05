// =======================================================
// src/memory/color_ram.rs — Color RAM
// =======================================================

use super::constants::COLOR_RAM_SIZE;

/* The 2114 colour RAM is only four bits wide. Address mirroring and nibble masking are therefore part of the device itself rather than the general memory router. */
pub struct ColorRAM {
	data: Vec<u8>,
}

impl ColorRAM {
	pub fn new() -> Self {
		Self {
			data: vec![0; COLOR_RAM_SIZE],
		}
	}

	/* A CPU read obtains only the four driven colour bits; the upper nibble is supplied by the residual data bus in Memory::cpu_read. */
	#[inline]
	pub fn read_low_nibble(&self, addr: u16) -> u8 {
		let idx = (addr as usize) & 0x3FF;
		self.data[idx] & 0x0F
	}

	/* Writes discard the upper nibble because no corresponding storage cells exist. */
	#[inline]
	pub fn write(&mut self, addr: u16, value: u8) {
		let idx = (addr as usize) & 0x3FF;
		self.data[idx] = value & 0x0F;
	}

	pub fn clear(&mut self) {
		for byte in &mut self.data {
			*byte = 0;
		}
	}
}

impl Default for ColorRAM {
	fn default() -> Self {
		Self::new()
	}
}