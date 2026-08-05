// =======================================================
// src/sid/envelope.rs — SID envelope generator
// =======================================================

use super::constants::envelope_rate_comparator;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
/* The envelope has three externally meaningful directions. Sustain remains part of DecaySustain because the level path simply stops at the programmed code while the shared rate and exponential counters continue to run. */
pub enum EnvelopePhase {
	/* Linear upward stepping towards full scale. */
	Attack,
	/* Exponentially divided downward stepping, held when the sustain code is reached. */
	DecaySustain,
	/* Exponentially divided downward stepping after GATE is cleared. */
	Release,
}

/* A voice envelope is built from two coupled timing paths: a 15-bit rate divider chooses when an event is due, and an exponential divider decides whether that event may change the eight-bit level. Gate and direction changes are pipelined separately, which is why changing GATE does not instantaneously change the visible envelope trajectory. */
pub struct Envelope {
	/* Current eight-bit DAC code exposed to voice conversion. */
	pub volume: u8,

	attack_rate: u8,
	decay_rate: u8,
	sustain_level: u8,
	release_rate: u8,

	gate: bool,
	phase: EnvelopePhase,

	/* Fifteen-bit rate counter implemented as the SID-style LFSR divider. */
	rate_lfsr: u16,
	/* Comparator hits arm a separate reset path rather than resetting the divider inline. */
	rate_reset_armed: bool,

	level_wait: u8,
	exponential_wait: u8,
	exponential_count: u8,
	exponential_period: u8,
	period_update: u8,

	/* Once release reaches zero, the level path remains locked until a new attack edge clears it. */
	zero_lock: bool,

	gate_d0: bool,
	gate_d1: bool,
	gate_d2: bool,

	direction_target: EnvelopePhase,
	direction_progress: u8,

	latched_level: u8,
}

impl Envelope {
	pub const fn new() -> Self {
		Self {
			attack_rate: 0,
			decay_rate: 0,
			sustain_level: 0,
			release_rate: 0,
			gate: false,
			phase: EnvelopePhase::Release,
			rate_lfsr: 0x7fff,
			rate_reset_armed: false,
			level_wait: 0,
			exponential_wait: 0,
			exponential_count: 0,
			exponential_period: 1,
			period_update: 0,
			volume: 0xaa,
			zero_lock: false,
			gate_d0: false,
			gate_d1: false,
			gate_d2: false,
			direction_target: EnvelopePhase::Release,
			direction_progress: 0,
			latched_level: 0xaa,
		}
	}

	/* Reset preserves the current analogue-visible level and rate-divider phase. The control logic returns to release, matching a chip reset that does not magically discharge the envelope DAC. */
	pub fn reset(&mut self) {
		let preserved_volume = self.volume;
		let preserved_rate = self.rate_lfsr;
		*self = Self::new();
		self.volume = preserved_volume;
		self.latched_level = preserved_volume;
		self.rate_lfsr = preserved_rate;
		self.zero_lock = preserved_volume == 0;
	}

	/* The upper and lower nibbles select independent rate-comparator periods for attack and decay. Updating them does not restart the current phase, so a running counter sees the new period immediately (C64-PRG-1982, SID envelope registers). */
	pub fn set_attack_decay(&mut self, value: u8) {
		self.attack_rate = value >> 4;
		self.decay_rate = value & 0x0f;
	}

	/* The four-bit sustain code is replicated into both nibbles because the envelope counter is eight bits wide. Sustain and release update independently of GATE, so a new sustain value can change the target of an envelope already in decay. */
	pub fn set_sustain_release(&mut self, value: u8) {
		self.sustain_level = (value & 0xf0) | (value >> 4);
		self.release_rate = value & 0x0f;
	}

	/* GATE is sampled into the direction-control pipeline. The caller changes the pin level here; attack or release begins only when that sampled edge reaches the control stage. */
	pub fn set_gate(&mut self, gate: bool) {
		self.gate = gate;
	}

	#[inline]
	/* The visible level is latched before gate sampling, direction control, level stepping, exponential division and rate-divider reset advance in hardware order. These paths therefore retain distinct one-cycle boundaries. */
	pub fn clock(&mut self) {
		self.latched_level = self.volume;

		if self.period_update != 0 {
			self.exponential_period = self.period_update;
			self.period_update = 0;
		}

		self.sample_gate();
		self.advance_direction_control();
		self.advance_level_path();
		self.advance_exponential_path();
		self.service_rate_reset();
		self.clock_rate_divider();
	}

	#[inline]
	/* GATE passes through three sampled stages. Rising and falling edges schedule direction changes rather than mutating the phase immediately. */
	fn sample_gate(&mut self) {
		let rising = !self.gate_d2 && self.gate_d1;
		let falling = self.gate_d2 && !self.gate_d1;

		self.gate_d2 = self.gate_d1;
		self.gate_d1 = self.gate_d0;
		self.gate_d0 = self.gate;

		if rising {
			self.direction_target = EnvelopePhase::Attack;
			self.direction_progress = 2;
		} else if falling {
			self.direction_target = EnvelopePhase::Release;
			self.direction_progress = if self.phase == EnvelopePhase::Attack { 2 } else { 1 };
		}
	}

