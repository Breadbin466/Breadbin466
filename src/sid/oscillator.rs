// =======================================================
// src/sid/oscillator.rs — SID oscillator
// =======================================================

/* SID oscillator, waveform-line, noise and synchronisation state. */

use super::constants::{
	COMBINED_WAVEFORM_MSB_CLEAR_MASK, NEVER, NOISE_EVENT_MASK, NOISE_MASK, NOISE_OUTPUT_TAPS,
	NOISE_RESET_VALUE, NOISE_TEST_CHARGE_MAX, NOISE_TEST_LEAK_INTERVAL_CYCLES,
	NOISE_TEST_LOGIC_THRESHOLD, PHASE_MASK, PHASE_RESET_VALUE, SYNC_EVENT_MASK,
	WAVEFORM_FLOAT_CHARGE_MAX, WAVEFORM_FLOAT_HOLD_CYCLES, WAVEFORM_FLOAT_LEAK_INTERVAL_CYCLES,
	WAVEFORM_FLOAT_THRESHOLD, WAVEFORM_MASK, WAVEFORM_PIPELINE_RESET_VALUE,
};
use super::waveforms::{GeneratedWaveShapes, triangle_from_phase};

#[derive(Clone, Copy)]
#[repr(transparent)]
/* The four waveform control bits select analogue line drivers rather than mutually exclusive digital functions. Several selections interact on shared lines, while no selection exposes the decaying waveform bus. */
struct WaveformSelect(u8);

impl WaveformSelect {
	const fn from_raw(raw: u8) -> Self {
		Self(raw & 0x0f)
	}

	const fn raw(self) -> u8 {
		self.0
	}

	const fn is_off(self) -> bool {
		self.0 == 0
	}

	const fn has(self, flag: u8) -> bool {
		self.0 & flag != 0
	}

	const fn is_combined(self) -> bool {
		self.0.count_ones() >= 2
	}

	fn combine(self, tri: u16, saw: u16, pulse: u16, noise: u16, open_bus: u16) -> u16 {
		if self.is_off() {
			return open_bus;
		}

		let mut lines = WAVEFORM_MASK;
		if self.has(0x01) {
			lines &= tri;
		}
		if self.has(0x02) {
			lines &= saw;
		}
		if self.has(0x04) {
			lines &= pulse;
		}
		if self.has(0x08) {
			lines &= noise;
		}
		lines
	}
}

/* Noise is a 23-stage LFSR whose selected stages drive the twelve waveform lines. Clocking, TEST-mode charge leakage and combined-waveform pull-downs are modelled separately because each can alter a different snapshot of the register. */
struct NoiseGenerator {
	state: u32,

	visible: u16,

	captured: u32,

	captured_charge: [u16; 23],

	transfer_at: u64,

	charge: [u16; 23],

	next_leak: u64,
}

impl NoiseGenerator {
	const fn new() -> Self {
		let state = NOISE_RESET_VALUE & NOISE_MASK;
		Self {
			state,
			visible: Self::project(state),
			captured: state,
			captured_charge: [NOISE_TEST_CHARGE_MAX; 23],
			transfer_at: NEVER,
			charge: [NOISE_TEST_CHARGE_MAX; 23],
			next_leak: NEVER,
		}
	}

	/* TEST stops normal shifting and exposes stored charge in each stage to a slow relaxation process. */
	fn enter_test(&mut self, cycle: u64) {
		self.captured = self.state;
		self.transfer_at = NEVER;

		for (stage, charge) in self.charge.iter_mut().enumerate() {
			*charge = if self.state & (1u32 << stage) == 0 {
				0
			} else {
				NOISE_TEST_CHARGE_MAX
			};
		}
		self.captured_charge = self.charge;
		self.next_leak = cycle.wrapping_add(u64::from(NOISE_TEST_LEAK_INTERVAL_CYCLES));
	}

