// =======================================================
// src/sid/response.rs — SID analogue response functions
// =======================================================

/*
 * Register-derived and source-level analogue response functions.
 *
 * These helpers contain stateless transfer laws shared by the filter control
 * and cycle paths. Keeping them separate from dynamic integrator state makes
 * the main filter orchestration easier to follow.
 */

use super::constants::{
	FILTER_BANDPASS_GAIN_CUTOFF_SLOPE, FILTER_BANDPASS_GAIN_RESONANCE_DROP,
	FILTER_BANDPASS_GAIN_TRIM_CUTOFF_SLOPE, FILTER_BANDPASS_GAIN_TRIM_ZERO,
	FILTER_BANDPASS_GAIN_ZERO_CUTOFF, FILTER_BANDPASS_OUTPUT_TRIM,
	FILTER_BANDPASS_COMMON_MODE_FC_0, FILTER_BANDPASS_COMMON_MODE_FC_2047,
	FILTER_HIGHPASS_COMMON_MODE_FC_0, FILTER_HIGHPASS_COMMON_MODE_FC_2047,
	FILTER_HIGHPASS_GAIN_CUTOFF_RESONANCE_DROP, FILTER_HIGHPASS_GAIN_CUTOFF_SLOPE,
	FILTER_HIGHPASS_GAIN_RESONANCE_DROP, FILTER_HIGHPASS_GAIN_ZERO,
	FILTER_HIGHPASS_HIGH_CUTOFF_EXPONENT, FILTER_HIGHPASS_HIGH_CUTOFF_KNEE,
	FILTER_HIGHPASS_HIGH_CUTOFF_LOAD, FILTER_HIGHPASS_RESONANCE_TRIM_C0,
	FILTER_HIGHPASS_RESONANCE_TRIM_C1, FILTER_HIGHPASS_RESONANCE_TRIM_C2,
	FILTER_HIGHPASS_RESONANCE_TRIM_C3, FILTER_LOWPASS_COMMON_MODE_FC_0,
	FILTER_LOWPASS_COMMON_MODE_FC_2047, FILTER_LOWPASS_GAIN_AT_MID_RESONANCE,
	FILTER_LOWPASS_GAIN_RESONANCE_DROP, FILTER_LOWPASS_OUTPUT_TRIM,
	FILTER_LOW_BAND_GAIN_CUTOFF_SLOPE, FILTER_LOW_BAND_GAIN_RESONANCE_DROP,
	FILTER_LOW_BAND_GAIN_ZERO_CUTOFF, FILTER_LOW_BAND_LOW_RESONANCE_LIFT,
	FILTER_OPERATING_BINS, FILTER_SINGLE_MODE_CUTOFF_GAIN, FILTER_SINGLE_MODE_RESONANCE_GAIN,
	PURE_SAW_MIXER_LINEAR, PURE_SAW_MIXER_QUADRATIC, SID_ANALOGUE_MAX_VOLTS,
	SID_ANALOGUE_MIN_VOLTS, SID_ANALOGUE_QUIESCENT_VOLTS, SID_MIXER_VOICE_AC_GAIN,
	SID_VOICE_AC_VOLTS, SID_VOICE_BASE_VOLTS, SID_VOICE_ENVELOPE_BIAS_VOLTS,
	SID_VOLUME_DAC_BRANCH_WEIGHTS, SID_VOLUME_DAC_COMMON_COMPRESSION,
	SID_VOLUME_DAC_CONDUCTANCE_SCALE,
};
use super::dac::VoiceSignal;

#[inline(always)]
pub(super) fn analogue_span() -> f32 {
	SID_ANALOGUE_MAX_VOLTS - SID_ANALOGUE_MIN_VOLTS
}

#[inline(always)]
pub(super) fn normalise_voltage(voltage: f32) -> f32 {
	((voltage - SID_ANALOGUE_MIN_VOLTS) / analogue_span()).clamp(0.0, 1.0)
}

#[inline(always)]
pub(super) fn filter_mode_quiescent_level() -> f32 {
	normalise_voltage(SID_ANALOGUE_QUIESCENT_VOLTS)
}

