// =======================================================
// src/sid/envelope.rs — SID envelope generator
// =======================================================

/* SID envelope generator. */

use super::constants::envelope_rate_period;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum EnvelopeMotion {
	Rising,
	FallingToSustain,
	FallingToZero,
}

#[derive(Clone, Copy)]
struct GateHistory {
	input: bool,
	stage_a: bool,
	stage_b: bool,
}

impl GateHistory {
	const fn new() -> Self {
		Self {
			input: false,
			stage_a: false,
			stage_b: false,
		}
	}

	#[inline]
	fn advance(&mut self) -> Option<bool> {
		let rose = !self.stage_b && self.stage_a;
		let fell = self.stage_b && !self.stage_a;

		self.stage_b = self.stage_a;
		self.stage_a = self.input;

		if rose {
			Some(true)
		} else if fell {
			Some(false)
		} else {
			None
		}
	}
}

/* The envelope generator owns the eight-bit amplitude code and the timing state that moves it. Register writes alter rate selections and the sustain target without restarting the timing machinery. GATE is sampled through a short control pipeline, while amplitude changes are scheduled through separate rate and exponential timing paths. */
pub struct Envelope {
	pub volume: u8,

	rise_rate: u8,
	decay_rate: u8,
	hold_level: u8,
	fall_rate: u8,

	gate: GateHistory,
	motion: EnvelopeMotion,
	requested_motion: EnvelopeMotion,
	motion_delay: u8,

	/* The physical 15-bit rate LFSR has one 32,767-state orbit. Storing the orbit phase is an exact coordinate transform of that state: phase zero is $7fff and each ordinary PHI2 advance increments the phase. This removes the per-cycle LFSR reconstruction while preserving every comparator event and the free-running divider phase across register changes and reset. */
	rate_phase: u16,
	/* The selected comparator position changes only on an ADSR write or a
	 * motion-pipeline transition.  Caching it removes the rate-selection tree
	 * from each of the three per-PHI2 envelope clocks. */
	rate_target_phase: u16,
	matched_rate: bool,

	amplitude_delay: u8,
	curve_delay: u8,
	curve_count: u8,
	curve_divisor: u8,
	queued_curve_divisor: u8,

	floor_hold: bool,
	readback: u8,
}

impl Envelope {
	pub const fn new() -> Self {
		Self {
			volume: 0xaa,
			rise_rate: 0,
			decay_rate: 0,
			hold_level: 0,
			fall_rate: 0,
			gate: GateHistory::new(),
			motion: EnvelopeMotion::FallingToZero,
			requested_motion: EnvelopeMotion::FallingToZero,
			motion_delay: 0,
			rate_phase: 0,
			rate_target_phase: (envelope_rate_period(0) - 1) as u16,
			matched_rate: false,
			amplitude_delay: 0,
			curve_delay: 0,
			curve_count: 0,
			curve_divisor: 1,
			queued_curve_divisor: 0,
			floor_hold: false,
			readback: 0xaa,
		}
	}

	/* Reset returns the control paths to their release state without forcing an artificial change in the analogue-visible envelope code or in the free-running rate sequence. */
	pub fn reset(&mut self) {
		let volume = self.volume;
		let rate_phase = self.rate_phase;
		*self = Self::new();
		self.volume = volume;
		self.readback = volume;
		self.rate_phase = rate_phase;
		self.floor_hold = volume == 0;
	}

	/* The attack/decay register selects the rising rate with its upper nibble and the programmed-decay rate with its lower nibble. */
	pub fn set_attack_decay(&mut self, value: u8) {
		self.rise_rate = value >> 4;
		self.decay_rate = value & 0x0f;
		self.refresh_rate_target_phase();
	}

	/* The sustain/release register expands the four-bit sustain setting to the eight-bit envelope scale and independently selects the release rate. */
	pub fn set_sustain_release(&mut self, value: u8) {
		self.hold_level = (value & 0xf0) | (value >> 4);
		self.fall_rate = value & 0x0f;
		self.refresh_rate_target_phase();
	}

	/* A rising GATE unlocks the counter immediately. Counting direction follows the sampled control path. */
	pub fn set_gate(&mut self, gate: bool) {
		if gate && !self.gate.input {
			self.floor_hold = false;
		}
		self.gate.input = gate;
		self.refresh_rate_target_phase();
	}

	#[inline]
	/* One call advances one SID clock while preserving the internal ordering between readback, deferred divisor changes, GATE sampling, direction control, amplitude motion, exponential timing and the rate sequence. */
	pub fn clock(&mut self) {
		self.readback = self.volume;
		/* Between sparse rate and GATE events the complete control pipeline is
		 * quiescent.  In that overwhelmingly common state, advancing the exact
		 * orbit coordinate is the only observable work required this PHI2. */
		if self.queued_curve_divisor == 0
			&& self.gate.input == self.gate.stage_a
			&& self.gate.stage_a == self.gate.stage_b
			&& self.motion_delay == 0
			&& self.amplitude_delay == 0
			&& self.curve_delay == 0
			&& !self.matched_rate
		{
			if self.rate_phase == self.rate_target_phase {
				self.matched_rate = true;
			} else {
				self.rate_phase += 1;
				if self.rate_phase == 32_767 {
					self.rate_phase = 0;
				}
			}
			return;
		}
		self.commit_curve_divisor();
		self.sample_gate_transition();
		self.commit_requested_motion();
		self.clock_amplitude_path();
		self.clock_curve_path();
		self.consume_rate_match();
		self.clock_rate_sequence();
	}

