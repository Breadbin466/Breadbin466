// =======================================================
// src/sid/resampler.rs — SID audio resampler
// =======================================================

/* Causal conversion from the SID clock domain to the host audio rate. */

use std::f64::consts::PI;

use super::constants::{
	SID_RESAMPLER_BUTTERWORTH_Q, SID_RESAMPLER_FIRST_CUTOFF_HZ, SID_RESAMPLER_FIRST_TAPS,
	SID_RESAMPLER_PHASES, SID_RESAMPLER_SECOND_CUTOFF_HZ, SID_RESAMPLER_SECOND_DECIMATION,
};

/* The first stage converts the PAL SID clock to an integer intermediate rate with a polyphase windowed-sinc FIR. Fractional phase interpolation avoids quantising output times to source cycles. */
struct FractionalFirStage {
	source_frequency: u64,
	target_frequency: u64,
	target_frequency_inverse: f64,
	phase_accumulator: u64,
	ring: Box<[f32]>,
	coefficients: Box<[f32]>,
	write_index: usize,
	tap_count: usize,
}

impl FractionalFirStage {
	/* The rational clock ratio is kept as integer frequencies so output scheduling cannot accumulate floating-point timing drift. */
	fn new(source_frequency: f64, target_frequency: f64, cutoff: f64, tap_count: usize) -> Self {
		let source_frequency = source_frequency.round() as u64;
		let target_frequency = target_frequency.round() as u64;
		assert!(source_frequency > target_frequency && target_frequency > 0);
		assert!(tap_count >= 3 && tap_count & 1 == 1);
		Self {
			source_frequency,
			target_frequency,
			target_frequency_inverse: 1.0 / target_frequency as f64,
			phase_accumulator: 0,
			ring: vec![0.0; tap_count * 2].into_boxed_slice(),
			coefficients: Self::build_coefficients(source_frequency as f64, cutoff, tap_count),
			write_index: 0,
			tap_count,
		}
	}

	/* Each polyphase row is independently normalised, preserving DC gain while fractional delay moves between neighbouring source-cycle positions. */
	fn build_coefficients(source_frequency: f64, cutoff: f64, tap_count: usize) -> Box<[f32]> {
		let cutoff = cutoff.min(source_frequency * 0.49);
		let normalised_cutoff = cutoff / source_frequency;
		let centre = (tap_count - 1) as f64 * 0.5;
		let mut table = vec![0.0f32; (SID_RESAMPLER_PHASES + 1) * tap_count];

		for phase in 0..=SID_RESAMPLER_PHASES {
			let fractional_back = phase as f64 / SID_RESAMPLER_PHASES as f64;
			let row = &mut table[phase * tap_count..(phase + 1) * tap_count];
			let mut sum = 0.0f64;
			for (tap, coefficient) in row.iter_mut().enumerate() {
				let distance = tap as f64 - centre - fractional_back;
				let angle = 2.0 * PI * normalised_cutoff * distance;
				let sinc = if angle.abs() < 1.0e-14 {
					1.0
				} else {
					angle.sin() / angle
				};
				let position = tap as f64 / (tap_count - 1) as f64;
				let window = 0.35875 - 0.48829 * (2.0 * PI * position).cos()
					+ 0.14128 * (4.0 * PI * position).cos()
					- 0.01168 * (6.0 * PI * position).cos();
				let value = 2.0 * normalised_cutoff * sinc * window;
				*coefficient = value as f32;
				sum += value;
			}
			let inverse_sum = (1.0 / sum) as f32;
			for coefficient in row {
				*coefficient *= inverse_sum;
			}
		}
		table.into_boxed_slice()
	}

