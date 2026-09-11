// =======================================================
// src/cia/cia.rs — MOS 6526A core state and register behaviour
// =======================================================

use super::constants::*;
use super::serial::SerialShiftRegister;
use super::timer::Timer;
use super::tod::TimeOfDay;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/* Timer B can count PHI2, CNT, Timer A underflows, or Timer A underflows occurring while CNT is high (MOS-6526-1981, Control Register B). */
enum TimerBInputMode {
	Phi2,
	Cnt,
	TimerAUnderflow,
	TimerAUnderflowWhileCntHigh,
}

impl TimerBInputMode {
	#[inline(always)]
	fn from_crb(crb: u8) -> Self {
		match (crb & CRB_INMODE_MASK) >> 5 {
			0 => Self::Phi2,
			1 => Self::Cnt,
			2 => Self::TimerAUnderflow,
			_ => Self::TimerAUnderflowWhileCntHigh,
		}
	}
}

/* The core owns register-visible state and pin history; board-specific wiring is layered by Cia1 and Cia2.

The interrupt pipeline has four distinct stages. Sources are latched in icr as soon as they occur. Newly raised enabled sources enter irq_stage_now and assert IRQ on the next tick. Unmasking an already latched source enters irq_stage_next, which advances to irq_stage_now before asserting IRQ. The pending request is independent of the readable source bits: a Timer B acknowledgement collision can clear its flag without cancelling the IRQ request. Reading ICR lowers the line immediately and records the visible source bits in icr_ack. Qualified IR remains visible for the next readback cycle without reasserting the pin. Those acknowledged bits remain present until the following tick applies icr_clear_next, allowing events already in flight to retain their cycle ordering. */
pub struct Cia {
	pub ta: Timer,
	pub tb: Timer,
	pub tod: TimeOfDay,
	pub sdr: SerialShiftRegister,
	pub pra: u8,
	pub prb: u8,
	pub ddra: u8,
	pub ddrb: u8,
	pub icr: u8,
	pub icr_mask: u8,
	pub irq_line: bool,
	/* IR remains visible on the readback path for the acknowledgement
	 * cycle, independently of the released IRQ pin. */
	pub icr_ir_readback: bool,
	/* Qualified IR sampled before the preceding read cancelled IRQ. */
	pub icr_ir_readback_next: bool,
	/* ICR write selection is retained across adjacent bus cycles. */
	pub icr_write_active: bool,
	pub icr_write_previous: bool,
	/* Enabled sources eligible to assert IRQ during the current tick. */
	pub irq_stage_now: u8,
	/* Enabled sources deferred until the following pipeline stage. */
	pub irq_stage_next: u8,
	/* Source bits returned by the most recent destructive ICR read. */
	pub icr_ack: u8,
	/* Requests removal of acknowledged source bits on the next tick. */
	pub icr_clear_next: bool,
	pub cnt_pin: bool,
	pub cnt_prev: bool,
	pub sp_pin: bool,
	pub flag_pin: bool,
	pub flag_prev: bool,
}

impl Cia {
	/* Construction establishes the released-pin, stopped-timer and clear-interrupt state visible after reset. */
	pub fn new() -> Self {
		let core = Self {
			ta: Timer::new(),
			tb: Timer::new(),
			tod: TimeOfDay {
				hours: 1,
				latch_hours: 1,
				current_val: 0x0100_0000,
				..TimeOfDay::default()
			},
			sdr: SerialShiftRegister::default(),
			pra: 0x00,
			prb: 0x00,
			ddra: 0,
			ddrb: 0,
			icr: 0,
			icr_mask: 0,
			irq_line: false,
			icr_ir_readback: false,
			icr_ir_readback_next: false,
			icr_write_active: false,
			icr_write_previous: false,
			irq_stage_now: 0,
			irq_stage_next: 0,
			icr_ack: 0,
			icr_clear_next: false,
			cnt_pin: true,
			cnt_prev: true,
			sp_pin: true,
			flag_pin: true,
			flag_prev: true,
		};
		core
	}

	/* Reset replaces the complete core state so no deferred interrupt, edge or serial operation survives. */
	pub fn reset(&mut self) {
		*self = Self::new();
	}

	#[inline(always)]
	/* Every interrupt source is latched independently, while IRQ is asserted only when at least one latched source is enabled by the mask (MOS-6526-1981, Interrupt Control Register). */
	fn raise_source(&mut self, source: u8) {
		self.icr |= source;
		let enabled = source & self.icr_mask & 0x1F;
		if enabled != 0 {
			self.irq_stage_now |= enabled;
		}
	}