	/* Leaving TEST converts the surviving stage charges back into logic levels, then schedules one transfer so the released register resumes from the sampled analogue state. */
	fn leave_test(&mut self, cycle: u64) {
		let mut sampled = 0u32;
		for (stage, &charge) in self.charge.iter().enumerate() {
			if charge >= NOISE_TEST_LOGIC_THRESHOLD {
				sampled |= 1u32 << stage;
			}
		}
		self.state = sampled & NOISE_MASK;
		self.visible = Self::project(self.state);
		self.captured = self.state;
		self.captured_charge = self.charge;
		self.transfer_at = cycle.wrapping_add(1);
		self.next_leak = NEVER;
	}

	#[inline]
	/* The noise register captures its old state on the designated accumulator transition and shifts two SID cycles later. Separating capture from transfer allows combined-waveform loading during the intervening window. */
	fn clock(&mut self, rising_bits: u32, cycle: u64) {
		if rising_bits & NOISE_EVENT_MASK == 0 || self.transfer_at != NEVER {
			return;
		}
		self.captured = self.state;
		self.captured_charge = self.charge;
		self.transfer_at = cycle.wrapping_add(2);
	}

	#[inline]
	fn execute_transfer(&mut self, cycle: u64) {
		if self.transfer_at == NEVER || cycle < self.transfer_at {
			return;
		}

		let low_tap = self.captured & 1;
		let mid_tap = (self.captured >> 5) & 1;
		let feedback = (low_tap ^ mid_tap) & 1;

		let mut next_charge = [0u16; 23];
		let mut stage = 0usize;
		while stage < 22 {
			next_charge[stage] = self.captured_charge[stage + 1];
			stage += 1;
		}
		next_charge[22] = if feedback == 0 {
			0
		} else {
			NOISE_TEST_CHARGE_MAX
		};

		self.state = ((self.captured >> 1) | (feedback << 22)) & NOISE_MASK;
		self.charge = next_charge;
		self.visible = Self::project(self.state);
		self.transfer_at = NEVER;
	}

	fn relax(&mut self, cycle: u64) {
		if self.next_leak == NEVER || cycle < self.next_leak {
			return;
		}

		let interval = u64::from(NOISE_TEST_LEAK_INTERVAL_CYCLES);
		let elapsed_steps = ((cycle - self.next_leak) / interval) + 1;
		self.next_leak = self
			.next_leak
			.wrapping_add(elapsed_steps.wrapping_mul(interval));

		let mut changed = false;
		let mut step = 0u64;
		while step < elapsed_steps {
			for stage in 0..23 {
				let charge = self.charge[stage];
				if charge < NOISE_TEST_CHARGE_MAX {
					let remaining = NOISE_TEST_CHARGE_MAX - charge;
					let increment = (remaining / 23).max(1);
					self.charge[stage] =
						charge.saturating_add(increment).min(NOISE_TEST_CHARGE_MAX);
				}

				if self.charge[stage] >= NOISE_TEST_LOGIC_THRESHOLD
					&& self.state & (1u32 << stage) == 0
				{
					self.state |= 1u32 << stage;
					changed = true;
				}
			}
			step += 1;
		}

		if changed {
			self.visible = Self::project(self.state);
			if self.transfer_at == NEVER {
				self.captured = self.state;
				self.captured_charge = self.charge;
			}
		}
	}

	#[inline]
	fn pull_down(&mut self, waveform: u16, cycle: u64) {
		let affects_capture =
			self.transfer_at != NEVER && self.transfer_at <= cycle.wrapping_add(1);

		for (line, &stage) in NOISE_OUTPUT_TAPS.iter().enumerate() {
			let line_mask = 1u16 << (11 - line);
			if waveform & line_mask != 0 {
				continue;
			}

			let index = usize::from(stage);
			let charge = self.charge[index];
			let decrement = (charge / 12).max(1);
			self.charge[index] = charge.saturating_sub(decrement);

			if affects_capture {
				let captured_charge = self.captured_charge[index];
				let captured_decrement = (captured_charge / 12).max(1);
				self.captured_charge[index] = captured_charge.saturating_sub(captured_decrement);
			}

			if self.charge[index] < NOISE_TEST_LOGIC_THRESHOLD {
				self.state &= !(1u32 << index);
			}
			if affects_capture && self.captured_charge[index] < NOISE_TEST_LOGIC_THRESHOLD {
				self.captured &= !(1u32 << index);
			}
		}

		self.state &= NOISE_MASK;
		self.captured &= NOISE_MASK;
		self.visible = Self::project(self.state);
	}

