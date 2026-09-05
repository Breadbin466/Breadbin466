// =======================================================
// src/sid/bus.rs — SID internal bus
// =======================================================

/* SID internal data-line retention model. */

use super::constants::DATA_BUS_HOLD_CYCLES;

#[derive(Clone, Copy, Debug)]
/* The SID does not return a fixed value from write-only registers. Charge retained on its eight internal data lines remains observable for a finite time and then loses one bits independently. All lines are refreshed by the same bus drive, so their observable decay can be represented by one common age and the next line-expiry event. */
pub struct InternalDataBus {
	/* Logical level reconstructed from the lines whose retained charge has not yet decayed below the readable threshold. */
	value: u8,
	/* SID clocks elapsed since the most recent driven bus value. */
	age: u32,
	/* Age at which the next currently-high data line becomes unreadable. */
	next_expiry: u32,
}

impl InternalDataBus {
	pub const fn new() -> Self {
		Self {
			value: 0,
			age: 0,
			next_expiry: u32::MAX,
		}
	}

	/* Reset removes all retained charge immediately, unlike ordinary decay, so subsequent reads start from a fully undriven bus rather than from the last written value. */
	pub fn reset(&mut self) {
		self.value = 0;
		self.age = 0;
		self.next_expiry = u32::MAX;
	}

	#[inline(always)]
	/* A CPU write drives all eight lines at once: zero discharges a line immediately, while one restores it to full charge and restarts that line retention interval. */
	pub fn drive(&mut self, value: u8) {
		self.value = value;
		self.age = 0;
		self.next_expiry = Self::next_expiry_for(value, 0);
	}

	#[inline(always)]
	/* A readable register actively drives the bus for this access. The returned byte also becomes the retained charge observed by later reads from write-only addresses. */
	pub fn read_driven(&mut self, value: u8) -> u8 {
		self.drive(value);
		value
	}

	#[inline(always)]
	/* A write-only or unimplemented read samples the charge already present on the internal bus. Reading does not inject a second artificial decay step; the retained value fades only as the undriven bus ages with SID clocks (SID-SCHEMATICS-DATA-BUS). */
	pub const fn read_floating(&self) -> u8 {
		self.value
	}

	#[inline(always)]
	/* Retention intervals are threshold events: intermediate line charge is not otherwise observable through the digital register interface. Most SID clocks therefore only increment one age counter; per-line work occurs only when an expiry boundary is crossed. */
	pub fn clock(&mut self) {
		if self.value == 0 {
			return;
		}

		self.age = self.age.saturating_add(1);
		if self.age < self.next_expiry {
			return;
		}

		let mut retained = 0u8;
		for (bit, hold_cycles) in DATA_BUS_HOLD_CYCLES.iter().copied().enumerate() {
			if self.value & (1 << bit) != 0 && self.age < hold_cycles.max(1) {
				retained |= 1 << bit;
			}
		}
		self.value = retained;
		self.next_expiry = Self::next_expiry_for(retained, self.age);
	}

	#[inline(always)]
	fn next_expiry_for(value: u8, age: u32) -> u32 {
		let mut next = u32::MAX;
		let mut bit = 0usize;
		while bit < 8 {
			if value & (1 << bit) != 0 {
				let expiry = DATA_BUS_HOLD_CYCLES[bit].max(1);
				if expiry > age && expiry < next {
					next = expiry;
				}
			}
			bit += 1;
		}
		next
	}
}

impl Default for InternalDataBus {
	fn default() -> Self {
		Self::new()
	}
}