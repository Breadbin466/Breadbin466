// =======================================================
// src/sid/sid.rs — SID chip core
// =======================================================

/* Cycle-level MOS 6581 core and register interface. */

use super::bus::InternalDataBus;
use super::constants::REGISTER_MASK;
use super::dac::VoiceLevelConverter;
use super::envelope::Envelope;
use super::filter::Filter;
use super::oscillator::Oscillator;
use super::waveforms::GeneratedWaveShapes;

/* The core advances one SID clock at a time. Each cycle first ages the internal data bus, then clocks all three phase accumulators and envelope generators, evaluates the waveform ring, applies oscillator synchronisation, converts the digital voice codes through the modelled DACs, and finally advances the shared filter and C64 output stage. Keeping that order explicit preserves the chip-visible relationships between phase, sync, envelope and analogue output. */
pub struct Mos6581 {
	/* Oscillators and envelopes remain public because the inspector exposes chip state directly; normal emulation still reaches them only through SID register accesses. */
	pub oscillators: [Oscillator; 3],
	pub envelopes: [Envelope; 3],
	waveforms: GeneratedWaveShapes,
	voice_converter: VoiceLevelConverter,
	filter: Filter,
	data_bus: InternalDataBus,
	/* Last fully filtered cycle-domain sample, retained for inspection and reset bookkeeping. */
	last_sample: i32,
	/* Rendering may be suppressed without freezing chip time, while clocking_enabled stops the SID entirely for machine-level pause and reset control. */
	pub rendering_enabled: bool,
	pub clocking_enabled: bool,
	/* POTX and POTY are externally supplied conversion results; the SID core only returns their most recently latched values. */
	pub pot_x: u8,
	pub pot_y: u8,
}

impl Mos6581 {
	/* Builds the fixed 6581R4AR/PAL signal path used by Breadbin466: three oscillators, three envelopes, one shared filter, the internal data bus and the board-level output coupling stage. */
	pub fn new() -> Self {
		Self {
			oscillators: std::array::from_fn(|_| Oscillator::new()),
			envelopes: std::array::from_fn(|_| Envelope::new()),
			waveforms: GeneratedWaveShapes::new(),
			voice_converter: VoiceLevelConverter::new(),
			filter: Filter::new(),
			data_bus: InternalDataBus::new(),
			last_sample: 0,
			rendering_enabled: true,
			clocking_enabled: true,
			pot_x: 0xff,
			pot_y: 0xff,
		}
	}

	/* Reset clears the digital control state and the filter history without recreating the precomputed waveform, DAC and filter tables. */
	pub fn reset(&mut self) {
		for oscillator in &mut self.oscillators {
			oscillator.reset();
		}
		for envelope in &mut self.envelopes {
			envelope.reset();
		}
		self.filter.reset();
		self.data_bus.reset();
		self.last_sample = 0;
	}

	/* One call represents one PHI2 cycle. Clocking may continue while rendering is disabled so oscillator phase, sync and envelopes remain temporally correct during muted or fast-forward operation. */
	#[inline(always)]
	pub fn tick(&mut self) -> Option<i32> {
		if !self.clocking_enabled {
			return None;
		}
		self.data_bus.clock();

		let [oscillator_0, oscillator_1, oscillator_2] = &mut self.oscillators;
		oscillator_0.clock_accumulator();
		oscillator_1.clock_accumulator();
		oscillator_2.clock_accumulator();

		let [envelope_0, envelope_1, envelope_2] = &mut self.envelopes;
		envelope_0.clock();
		envelope_1.clock();
		envelope_2.clock();

		/* Voice evaluation follows the physical modulation ring: voice 3 modulates voice 1, voice 1 modulates voice 2, and voice 2 modulates voice 3. The newly evaluated code is also fed back into combined-waveform and noise-line state before being latched for reads. */
		let waveform_0 = oscillator_0.evaluate_output(oscillator_2.accumulator, &self.waveforms);
		oscillator_0.apply_combined_feedback(waveform_0);
		oscillator_0.latch_output(waveform_0);

		let waveform_1 = oscillator_1.evaluate_output(oscillator_0.accumulator, &self.waveforms);
		oscillator_1.apply_combined_feedback(waveform_1);
		oscillator_1.latch_output(waveform_1);

		let waveform_2 = oscillator_2.evaluate_output(oscillator_1.accumulator, &self.waveforms);
		oscillator_2.apply_combined_feedback(waveform_2);
		oscillator_2.latch_output(waveform_2);

		let rising_0 = oscillator_0.msb_rising;
		let rising_1 = oscillator_1.msb_rising;
		let rising_2 = oscillator_2.msb_rising;
		let sync_0 = oscillator_0.sync_enabled;
		let sync_1 = oscillator_1.sync_enabled;
		let sync_2 = oscillator_2.sync_enabled;

		if rising_0 && sync_1 && !(sync_0 && rising_2) {
			oscillator_1.synchronise();
		}
		if rising_1 && sync_2 && !(sync_1 && rising_0) {
			oscillator_2.synchronise();
		}
		if rising_2 && sync_0 && !(sync_2 && rising_1) {
			oscillator_0.synchronise();
		}

		/* Host muting suppresses only analogue rendering. Waveform feedback
		 * and readable OSC3 state remain clocked with the machine. */
		if !self.rendering_enabled {
			return None;
		}

		let level_0 = envelope_0.volume;
		let level_1 = envelope_1.volume;
		let level_2 = envelope_2.volume;
		let voice_0 = self.voice_converter.output(
			waveform_0,
			level_0,
			oscillator_0.output_is_frozen(),
			oscillator_0.waveform,
		);
		let voice_1 = self.voice_converter.output(
			waveform_1,
			level_1,
			oscillator_1.output_is_frozen(),
			oscillator_1.waveform,
		);
		let voice_2 = self.voice_converter.output(
			waveform_2,
			level_2,
			oscillator_2.output_is_frozen(),
			oscillator_2.waveform,
		);

		self.last_sample = self.filter.clock([voice_0, voice_1, voice_2]);
		Some(self.last_sample)
	}

