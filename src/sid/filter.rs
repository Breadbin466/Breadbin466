// =======================================================
// src/sid/filter.rs — SID nonlinear filter
// =======================================================

/*
 * Nonlinear 6581 filter orchestration.
 *
 * This module owns register state, precomputed transfer tables and the cycle
 * path that combines routing, nonlinear integration, analogue mixing, the
 * volume DAC and the C64 output stage. Static response laws, routing topology
 * and integrator mechanics live in focused sibling modules.
 */

use super::constants::{
	COMBINED_OUTPUT_COMMON_MODE, FILTER_BANDPASS_LOW_COUPLING_KNEE,
	FILTER_BANDPASS_LOW_COUPLING_MAX, FILTER_BYPASS_ACTIVITY_LINEAR_GAIN,
	FILTER_BYPASS_ACTIVITY_QUADRATIC_GAIN, FILTER_CUTOFF_STEP_CHARGE_GAIN,
	FILTER_CUTOFF_STEP_CHARGE_LIMIT_CODES, FILTER_DIRECTIONAL_LOADING_GAIN,
	FILTER_INPUT_COMMON_MODE_ONE_VOICE, FILTER_HIGHPASS_LOW_COUPLING_DECAY,
	FILTER_HIGHPASS_LOW_COUPLING_ZERO, FILTER_INPUT_GAIN, FILTER_MODE_POLARITY, FILTER_OPERATING_BINS,
	FILTER_OUTPUT_GAIN, FILTER_SOURCE_ENERGY_KNEE, FILTER_SOURCE_ENERGY_TRACKING,
	FILTER_SOURCE_RESONANCE_FLOOR, FILTER_SUBSTEPS, FILTER_SYMMETRIC_LOADING_GAIN,
	PURE_BYPASS_ATTACK_COMMON_MODE_ONE_VOICE, PURE_BYPASS_ATTACK_COMMON_MODE_SCALE,
	PURE_BYPASS_ATTACK_COMMON_MODE_THREE_VOICE_DELTA, PURE_BYPASS_ATTACK_COMMON_MODE_TWO_VOICE_DELTA,
	PURE_BYPASS_OUTPUT_COMMON_MODE, PURE_PULSE_GATE_RELAXATION_CHARGE,
	SID_MIXER_CONDUCTANCE_PER_INPUT, SID_MIXER_THREE_VOICE_ACTIVITY_LIFT,
	SID_MIXER_TWO_VOICE_ACTIVITY_LOAD, SID_MIXER_TWO_VOICE_VOLUME_GAIN_AT_CODE_9,
	SID_MIXER_TWO_VOICE_VOLUME_GAIN_PER_CODE, SID_OUTPUT_SCALE,
	SID_RESONANCE_OUTPUT_LIFT, SID_VOICE_BASE_VOLTS,
	SID_VOLUME_CONDUCTANCE_DIVISOR, SID_VOLUME_DAC_BIAS_FULL, SID_VOLUME_OUTPUT_CENTRE,
	SID_VOLUME_OUTPUT_GAIN, SID_VOLUME_TRANSITION_FAST_CHARGE_FULL_SCALE,
	SID_VOLUME_TRANSITION_SLOW_CHARGE_FULL_SCALE,
};
use super::control::{ControlCache, FilterRegisters};
use super::dac::VoiceSignal;
use super::integrator::{FilterOutputs, IntegratorState};
use super::mixing::{ExternalInputState, MixingTopology, RoutedSignals, frozen_dac_operating_point};
use super::response::{
	filter_mode_output_gain, filter_mode_quiescent_level, filter_output_common_mode, normalise_voltage,
	operating_position,
	volume_dac_conductance_code,
};
use super::model::AudioOutStage;
use super::tables::{
	AnalogueTransferSurface, FilterCoefficients, TransferCurves, build_analogue_transfer_surface,
	build_filter_coefficients,
};
use super::transients::TransientState;

/* The 6581 filter combines input routing, a nonlinear state-variable core, analogue summer/mixer/volume transfer surfaces and the C64 output coupling network. Register routing decides which sources enter the filter or bypass it, while voice 3 may be disconnected only from the unfiltered mixer path. */
pub struct Filter {
	registers: FilterRegisters,
	transients: TransientState,
	external: ExternalInputState,
	state: IntegratorState,
	coefficients: Box<[FilterCoefficients; 2048]>,
	curves: TransferCurves,
	output_stage: AudioOutStage,
	topology: MixingTopology,
	control: ControlCache,
}

