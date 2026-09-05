// =======================================================
// src/memory/ram.rs — RAM Controller
// =======================================================

use super::constants::RAM_SIZE;

/* RAMController stores the physical 64 KiB DRAM independently of whichever ROM, I/O device or cartridge the PLA currently exposes above it. */
pub struct RAMController {
	data: Box<[u8; RAM_SIZE]>,
}

impl RAMController {
	/* Normal construction starts from cleared RAM. This provides a stable cold-start state without inventing cache flags or other software-visible data. */
	pub fn new() -> Self {
		Self {
			data: Box::new([0u8; RAM_SIZE]),
		}
	}

	/* Clear restores the defined cold-start RAM state. */
	pub fn clear(&mut self) {
		self.data.fill(0x00);
	}

	#[inline(always)]
	pub fn read(&self, addr: u16) -> u8 {
		self.data[addr as usize]
	}

	#[inline(always)]
	pub fn write(&mut self, addr: u16, value: u8) {
		self.data[addr as usize] = value;
	}
}

impl Default for RAMController {
	fn default() -> Self {
		Self::new()
	}
}