	#[inline(always)]
	/* Alarm matching is checked after every TOD write as well as after a clock increment, so software can trigger the alarm by programming the compared value. */
	fn compare_tod_alarm(&mut self) {
		if self.tod.compare_alarm() {
			self.raise_source(ICR_ALRM);
		}
	}

	#[inline(always)]
	/* Deferred ICR clearing and IRQ staging keep register accesses, source latching and pin changes ordered across host cycles. The idle exit is taken only when no state machine or external edge can change visible CIA state. */
	pub fn tick(&mut self, tod_pulse: bool) -> bool {
		/* Consecutive ICR reads can observe qualified IR after the first
		 * read has released IRQ (CIA-ICR-MEASUREMENTS). */
		self.icr_write_previous = self.icr_write_active;
		self.icr_write_active = false;
		self.icr_ir_readback = self.icr_ir_readback_next;
		self.icr_ir_readback_next = false;
		/* Timer B source clearing uses the preceding read strobe
		 * (CIA-TIMER-B-MEASUREMENTS). */
		let acknowledge_cycle = self.icr_clear_next;
		if self.icr_clear_next {
			self.icr &= !self.icr_ack;
			self.icr_ack = 0;
			self.icr_clear_next = false;
		}

		let irq_due = self.irq_stage_now;
		self.irq_stage_now = self.irq_stage_next;
		self.irq_stage_next = 0;
		if irq_due != 0 {
			self.irq_line = true;
		}

		/* Timer B's gated Timer A mode uses the CNT level sampled for the cycle in which the underflow becomes visible, not the level after edge detection updates the pin history. */
		let cnt_high_delayed = self.cnt_prev;
		let cnt_rising = self.cnt_pin && !self.cnt_prev;
		self.cnt_prev = self.cnt_pin;

		let flag_falling = self.flag_prev && !self.flag_pin;
		self.flag_prev = self.flag_pin;

		/* No latent pipeline state or sampled edge can change externally visible state, so returning here is cycle-equivalent to running the inactive paths below. */
		if self.irq_stage_now == 0
			&& self.irq_stage_next == 0
			&& self.ta.is_idle()
			&& self.tb.is_idle()
			&& !self.sdr.shifting
			&& self.sdr.irq_delay == 0
			&& !flag_falling
			&& !tod_pulse
			&& !cnt_rising
		{
			return self.irq_line;
		}

		/* Both timers advance before newly observed CNT edges are queued. An edge detected during this host cycle therefore affects the timer pipeline rather than retroactively changing the decrement already in progress. */
		let ta_uf = self.ta.step();
		let tb_uf = self.tb.step();

		if cnt_rising {
			if (self.ta.cr & CRA_INMODE) != 0 {
				self.ta.observe_cnt_edge();
			}
			if TimerBInputMode::from_crb(self.tb.cr) == TimerBInputMode::Cnt {
				self.tb.observe_cnt_edge();
			}
		}
		if ta_uf {
			match TimerBInputMode::from_crb(self.tb.cr) {
				TimerBInputMode::TimerAUnderflow => self.tb.observe_cnt_edge(),
				TimerBInputMode::TimerAUnderflowWhileCntHigh if cnt_high_delayed => {
					self.tb.observe_cnt_edge()
				}
				_ => {}
			}
		}

		let tod_alarm = if tod_pulse {
			let ticks_per_tenth = if (self.ta.cr & CRA_TODIN) != 0 { 5 } else { 6 };
			self.tod.tick(ticks_per_tenth)
		} else {
			false
		};

		if self.sdr.input_mode && cnt_rising && !self.sdr.shifting
			&& self.sdr.irq_delay == 0 {
			self.sdr.start_input();
		}

		let sdr_clock = if self.sdr.input_mode {
			cnt_rising
		} else {
			ta_uf && (self.ta.cr & CRA_SPMODE) != 0
		};
		let (_, sdr_irq) = self.sdr.tick(sdr_clock, self.sp_pin);

		/* Sources raised by the same host cycle are merged before entering the interrupt pipeline, preserving simultaneous events in one ICR update. */
		let mut pending = 0u8;
		if ta_uf {
			pending |= ICR_TA;
		}
		if tb_uf {
			pending |= ICR_TB;
		}
		if tod_alarm {
			pending |= ICR_ALRM;
		}
		if sdr_irq {
			pending |= ICR_SP;
		}
		if flag_falling {
			pending |= ICR_FLAG;
		}

		if pending != 0 {
			self.raise_source(pending);
		}
		/* Acknowledge wins for the readable Timer B flag, but the enabled
		 * event has already entered the independent IRQ path. */
		if tb_uf && acknowledge_cycle {
			self.icr &= !ICR_TB;
		}

		self.irq_line
	}