impl Filter {
	/* Expensive coefficient and analogue transfer surfaces are built once. Their indexed form keeps the cycle path deterministic and avoids solving transistor-network approximations for every sample. */
	pub fn new() -> Self {
		let analogue_transfer = build_analogue_transfer_surface();
		let quiescent = filter_mode_quiescent_level();
		let summer_resting_output = std::array::from_fn(|count| {
			if count >= 2 {
				analogue_transfer.sample(count as f32, quiescent)
			} else {
				0.0
			}
		});
		let topology = MixingTopology::new();
		let summer_curves = std::array::from_fn(|count| {
			analogue_transfer.build_load_curve(AnalogueTransferSurface::load_position(count as f32))
		});
		let mixer_curves = std::array::from_fn(|count| {
			analogue_transfer.build_load_curve(AnalogueTransferSurface::load_position(
				count as f32 * SID_MIXER_CONDUCTANCE_PER_INPUT,
			))
		});
		let volume_curves = std::array::from_fn(|volume| {
			let conductance = volume_dac_conductance_code(volume as u8)
				/ SID_VOLUME_CONDUCTANCE_DIVISOR;
			let mut curve =
				analogue_transfer.build_load_curve(AnalogueTransferSurface::load_position(conductance));
			let volume_fraction = volume as f32 / 15.0;
			let volume_dac_bias = SID_VOLUME_DAC_BIAS_FULL * volume_fraction;
			for entry in curve.iter_mut() {
				*entry = (*entry - SID_VOLUME_OUTPUT_CENTRE + volume_dac_bias)
					* SID_VOLUME_OUTPUT_GAIN;
			}
			curve
		});
		let coefficients = build_filter_coefficients();
		let control = ControlCache::new(&coefficients[0]);
		Self {
			registers: FilterRegisters::new(),
			transients: TransientState::new(),
			external: ExternalInputState::new(),
			state: IntegratorState::default(),
			coefficients,
			curves: TransferCurves {
				summer: summer_curves,
				mixer: mixer_curves,
				volume: volume_curves,
				summer_resting_output,
			},
			output_stage: AudioOutStage::new(),
			topology,
			control,
		}
	}

	/* Reset clears register-controlled topology and dynamic analogue state while retaining the expensive immutable transfer tables. */
	pub fn reset(&mut self) {
		self.registers.reset();
		self.transients.reset();
		self.state = IntegratorState::default();
		self.topology.update(self.registers.routing, self.registers.mode);
		self.refresh_filter_controls();
		self.control.mode_output_gain =
			filter_mode_output_gain(self.registers.mode, usize::from(self.registers.resonance), self.registers.cutoff);
		self.refresh_register_output_state();
		self.output_stage.reset();
	}

	#[inline]
	/* EXT IN is converted to the same normalised analogue domain as the three voices before routing, so filtered and bypassed external audio follow the programmed SID topology. */
	pub fn set_external_input(&mut self, sample: i16) {
		self.external.set_sample(sample);
	}

	/* CONTROL reaches the SID before the following MODE/VOL write. Deferring
	 * excitation until the pulse voice is clocked at nonzero volume preserves
	 * that bus ordering while retaining an explicit logical GATE edge. */
	pub fn mark_pure_pulse_gate_rise(&mut self) {
		self.transients.pulse_gate_pending = true;
	}

	pub fn mark_pure_saw_gate_rise(&mut self, voice: usize) {
		self.transients.saw_mixer_blend[voice] = 0.0;
	}

	#[inline]
	/* FC LO supplies only the three least-significant cutoff bits; the remaining bits retain the last FC HI value. */
	pub fn write_cutoff_low(&mut self, value: u8) {
		self.registers.cutoff_write_base = self.registers.cutoff;
		self.registers.cutoff_low_pending = true;
		self.registers.cutoff = (self.registers.cutoff & 0x07f8) | u16::from(value & 7);
		self.refresh_filter_controls();
		self.refresh_mode_output_gain();
		self.refresh_register_output_state();
	}

