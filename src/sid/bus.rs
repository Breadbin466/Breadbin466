// =======================================================
// src/sid/bus.rs — SID internal data-line charge model
// =======================================================

use super::constants::{
	DATA_BUS_CHARGE_MAX, DATA_BUS_HOLD_CYCLES,
};

#[derive(Clone, Copy, Debug)]
/* The SID does not return a fixed value from write-only registers. Charge retained on its eight internal data lines remains observable for a finite time and then loses one bits independently, so the model stores charge per line rather than a single expiry timestamp. */
pub struct InternalDataBus {
	/* Logical level reconstructed from the lines whose stored charge has not yet decayed to zero. */
	value: u8,
	/* Per-line charge allows ones to disappear independently instead of expiring as one byte. */
	line_charge: [u16; 8],
	/* Carries fractional leakage between SID cycles so integer decay preserves the configured hold time. */
	leak_phase: u32,
}

impl InternalDataBus {
	pub const fn new() -> Self {
		Self {
			value: 0,
			line_charge: [0; 8],
			leak_phase: 0,
		}
	}

	/* Reset removes all retained charge immediately, unlike ordinary decay, so subsequent reads start from a fully undriven bus rather than from the last written value. */
	pub fn reset(&mut self) {
		self.value = 0;
		self.line_charge = [0; 8];
		self.leak_phase = 0;
	}

	#[inline(always)]
	/* A CPU write drives all eight lines at once: zero discharges a line immediately, while one restores it to full charge and restarts its independent decay. */
	pub fn drive(&mut self, value: u8) {
		self.value = value;
		for bit in 0..8 {
			self.line_charge[bit] = if value & (1 << bit) != 0 {
				DATA_BUS_CHARGE_MAX
			} else {
				0
			};
		}
	}

	#[inline(always)]
	/* A readable register actively drives the bus for this access. The returned byte also becomes the retained charge observed by later reads from write-only addresses. */
	pub fn read_driven(&mut self, value: u8) -> u8 {
		self.drive(value);
		value
	}

	#[inline(always)]
	/* A write-only or unimplemented read samples the charge already present on the bus. The sampled byte is returned before the read halves the remaining charge, so the observed value and the post-read state remain distinct. */
	pub fn read_floating(&mut self) -> u8 {
		let observed = self.value;
		for charge in &mut self.line_charge {
			*charge /= 2;
		}
		self.rebuild_value();
		observed
	}

	#[inline(always)]
	/* Fractional leakage is accumulated so the configured hold time is preserved without requiring floating-point state or dropping sub-cycle decay. */
	pub fn clock(&mut self) {
		self.leak_phase = self.leak_phase.wrapping_add(u32::from(DATA_BUS_CHARGE_MAX));
		let decrement = self.leak_phase / DATA_BUS_HOLD_CYCLES;
		self.leak_phase %= DATA_BUS_HOLD_CYCLES;
		if decrement == 0 {
			return;
		}

		for charge in &mut self.line_charge {
			*charge = charge.saturating_sub(decrement as u16);
		}
		self.rebuild_value();
	}

	#[inline(always)]
	fn rebuild_value(&mut self) {
		let mut retained = 0u8;
		for bit in 0..8 {
			if self.line_charge[bit] != 0 {
				retained |= 1 << bit;
			}
		}
		self.value &= retained;
	}
}

impl Default for InternalDataBus {
	fn default() -> Self { Self::new() }
}