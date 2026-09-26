// =======================================================
// src/ui/audio_resampler.rs — Host fallback sample-rate conversion
// =======================================================

use std::f64::consts::PI;

const TAPS: usize = 96;
const PHASES: usize = 1024;
const HISTORY: usize = 128;

/* Only fallback devices use this causal, windowed-sinc converter. A Blackman
 * window limits truncation ringing; the cutoff follows the smaller Nyquist
 * frequency. Normalised phase tables preserve DC gain. The 48-sample delay
 * is about 1.1 ms at 44.1 kHz and does not depend on callback block sizes.
 * All coefficient construction and conversion happen outside the callback. */
pub(super) struct OutputResampler {
	coefficients: Vec<[f32; TAPS]>,
	history: [f32; HISTORY * 2],
	input_count: u64,
	next_output: u64,
	input_rate: u32,
	output_rate: u32,
}

impl OutputResampler {
	pub fn new(input_rate: u32, output_rate: u32) -> Self {
		assert!(input_rate > 0 && output_rate > 0);
		let cutoff = 0.45 * (output_rate as f64 / input_rate as f64).min(1.0);
		let coefficients = (0..PHASES)
			.map(|phase| {
				let fraction = phase as f64 / PHASES as f64;
				let mut taps = [0.0; TAPS];
				let mut sum = 0.0;
				for (index, tap) in taps.iter_mut().enumerate() {
					let x = index as f64 - (TAPS / 2 - 1) as f64 - fraction;
					let argument = 2.0 * PI * cutoff * x;
					let sinc = if argument.abs() < 1e-12 {
						1.0
					} else {
						argument.sin() / argument
					};
					let angle = PI * x / (TAPS / 2) as f64;
					let window = 0.42 + 0.5 * angle.cos() + 0.08 * (2.0 * angle).cos();
					*tap = (2.0 * cutoff * sinc * window) as f32;
					sum += *tap;
				}
				for tap in &mut taps {
					*tap /= sum;
				}
				taps
			})
			.collect();
		Self {
			coefficients,
			history: [0.0; HISTORY * 2],
			input_count: 0,
			next_output: 0,
			input_rate,
			output_rate,
		}
	}
	pub fn reset(&mut self) {
		self.history.fill(0.0);
		self.input_count = 0;
		self.next_output = 0;
	}

	pub fn process(&mut self, input: &[f32], output: &mut Vec<f32>) {
		output.clear();
		for &sample in input {
			let slot = self.input_count as usize % HISTORY;
			self.history[slot] = sample;
			self.history[slot + HISTORY] = sample;
			self.input_count += 1;
			loop {
				let centre = self.next_output / self.output_rate as u64;
				if centre + (TAPS / 2) as u64 >= self.input_count {
					break;
				}
				let phase = ((self.next_output % self.output_rate as u64) * PHASES as u64
					/ self.output_rate as u64) as usize;
				/* Duplicated history makes the entire causal window contiguous.
				 * Before stream start, the unwritten tail supplies the same zero history. */
				let start = centre.wrapping_sub((TAPS / 2 - 1) as u64) as usize % HISTORY;
				let samples = &self.history[start..start + TAPS];
				let mut value = 0.0;
				for (sample, coefficient) in samples.iter().zip(&self.coefficients[phase]) {
					value += sample * coefficient;
				}
				output.push(value);
				self.next_output += self.input_rate as u64;
			}
		}
	}
}