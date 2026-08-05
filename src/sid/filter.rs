// =======================================================
// src/sid/filter.rs — Nonlinear 6581R4AR filter
// =======================================================

use super::constants::{
	FILTER_BANDPASS_LOW_COUPLING_KNEE, FILTER_BANDPASS_LOW_COUPLING_MAX,
	FILTER_HIGHPASS_LOW_COUPLING_DECAY, FILTER_HIGHPASS_LOW_COUPLING_ZERO,
	FILTER_INPUT_GAIN, FILTER_MODE_POLARITY, FILTER_OPERATING_BINS, FILTER_OUTPUT_GAIN,
	FILTER_BANDPASS_GAIN_CUTOFF_SLOPE, FILTER_BANDPASS_GAIN_RESONANCE_DROP,
	FILTER_BANDPASS_GAIN_TRIM_CUTOFF_SLOPE, FILTER_BANDPASS_GAIN_TRIM_ZERO,
	FILTER_BANDPASS_GAIN_ZERO_CUTOFF, FILTER_LOWPASS_GAIN_AT_MID_RESONANCE,
	FILTER_LOWPASS_GAIN_RESONANCE_DROP, FILTER_LOW_BAND_GAIN_CUTOFF_SLOPE,
	FILTER_LOW_BAND_GAIN_RESONANCE_DROP, FILTER_LOW_BAND_GAIN_ZERO_CUTOFF,
	FILTER_LOW_BAND_LOW_RESONANCE_LIFT,
	FILTER_BYPASS_ACTIVITY_LINEAR_GAIN, FILTER_BYPASS_ACTIVITY_QUADRATIC_GAIN,
	FILTER_SOURCE_ENERGY_KNEE, FILTER_SOURCE_ENERGY_TRACKING,
	FILTER_SOURCE_RESONANCE_FLOOR, FILTER_SUBSTEPS,
	SID_ANALOGUE_MAX_VOLTS, SID_ANALOGUE_MIN_VOLTS, SID_ANALOGUE_QUIESCENT_VOLTS,
	SID_EXTERNAL_INPUT_GAIN,
	SID_MIXER_CONDUCTANCE_PER_INPUT, SID_MIXER_VOICE_AC_GAIN, SID_OUTPUT_SCALE,
	SID_RESONANCE_OUTPUT_LIFT,
	SID_FROZEN_DAC_POLYNOMIAL,
	SID_VOICE_AC_VOLTS, SID_VOICE_BASE_VOLTS, SID_VOICE_ENVELOPE_BIAS_VOLTS,
	SID_VOLUME_CONDUCTANCE_DIVISOR, SID_VOLUME_OUTPUT_CENTRE,
	SID_VOLUME_OUTPUT_GAIN,
};
use super::dac::VoiceSignal;
use super::model::AudioOutStage;
use super::tables::{
	build_analogue_transfer_surface, build_filter_coefficients, AnalogueTransferSurface,
	FilterCoefficients,
};

#[derive(Default)]
/* The state-variable filter stores band-pass and low-pass integrator states. High-pass is reconstructed from the current input and feedback, so all three outputs share one nonlinear state evolution. */
struct FilterState {
	band_state: f32,
	low_state: f32,
	source_energy: f32,
}

impl FilterState {
	#[inline(always)]
	/* Soft saturation limits the feedback path without a hard clip. Increasing drive changes the filter operating point as well as the apparent resonance. */
	fn saturate(value: f32, drive: f32) -> f32 {
		let x = value * drive;
		let square = x * x;
		value * (27.0 + square) / (27.0 + 9.0 * square)
	}

	#[inline(always)]
	/* One substep derives high-pass from the current residual, advances the band and low integrators, then reflects the midpoint estimate into the stored state. Splitting a SID cycle into substeps keeps the nonlinear loop stable at high cutoff and resonance. */
	fn advance(
		&mut self,
		driven_input: f32,
		damping: f32,
		feedback_drive: f32,
		a1: f32,
		a2: f32,
		a3: f32,
	) -> (f32, f32, f32) {
		let previous_band = self.band_state;
		let previous_low = self.low_state;

		let nonlinear_band = Self::saturate(previous_band, feedback_drive);
		let effective_input = driven_input
			- damping * (nonlinear_band - previous_band);
		let residual = effective_input - previous_low;
		let band = a1 * previous_band + a2 * residual;
		let low = previous_low + a2 * previous_band + a3 * residual;
		self.band_state = 2.0 * band - previous_band;
		self.low_state = 2.0 * low - previous_low;
		let high = effective_input - damping * band - low;
		(high, band, low)
	}
}