#[inline(always)]
pub(super) fn filter_voice_signal(signal: VoiceSignal) -> f32 {
	let voltage = SID_VOICE_BASE_VOLTS
		+ signal.waveform * signal.envelope * SID_VOICE_AC_VOLTS
		+ signal.envelope * SID_VOICE_ENVELOPE_BIAS_VOLTS;
	normalise_voltage(voltage)
}

#[inline(always)]
pub(super) fn mixer_voice_signal(signal: VoiceSignal, saw_blend: f32) -> f32 {
	let waveform = if signal.waveform_selection == 2 {
		let settled = PURE_SAW_MIXER_LINEAR * signal.waveform
			+ PURE_SAW_MIXER_QUADRATIC * signal.waveform * signal.waveform;
		signal.waveform + saw_blend * (settled - signal.waveform)
	} else {
		signal.waveform
	};
	let voltage = SID_VOICE_BASE_VOLTS
		+ waveform * signal.envelope * SID_VOICE_AC_VOLTS * SID_MIXER_VOICE_AC_GAIN
		+ signal.envelope * SID_VOICE_ENVELOPE_BIAS_VOLTS;
	normalise_voltage(voltage)
}

#[inline(always)]
/* The nonlinear transfer surface is indexed by the present operating point, not merely by the register code. Input and integrator state therefore influence the local amplifier response used this cycle. */
pub(super) fn operating_position(input: f32, band: f32, low: f32) -> (usize, usize, f32) {
	let activity = (0.45 * input.abs() + 0.33 * band.abs() + 0.22 * low.abs()).clamp(0.0, 1.0);
	let position = activity * (FILTER_OPERATING_BINS - 1) as f32;
	let lower = position as usize;
	let upper = (lower + 1).min(FILTER_OPERATING_BINS - 1);
	(lower, upper, position - lower as f32)
}

#[inline(always)]
pub(super) fn volume_dac_conductance_code(volume: u8) -> f32 {
	let mut branch_sum = 0.0;
	for (bit, weight) in SID_VOLUME_DAC_BRANCH_WEIGHTS.iter().enumerate() {
		if volume & (1 << bit) != 0 {
			branch_sum += *weight;
		}
	}
	SID_VOLUME_DAC_CONDUCTANCE_SCALE * branch_sum
		/ (1.0 + SID_VOLUME_DAC_COMMON_COMPRESSION * branch_sum)
}

#[inline(always)]
pub(super) fn filter_output_common_mode(mode: u8, cutoff: u16) -> f32 {
	let cutoff_position = cutoff as f32 / 2047.0;
	let interpolate = |at_zero: f32, at_full: f32| {
		at_zero + (at_full - at_zero) * cutoff_position
	};
	let mut common_mode = 0.0;
	if mode & 0x01 != 0 {
		common_mode += interpolate(
			FILTER_LOWPASS_COMMON_MODE_FC_0,
			FILTER_LOWPASS_COMMON_MODE_FC_2047,
		);
	}
	if mode & 0x02 != 0 {
		common_mode += interpolate(
			FILTER_BANDPASS_COMMON_MODE_FC_0,
			FILTER_BANDPASS_COMMON_MODE_FC_2047,
		);
	}
	if mode & 0x04 != 0 {
		common_mode += interpolate(
			FILTER_HIGHPASS_COMMON_MODE_FC_0,
			FILTER_HIGHPASS_COMMON_MODE_FC_2047,
		);
	}
	common_mode
}