	#[inline(always)]
	/* The phase accumulator schedules an intermediate sample whenever the target clock crosses a source-cycle boundary; convolution then interpolates between adjacent fractional-delay rows. */
	fn input(&mut self, sample: i32) -> Option<f32> {
		self.write_index = if self.write_index == 0 {
			self.tap_count - 1
		} else {
			self.write_index - 1
		};
		let sample = sample as f32;
		self.ring[self.write_index] = sample;
		self.ring[self.write_index + self.tap_count] = sample;

		self.phase_accumulator += self.target_frequency;
		if self.phase_accumulator < self.source_frequency {
			return None;
		}
		self.phase_accumulator -= self.source_frequency;

		let fractional_back = self.phase_accumulator as f64 * self.target_frequency_inverse;
		let phase_position = fractional_back.clamp(0.0, 1.0) * SID_RESAMPLER_PHASES as f64;
		let lower_phase = phase_position as usize;

		let fraction = (phase_position - lower_phase as f64) as f32;
		let (lower, upper) = self.convolve_pair(lower_phase);
		Some(lower + (upper - lower) * fraction)
	}

	#[inline(always)]
	/* Both phases share sample loads while retaining the original accumulation order in each phase. */
	fn convolve_pair(&self, phase: usize) -> (f32, f32) {
		let rows = &self.coefficients[phase * self.tap_count..(phase + 2) * self.tap_count];
		let (lower, upper) = rows.split_at(self.tap_count);
		let samples = &self.ring[self.write_index..self.write_index + self.tap_count];
		let mut lower_sum = 0.0f32;
		let mut upper_sum = 0.0f32;
		for ((sample, lower), upper) in samples.iter().zip(lower).zip(upper) {
			lower_sum += sample * lower;
			upper_sum += sample * upper;
		}
		(lower_sum, upper_sum)
	}

	/* Priming fills the FIR history with the first real sample, avoiding an artificial ramp from silence at stream start. */
	fn prime(&mut self, sample: i32) {
		self.phase_accumulator = 0;
		self.ring.fill(sample as f32);
		self.write_index = 0;
	}

	fn reset(&mut self) {
		self.phase_accumulator = 0;
		self.ring.fill(0.0);
		self.write_index = 0;
	}
}

#[derive(Clone, Copy)]
struct Biquad {
	b0: f64,
	b1: f64,
	b2: f64,
	a1: f64,
	a2: f64,
	x1: f64,
	x2: f64,
	y1: f64,
	y2: f64,
}

impl Biquad {
	/* Bilinear-transform coefficients create one Butterworth section at the fixed intermediate sample rate. */
	fn low_pass(sample_rate: f64, cutoff: f64, q: f64) -> Self {
		let omega = 2.0 * PI * cutoff / sample_rate;
		let cosine = omega.cos();
		let sine = omega.sin();
		let alpha = sine / (2.0 * q);
		let a0 = 1.0 + alpha;
		Self {
			b0: ((1.0 - cosine) * 0.5) / a0,
			b1: (1.0 - cosine) / a0,
			b2: ((1.0 - cosine) * 0.5) / a0,
			a1: (-2.0 * cosine) / a0,
			a2: (1.0 - alpha) / a0,
			x1: 0.0,
			x2: 0.0,
			y1: 0.0,
			y2: 0.0,
		}
	}

	#[inline(always)]
	fn process(&mut self, input: f64) -> f64 {
		let output = self.b0 * input + self.b1 * self.x1 + self.b2 * self.x2
			- self.a1 * self.y1
			- self.a2 * self.y2;
		self.x2 = self.x1;
		self.x1 = input;
		self.y2 = self.y1;
		self.y1 = output;
		output
	}

	fn prime(&mut self, sample: f64) {
		self.x1 = sample;
		self.x2 = sample;
		self.y1 = sample;
		self.y2 = sample;
	}

	fn reset(&mut self) {
		self.x1 = 0.0;
		self.x2 = 0.0;
		self.y1 = 0.0;
		self.y2 = 0.0;
	}
}

/* The second stage removes remaining ultrasonic energy with four causal Butterworth sections before integer decimation to the host rate. */
struct CausalDecimator {
	sections: [Biquad; 4],
	phase: u64,
}

