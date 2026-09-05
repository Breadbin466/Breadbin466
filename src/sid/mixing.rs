// =======================================================
// src/sid/mixing.rs — SID filter and bypass mixing
// =======================================================

/*
 * SID filter and bypass routing topology.
 *
 * Each voice and EXT IN independently joins the filter input or the bypass
 * mixer. Voice 3 suppression affects only its unfiltered path. The cached
 * membership masks and reciprocals keep register decoding out of the cycle
 * path.
 */

use super::constants::{
	PURE_BYPASS_ATTACK_SCALE_BY_SELECTION, PURE_BYPASS_ATTACK_WAVEFORM_STATE_COUPLING,
	SID_EXTERNAL_INPUT_GAIN, SID_FROZEN_DAC_POLYNOMIAL, SID_MIXER_VOICE_AC_GAIN,
	SID_VOICE_AC_VOLTS, SID_VOICE_BASE_VOLTS,
};
use super::dac::VoiceSignal;
use super::response::{filter_voice_signal, mixer_voice_signal, normalise_voltage};

/* EXT IN is stored once in the normalised source domain and cached in the filter-summer and bypass-mixer voltage domains. */
pub(super) struct ExternalInputState {
	pub(super) raw: f32,
	pub(super) filter_signal: f32,
	pub(super) mixer_signal: f32,
}

impl ExternalInputState {
	pub(super) fn new() -> Self {
		let resting = normalise_voltage(SID_VOICE_BASE_VOLTS);
		Self {
			raw: 0.0,
			filter_signal: resting,
			mixer_signal: resting,
		}
	}

	pub(super) fn set_sample(&mut self, sample: i16) {
		self.raw = f32::from(sample) / 32768.0 * SID_EXTERNAL_INPUT_GAIN;
		self.filter_signal =
			normalise_voltage(SID_VOICE_BASE_VOLTS + self.raw * SID_VOICE_AC_VOLTS);
		self.mixer_signal = normalise_voltage(
			SID_VOICE_BASE_VOLTS + self.raw * SID_VOICE_AC_VOLTS * SID_MIXER_VOICE_AC_GAIN,
		);
	}
}

#[derive(Clone, Copy)]
pub(super) struct MixingTopology {
	pub(super) filter_membership: [bool; 4],
	pub(super) bypass_membership: [bool; 4],
	pub(super) filter_input_count: usize,
	pub(super) bypass_input_count: usize,
	pub(super) filter_output_count: usize,
	pub(super) mixer_input_count: usize,
	pub(super) filter_input_reciprocal: f32,
	pub(super) summer_input_reciprocal: f32,
	pub(super) mixer_input_reciprocal: f32,
	pub(super) filter_internal_mask: u8,
	pub(super) bypass_internal_mask: u8,
}

impl MixingTopology {
	pub(super) fn new() -> Self {
		Self {
			filter_membership: [false; 4],
			bypass_membership: [true; 4],
			filter_input_count: 0,
			bypass_input_count: 4,
			filter_output_count: 0,
			mixer_input_count: 4,
			filter_input_reciprocal: 0.0,
			summer_input_reciprocal: 0.5,
			mixer_input_reciprocal: 0.25,
			filter_internal_mask: 0,
			bypass_internal_mask: 0x07,
		}
	}

	/* Routing is recomputed whenever either register changes because voice-three suppression affects only its unfiltered bypass path, not its contribution to the filter core. */
	pub(super) fn update(&mut self, routing: u8, mode: u8) {
		let voice_three_off = mode & 0x08 != 0;
		self.filter_input_count = 0;
		self.bypass_input_count = 0;
		self.filter_internal_mask = 0;
		self.bypass_internal_mask = 0;
		for index in 0..4 {
			let filtered = routing & (1 << index) != 0;
			self.filter_membership[index] = filtered;
			self.bypass_membership[index] = !filtered && !(index == 2 && voice_three_off);
			self.filter_input_count += usize::from(filtered);
			self.bypass_input_count += usize::from(self.bypass_membership[index]);
			if index < 3 {
				if filtered {
					self.filter_internal_mask |= 1 << index;
				}
				if self.bypass_membership[index] {
					self.bypass_internal_mask |= 1 << index;
				}
			}
		}
		self.filter_output_count = usize::from(mode & 0x01 != 0)
			+ usize::from(mode & 0x02 != 0)
			+ usize::from(mode & 0x04 != 0);
		self.mixer_input_count = (self.bypass_input_count + self.filter_output_count).min(7);
		self.filter_input_reciprocal = if self.filter_input_count == 0 {
			0.0
		} else {
			1.0 / self.filter_input_count as f32
		};
		self.summer_input_reciprocal = 1.0 / (2 + self.filter_input_count) as f32;
		self.mixer_input_reciprocal = if self.mixer_input_count == 0 {
			0.0
		} else {
			1.0 / self.mixer_input_count as f32
		};
	}
}

pub(super) struct RoutedSignals {
	pub(super) filter_input_sum: f32,
	pub(super) filter_source_energy: f32,
	pub(super) filter_input_activity: f32,
	pub(super) bypass_input_sum: f32,
	pub(super) bypass_envelope_activity: f32,
	pub(super) pure_bypass_envelope_activity: f32,
	pub(super) pure_bypass_attack_drive: f32,
	pub(super) pure_pulse_envelope_activity: f32,
}