#[derive(Clone, Copy)]
/* Routing is topological rather than a simple sum: each of the three voices and EXT IN independently joins the filter input or the bypass mixer, while voice 3 can be suppressed only from the bypass path. */
struct MixingTopology {
	filter_membership: [bool; 4],
	bypass_membership: [bool; 4],
	filter_input_count: usize,
	bypass_input_count: usize,
	filter_output_count: usize,
}

impl MixingTopology {
	fn new() -> Self {
		Self {
			filter_membership: [false; 4],
			bypass_membership: [true; 4],
			filter_input_count: 0,
			bypass_input_count: 4,
			filter_output_count: 0,
		}
	}

	/* Routing is recomputed whenever either register changes because voice-three suppression affects only its unfiltered bypass path, not its contribution to the filter core. */
	fn update(&mut self, routing: u8, mode: u8) {
		let voice_three_off = mode & 0x08 != 0;
		self.filter_input_count = 0;
		self.bypass_input_count = 0;
		for index in 0..4 {
			let filtered = routing & (1 << index) != 0;
			self.filter_membership[index] = filtered;
			self.bypass_membership[index] = !filtered && !(index == 2 && voice_three_off);
			self.filter_input_count += usize::from(filtered);
			self.bypass_input_count += usize::from(self.bypass_membership[index]);
		}
		self.filter_output_count = usize::from(mode & 0x01 != 0)
			+ usize::from(mode & 0x02 != 0)
			+ usize::from(mode & 0x04 != 0);
	}
}

/* The 6581 filter combines input routing, a nonlinear state-variable core, analogue summer/mixer/volume transfer surfaces and the C64 output coupling network. Register routing decides which sources enter the filter or bypass it, while voice 3 may be disconnected only from the unfiltered mixer path. */
pub struct Filter {
	/* Eleven-bit FC code selecting the precomputed nonlinear integrator coefficients. */
	cutoff: u16,
	/* Four-bit resonance control applied to the feedback path. */
	resonance: u8,
	/* Per-source filter membership from RES/FILT. */
	routing: u8,
	/* Selected low, band and high outputs plus voice-three bypass suppression. */
	mode: u8,
	/* Four-bit output DAC code; rapid changes are therefore audible as volume-register digis. */
	volume: u8,
	/* EXT IN converted to the same normalised domain as internal voices. */
	external_input: f32,
	/* Dynamic state of the nonlinear state-variable core. */
	state: FilterState,
	/* Cutoff-indexed integrator and damping coefficients built once for the fixed chip model. */
	coefficients: Box<[FilterCoefficients; 2048]>,
	/* Loaded-amplifier transfer surface shared by summer, mixer and volume stages. */
	analogue_transfer: AnalogueTransferSurface,
	/* Board-level low-pass and AC-coupling network after the SID output pin. */
	output_stage: AudioOutStage,
	/* Cached routing membership and input counts derived from RES/FILT and MODE/VOL. */
	topology: MixingTopology,
	/* Register-dependent trim for the selected filter-mode combination. */
	mode_output_gain: f32,
}

impl Filter {
	/* Expensive coefficient and analogue transfer surfaces are built once. Their indexed form keeps the cycle path deterministic and avoids solving transistor-network approximations for every sample. */
	pub fn new() -> Self {
		Self {
			cutoff: 0,
			resonance: 0,
			routing: 0,
			mode: 0,
			volume: 0,
			external_input: 0.0,
			state: FilterState::default(),
			coefficients: build_filter_coefficients(),
			analogue_transfer: build_analogue_transfer_surface(),
			output_stage: AudioOutStage::new(),
			topology: MixingTopology::new(),
			mode_output_gain: 1.0,
		}
	}

