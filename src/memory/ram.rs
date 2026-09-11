// =======================================================
// src/memory/ram.rs — RAM Controller
// =======================================================

use super::constants::RAM_SIZE;

/* RAMController stores the physical 64 KiB DRAM independently of whichever ROM, I/O device or cartridge the PLA currently exposes above it. */
pub struct RAMController {
	data: Box<[u8; RAM_SIZE]>,
}

impl RAMController {
/* Normal construction uses the deterministic power-on pattern so tests and resets never depend on host allocator contents. */
	pub fn new() -> Self {
		Self::new_with_deterministic_power_on_pattern(0xDEADBEEF)
	}

	/* Real DRAM powers up in a board- and chip-dependent pattern. A deterministic pseudo-pattern preserves non-zero startup behaviour while keeping emulator runs reproducible. */
	pub fn new_with_deterministic_power_on_pattern(seed: u32) -> Self {
		let mut data = Box::new([0u8; RAM_SIZE]);
		let mut rng = seed;
		for addr in 0..RAM_SIZE {
			let col = addr & 0xFF;
			let row = (addr >> 8) & 0xFF;
			rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
			let col_bias = ((col as u32) * 73) & 0xFF;
			let row_val = ((rng >> 8) ^ row as u32) & 0xFF;
			let combined = (col_bias ^ row_val ^ (rng >> 16)) as u8;
			let value = if (rng & 0x03) == 0 {
				combined & 0x0F
			} else if (rng & 0x03) == 1 {
				combined | 0xF0
			} else {
				combined
			};
			data[addr] = value;
		}
		Self { data }
	}

/* A cold reset restores the same reproducible power-on pattern as construction. */
	pub fn clear(&mut self) {
		self.data = Self::new().data;
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