	#[inline]
	/* FC HI replaces the upper eight bits of the eleven-bit cutoff code without disturbing FC LO. */
	pub fn write_cutoff_high(&mut self, value: u8) {
		let previous = if self.registers.cutoff_low_pending {
			self.registers.cutoff_write_base
		} else {
			self.registers.cutoff
		};
		let next = (self.registers.cutoff & 7) | (u16::from(value) << 3);
		let step = i32::from(next) - i32::from(previous);
		self.registers.cutoff_dac_displacement_codes = (step as f32 * FILTER_CUTOFF_STEP_CHARGE_GAIN)
			.round()
			.clamp(
				-f32::from(FILTER_CUTOFF_STEP_CHARGE_LIMIT_CODES),
				f32::from(FILTER_CUTOFF_STEP_CHARGE_LIMIT_CODES),
			) as i16;
		self.registers.cutoff = next;
		self.registers.cutoff_low_pending = false;
		self.refresh_filter_controls();
		self.refresh_mode_output_gain();
		self.refresh_register_output_state();
	}

	#[inline]
	/* RES/FILT simultaneously selects filter inputs and resonance. Rebuilding the topology here keeps register decoding out of the per-cycle analogue path. */
	pub fn write_signal_routing(&mut self, value: u8) {
		self.registers.routing = value & 0x0f;
		self.registers.resonance = value >> 4;
		self.topology.update(self.registers.routing, self.registers.mode);
		self.refresh_filter_controls();
		self.refresh_mode_output_gain();
		self.refresh_register_output_state();
	}

	#[inline]
	/* MODE/VOL selects low, band and high outputs, suppresses voice 3 only from the bypass path, and changes the four-bit output DAC used by software digis. */
	pub fn write_mode_and_volume(&mut self, value: u8) {
		let previous_conductance = self.transients.volume_conductance_code;
		self.registers.volume = value & 0x0f;
		self.transients.volume_conductance_code = volume_dac_conductance_code(self.registers.volume);
		let full_conductance = volume_dac_conductance_code(15);
		let conductance_step =
			(previous_conductance - self.transients.volume_conductance_code) / full_conductance;
		self.transients.volume_fast_charge +=
			conductance_step * SID_VOLUME_TRANSITION_FAST_CHARGE_FULL_SCALE;
		self.transients.volume_slow_charge +=
			conductance_step * SID_VOLUME_TRANSITION_SLOW_CHARGE_FULL_SCALE;
		self.registers.mode = value >> 4;
		self.topology.update(self.registers.routing, self.registers.mode);
		self.refresh_mode_output_gain();
		self.refresh_register_output_state();
	}

	#[inline(always)]
	fn summer_transfer(&self, input_sum: f32) -> f32 {
		let input_count = 2 + self.topology.filter_input_count;
		let quiescent = filter_mode_quiescent_level();
		let average =
			(input_sum + 2.0 * quiescent) * self.topology.summer_input_reciprocal;
		let output = AnalogueTransferSurface::sample_curve(&self.curves.summer[input_count], average);
		output - self.curves.summer_resting_output[input_count]
	}

	#[inline(always)]
	fn mixer_transfer(&self, input_sum: f32) -> f32 {
		let input_count = self.topology.mixer_input_count;
		let average = if input_count == 0 {
			filter_mode_quiescent_level()
		} else {
			input_sum * self.topology.mixer_input_reciprocal
		};
		AnalogueTransferSurface::sample_curve(&self.curves.mixer[input_count], average)
	}

	#[inline(always)]
	fn volume_transfer(&self, value: f32) -> f32 {
		AnalogueTransferSurface::sample_curve(
			&self.curves.volume[usize::from(self.registers.volume)],
			value,
		)
	}

	#[inline]
	/* MODE/VOL, RES/FILT and FC writes are sparse compared with SID clocks.
	 * Cache terms that depend only on those registers so the analogue hot path
	 * performs only genuinely cycle-varying work. */
	fn refresh_register_output_state(&mut self) {
		self.control.filter_common_mode =
			filter_output_common_mode(self.registers.mode, self.registers.cutoff);
		self.control.volume_fraction = f32::from(self.registers.volume) / 15.0;
		self.control.two_voice_volume_gain_offset = SID_MIXER_TWO_VOICE_VOLUME_GAIN_AT_CODE_9 - 1.0
			+ SID_MIXER_TWO_VOICE_VOLUME_GAIN_PER_CODE * (f32::from(self.registers.volume) - 9.0);
		self.control.combined_common_mode_gain = COMBINED_OUTPUT_COMMON_MODE * self.control.volume_fraction;
		self.control.pure_bypass_common_mode_gain = PURE_BYPASS_OUTPUT_COMMON_MODE * self.control.volume_fraction;
		self.control.scaled_filter_common_mode =
			self.control.filter_common_mode * self.control.volume_fraction;
		self.control.frozen_dac_bias_mode =
			self.registers.cutoff == 0x07ff && self.registers.resonance == 0 && self.registers.routing == 0x03;
	}

