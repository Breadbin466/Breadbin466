// =======================================================
// src/sid/integrator.rs — SID filter integrator
// =======================================================

/*
 * Nonlinear state-variable integrator used by the 6581 filter.
 *
 * The persistent state consists of the band-pass and low-pass integrators.
 * High-pass is reconstructed from the current input and feedback, so all
 * three filter outputs share one nonlinear state evolution.
 */

#[derive(Clone, Copy, Default)]
pub(super) struct FilterOutputs {
	pub(super) high_pass: f32,
	pub(super) band_pass: f32,
	pub(super) low_pass: f32,
}

#[derive(Default)]
pub(super) struct IntegratorState {
	pub(super) band_state: f32,
	pub(super) low_state: f32,
	pub(super) filter_source_energy: f32,
}

impl IntegratorState {
	#[inline(always)]
	/* Soft saturation limits the feedback path without a hard clip. Increasing drive changes the filter operating point as well as the apparent resonance. */
	pub(super) fn saturate(value: f32, drive: f32) -> f32 {
		let x = value * drive;
		let square = x * x;
		value * (27.0 + square) / (27.0 + 9.0 * square)
	}

	#[inline(always)]
	/* Intermediate substeps advance only the persistent integrator state. High-pass is reconstructed only on the final substep because earlier values are overwritten before they can affect SID state or output. */
	pub(super) fn advance_state(
		&mut self,
		driven_input: f32,
		damping: f32,
		feedback_drive: f32,
		a1: f32,
		a2: f32,
		a3: f32,
	) {
		let previous_band = self.band_state;
		let previous_low = self.low_state;
		let nonlinear_band = Self::saturate(previous_band, feedback_drive);
		let effective_input = driven_input - damping * (nonlinear_band - previous_band);
		let residual = effective_input - previous_low;
		let band = a1 * previous_band + a2 * residual;
		let low = previous_low + a2 * previous_band + a3 * residual;
		self.band_state = 2.0 * band - previous_band;
		self.low_state = 2.0 * low - previous_low;
	}

	#[inline(always)]
	/* The final substep advances the same persistent state and additionally reconstructs the three observable filter outputs. */
	pub(super) fn advance_output(
		&mut self,
		driven_input: f32,
		damping: f32,
		feedback_drive: f32,
		a1: f32,
		a2: f32,
		a3: f32,
	) -> FilterOutputs {
		let previous_band = self.band_state;
		let previous_low = self.low_state;
		let nonlinear_band = Self::saturate(previous_band, feedback_drive);
		let effective_input = driven_input - damping * (nonlinear_band - previous_band);
		let residual = effective_input - previous_low;
		let band = a1 * previous_band + a2 * residual;
		let low = previous_low + a2 * previous_band + a3 * residual;
		self.band_state = 2.0 * band - previous_band;
		self.low_state = 2.0 * low - previous_low;
		let high = effective_input - damping * band - low;
		FilterOutputs {
			high_pass: high,
			band_pass: band,
			low_pass: low,
		}
	}
}