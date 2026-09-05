// =======================================================
// src/memory/bus.rs — Memory bus state
// =======================================================

use crate::memory::constants::FLOAT_HOLD_CYCLES;
/* BusState models the last byte driven onto the shared motherboard data bus. Devices that expose fewer than eight bits, or no device at all, combine their result with this residual value. */
pub struct BusState {
	floating_byte: u8,
	last_update_cycle: u64,
}

impl BusState {
	/* Construction starts at the pulled-up idle value with no recent bus driver. */
	pub fn new() -> Self {
		Self {
			floating_byte: 0xFF,
			last_update_cycle: 0,
		}
	}

	/* Every completed CPU or VIC transfer refreshes both the retained byte and its decay origin. */
	#[inline(always)]
	pub fn update(&mut self, value: u8, cycle: u64) {
		self.floating_byte = value;
		self.last_update_cycle = cycle;
	}

	/* An undriven read first sees the recent bus value, then the model returns the pulled-up idle level once the hold interval expires. */
	#[inline(always)]
	pub fn get_floating(&self, cycle: u64) -> u8 {
		if cycle.wrapping_sub(self.last_update_cycle) <= FLOAT_HOLD_CYCLES {
			self.floating_byte
		} else {
			0xFF
		}
	}

	#[inline(always)]
	pub fn latched_value(&self) -> u8 {
		self.floating_byte
	}

	/* Reset discards both the retained byte and its age so pre-reset traffic cannot leak into the restarted machine. */
	pub fn reset(&mut self) {
		*self = Self::new();
	}
}

impl Default for BusState {
	fn default() -> Self {
		Self::new()
	}
}