// =======================================================
// src/fdd1541/via_timers.rs — MOS 6522 timer state transitions
// =======================================================

use super::{
	constants::{VIA_IFR_IRQ, VIA_IFR_T1, VIA_IFR_T2},
	via::{ViaChip, shift_register_mode},
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
					/* A one-shot expiry clocks the same output flip-flop as
					continuous mode, but subsequent expiries are inhibited.
					(VIA-HARDWARE-DIAGNOSTICS, via10 through via14) */
					self.pb7 = !self.pb7;
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
	/* Timer 2 decrements as a free-running 16-bit counter even though its interrupt is one-shot. In Timer 2 shift modes, the low byte reloads every N+2 clocks and drives CB1; the high byte still receives the borrow, so the timer interrupt remains a separate one-shot. (MOS-6522-DATASHEET, figure 22; VIA-HARDWARE-DIAGNOSTICS, via20 and via21) */
	fn decrement_timer_2(&mut self) {
		self.t2_counter = self.t2_counter.wrapping_sub(1);
		self.t2_zero_pending = self.t2_counter == 0;

		if (self.t2_counter & 0x00FF) == 0x00FE {
			let mode = shift_register_mode(self.acr);
			let clock_ready = matches!(mode, 1 | 4 | 5);
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