	#[inline]
	/* Peek exposes register values without performing the destructive or latching side effects of a CPU read. It is used by inspection paths that must not perturb emulation state. */
	pub fn peek(&self, reg: u8) -> u8 {
		match reg {
			PRA => self.pra,
			PRB => self.prb,
			DDRA => self.ddra,
			DDRB => self.ddrb,
			TALO => (self.ta.counter & 0x00FF) as u8,
			TAHI => (self.ta.counter >> 8) as u8,
			TBLO => (self.tb.counter & 0x00FF) as u8,
			TBHI => (self.tb.counter >> 8) as u8,
			TOD10THS => {
				if self.tod.latched {
					self.tod.latch_tenths
				} else {
					self.tod.tenths
				}
			}
			TODSEC => {
				if self.tod.latched {
					self.tod.latch_seconds
				} else {
					self.tod.seconds
				}
			}
			TODMIN => {
				if self.tod.latched {
					self.tod.latch_minutes
				} else {
					self.tod.minutes
				}
			}
			TODHR => {
				if self.tod.latched {
					self.tod.latch_hours
				} else {
					self.tod.hours
				}
			}
			SDR => self.sdr.data,
			ICR => (self.icr & 0x1F) | if self.irq_line || self.icr_ir_readback { ICR_IR } else { 0 },
			CRA => self.ta.cr,
			CRB => self.tb.cr,
			_ => 0xFF,
		}
	}

	#[inline]
	/* Reading TOD hours latches the clock, reading TOD tenths releases it, and reading ICR returns the sources before clearing their interrupt indication (MOS-6526-1981, Time of Day Clock and Interrupt Control Register). */
	pub fn read(&mut self, reg: u8) -> u8 {
		match reg {
			PRA => self.pra,
			PRB => self.prb,
			DDRA => self.ddra,
			DDRB => self.ddrb,
			TALO => (self.ta.counter & 0x00FF) as u8,
			TAHI => (self.ta.counter >> 8) as u8,
			TBLO => (self.tb.counter & 0x00FF) as u8,
			TBHI => (self.tb.counter >> 8) as u8,
			TOD10THS => {
				let val = if self.tod.latched {
					self.tod.latch_tenths
				} else {
					self.tod.tenths
				};
				self.tod.latched = false;
				val
			}
			TODSEC => {
				if self.tod.latched {
					self.tod.latch_seconds
				} else {
					self.tod.seconds
				}
			}
			TODMIN => {
				if self.tod.latched {
					self.tod.latch_minutes
				} else {
					self.tod.minutes
				}
			}
			TODHR => {
				self.tod.latch();
				self.tod.latch_hours
			}
			SDR => self.sdr.data,
			/* Reading ICR lowers the external IRQ immediately, but source bits are cleared on the following tick so events already in flight can still be ordered correctly. */
			ICR => {
				let visible = self.icr & 0x1F;
				let val = visible | if self.irq_line || self.icr_ir_readback { ICR_IR } else { 0 };
				self.icr_ir_readback_next = self.irq_line || self.irq_stage_now != 0;
				self.icr_ack |= visible;
				self.icr_clear_next = true;
				self.irq_line = false;
				self.irq_stage_now = 0;
				self.irq_stage_next = 0;
				val
			}
			CRA => self.ta.cr,
			CRB => self.tb.cr,
			_ => 0xFF,
		}
	}

