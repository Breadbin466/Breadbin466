// =======================================================
// src/sid/control.rs — SID filter control state
// =======================================================

/*
 * Filter register and derived-control state.
 *
 * Raw FC, RES/FILT and MODE/VOL state is kept distinct from the values derived
 * from those registers. The derived cache changes only on register writes,
 * keeping sparse control work out of the per-cycle analogue path.
 */

use super::constants::{
	FILTER_HIGHPASS_LOW_COUPLING_ZERO, FILTER_OPERATING_BINS, FILTER_SOURCE_RESONANCE_FLOOR,
	SID_MIXER_TWO_VOICE_VOLUME_GAIN_AT_CODE_9, SID_MIXER_TWO_VOICE_VOLUME_GAIN_PER_CODE,
};
use super::tables::FilterCoefficients;

/* Raw filter registers and cutoff-DAC transition state. Keeping register storage separate from the derived cache makes the distinction between programmed state and precomputed response explicit. */
pub(super) struct FilterRegisters {
	pub(super) cutoff: u16,
	pub(super) cutoff_dac_displacement_codes: i16,
	pub(super) cutoff_write_base: u16,
	pub(super) cutoff_low_pending: bool,
	pub(super) resonance: u8,
	pub(super) routing: u8,
	pub(super) mode: u8,
	pub(super) volume: u8,
}

impl FilterRegisters {
	pub(super) fn new() -> Self {
		Self {
			cutoff: 0,
			cutoff_dac_displacement_codes: 0,
			cutoff_write_base: 0,
			cutoff_low_pending: false,
			resonance: 0,
			routing: 0,
			mode: 0,
			volume: 0,
		}
	}

	pub(super) fn reset(&mut self) {
		*self = Self::new();
	}
}

pub(super) struct ControlCache {
	pub(super) effective_cutoff_index: usize,
	pub(super) integrator_gain: [f32; FILTER_OPERATING_BINS],
	pub(super) input_drive: [f32; FILTER_OPERATING_BINS],
	pub(super) feedback_drive: [f32; FILTER_OPERATING_BINS],
	pub(super) damping: f32,
	pub(super) source_gain_modulation: f32,
	pub(super) high_pass_low_coupling: f32,
	pub(super) band_pass_low_coupling: f32,
	pub(super) resonance_position: f32,
	pub(super) resonance_index: usize,
	pub(super) resonance_feedback_scale: f32,
	pub(super) source_modulation_resonance_scale: f32,
	pub(super) resonance_output_gain: f32,
	pub(super) mode_output_gain: f32,
	pub(super) filter_common_mode: f32,
	pub(super) volume_fraction: f32,
	pub(super) two_voice_volume_gain_offset: f32,
	pub(super) combined_common_mode_gain: f32,
	pub(super) pure_bypass_common_mode_gain: f32,
	pub(super) scaled_filter_common_mode: f32,
	pub(super) frozen_dac_bias_mode: bool,
}

impl ControlCache {
	pub(super) fn new(initial: &FilterCoefficients) -> Self {
		Self {
			effective_cutoff_index: 0,
			integrator_gain: initial.integrator_gain[0],
			input_drive: initial.input_drive,
			feedback_drive: initial.feedback_drive,
			damping: initial.damping[0],
			source_gain_modulation: initial.source_gain_modulation,
			high_pass_low_coupling: FILTER_HIGHPASS_LOW_COUPLING_ZERO,
			band_pass_low_coupling: 0.0,
			resonance_position: 0.0,
			resonance_index: 0,
			resonance_feedback_scale: 0.78,
			source_modulation_resonance_scale: FILTER_SOURCE_RESONANCE_FLOOR,
			resonance_output_gain: 1.0,
			mode_output_gain: 1.0,
			filter_common_mode: 0.0,
			volume_fraction: 0.0,
			two_voice_volume_gain_offset: SID_MIXER_TWO_VOICE_VOLUME_GAIN_AT_CODE_9 - 1.0
				- 9.0 * SID_MIXER_TWO_VOICE_VOLUME_GAIN_PER_CODE,
			combined_common_mode_gain: 0.0,
			pure_bypass_common_mode_gain: 0.0,
			scaled_filter_common_mode: 0.0,
			frozen_dac_bias_mode: false,
		}
	}
}