	#[inline]
	fn commit_curve_divisor(&mut self) {
		if self.queued_curve_divisor != 0 {
			self.curve_divisor = self.queued_curve_divisor;
			self.queued_curve_divisor = 0;
		}
	}

	#[inline]
	fn sample_gate_transition(&mut self) {
		match self.gate.advance() {
			Some(true) => {
				self.requested_motion = EnvelopeMotion::Rising;
				self.motion_delay = 2;
			}
			Some(false) => {
				self.requested_motion = EnvelopeMotion::FallingToZero;
				self.motion_delay = if self.motion == EnvelopeMotion::Rising {
					2
				} else {
					1
				};
			}
			None => {}
		}
	}

	#[inline]
	fn commit_requested_motion(&mut self) {
		if self.motion_delay == 0 {
			return;
		}

		self.motion_delay -= 1;

		if self.requested_motion == EnvelopeMotion::Rising && self.motion_delay == 1 {
			self.refresh_rate_target_phase();
			return;
		}

		if self.motion_delay == 0 {
			self.motion = self.requested_motion;
		}
		self.refresh_rate_target_phase();
	}

	#[inline]
	fn clock_amplitude_path(&mut self) {
		if self.amplitude_delay == 0 {
			return;
		}

		self.amplitude_delay -= 1;
		if self.amplitude_delay != 0 || self.floor_hold {
			return;
		}

		match self.motion {
			EnvelopeMotion::Rising => {
				self.volume = self.volume.wrapping_add(1);
				if self.volume == 0xff {
					self.requested_motion = EnvelopeMotion::FallingToSustain;
					self.motion_delay = 3;
				}
				self.select_curve_divisor();
			}
			EnvelopeMotion::FallingToSustain => {
				if self.volume != self.hold_level {
					self.volume = self.volume.wrapping_sub(1);
					if self.volume == 0 {
						self.floor_hold = true;
					}
					self.select_curve_divisor();
				}
			}
			EnvelopeMotion::FallingToZero => {
				self.volume = self.volume.wrapping_sub(1);
				if self.volume == 0 {
					self.floor_hold = true;
				}
				self.select_curve_divisor();
			}
		}
	}

	#[inline]
	fn clock_curve_path(&mut self) {
		if self.curve_delay == 0 {
			return;
		}

		self.curve_delay -= 1;
		if self.curve_delay != 0 {
			return;
		}

		self.curve_count = 0;
		let amplitude_change_due = match self.motion {
			EnvelopeMotion::Rising => true,
			EnvelopeMotion::FallingToSustain => self.volume != self.hold_level,
			EnvelopeMotion::FallingToZero => true,
		};

		if amplitude_change_due {
			self.amplitude_delay = 1;
		}
	}

	#[inline]
	fn consume_rate_match(&mut self) {
		if !self.matched_rate {
			return;
		}

		self.matched_rate = false;
		self.rate_phase = 0;

		if self.rising_rate_selected() {
			self.curve_count = 0;
			self.amplitude_delay = 2;
		} else if !self.floor_hold {
			self.curve_count = self.curve_count.wrapping_add(1);
			if self.curve_count == self.curve_divisor {
				self.curve_delay = if self.curve_divisor == 1 { 1 } else { 2 };
			}
		}
	}

	#[inline]
	fn clock_rate_sequence(&mut self) {
		if self.rate_phase == self.rate_target_phase {
			self.matched_rate = true;
		} else {
			self.rate_phase += 1;
			if self.rate_phase == 32_767 {
				self.rate_phase = 0;
			}
		}
	}

	#[inline]
	/* R0 selects the rate and bypasses the exponential divider one cycle
	 * before cnt_up changes the amplitude counter direction. */
	fn rising_rate_selected(&self) -> bool {
		if self.motion_delay == 1 {
			self.requested_motion == EnvelopeMotion::Rising
		} else {
			self.motion == EnvelopeMotion::Rising
		}
	}

	#[inline]
	fn refresh_rate_target_phase(&mut self) {
		let rate = if self.rising_rate_selected() {
			self.rise_rate
		} else if self.gate.input {
			self.decay_rate
		} else {
			self.fall_rate
		};

		self.rate_target_phase = (envelope_rate_period(rate) - 1) as u16;
	}

	#[inline]
	fn select_curve_divisor(&mut self) {
		/* Five level detectors in the envelope counter select the exponential divider at the exact codes visible in the reconstructed 6581 logic. The last segment uses thirty rate events, not a binary divide-by-32 (SID-SCHEMATICS-ENVELOPE). */
		self.queued_curve_divisor = match self.volume {
			0xff | 0x00 => 1,
			0x5d => 2,
			0x36 => 4,
			0x1a => 8,
			0x0e => 16,
			0x06 => 30,
			_ => 0,
		};
	}

	#[inline]
	pub const fn read_level(&self) -> u8 {
		self.readback
	}
}

impl Default for Envelope {
	fn default() -> Self {
		Self::new()
	}
}