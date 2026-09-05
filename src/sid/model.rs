// =======================================================
// src/sid/model.rs — C64 SID output-stage model
// =======================================================

/* C64 motherboard audio coupling network following the SID output pin. */

use super::constants::{
	C64_AUDIO_ASSUMED_LOAD_RESISTANCE_OHMS, C64_AUDIO_COUPLING_CAPACITANCE_FARADS,
	C64_AUDIO_LOW_PASS_CAPACITANCE_FARADS, C64_AUDIO_LOW_PASS_RESISTANCE_OHMS, CLOCK_FREQUENCY_HZ,
	HIGH_PASS_FRACTION_BITS, LOW_PASS_FRACTION_BITS, OUTPUT_STATE_FRACTION_BITS,
};

/* The SID output pin is followed by the C64 motherboard coupling network. A low-pass pole models the output load, while the series coupling capacitor removes the DC operating point before samples reach the host audio stream. */
pub(super) struct AudioOutStage {
	low_pass_state: i64,
	high_pass_state: i64,
	low_pass_coefficient: i64,
	high_pass_coefficient: i64,
}

impl AudioOutStage {
	/* Coefficients are derived from the emulated PAL clock and board component values so the coupling response remains tied to machine time rather than host sample rate. */
	pub(super) fn new() -> Self {
		let dt = 1.0 / CLOCK_FREQUENCY_HZ;
		let low_pass_rc = f64::from(C64_AUDIO_LOW_PASS_RESISTANCE_OHMS)
			* f64::from(C64_AUDIO_LOW_PASS_CAPACITANCE_FARADS);
		let high_pass_rc = f64::from(C64_AUDIO_ASSUMED_LOAD_RESISTANCE_OHMS)
			* f64::from(C64_AUDIO_COUPLING_CAPACITANCE_FARADS);
		Self {
			low_pass_state: 0,
			high_pass_state: 0,
			low_pass_coefficient: ((dt / (dt + low_pass_rc))
				* f64::from(1u32 << LOW_PASS_FRACTION_BITS))
			.round() as i64,
			high_pass_coefficient: ((dt / (dt + high_pass_rc))
				* f64::from(1u32 << HIGH_PASS_FRACTION_BITS))
			.round() as i64,
		}
	}

	/* Clearing both capacitor states removes residual DC and low-frequency history at machine reset. */
	pub(super) fn reset(&mut self) {
		self.low_pass_state = 0;
		self.high_pass_state = 0;
	}

	#[inline(always)]
	/* The two RC sections are advanced at the SID clock in fixed point so their state remains causal and independent of the host audio sampling rate. */
	pub(super) fn process(&mut self, input: f32) -> i32 {
		let input_sample = input.round() as i32;
		let scaled_input = i64::from(input_sample) << OUTPUT_STATE_FRACTION_BITS;
		let low_pass_delta = self.low_pass_coefficient * (scaled_input - self.low_pass_state)
			>> LOW_PASS_FRACTION_BITS;
		self.low_pass_state += low_pass_delta;
		let high_pass_delta = self.high_pass_coefficient
			* (self.low_pass_state - self.high_pass_state)
			>> HIGH_PASS_FRACTION_BITS;
		self.high_pass_state += high_pass_delta;
		let output = (self.low_pass_state - self.high_pass_state) >> OUTPUT_STATE_FRACTION_BITS;
		output.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
	}
}

impl Default for AudioOutStage {
	fn default() -> Self {
		Self::new()
	}
}