	#[inline]
	fn effective_cutoff(&self) -> u16 {
		let displaced = self.registers.cutoff as i16 + self.registers.cutoff_dac_displacement_codes;
		displaced.clamp(0, 0x07ff) as u16
	}

	#[inline]
	fn refresh_filter_controls(&mut self) {
		let cutoff = self.effective_cutoff();
		self.control.effective_cutoff_index = usize::from(cutoff);
		let position = f32::from(cutoff);
		self.control.high_pass_low_coupling = FILTER_HIGHPASS_LOW_COUPLING_ZERO
			/ (1.0 + position / FILTER_HIGHPASS_LOW_COUPLING_DECAY);
		let square = position * position;
		self.control.band_pass_low_coupling = FILTER_BANDPASS_LOW_COUPLING_MAX * square
			/ (square + FILTER_BANDPASS_LOW_COUPLING_KNEE * FILTER_BANDPASS_LOW_COUPLING_KNEE);
		self.control.resonance_index = usize::from(self.registers.resonance & 0x0f);
		let coefficient = &self.coefficients[self.control.effective_cutoff_index];
		self.control.integrator_gain = coefficient.integrator_gain[self.control.resonance_index];
		self.control.input_drive = coefficient.input_drive;
		self.control.feedback_drive = coefficient.feedback_drive;
		self.control.damping = coefficient.damping[self.control.resonance_index];
		self.control.source_gain_modulation = coefficient.source_gain_modulation;
		self.control.resonance_position = self.control.resonance_index as f32 / 15.0;
		self.control.resonance_feedback_scale = 0.78 + 0.30 * self.control.resonance_position;
		self.control.source_modulation_resonance_scale = FILTER_SOURCE_RESONANCE_FLOOR
			+ (1.0 - FILTER_SOURCE_RESONANCE_FLOOR)
				* self.control.resonance_position
				* self.control.resonance_position;
		let lift_position = ((self.control.resonance_position - 8.0 / 15.0) / (7.0 / 15.0)).clamp(0.0, 1.0);
		self.control.resonance_output_gain =
			1.0 + SID_RESONANCE_OUTPUT_LIFT * lift_position * lift_position;
	}

	#[inline]
	fn refresh_mode_output_gain(&mut self) {
		let base = filter_mode_output_gain(
			self.registers.mode,
			usize::from(self.registers.resonance),
			self.effective_cutoff(),
		);
		let directional = match self.registers.mode & 0x07 {
			0x01 | 0x02 | 0x04 => {
				let mode_index = match self.registers.mode & 0x07 {
					0x01 => 0,
					0x02 => 1,
					_ => 2,
				};
				let position = self.registers.cutoff as f32 * 31.0 / 2047.0;
				let lower = position.floor() as usize;
				let upper = (lower + 1).min(31);
				let fraction = position - lower as f32;
				let curve = &FILTER_DIRECTIONAL_LOADING_GAIN[mode_index];
				let directional_gain = curve[lower] + (curve[upper] - curve[lower]) * fraction;
				let symmetric_curve = &FILTER_SYMMETRIC_LOADING_GAIN[mode_index];
				let symmetric_gain = symmetric_curve[lower]
					+ (symmetric_curve[upper] - symmetric_curve[lower]) * fraction;
				let displacement = self.registers.cutoff_dac_displacement_codes as f32
					/ FILTER_CUTOFF_STEP_CHARGE_LIMIT_CODES as f32;
				symmetric_gain * directional_gain.powf(displacement)
			}
			_ => 1.0,
		};
		self.control.mode_output_gain = base * directional;
	}