	#[inline]
	const fn output(&self) -> u16 {
		self.visible
	}

	const fn project(state: u32) -> u16 {
		let mut out = 0u16;
		let mut i = 0usize;
		while i < NOISE_OUTPUT_TAPS.len() {
			let stage = NOISE_OUTPUT_TAPS[i];
			out |= (((state >> stage) & 1) as u16) << (11 - i);
			i += 1;
		}
		out
	}
}

/* When no waveform is selected, the twelve waveform lines retain their previous charge and decay towards zero. A new driver captures a fresh value and cancels the floating interval. */
struct FloatingBus {
	value: u16,
	charge: [u16; 12],
	hold_until: u64,
	next_leak: u64,
}

impl FloatingBus {
	const fn new() -> Self {
		Self {
			value: 0,
			charge: [0; 12],
			hold_until: NEVER,
			next_leak: NEVER,
		}
	}

	fn capture(&mut self, value: u16, cycle: u64) {
		self.value = value & WAVEFORM_MASK;
		for bit in 0..12 {
			self.charge[bit] = if self.value & (1u16 << bit) == 0 {
				0
			} else {
				WAVEFORM_FLOAT_CHARGE_MAX
			};
		}
		self.hold_until = cycle.wrapping_add(u64::from(WAVEFORM_FLOAT_HOLD_CYCLES));
		self.next_leak = self
			.hold_until
			.wrapping_add(u64::from(WAVEFORM_FLOAT_LEAK_INTERVAL_CYCLES));
	}

	fn reset(&mut self) {
		self.value = 0;
		self.charge = [0; 12];
		self.hold_until = NEVER;
		self.next_leak = NEVER;
	}

	#[inline]
	fn sample(&mut self, cycle: u64) -> u16 {
		if self.value == 0 || cycle < self.hold_until || cycle < self.next_leak {
			return self.value;
		}

		let interval = u64::from(WAVEFORM_FLOAT_LEAK_INTERVAL_CYCLES);
		let elapsed_steps = ((cycle - self.next_leak) / interval) + 1;
		self.next_leak = self
			.next_leak
			.wrapping_add(elapsed_steps.wrapping_mul(interval));

		let mut step = 0u64;
		while step < elapsed_steps {
			for bit in 0..12 {
				let charge = self.charge[bit];
				let decrement = (charge / 12).max(1);
				self.charge[bit] = charge.saturating_sub(decrement);
			}
			step += 1;
		}

		let mut retained = 0u16;
		for bit in 0..12 {
			if self.charge[bit] >= WAVEFORM_FLOAT_THRESHOLD {
				retained |= 1u16 << bit;
			}
		}

		self.value = retained & WAVEFORM_MASK;
		if self.value == 0 {
			self.next_leak = NEVER;
		}
		self.value
	}
}

/* Each oscillator combines a 24-bit phase accumulator, pulse comparator, waveform line network, floating bus, 23-stage noise generator, TEST discharge behaviour, combined-waveform feedback and readback latch. The enclosing SID supplies the modulator relationship because sync and ring modulation form a mutually observable three-voice ring rather than independent generators. */
pub struct Oscillator {
	pub accumulator: u32,
	pub frequency: u16,
	pub pulse_width: u16,
	pub waveform: u8,
	pub sync_enabled: bool,
	pub ring_enabled: bool,
	pub test_enabled: bool,
	pub msb_rising: bool,
	pub last_sample: u16,