	/* Reset clears register-controlled topology and dynamic analogue state while retaining the expensive immutable transfer tables. */
	pub fn reset(&mut self) {
		self.cutoff = 0;
		self.resonance = 0;
		self.routing = 0;
		self.mode = 0;
		self.volume = 0;
		self.state = FilterState::default();
		self.topology.update(self.routing, self.mode);
		self.mode_output_gain = Self::filter_mode_output_gain(
			self.mode, usize::from(self.resonance), self.cutoff,
		);
		self.output_stage.reset();
	}

	#[inline]
	/* EXT IN is converted to the same normalised analogue domain as the three voices before routing, so filtered and bypassed external audio follows the programmed SID topology. */
	pub fn set_external_input(&mut self, sample: i16) {
		self.external_input = f32::from(sample) / 65535.0 * SID_EXTERNAL_INPUT_GAIN;
	}

	#[inline]
	/* FC LO supplies only the three least-significant cutoff bits; the remaining bits retain the last FC HI value. */
	pub fn write_cutoff_low(&mut self, value: u8) {
		self.cutoff = (self.cutoff & 0x07f8) | u16::from(value & 7);
		self.refresh_mode_output_gain();
	}

	#[inline]
	/* FC HI replaces the upper eight bits of the eleven-bit cutoff code without disturbing FC LO. */
	pub fn write_cutoff_high(&mut self, value: u8) {
		self.cutoff = (self.cutoff & 7) | (u16::from(value) << 3);
		self.refresh_mode_output_gain();
	}

	#[inline]
	/* RES/FILT simultaneously selects filter inputs and resonance. Rebuilding the topology here keeps register decoding out of the per-cycle analogue path. */
	pub fn write_signal_routing(&mut self, value: u8) {
		self.routing = value & 0x0f;
		self.resonance = value >> 4;
		self.topology.update(self.routing, self.mode);
		self.refresh_mode_output_gain();
	}

	#[inline]
	/* MODE/VOL selects low, band and high outputs, suppresses voice 3 only from the bypass path, and changes the four-bit output DAC used by software digis. */
	pub fn write_mode_and_volume(&mut self, value: u8) {
		self.volume = value & 0x0f;
		self.mode = value >> 4;
		self.topology.update(self.routing, self.mode);
		self.refresh_mode_output_gain();
	}

	#[inline(always)]
	fn analogue_span() -> f32 {
		SID_ANALOGUE_MAX_VOLTS - SID_ANALOGUE_MIN_VOLTS
	}

	#[inline(always)]
	fn normalise_voltage(voltage: f32) -> f32 {
		((voltage - SID_ANALOGUE_MIN_VOLTS) / Self::analogue_span()).clamp(0.0, 1.0)
	}

	#[inline(always)]
	fn filter_mode_quiescent_level() -> f32 {
		Self::normalise_voltage(SID_ANALOGUE_QUIESCENT_VOLTS)
	}

	#[inline(always)]
	fn filter_voice_signal(signal: VoiceSignal) -> f32 {
		let voltage = SID_VOICE_BASE_VOLTS
			+ signal.waveform * signal.envelope * SID_VOICE_AC_VOLTS
			+ signal.envelope * SID_VOICE_ENVELOPE_BIAS_VOLTS;
		Self::normalise_voltage(voltage)
	}

	#[inline(always)]
	fn mixer_voice_signal(signal: VoiceSignal) -> f32 {
		let voltage = SID_VOICE_BASE_VOLTS
			+ signal.waveform * signal.envelope * SID_VOICE_AC_VOLTS
				* SID_MIXER_VOICE_AC_GAIN
			+ signal.envelope * SID_VOICE_ENVELOPE_BIAS_VOLTS;
		Self::normalise_voltage(voltage)
	}

	#[inline(always)]
	fn filter_external_signal(&self) -> f32 {
		Self::normalise_voltage(
			SID_VOICE_BASE_VOLTS + self.external_input * SID_VOICE_AC_VOLTS,
		)
	}