	#[inline(always)]
	fn advance_filter_core(&mut self, signals: &mut RoutedSignals) -> FilterOutputs {
		let idle = self.topology.filter_input_count == 0
			&& self.state.filter_source_energy == 0.0
			&& self.state.band_state == 0.0
			&& self.state.low_state == 0.0;
		if idle {
			return FilterOutputs::default();
		}

		if self.topology.filter_input_count > 0 {
			signals.filter_source_energy *= self.topology.filter_input_reciprocal;
		}
		self.state.filter_source_energy +=
			FILTER_SOURCE_ENERGY_TRACKING * (signals.filter_source_energy - self.state.filter_source_energy);

		let filter_input = self.summer_transfer(signals.filter_input_sum) * FILTER_INPUT_GAIN;
		let (lower_bin, upper_bin, fraction) =
			operating_position(filter_input, self.state.band_state, self.state.low_state);
		let interpolate = |values: &[f32; FILTER_OPERATING_BINS]| {
			values[lower_bin] + (values[upper_bin] - values[lower_bin]) * fraction
		};
		let damping = self.control.damping;
		let feedback_drive =
			interpolate(&self.control.feedback_drive) * self.control.resonance_feedback_scale;
		let input_drive = interpolate(&self.control.input_drive);
		/* Interpolate the pole parameter directly, then apply source loading before
		 * constructing the trapezoidal integrator coefficients. Resonance already
		 * supplies the damping; recovering it from rounded coefficients is
		 * ill-conditioned at low cutoff and adds redundant divisions. */
		let source_energy_drive = self.state.filter_source_energy
			/ (self.state.filter_source_energy + FILTER_SOURCE_ENERGY_KNEE);
		let shift = self.control.source_gain_modulation * source_energy_drive
			* self.control.source_modulation_resonance_scale;
		let g = interpolate(&self.control.integrator_gain) + shift;
		let a1 = 1.0 / (1.0 + g * (g + damping));
		let a2 = g * a1;
		let a3 = g * g * a1;

		let driven_input = IntegratorState::saturate(filter_input, input_drive);
		let mut outputs = FilterOutputs::default();
		for _ in 0..FILTER_SUBSTEPS {
			outputs = self.state.advance(driven_input, damping, feedback_drive, a1, a2, a3);
		}
		outputs
	}

	#[inline(always)]
	fn select_filter_output(&self, outputs: FilterOutputs) -> f32 {
		let quiescent = filter_mode_quiescent_level();
		let low = quiescent + outputs.low_pass * FILTER_MODE_POLARITY[0];
		let band = quiescent
			+ (outputs.band_pass + self.control.band_pass_low_coupling * outputs.low_pass)
				* FILTER_MODE_POLARITY[1];
		let high = quiescent
			+ (outputs.high_pass + self.control.high_pass_low_coupling * outputs.low_pass)
				* FILTER_MODE_POLARITY[2];
		let sum = match self.registers.mode & 0x07 {
			0x00 => 0.0,
			0x01 => low,
			0x02 => band,
			0x03 => low + band,
			0x04 => high,
			0x05 => low + high,
			0x06 => band + high,
			_ => low + band + high,
		};
		let resting = quiescent * self.topology.filter_output_count as f32;
		resting
			+ (sum - resting)
				* self.control.mode_output_gain
				* self.control.resonance_output_gain
	}

