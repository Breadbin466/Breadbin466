// =======================================================
// src/fdd1541/via_timers.rs — MOS 6522 timer state transitions
// =======================================================

use super::{
	constants::{VIA_IFR_IRQ, VIA_IFR_T1, VIA_IFR_T2},
	via::{shift_register_mode, ViaChip},
};

impl ViaChip {
	#[inline(always)]
	/* Both timers advance once per VIA clock. The return value reports whether this cycle changed the externally visible IRQ condition. */
	pub fn tick(&mut self) -> bool {
		let shift_mode = shift_register_mode(self.acr);
		if self.ca2_pulse_cycles == 0 && self.cb2_pulse_cycles == 0 && shift_mode == 0 {
			if !self.t1_running && !self.t2_running {
				return (self.ifr & VIA_IFR_IRQ) != 0;
			}

			let timer_1_simple = !self.t1_running
				|| (!self.t1_reload_pending && !self.t1_start_delay && self.t1_counter != 0);
			let timer_2_next = self.t2_counter.wrapping_sub(1);
			let timer_2_simple = !self.t2_running
				|| (!self.t2_start_delay
					&& (self.acr & 0x20) == 0
					&& !self.t2_pulse_count_mode_previous
					&& self.t2_counter > 1
					&& (timer_2_next & 0x00FF) != 0x00FE);

			if timer_1_simple && timer_2_simple {
				if self.t1_running {
					self.t1_counter -= 1;
				}
				if self.t2_running {
					self.t2_counter = timer_2_next;
					self.t2_shift_edge = false;
				}
				return (self.ifr & VIA_IFR_IRQ) != 0;
			}
		}

		if self.ca2_pulse_cycles > 0 {
			self.ca2_pulse_cycles -= 1;
			if self.ca2_pulse_cycles == 0 {
				self.ca2_handshake = true;
			}
		}

		if self.cb2_pulse_cycles > 0 {
			self.cb2_pulse_cycles -= 1;
			if self.cb2_pulse_cycles == 0 {
				self.cb2_handshake = true;
			}
		}

		self.tick_timer_1();
		self.tick_timer_2();
		self.tick_shift_register(self.t2_shift_edge);
		(self.ifr & VIA_IFR_IRQ) != 0
	}

	#[inline(always)]
	/* Timer 1 underflow can reload continuously and optionally toggle PB7, making the timer both an interrupt source and a board-visible waveform generator. */
	fn tick_timer_1(&mut self) {
		if !self.t1_running {
			return;
		}

		if self.t1_reload_pending {
			self.t1_counter = self.t1_latch;
			self.t1_reload_pending = false;
		} else if self.t1_start_delay {
			self.t1_start_delay = false;
		} else {
			let previous = self.t1_counter;
			self.t1_counter = self.t1_counter.wrapping_sub(1);
			if previous == 0 {
				self.t1_reload_pending = true;
				if (self.acr & 0x40) != 0 {
					if self.t1_interrupts_enabled {
						self.set_flag(VIA_IFR_T1);
					}
					if self.t1_latch > 1 {
						self.pb7 = !self.pb7;
					}
				} else if !self.t1_one_shot_complete {
					self.set_flag(VIA_IFR_T1);
					self.pb7 = true;
					self.t1_one_shot_complete = true;
				}
			}
		}
	}

	#[inline(always)]
	/* Timer 2 is one-shot in clocked mode and can instead count PB6 transitions; its interrupt flag remains latched until acknowledged. */
	fn tick_timer_2(&mut self) {
		self.t2_shift_edge = false;
		if !self.t2_running {
			return;
		}

		let pb6_mode = (self.acr & 0x20) != 0;
		if pb6_mode != self.t2_pulse_count_mode_previous {
			self.t2_interrupt_issued = false;
			if !pb6_mode {
				if self.t2_counter == 0 {
					self.set_flag(VIA_IFR_T2);
					self.t2_interrupt_issued = true;
				}
			} else {
				self.t2_counter = self.t2_counter.wrapping_sub(1);
				self.t2_zero_pending = self.t2_counter == 0;
			}
		} else if self.t2_start_delay {
			self.t2_start_delay = false;
		} else if pb6_mode {
			if self.pb6_prev {
				self.t2_zero_pending = false;
			}
		} else {
			self.decrement_timer_2();
		}

		if self.t2_zero_pending {
			self.t2_zero_pending = false;
			if !self.t2_interrupt_issued {
				self.set_flag(VIA_IFR_T2);
				self.t2_interrupt_issued = true;
			}
		}

		self.t2_pulse_count_mode_previous = pb6_mode;
	}

	#[inline(always)]
	/* Timer 2 decrements as a free-running 16-bit counter even though its interrupt is one-shot. Selected low-byte wrap positions also generate the internal shift-register clock, with the initial wraps suppressed to reproduce the 6522 start-up phase. */
	fn decrement_timer_2(&mut self) {
		self.t2_counter = self.t2_counter.wrapping_sub(1);
		self.t2_zero_pending = self.t2_counter == 0;

		if (self.t2_counter & 0x00FF) == 0x00FE {
			self.t2_low_byte_wraps = self.t2_low_byte_wraps.saturating_add(1);
			let mode = shift_register_mode(self.acr);
			let clock_ready = mode == 4 && self.t2_low_byte_wraps > 1
				|| matches!(mode, 1 | 5) && self.t2_low_byte_wraps > 2;
			if clock_ready {
				let positive_edge = self.t2_shift_clock_high;
				self.t2_shift_clock_high = !self.t2_shift_clock_high;
				self.t2_shift_edge = positive_edge;
				self.t2_counter = (self.t2_counter & 0xFF00) | u16::from(self.t2_latch_lo);
				self.t2_zero_pending = false;
			}
		}
	}

}