	#[inline]
	/* CRB selects clock versus alarm TOD writes, ICR bit 7 selects mask set versus clear, and CRA selects serial input versus output mode (MOS-6526-1981, Register Map, Time of Day Clock and Interrupt Control Register). */
	pub fn write(&mut self, reg: u8, val: u8) {
		match reg {
			PRA => self.pra = val,
			PRB => self.prb = val,
			DDRA => self.ddra = val,
			DDRB => self.ddrb = val,
			TALO => self.ta.write_latch_lo(val),
			TAHI => self.ta.write_latch_hi(val),
			TBLO => self.tb.write_latch_lo(val),
			TBHI => self.tb.write_latch_hi(val),
			TOD10THS => {
				if (self.tb.cr & CRB_ALARM) != 0 {
					self.tod.alarm = (self.tod.alarm & !0x0000_00FF) | (val & 0x0F) as u32;
				} else {
					self.tod.tenths = val & 0x0F;
					/* Restarting a stopped clock also restarts its mains-frequency divider; a write while running preserves the divider phase (CIA-TOD-MEASUREMENTS, hzsync0 and hzsync1). */
					if !self.tod.running {
						self.tod.divider = 0;
					}
					self.tod.running = true;
					self.tod.pack_current();
				}
				self.compare_tod_alarm();
			}
			TODSEC => {
				if (self.tb.cr & CRB_ALARM) != 0 {
					self.tod.alarm = (self.tod.alarm & !0x0000_FF00) | (((val & 0x7F) as u32) << 8);
				} else {
					self.tod.seconds = val & 0x7F;
					self.tod.pack_current();
				}
				self.compare_tod_alarm();
			}
			TODMIN => {
				if (self.tb.cr & CRB_ALARM) != 0 {
					self.tod.alarm =
						(self.tod.alarm & !0x00FF_0000) | (((val & 0x7F) as u32) << 16);
				} else {
					self.tod.minutes = val & 0x7F;
					self.tod.pack_current();
				}
				self.compare_tod_alarm();
			}
			TODHR => {
				if (self.tb.cr & CRB_ALARM) != 0 {
					self.tod.alarm =
						(self.tod.alarm & !0xFF00_0000) | (((val & 0x9F) as u32) << 24);
				} else {
					let hours = val & 0x9F;
					self.tod.hours = if (hours & 0x1F) == 0x12 {
						hours ^ 0x80
					} else {
						hours
					};
					self.tod.running = false;
					self.tod.pack_current();
				}
				self.compare_tod_alarm();
			}
			SDR => self.sdr.write_data(val, (self.ta.cr & CRA_SPMODE) != 0),
			ICR => {
				if (val & 0x80) != 0 {
					self.icr_mask |= val & 0x7F;
				} else {
					self.icr_mask &= !(val & 0x7F);
					/* During adjacent ICR writes, the second mask value can
					 * cancel a request still in flight. An isolated mask write
					 * preserves an already qualified request; neither releases
					 * an asserted pin (CIA-ICR-MEASUREMENTS). */
					if self.icr_write_previous {
						self.irq_stage_now &= self.icr_mask;
						self.irq_stage_next &= self.icr_mask;
					}
				}
				self.icr_write_active = true;
				let enabled = self.icr & self.icr_mask & 0x1F;
				if enabled != 0 && !self.irq_line {
					self.irq_stage_next |= enabled;
				}
			}
			/* Changing serial direction aborts the current shift operation before the timer control state is updated, preventing a partially transferred byte from surviving a mode change. */
			CRA => {
				let output_mode = (val & CRA_SPMODE) != 0;
				let was_output_mode = (self.ta.cr & CRA_SPMODE) != 0;
				if output_mode != was_output_mode {
					self.sdr.stop();
				}
				self.sdr.input_mode = !output_mode;
				self.ta.write_cr(val, (val & CRA_INMODE) == 0);
			}
			CRB => {
				self.tb
					.write_cr(val, TimerBInputMode::from_crb(val) == TimerBInputMode::Phi2);
			}
			_ => {}
		}
	}

	/* The board wrappers use these pin-level timer outputs when PBON redirects Timer A or Timer B onto PB6 or PB7. */
	pub fn timer_outputs(&self) -> (bool, bool) {
		(self.ta.timer_output(), self.tb.timer_output())
	}

	/* Pin setters record levels only; edges are derived once in tick so every consumer observes the same transition. */
	pub fn set_cnt_pin(&mut self, state: bool) {
		self.cnt_pin = state;
	}
	pub fn set_flag_pin(&mut self, state: bool) {
		self.flag_pin = state;
	}
	pub fn serial_output_active(&self) -> bool {
		(self.ta.cr & CRA_SPMODE) != 0
	}
	pub fn sp_output(&self) -> bool {
		if self.serial_output_active() {
			self.sdr.output_bit
		} else {
			true
		}
	}
}