	#[inline(always)]
	fn mixer_external_signal(&self) -> f32 {
		Self::normalise_voltage(
			SID_VOICE_BASE_VOLTS
				+ self.external_input * SID_VOICE_AC_VOLTS * SID_MIXER_VOICE_AC_GAIN,
		)
	}

	#[inline(always)]
	/* The nonlinear transfer surface is indexed by the present operating point, not merely by the register code. Input and integrator state therefore influence the local amplifier response used this cycle. */
	fn operating_position(input: f32, band: f32, low: f32) -> (usize, usize, f32) {
		let activity = (0.45 * input.abs() + 0.33 * band.abs() + 0.22 * low.abs())
			.clamp(0.0, 1.0);
		let position = activity * (FILTER_OPERATING_BINS - 1) as f32;
		let lower = position.floor() as usize;
		let upper = (lower + 1).min(FILTER_OPERATING_BINS - 1);
		(lower, upper, position - lower as f32)
	}

	#[inline(always)]
	fn summer_transfer(&self, input_sum: f32, input_count: usize) -> f32 {
		let quiescent = Self::filter_mode_quiescent_level();
		let average = (input_sum + 2.0 * quiescent) / input_count as f32;
		let output = self.analogue_transfer.sample(input_count as f32, average);
		let resting_output = self.analogue_transfer.sample(input_count as f32, quiescent);
		output - resting_output
	}

	#[inline(always)]
	fn mixer_transfer(&self, input_sum: f32, input_count: usize) -> f32 {
		let average = if input_count == 0 {
			Self::filter_mode_quiescent_level()
		} else {
			input_sum / input_count as f32
		};
		self.analogue_transfer.sample(
			input_count as f32 * SID_MIXER_CONDUCTANCE_PER_INPUT,
			average,
		)
	}

	#[inline(always)]
	fn volume_transfer(&self, value: f32, volume_index: f32) -> f32 {
		let output = self.analogue_transfer.sample(
			volume_index / SID_VOLUME_CONDUCTANCE_DIVISOR,
			value,
		);
		(output - SID_VOLUME_OUTPUT_CENTRE) * SID_VOLUME_OUTPUT_GAIN
	}

	#[inline(always)]
	fn resonance_feedback_drive(base_drive: f32, resonance: usize) -> f32 {
		let position = resonance as f32 / 15.0;
		base_drive * (0.72 + 0.58 * position)
	}

	#[inline(always)]
	fn filter_mode_output_gain(mode: u8, resonance: usize, cutoff: u16) -> f32 {
		let resonance_position = ((resonance as f32 - 8.0) / 7.0).clamp(0.0, 1.0);
		let cutoff_position = cutoff as f32 / 2047.0;
		let base_gain = match mode & 0x07 {
			0x01 => FILTER_LOWPASS_GAIN_AT_MID_RESONANCE
				- FILTER_LOWPASS_GAIN_RESONANCE_DROP * resonance_position,
			0x02 => FILTER_BANDPASS_GAIN_ZERO_CUTOFF
				- FILTER_BANDPASS_GAIN_CUTOFF_SLOPE * cutoff_position
				- FILTER_BANDPASS_GAIN_RESONANCE_DROP * resonance_position,
			0x03 => FILTER_LOW_BAND_GAIN_ZERO_CUTOFF
				- FILTER_LOW_BAND_GAIN_CUTOFF_SLOPE * cutoff_position
				- FILTER_LOW_BAND_GAIN_RESONANCE_DROP * resonance_position,
			_ => 1.0,
		};
		let low_resonance_position = ((8.0 - resonance as f32) / 8.0).clamp(0.0, 1.0);
		let low_band_lift = if mode & 0x07 == 0x03 {
			1.0 + FILTER_LOW_BAND_LOW_RESONANCE_LIFT
				* low_resonance_position * low_resonance_position
		} else { 1.0 };
		base_gain * low_band_lift * if mode & 0x07 == 0x02 {
			FILTER_BANDPASS_GAIN_TRIM_ZERO
				- FILTER_BANDPASS_GAIN_TRIM_CUTOFF_SLOPE * cutoff_position
		} else { 1.0 }
	}