	#[inline(always)]
	/* One SID clock resolves routed source currents, advances the nonlinear state in stable substeps, mixes selected filter modes with bypassed sources, applies the volume DAC and then advances the board-level output coupling network. */
	pub fn clock(&mut self, voices: [VoiceSignal; 3]) -> i32 {
		for (blend, voice) in self.transients.saw_mixer_blend.iter_mut().zip(voices.iter()) {
			let active = voice.waveform_selection == 2 && voice.envelope > 0.02;
			let target = f32::from(active);
			*blend += self.transients.saw_mixer_tracking * (target - *blend);
		}
		let combined_activity: f32 = voices
			.iter()
			.filter(|voice| voice.combined)
			.map(|voice| voice.envelope)
			.sum();
		let source_resting = normalise_voltage(SID_VOICE_BASE_VOLTS);
		let mut signals = RoutedSignals::collect(
			&self.topology,
			&voices,
			&self.transients.saw_mixer_blend,
			self.external.filter_signal,
			self.external.mixer_signal,
			self.external.raw,
			source_resting,
		);
		let filter_outputs = self.advance_filter_core(&mut signals);
		let filter_output = self.select_filter_output(filter_outputs);

		let raw_mixer_output = self.mixer_transfer(signals.bypass_input_sum + filter_output);
		let bypass_is_quiescent =
			self.topology.bypass_internal_mask == 0 && self.external.raw == 0.0;
		let mixer_output = if bypass_is_quiescent {
			raw_mixer_output
		} else {
			let mixer_quiescent = filter_mode_quiescent_level();
			let two_voice_activity =
				(1.0 - (signals.bypass_envelope_activity - 2.0) * (signals.bypass_envelope_activity - 2.0)).clamp(0.0, 1.0);
			let three_voice_activity = (signals.bypass_envelope_activity - 2.0).clamp(0.0, 1.0);
			let mixer_activity_gain = 1.0 - SID_MIXER_TWO_VOICE_ACTIVITY_LOAD * two_voice_activity
				+ SID_MIXER_THREE_VOICE_ACTIVITY_LIFT * three_voice_activity;
			let two_voice_volume_gain =
				1.0 + two_voice_activity * self.control.two_voice_volume_gain_offset;
			mixer_quiescent
				+ (raw_mixer_output - mixer_quiescent) * mixer_activity_gain * two_voice_volume_gain
		};
		let volume_stage_output = if self.control.frozen_dac_bias_mode {
			frozen_dac_operating_point(self.control.frozen_dac_bias_mode, self.registers.mode, self.registers.volume, &voices)
				.unwrap_or_else(|| self.volume_transfer(mixer_output))
		} else {
			self.volume_transfer(mixer_output)
		};
		let bypass_output_gain = if self.topology.filter_output_count == 0 {
			1.0 + FILTER_BYPASS_ACTIVITY_LINEAR_GAIN * signals.bypass_envelope_activity
				- FILTER_BYPASS_ACTIVITY_QUADRATIC_GAIN * signals.bypass_envelope_activity * signals.bypass_envelope_activity
		} else {
			1.0
		};
		let volume_fraction = self.control.volume_fraction;
		if self.topology.bypass_internal_mask == 0 {
			self.transients.previous_bypass_activity = 0.0;
		} else {
			let rising_pure_bypass_activity =
				signals.pure_bypass_envelope_activity > self.transients.previous_bypass_activity;
			if rising_pure_bypass_activity {
				let pure_two_voice_activity =
					(1.0 - (signals.pure_bypass_envelope_activity - 2.0) * (signals.pure_bypass_envelope_activity - 2.0))
						.clamp(0.0, 1.0);
				let pure_three_voice_activity = (signals.pure_bypass_envelope_activity - 2.0).clamp(0.0, 1.0);
				let load_coefficient = PURE_BYPASS_ATTACK_COMMON_MODE_ONE_VOICE
					+ PURE_BYPASS_ATTACK_COMMON_MODE_TWO_VOICE_DELTA * pure_two_voice_activity
					+ PURE_BYPASS_ATTACK_COMMON_MODE_THREE_VOICE_DELTA * pure_three_voice_activity;
				self.transients.bypass_attack_charge = PURE_BYPASS_OUTPUT_COMMON_MODE
					* signals.pure_bypass_attack_drive
					* load_coefficient.max(0.0)
					* PURE_BYPASS_ATTACK_COMMON_MODE_SCALE;
			}
			self.transients.previous_bypass_activity = signals.pure_bypass_envelope_activity;
		}
		if self.transients.pulse_gate_pending && signals.pure_pulse_envelope_activity > 0.0 && self.registers.volume > 0 {
			let pulse_gate_charge = PURE_PULSE_GATE_RELAXATION_CHARGE * volume_fraction;
			self.transients.pulse_gate_fast_charge = pulse_gate_charge;
			self.transients.pulse_gate_slow_charge = pulse_gate_charge;
			self.transients.pulse_gate_pending = false;
		}
		let common_mode_displacement = self.control.combined_common_mode_gain * combined_activity
			+ self.control.pure_bypass_common_mode_gain * signals.pure_bypass_envelope_activity
			+ FILTER_INPUT_COMMON_MODE_ONE_VOICE * signals.filter_input_activity * volume_fraction
			+ self.control.scaled_filter_common_mode
			+ self.transients.bypass_attack_charge * volume_fraction;
		let board_coupled_output = self.output_stage.process(
			(volume_stage_output - common_mode_displacement)
				* bypass_output_gain
				* FILTER_OUTPUT_GAIN
				* SID_OUTPUT_SCALE,
		);
		let output = board_coupled_output as f32
			+ self.transients.volume_fast_charge
			+ self.transients.volume_slow_charge
			+ self.transients.pulse_gate_fast_charge
			- self.transients.pulse_gate_slow_charge;
		/* Positive finite decay factors preserve signed zero, so idle charges need no branches. */
		self.transients.volume_fast_charge *= self.transients.volume_fast_decay;
		self.transients.volume_slow_charge *= self.transients.volume_slow_decay;
		self.transients.bypass_attack_charge *= self.transients.bypass_attack_decay;
		self.transients.pulse_gate_fast_charge *= self.transients.pulse_gate_fast_decay;
		self.transients.pulse_gate_slow_charge *= self.transients.pulse_gate_slow_decay;
		output.round() as i32
	}
}

impl Default for Filter {
	fn default() -> Self {
		Self::new()
	}
}