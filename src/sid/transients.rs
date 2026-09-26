// =======================================================
// src/sid/transients.rs — SID analogue transient state
// =======================================================

/*
 * Dynamic output-node and bypass-mixer transients.
 *
 * These states evolve independently of the filter integrators: D418 charge
 * injection, bypass-attack displacement, pulse GATE relaxation and pure-saw
 * mixer settling. Decay coefficients are fixed for the active SID clock.
 */

use super::constants::{
	CLOCK_FREQUENCY_HZ, PURE_BYPASS_ATTACK_CHARGE_TIME_SECONDS,
	PURE_PULSE_GATE_RELAXATION_FAST_SECONDS, PURE_PULSE_GATE_RELAXATION_SLOW_SECONDS,
	PURE_SAW_MIXER_SETTLING_SECONDS, SID_VOLUME_TRANSITION_FAST_CHARGE_TIME_SECONDS,
	SID_VOLUME_TRANSITION_SLOW_CHARGE_TIME_SECONDS,
};

pub(super) struct TransientState {
	pub(super) volume_fast_charge: f32,
	pub(super) volume_slow_charge: f32,
	pub(super) bypass_attack_charge: f32,
	pub(super) pulse_gate_fast_charge: f32,
	pub(super) pulse_gate_slow_charge: f32,
	pub(super) volume_fast_decay: f32,
	pub(super) volume_slow_decay: f32,
	pub(super) bypass_attack_decay: f32,
	pub(super) pulse_gate_fast_decay: f32,
	pub(super) pulse_gate_slow_decay: f32,
	pub(super) volume_conductance_code: f32,
	pub(super) previous_bypass_activity: f32,
	pub(super) pulse_gate_pending: bool,
	pub(super) saw_mixer_blend: [f32; 3],
	pub(super) saw_mixer_tracking: f32,
}

impl TransientState {
	pub(super) fn new() -> Self {
		Self {
			volume_conductance_code: 0.0,
			volume_fast_charge: 0.0,
			volume_slow_charge: 0.0,
			volume_fast_decay: (-1.0
				/ (SID_VOLUME_TRANSITION_FAST_CHARGE_TIME_SECONDS * CLOCK_FREQUENCY_HZ as f32))
				.exp(),
			volume_slow_decay: (-1.0
				/ (SID_VOLUME_TRANSITION_SLOW_CHARGE_TIME_SECONDS * CLOCK_FREQUENCY_HZ as f32))
				.exp(),
			bypass_attack_charge: 0.0,
			previous_bypass_activity: 0.0,
			bypass_attack_decay: (-1.0
				/ (PURE_BYPASS_ATTACK_CHARGE_TIME_SECONDS * CLOCK_FREQUENCY_HZ as f32))
				.exp(),
			pulse_gate_fast_charge: 0.0,
			pulse_gate_slow_charge: 0.0,
			pulse_gate_fast_decay: (-1.0
				/ (PURE_PULSE_GATE_RELAXATION_FAST_SECONDS * CLOCK_FREQUENCY_HZ as f32))
				.exp(),
			pulse_gate_slow_decay: (-1.0
				/ (PURE_PULSE_GATE_RELAXATION_SLOW_SECONDS * CLOCK_FREQUENCY_HZ as f32))
				.exp(),
			pulse_gate_pending: false,
			saw_mixer_blend: [0.0; 3],
			saw_mixer_tracking: 1.0
				- (-1.0 / (PURE_SAW_MIXER_SETTLING_SECONDS * CLOCK_FREQUENCY_HZ as f32)).exp(),
		}
	}

	pub(super) fn reset(&mut self) {
		self.volume_conductance_code = 0.0;
		self.volume_fast_charge = 0.0;
		self.volume_slow_charge = 0.0;
		self.bypass_attack_charge = 0.0;
		self.previous_bypass_activity = 0.0;
		self.pulse_gate_fast_charge = 0.0;
		self.pulse_gate_slow_charge = 0.0;
		self.pulse_gate_pending = false;
		self.saw_mixer_blend = [0.0; 3];
	}
}