	/* Feeds the EXT IN pin into the same routing and filter topology as the three internal voices. EXT IN is sampled into the filter path independently of register writes and is consumed on subsequent SID clocks. */
	pub fn input(&mut self, sample: i16) {
		self.filter.set_external_input(sample);
	}

	/* Every write drives the SID data bus before the addressed register consumes the value. The register file repeats every 32 bytes, matching the five decoded address lines (C64-PRG-1982, 6581 register map). */
	#[inline]
	pub fn write(&mut self, address: u16, value: u8) {
		self.data_bus.drive(value);
		match (address & REGISTER_MASK) as u8 {
			0x00 => self.oscillators[0].write_frequency_lo(value),
			0x01 => self.oscillators[0].write_frequency_hi(value),
			0x02 => self.oscillators[0].write_pulse_width_lo(value),
			0x03 => self.oscillators[0].write_pulse_width_hi(value),
			0x04 => self.write_voice_control(0, value),
			0x05 => self.envelopes[0].set_attack_decay(value),
			0x06 => self.envelopes[0].set_sustain_release(value),
			0x07 => self.oscillators[1].write_frequency_lo(value),
			0x08 => self.oscillators[1].write_frequency_hi(value),
			0x09 => self.oscillators[1].write_pulse_width_lo(value),
			0x0a => self.oscillators[1].write_pulse_width_hi(value),
			0x0b => self.write_voice_control(1, value),
			0x0c => self.envelopes[1].set_attack_decay(value),
			0x0d => self.envelopes[1].set_sustain_release(value),
			0x0e => self.oscillators[2].write_frequency_lo(value),
			0x0f => self.oscillators[2].write_frequency_hi(value),
			0x10 => self.oscillators[2].write_pulse_width_lo(value),
			0x11 => self.oscillators[2].write_pulse_width_hi(value),
			0x12 => self.write_voice_control(2, value),
			0x13 => self.envelopes[2].set_attack_decay(value),
			0x14 => self.envelopes[2].set_sustain_release(value),
			0x15 => self.filter.write_cutoff_low(value),
			0x16 => self.filter.write_cutoff_high(value),
			0x17 => self.filter.write_signal_routing(value),
			0x18 => self.filter.write_mode_and_volume(value),
			_ => {}
		}
	}

	#[inline(always)]
	/* A voice control write reaches oscillator and envelope in the same bus transaction because TEST, SYNC, RING, waveform selection and GATE share one physical register. */
	fn write_voice_control(&mut self, voice: usize, value: u8) {
		if value & 0xf1 == 0x41 {
			self.filter.mark_pure_pulse_gate_rise();
		}
		if value & 0xf1 == 0x21 {
			self.filter.mark_pure_saw_gate_rise(voice);
		}
		self.oscillators[voice].write_control(value);
		self.envelopes[voice].set_gate(value & 0x01 != 0);
	}

	/* Only POTX, POTY, OSC3 and ENV3 actively drive read data. Other addresses expose the decaying internal data bus left by earlier CPU accesses (C64-PRG-1982, SID readable registers). */
	#[inline]
	pub fn read(&mut self, address: u16) -> u8 {
		let direct = match address & REGISTER_MASK {
			0x19 => Some(self.pot_x),
			0x1a => Some(self.pot_y),
			0x1b => Some(self.oscillators[2].read_output()),
			0x1c => Some(self.envelopes[2].read_level()),
			_ => None,
		};
		if let Some(value) = direct {
			self.data_bus.read_driven(value)
		} else {
			self.data_bus.read_floating()
		}
	}
}

impl Default for Mos6581 {
	fn default() -> Self {
		Self::new()
	}
}