	pulse_output: u16,
	noise: NoiseGenerator,
	cycle: u64,
	readback: u16,
	bus: FloatingBus,
}

impl Oscillator {
	pub const fn new() -> Self {
		Self {
			accumulator: PHASE_RESET_VALUE,
			frequency: 0,
			pulse_width: 0,
			waveform: 0,
			sync_enabled: false,
			ring_enabled: false,
			test_enabled: false,
			msb_rising: false,
			last_sample: WAVEFORM_PIPELINE_RESET_VALUE,
			pulse_output: WAVEFORM_MASK,
			noise: NoiseGenerator::new(),
			cycle: 0,
			readback: WAVEFORM_PIPELINE_RESET_VALUE,
			bus: FloatingBus::new(),
		}
	}

	/* Reset clears phase, control and retained waveform charge together so OSC3 and future combined-waveform evaluation restart from a defined digital state. */
	pub fn reset(&mut self) {
		let preserved = self.accumulator;
		*self = Self::new();
		self.accumulator = preserved;
	}

	#[inline]
	/* Frequency writes are immediately visible to the next accumulator clock; there is no separate shadow register. */
	pub fn write_frequency_lo(&mut self, value: u8) {
		self.frequency = (self.frequency & 0xff00) | u16::from(value);
	}

	#[inline]
	pub fn write_frequency_hi(&mut self, value: u8) {
		self.frequency = (self.frequency & 0x00ff) | (u16::from(value) << 8);
	}

	#[inline]
	pub fn write_pulse_width_lo(&mut self, value: u8) {
		self.pulse_width = (self.pulse_width & 0x0f00) | u16::from(value);
	}

	#[inline]
	/* Only the low nibble is decoded, completing the twelve-bit pulse-width comparator value. */
	pub fn write_pulse_width_hi(&mut self, value: u8) {
		self.pulse_width = (self.pulse_width & 0x00ff) | ((u16::from(value) & 0x0f) << 8);
	}

	/* CONTROL changes waveform selection and the TEST, RING and SYNC paths together. TEST resets phase, changes the electrical behaviour of the noise register and starts timed transitions when its state changes. */
	pub fn write_control(&mut self, value: u8) {
		let old_test = self.test_enabled;
		let old_wave = self.waveform;

		self.sync_enabled = value & 0x02 != 0;
		self.ring_enabled = value & 0x04 != 0;
		self.test_enabled = value & 0x08 != 0;
		self.waveform = value >> 4;

		if old_wave != self.waveform {
			match self.waveform {
				0 => self.bus.capture(self.last_sample, self.cycle),
				_ => self.bus.reset(),
			}
		}

		if !old_test && self.test_enabled {
			self.reset_phase();
			self.noise.enter_test(self.cycle);
		} else if old_test && !self.test_enabled {
			self.noise.leave_test(self.cycle);
		}
	}

	#[inline]
	/* Frequency is added once per SID clock and newly rising phase bits are captured before waveform evaluation. Bit 19 clocks the delayed noise-transfer path, while bit 23 participates in hard sync. */
	pub fn clock_accumulator(&mut self) {
		self.cycle = self.cycle.wrapping_add(1);

		if self.test_enabled {
			self.reset_phase();
			self.noise.relax(self.cycle);
			return;
		}

		self.noise.execute_transfer(self.cycle);

		let old_acc = self.accumulator;
		let new_acc = old_acc.wrapping_add(u32::from(self.frequency)) & PHASE_MASK;
		let rising = (!old_acc) & new_acc;

		self.accumulator = new_acc;
		self.msb_rising = rising & SYNC_EVENT_MASK != 0;
		self.noise.clock(rising, self.cycle);
	}