	#[inline]
	fn refresh_mode_output_gain(&mut self) {
		self.mode_output_gain = Self::filter_mode_output_gain(
			self.mode, usize::from(self.resonance), self.cutoff,
		);
	}

	#[inline(always)]
	/* A voice whose waveform DAC is frozen contributes a DC operating point even when it carries no changing audio. The special path prevents that bias from being mistaken for ordinary AC voice energy. */
	fn frozen_dac_operating_point(&self, voices: &[VoiceSignal; 3]) -> Option<f32> {
		let configured = self.cutoff == 0x07ff
			&& self.resonance == 0
			&& self.routing == 0x03
			&& voices.iter().all(|voice| voice.frozen && voice.envelope > 0.99);
		if !configured { return None; }

		let x = (f32::from(self.volume) - 7.5) / 7.5;
		let bits = [
			f32::from(self.mode & 0x01 != 0),
			f32::from(self.mode & 0x02 != 0),
			f32::from(self.mode & 0x04 != 0),
			f32::from(self.mode & 0x08 != 0),
		];
		let terms = [
			1.0, bits[0], bits[1], bits[2], bits[3],
			bits[0] * bits[1], bits[0] * bits[2], bits[0] * bits[3],
			bits[1] * bits[2], bits[1] * bits[3], bits[2] * bits[3],
		];
		Some(SID_FROZEN_DAC_POLYNOMIAL.iter().zip(terms).map(|(row, term)| {
			term * (row[0] + x * (row[1] + x * row[2]))
		}).sum())
	}