	#[inline]
	/* Direction changes are committed after the sampled GATE edge has crossed the pipeline. A new attack also clears the zero lock so an envelope held at zero can move again. */
	fn advance_direction_control(&mut self) {
		if self.direction_progress == 0 {
			return;
		}

		self.direction_progress -= 1;

		if self.direction_target == EnvelopePhase::Attack && self.direction_progress == 1 {

			return;
		}

		if self.direction_progress == 0 {
			self.phase = self.direction_target;
			if self.phase == EnvelopePhase::Attack {
				self.zero_lock = false;
				self.exponential_count = 0;
			}
		}
	}

	#[inline]
	/* A permitted level event changes the counter by exactly one. Attack wraps upward; decay and release move downward, and reaching zero engages the lock that produces the classic envelope hold until a later attack clears it. */
	fn advance_level_path(&mut self) {
		if self.level_wait == 0 {
			return;
		}

		self.level_wait -= 1;
		if self.level_wait != 0 || self.zero_lock {
			return;
		}

		match self.phase {
			EnvelopePhase::Attack => {
				self.volume = self.volume.wrapping_add(1);
				if self.volume == 0xff {
					self.direction_target = EnvelopePhase::DecaySustain;
					self.direction_progress = 3;
				}
				self.update_exponential_period();
			}
			EnvelopePhase::DecaySustain => {
				if self.volume != self.sustain_level {
					self.volume = self.volume.wrapping_sub(1);
					if self.volume == 0 {
						self.zero_lock = true;
					}
					self.update_exponential_period();
				}
			}
			EnvelopePhase::Release => {
				self.volume = self.volume.wrapping_sub(1);
				if self.volume == 0 {
					self.zero_lock = true;
				}
				self.update_exponential_period();
			}
		}
	}

	#[inline]
	/* Attack bypasses exponential division. Decay and release require a programmable number of rate events before the next level step, producing the characteristic segmented exponential slope. */
	fn advance_exponential_path(&mut self) {
		if self.exponential_wait == 0 {
			return;
		}

		self.exponential_wait -= 1;
		if self.exponential_wait != 0 {
			return;
		}

		self.exponential_count = 0;
		let should_step = match self.phase {
			EnvelopePhase::Attack => false,
			EnvelopePhase::DecaySustain => self.volume != self.sustain_level,
			EnvelopePhase::Release => true,
		};
		if should_step {
			self.level_wait = 1;
		}
	}

	#[inline]
	/* The rate divider resets through a delayed arm rather than on the comparison cycle itself. Retaining this separation is necessary for rate-counter timing anomalies after parameter changes. */
	fn service_rate_reset(&mut self) {
		if !self.rate_reset_armed {
			return;
		}

		self.rate_reset_armed = false;
		self.rate_lfsr = 0x7fff;

		if self.phase == EnvelopePhase::Attack {
			self.exponential_count = 0;
			self.level_wait = 2;
		} else if !self.zero_lock {
			self.exponential_count = self.exponential_count.wrapping_add(1);
			if self.exponential_count == self.exponential_period {
				self.exponential_wait = if self.exponential_period == 1 { 1 } else { 2 };
			}
		}
	}

	#[inline]
	/* The fifteen-bit LFSR is used as a deterministic divider. A comparator hit schedules both the exponential path and a later divider reset. */
	fn clock_rate_divider(&mut self) {
		let selected_period = self.current_rate_period();
		if self.rate_lfsr == selected_period {
			self.rate_reset_armed = true;
			return;
		}

		let feedback = ((self.rate_lfsr << 14) ^ (self.rate_lfsr << 13)) & 0x4000;
		self.rate_lfsr = (self.rate_lfsr >> 1) | feedback;
	}

	#[inline]
	/* Attack, decay and release select different comparator periods from the programmed nibbles; sustain reuses the decay phase but suppresses level changes at the target value. */
	fn current_rate_period(&self) -> u16 {
		let rate = if self.direction_target == EnvelopePhase::Attack && self.direction_progress == 1 {
			self.decay_rate
		} else {
			match self.phase {
				EnvelopePhase::Attack => self.attack_rate,
				EnvelopePhase::DecaySustain => self.decay_rate,
				EnvelopePhase::Release => self.release_rate,
			}
		};
		envelope_rate_comparator(rate)
	}

	#[inline]
	/* The exponential period changes only at specific envelope levels; the new divisor is deferred to the next cycle so a threshold crossing cannot retroactively change the event that produced it. */
	fn update_exponential_period(&mut self) {

		self.period_update = match self.volume {
			0xff | 0x00 => 1,
			0x60 => 2,
			0x30 => 4,
			0x18 => 8,
			0x0c => 16,
			0x06 => 32,
			_ => 0,
		};
	}

	#[inline]
	pub const fn read_level(&self) -> u8 {
		self.latched_level
	}
}

impl Default for Envelope {
	fn default() -> Self {
		Self::new()
	}
}