	#[inline]
	/* Combined waveforms feed resolved oscillator lines back into the noise and floating-bus networks after waveform evaluation, so the feedback affects later state rather than the code already observed this cycle. */
	pub fn apply_combined_feedback(&mut self, output: u16) {
		let selection = WaveformSelect::from_raw(self.waveform);
		if selection.has(0x02) && selection.is_combined() && output & 0x0800 == 0 {
			self.accumulator &= COMBINED_WAVEFORM_MSB_CLEAR_MASK;
			self.msb_rising = false;
		}
	}

	#[inline]
	/* Hard sync clears phase only; programmed frequency, waveform controls, TEST state and the noise generator continue from their existing state. */
	pub fn synchronise(&mut self) {
		self.reset_phase();
	}

	#[inline]
	/* A hard-sync reset changes phase only. Noise state, waveform-line charge and register programming continue independently. */
	fn reset_phase(&mut self) {
		self.accumulator = 0;
		self.msb_rising = false;
	}

	#[inline]
	/* Waveform output is evaluated from the current phase before hard-sync resets are applied. Ring modulation substitutes the modulator MSB into the triangle inversion path; pulse and noise remain driven by their own comparators and register taps. */
	pub fn evaluate_output(&mut self, modulator_acc: u32, cache: &GeneratedWaveShapes) -> u16 {
		let phase = self.phase();
		let sel = WaveformSelect::from_raw(self.waveform);

		let result = if sel.is_off() {
			self.bus.sample(self.cycle)
		} else {
			self.waveform_at_phase(phase, modulator_acc, cache)
		};

		self.pulse_output = if phase >= self.pulse_width {
			WAVEFORM_MASK
		} else {
			0
		};

		result & WAVEFORM_MASK
	}

	#[inline]
	/* The latched code feeds OSC3 reads instead of recomputing the live waveform and becomes the starting charge when the waveform drivers are later disabled. */
	pub fn latch_output(&mut self, output: u16) {
		let latched = output & WAVEFORM_MASK;
		let sel = WaveformSelect::from_raw(self.waveform);

		if self.noise_is_driven(sel) {
			self.noise.pull_down(latched, self.cycle);
		}

		self.last_sample = latched;
		self.readback = latched;
	}

	#[inline]
	pub const fn read_output(&self) -> u8 {
		(self.readback >> 4) as u8
	}

	#[inline(always)]
	pub const fn output_is_frozen(&self) -> bool {
		self.test_enabled && self.waveform == 0x04
	}

	#[inline]
	fn noise_is_driven(&self, sel: WaveformSelect) -> bool {
		sel.has(0x08) && sel.raw() & 0x07 != 0 && !self.test_enabled
	}

	#[inline]
	fn phase(&self) -> u16 {
		((self.accumulator >> 12) as u16) & WAVEFORM_MASK
	}

	#[inline]
	/* The selected drivers are evaluated from a common phase snapshot before line loading is applied. This keeps waveform combination an electrical interaction rather than an ordering-dependent arithmetic mix. */
	fn waveform_at_phase(
		&self,
		phase: u16,
		modulator_acc: u32,
		cache: &GeneratedWaveShapes,
	) -> u16 {
		let sel = WaveformSelect::from_raw(self.waveform);
		let modulator_msb_is_low = modulator_acc & SYNC_EVENT_MASK == 0;
		let ring_active = self.ring_enabled && sel.has(0x01) && !sel.has(0x02);

		let tri_phase = if ring_active && modulator_msb_is_low {
			phase ^ 0x0800
		} else {
			phase
		};

		let triangle = triangle_from_phase(tri_phase);
		let saw = phase;
		let pulse = if self.test_enabled {
			WAVEFORM_MASK
		} else {
			self.pulse_output
		};
		let noise = self.noise.output();

		let ideal = sel.combine(triangle, saw, pulse, noise, self.last_sample);

		if cache.needs_line_loading(sel.raw()) {
			cache.apply_line_loading(sel.raw(), ideal) & WAVEFORM_MASK
		} else {
			ideal & WAVEFORM_MASK
		}
	}
}

impl Default for Oscillator {
	fn default() -> Self {
		Self::new()
	}
}