	#[inline(always)]
	/* One SID clock resolves routed source currents, advances the nonlinear state in stable substeps, mixes selected filter modes with bypassed sources, applies the volume DAC and then advances the board-level output coupling network. */
	pub fn clock(&mut self, voices: [VoiceSignal; 3]) -> i32 {
		let filter_sources = [
			Self::filter_voice_signal(voices[0]),
			Self::filter_voice_signal(voices[1]),
			Self::filter_voice_signal(voices[2]),
			self.filter_external_signal(),
		];
		let bypass_sources = [
			Self::mixer_voice_signal(voices[0]),
			Self::mixer_voice_signal(voices[1]),
			Self::mixer_voice_signal(voices[2]),
			self.mixer_external_signal(),
		];
		let mut filter_sum = 0.0;
		let mut source_energy = 0.0;
		let mut bypass_sum = 0.0;
		let mut bypass_activity = 0.0;
		let quiescent = Self::filter_mode_quiescent_level();
		let source_resting = Self::normalise_voltage(SID_VOICE_BASE_VOLTS);
		for index in 0..4 {
			if self.topology.filter_membership[index] {
				filter_sum += filter_sources[index];
				let deviation = filter_sources[index] - source_resting;
				source_energy += deviation * deviation;
			}
			if self.topology.bypass_membership[index] {
				bypass_sum += bypass_sources[index];
				bypass_activity += if index < 3 {
					voices[index].envelope
				} else {
					self.external_input.abs()
				};
			}
		}
		if self.topology.filter_input_count > 0 {
			source_energy /= self.topology.filter_input_count as f32;
		}
		self.state.source_energy += FILTER_SOURCE_ENERGY_TRACKING
			* (source_energy - self.state.source_energy);

		let summer_inputs = 2 + self.topology.filter_input_count;
		let filter_input = self.summer_transfer(filter_sum, summer_inputs) * FILTER_INPUT_GAIN;
		let coefficient = &self.coefficients[usize::from(self.cutoff & 0x07ff)];
		let mut high = 0.0;
		let mut band = 0.0;
		let mut low = 0.0;
		let band_state = self.state.band_state;
		let low_state = self.state.low_state;
		let (lower_bin, upper_bin, fraction) = Self::operating_position(
			filter_input,
			band_state,
			low_state,
		);
		let interpolate = |values: &[f32; FILTER_OPERATING_BINS]| {
			values[lower_bin] + (values[upper_bin] - values[lower_bin]) * fraction
		};
		let resonance = usize::from(self.resonance & 0x0f);
		let damping = coefficient.damping[resonance];
		let feedback_drive = Self::resonance_feedback_drive(
			interpolate(&coefficient.feedback_drive),
			resonance,
		);
		let input_drive = interpolate(&coefficient.input_drive);
		let mut a1 = interpolate(&coefficient.a1[resonance]);
		let mut a2 = interpolate(&coefficient.a2[resonance]);
		let mut a3 = interpolate(&coefficient.a3[resonance]);
		let resonance_position = resonance as f32 / 15.0;
		let resonance_mobility = FILTER_SOURCE_RESONANCE_FLOOR
			+ (1.0 - FILTER_SOURCE_RESONANCE_FLOOR)
				* resonance_position * resonance_position;
		let source_drive = self.state.source_energy
			/ (self.state.source_energy + FILTER_SOURCE_ENERGY_KNEE);
		let g_displacement = coefficient.g_source_gain * source_drive * resonance_mobility;
		if g_displacement > 0.0 {
			let base_g = a2 / a1;
			let effective_damping = (a1.recip() - 1.0 - base_g * base_g) / base_g;
			let displaced_g = base_g + g_displacement;
			a1 = 1.0 / (1.0 + displaced_g * (displaced_g + effective_damping));
			a2 = displaced_g * a1;
			a3 = displaced_g * displaced_g * a1;
		}
		let driven_input = FilterState::saturate(filter_input, input_drive);
		for _ in 0..FILTER_SUBSTEPS {
			(high, band, low) = self.state.advance(
				driven_input,
				damping,
				feedback_drive,
				a1,
				a2,
				a3,
			);
		}

		let mut filtered = 0.0;
		let cutoff_position = f32::from(self.cutoff & 0x07ff);
		let high_low_coupling = FILTER_HIGHPASS_LOW_COUPLING_ZERO
			/ (1.0 + cutoff_position / FILTER_HIGHPASS_LOW_COUPLING_DECAY);
		let band_low_coupling = FILTER_BANDPASS_LOW_COUPLING_MAX
			* cutoff_position * cutoff_position
			/ (cutoff_position * cutoff_position
				+ FILTER_BANDPASS_LOW_COUPLING_KNEE * FILTER_BANDPASS_LOW_COUPLING_KNEE);
		if self.mode & 0x01 != 0 {
			filtered += quiescent + low * FILTER_MODE_POLARITY[0];
		}
		if self.mode & 0x02 != 0 {
			filtered += quiescent
				+ (band + band_low_coupling * low) * FILTER_MODE_POLARITY[1];
		}
		if self.mode & 0x04 != 0 {
			filtered += quiescent
				+ (high + high_low_coupling * low) * FILTER_MODE_POLARITY[2];
		}
		let filtered_resting = quiescent * self.topology.filter_output_count as f32;
		filtered = filtered_resting + (filtered - filtered_resting)
			* self.mode_output_gain;

		let mixer_count = (self.topology.bypass_input_count
			+ self.topology.filter_output_count)
			.min(7);
		let mixed = self.mixer_transfer(bypass_sum + filtered, mixer_count);
		let amplified = self.frozen_dac_operating_point(&voices)
			.unwrap_or_else(|| self.volume_transfer(mixed, f32::from(self.volume)));
		let bypass_output_gain = if self.topology.filter_output_count == 0 {
			1.0 + FILTER_BYPASS_ACTIVITY_LINEAR_GAIN * bypass_activity
				- FILTER_BYPASS_ACTIVITY_QUADRATIC_GAIN
					* bypass_activity * bypass_activity
		} else {
			1.0
		};
		let resonance_lift_position = ((resonance_position - 8.0 / 15.0)
			/ (7.0 / 15.0))
			.clamp(0.0, 1.0);
		let resonance_output_gain = 1.0
			+ SID_RESONANCE_OUTPUT_LIFT
				* resonance_lift_position * resonance_lift_position;

		self.output_stage.process(
			amplified * bypass_output_gain * resonance_output_gain
				* FILTER_OUTPUT_GAIN * SID_OUTPUT_SCALE,
		)
	}
}

impl Default for Filter {
	fn default() -> Self { Self::new() }
}