impl CausalDecimator {
	fn new(source_frequency: f64, cutoff: f64) -> Self {
		Self {
			sections: SID_RESAMPLER_BUTTERWORTH_Q
				.map(|q| Biquad::low_pass(source_frequency, cutoff, q)),
			phase: 0,
		}
	}

	fn prime(&mut self, sample: f32) {
		for section in &mut self.sections {
			section.prime(f64::from(sample));
		}
		self.phase = 0;
	}

	#[inline(always)]
	/* All four sections run for every intermediate sample; integer decimation occurs only after the anti-aliasing cascade has updated its state. */
	fn input(&mut self, sample: f32) -> Option<i32> {
		let mut filtered = f64::from(sample);
		for section in &mut self.sections {
			filtered = section.process(filtered);
		}
		self.phase += 1;
		if self.phase < SID_RESAMPLER_SECOND_DECIMATION {
			return None;
		}
		self.phase = 0;
		Some(filtered.round() as i32)
	}

	fn reset(&mut self) {
		for section in &mut self.sections {
			section.reset();
		}
		self.phase = 0;
	}
}

/* AudioRateConverter preserves SID-cycle causality while translating the fixed PAL clock to the active host rate. Every SID-cycle sample is accepted in order; a fractional polyphase FIR first produces a four-times oversampled stream, then a causal low-pass stage decimates by four and yields output only on host sample boundaries. */
pub struct AudioRateConverter {
	/* Rational cycle-to-intermediate-rate conversion with phase-indexed FIR coefficients. */
	first: FractionalFirStage,
	/* Anti-alias filtering and fixed four-to-one decimation to the host rate. */
	second: CausalDecimator,
	/* Priming repeats the first real sample instead of inventing silence before stream start. */
	prime_first: bool,
	prime_second: bool,
}

impl AudioRateConverter {
	/* Construction derives both conversion stages from the emulated clock and requested host rate, keeping sample timing stable when the frontend changes audio devices. */
	pub fn new(clock_frequency: f64, sampling_frequency: f64) -> Self {
		let sampling_frequency = sampling_frequency.round();
		assert!(sampling_frequency > 0.0);
		let intermediate_frequency = sampling_frequency * SID_RESAMPLER_SECOND_DECIMATION as f64;
		/* The pass-band remains capped at the analogue design target while also staying
		 * below the Nyquist limit of the selected host rate. This keeps the 44.1 kHz
		 * response unchanged and permits native 48 kHz WASAPI endpoints without a
		 * second frontend resampler or a mismatched stream clock. */
		let host_nyquist_margin = sampling_frequency * 0.45;
		let first_cutoff = SID_RESAMPLER_FIRST_CUTOFF_HZ.min(host_nyquist_margin);
		let second_cutoff = SID_RESAMPLER_SECOND_CUTOFF_HZ.min(host_nyquist_margin);
		Self {
			first: FractionalFirStage::new(
				clock_frequency,
				intermediate_frequency,
				first_cutoff,
				SID_RESAMPLER_FIRST_TAPS,
			),
			second: CausalDecimator::new(intermediate_frequency, second_cutoff),
			prime_first: true,
			prime_second: true,
		}
	}

	#[inline(always)]
	/* Feeds one cycle-domain sample through both stages; None means that no host-rate sample is due yet. */
	pub fn accept_cycle_sample(&mut self, sample: i32) -> Option<i32> {
		if self.prime_first {
			self.first.prime(sample);
			self.prime_first = false;
		}

		let intermediate = self.first.input(sample)?;
		if self.prime_second {
			self.second.prime(intermediate);
			self.prime_second = false;
		}
		self.second.input(intermediate)
	}

	/* Reset clears FIR history, decimator phase and biquad state together so the first post-reset sample cannot contain energy from the previous stream. */
	pub fn reset(&mut self) {
		self.first.reset();
		self.second.reset();
		self.prime_first = true;
		self.prime_second = true;
	}
}