impl RoutedSignals {
	pub(super) fn collect(
		topology: &MixingTopology,
		voices: &[VoiceSignal; 3],
		saw_mixer_blend: &[f32; 3],
		filter_external_signal: f32,
		mixer_external_signal: f32,
		external_input: f32,
		source_resting: f32,
	) -> Self {
		let mut signals = Self {
			filter_input_sum: 0.0,
			filter_source_energy: 0.0,
			filter_input_activity: 0.0,
			bypass_input_sum: 0.0,
			bypass_envelope_activity: 0.0,
			pure_bypass_envelope_activity: 0.0,
			pure_bypass_attack_drive: 0.0,
			pure_pulse_envelope_activity: 0.0,
		};

		macro_rules! add_filter {
			($index:expr) => {
				add_filter_voice(
					&mut signals.filter_input_sum,
					&mut signals.filter_source_energy,
					&mut signals.filter_input_activity,
					voices[$index],
					source_resting,
				)
			};
		}
		match topology.filter_internal_mask {
			0 => {}
			1 => add_filter!(0),
			2 => add_filter!(1),
			3 => { add_filter!(0); add_filter!(1); }
			4 => add_filter!(2),
			5 => { add_filter!(0); add_filter!(2); }
			6 => { add_filter!(1); add_filter!(2); }
			_ => { add_filter!(0); add_filter!(1); add_filter!(2); }
		}

		macro_rules! add_bypass {
			($index:expr) => {
				add_bypass_voice(
					&mut signals.bypass_input_sum,
					&mut signals.bypass_envelope_activity,
					&mut signals.pure_bypass_envelope_activity,
					&mut signals.pure_bypass_attack_drive,
					&mut signals.pure_pulse_envelope_activity,
					voices[$index],
					saw_mixer_blend[$index],
					source_resting,
				)
			};
		}
		match topology.bypass_internal_mask {
			0 => {}
			1 => add_bypass!(0),
			2 => add_bypass!(1),
			3 => { add_bypass!(0); add_bypass!(1); }
			4 => add_bypass!(2),
			5 => { add_bypass!(0); add_bypass!(2); }
			6 => { add_bypass!(1); add_bypass!(2); }
			_ => { add_bypass!(0); add_bypass!(1); add_bypass!(2); }
		}

		if topology.filter_membership[3] {
			signals.filter_input_sum += filter_external_signal;
			let deviation = filter_external_signal - source_resting;
			signals.filter_source_energy += deviation * deviation;
		}
		if topology.bypass_membership[3] {
			signals.bypass_input_sum += mixer_external_signal;
			signals.bypass_envelope_activity += external_input.abs();
		}

		signals
	}
}

pub(super) fn filter_voice_contribution(signal: VoiceSignal, source_resting: f32) -> (f32, f32) {
	if signal.envelope == 0.0 {
		return (source_resting, 0.0);
	}
	let source = filter_voice_signal(signal);
	let deviation = source - source_resting;
	(source, deviation * deviation)
}

#[inline(always)]
pub(super) fn add_filter_voice(
	filter_input_sum: &mut f32,
	filter_source_energy: &mut f32,
	filter_input_activity: &mut f32,
	signal: VoiceSignal,
	source_resting: f32,
) {
	let (source, energy) = filter_voice_contribution(signal, source_resting);
	*filter_input_sum += source;
	*filter_source_energy += energy;
	*filter_input_activity += signal.envelope;
}

#[inline(always)]
pub(super) fn add_bypass_voice(
	bypass_input_sum: &mut f32,
	bypass_envelope_activity: &mut f32,
	pure_bypass_envelope_activity: &mut f32,
	pure_bypass_attack_drive: &mut f32,
	pure_pulse_envelope_activity: &mut f32,
	signal: VoiceSignal,
	saw_blend: f32,
	source_resting: f32,
) {
	if signal.envelope == 0.0 {
		*bypass_input_sum += source_resting;
		return;
	}
	*bypass_input_sum += mixer_voice_signal(signal, saw_blend);
	*bypass_envelope_activity += signal.envelope;
	if !signal.combined && !signal.frozen {
		*pure_bypass_envelope_activity += signal.envelope;
		if signal.waveform_selection == 4 {
			*pure_pulse_envelope_activity += signal.envelope;
		}
		let waveform_state_load = (1.0
			+ PURE_BYPASS_ATTACK_WAVEFORM_STATE_COUPLING * signal.waveform)
			.clamp(0.5, 1.5);
		*pure_bypass_attack_drive += signal.envelope
			* PURE_BYPASS_ATTACK_SCALE_BY_SELECTION[usize::from(signal.waveform_selection)]
			* waveform_state_load;
	}
}

#[inline(always)]
/* A voice whose waveform DAC is frozen contributes a DC operating point even when it carries no changing audio. The special path prevents that bias from being mistaken for ordinary AC voice energy. */
pub(super) fn frozen_dac_operating_point(
	frozen_dac_bias_mode: bool,
	mode: u8,
	volume: u8,
	voices: &[VoiceSignal; 3],
) -> Option<f32> {
	let configured = frozen_dac_bias_mode
		&& voices
			.iter()
			.all(|voice| voice.frozen && voice.envelope > 0.99);
	if !configured {
		return None;
	}

	let x = (f32::from(volume) - 7.5) / 7.5;
	let bits = [
		f32::from(mode & 0x01 != 0),
		f32::from(mode & 0x02 != 0),
		f32::from(mode & 0x04 != 0),
		f32::from(mode & 0x08 != 0),
	];
	let terms = [
		1.0,
		bits[0],
		bits[1],
		bits[2],
		bits[3],
		bits[0] * bits[1],
		bits[0] * bits[2],
		bits[0] * bits[3],
		bits[1] * bits[2],
		bits[1] * bits[3],
		bits[2] * bits[3],
	];
	Some(
		SID_FROZEN_DAC_POLYNOMIAL
			.iter()
			.zip(terms)
			.map(|(row, term)| term * (row[0] + x * (row[1] + x * row[2])))
			.sum(),
	)
}