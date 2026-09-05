// =======================================================
// src/motherboard/scheduler.rs — Cycle-Exact Timing State
// =======================================================

use super::constants::PAL_TOD_INPUT_PERIOD_CYCLES;

/* Scheduler tracks the motherboard master-cycle count and derives the 50 Hz TOD input supplied to both CIAs from the PAL cycle stream. */
pub struct Scheduler {
	pub total_cycles: u64,
	pub tod_counter: u32,
	pub tod_period: u32,
}

impl Scheduler {
	pub fn new() -> Self {
		let tod_period = PAL_TOD_INPUT_PERIOD_CYCLES;
		Self {
			total_cycles: 0,
			tod_counter: tod_period,
			tod_period,
		}
	}

	pub fn reset(&mut self) {
		self.total_cycles = 0;
		self.tod_counter = self.tod_period;
	}

	/* The pulse is one host cycle wide; each CIA performs its own divider and BCD timekeeping after receiving it. */
	#[inline(always)]
	pub fn advance_tod(&mut self) -> bool {
		self.tod_counter -= 1;
		if self.tod_counter == 0 {
			self.tod_counter = self.tod_period;
			true
		} else {
			false
		}
	}
}