#[inline(always)]
pub(super) fn filter_mode_output_gain(mode: u8, resonance: usize, cutoff: u16) -> f32 {
	let resonance_position = ((resonance as f32 - 8.0) / 7.0).clamp(0.0, 1.0);
	let cutoff_position = cutoff as f32 / 2047.0;
	let base_gain = match mode & 0x07 {
		0x01 => {
			(FILTER_LOWPASS_GAIN_AT_MID_RESONANCE
				- FILTER_LOWPASS_GAIN_RESONANCE_DROP * resonance_position)
				* FILTER_LOWPASS_OUTPUT_TRIM
		}
		0x02 => {
			(FILTER_BANDPASS_GAIN_ZERO_CUTOFF
				- FILTER_BANDPASS_GAIN_CUTOFF_SLOPE * cutoff_position
				- FILTER_BANDPASS_GAIN_RESONANCE_DROP * resonance_position)
				* FILTER_BANDPASS_OUTPUT_TRIM
		}
		0x03 => {
			FILTER_LOW_BAND_GAIN_ZERO_CUTOFF
				- FILTER_LOW_BAND_GAIN_CUTOFF_SLOPE * cutoff_position
				- FILTER_LOW_BAND_GAIN_RESONANCE_DROP * resonance_position
		}
		0x04 => {
			FILTER_HIGHPASS_GAIN_ZERO
				- FILTER_HIGHPASS_GAIN_CUTOFF_SLOPE * cutoff_position
				- FILTER_HIGHPASS_GAIN_RESONANCE_DROP * resonance_position * resonance_position
				- FILTER_HIGHPASS_GAIN_CUTOFF_RESONANCE_DROP
					* cutoff_position
					* resonance_position
					* resonance_position
		}
		_ => 1.0,
	};
	let low_resonance_position = ((8.0 - resonance as f32) / 8.0).clamp(0.0, 1.0);
	let low_band_lift = if mode & 0x07 == 0x03 {
		1.0 + FILTER_LOW_BAND_LOW_RESONANCE_LIFT
			* low_resonance_position
			* low_resonance_position
	} else {
		1.0
	};
	let bandpass_trim = if mode & 0x07 == 0x02 {
		FILTER_BANDPASS_GAIN_TRIM_ZERO
			- FILTER_BANDPASS_GAIN_TRIM_CUTOFF_SLOPE * cutoff_position
	} else {
		1.0
	};
	/* The high-pass path develops an additional smooth load near the upper FC rail. This register-derived term is evaluated only when filter controls change. */
	let highpass_loading = if mode & 0x07 == 0x04 {
		let cutoff_power = cutoff_position.powf(FILTER_HIGHPASS_HIGH_CUTOFF_EXPONENT);
		let knee_power =
			FILTER_HIGHPASS_HIGH_CUTOFF_KNEE.powf(FILTER_HIGHPASS_HIGH_CUTOFF_EXPONENT);
		let loading_position = cutoff_power / (cutoff_power + knee_power);
		1.0 / (1.0 + FILTER_HIGHPASS_HIGH_CUTOFF_LOAD * loading_position)
	} else {
		1.0
	};
	/* High-frequency output loading also depends smoothly on resonance. The factor remains exactly one at FC=$000 so the low-cutoff operating point is preserved. */
	let highpass_resonance_trim = if mode & 0x07 == 0x04 {
		let centred_resonance = resonance as f32 / 15.0 - 0.5;
		let resonance_curve = FILTER_HIGHPASS_RESONANCE_TRIM_C0
			+ centred_resonance
				* (FILTER_HIGHPASS_RESONANCE_TRIM_C1
					+ centred_resonance
						* (FILTER_HIGHPASS_RESONANCE_TRIM_C2
							+ centred_resonance * FILTER_HIGHPASS_RESONANCE_TRIM_C3));
		1.0 + cutoff_position * resonance_curve
	} else {
		1.0
	};
	let single_mode_loading = match mode & 0x07 {
		0x01 | 0x02 | 0x04 => {
			let mode_index = match mode & 0x07 {
				0x01 => 0,
				0x02 => 1,
				_ => 2,
			};
			let position = cutoff as f32 * 31.0 / 2047.0;
			let lower = position.floor() as usize;
			let upper = (lower + 1).min(31);
			let fraction = position - lower as f32;
			let curve = &FILTER_SINGLE_MODE_CUTOFF_GAIN[mode_index];
			(curve[lower] + (curve[upper] - curve[lower]) * fraction)
				* FILTER_SINGLE_MODE_RESONANCE_GAIN[mode_index][resonance]
		}
		_ => 1.0,
	};
	base_gain
		* low_band_lift
		* bandpass_trim
		* highpass_loading
		* highpass_resonance_trim
		* single_mode_loading
}