// =======================================================
// src/sid/waveforms.rs — MOS 6581R4AR waveform coupling
// =======================================================

use super::constants::{
	COMBINED_WAVEFORM_DIRECTIONAL_SKEW, COMBINED_WAVEFORM_DISTANCE_DECAY,
	COMBINED_WAVEFORM_INTERACTION_GAIN, COMBINED_WAVEFORM_NEIGHBOUR_COUPLING,
	COMBINED_WAVEFORM_LOADING_SCALE,
	COMBINED_WAVEFORM_NOISE_LOAD,
	COMBINED_WAVEFORM_PULSE_LOAD, COMBINED_WAVEFORM_RETENTION_THRESHOLD,
	COMBINED_WAVEFORM_SAW_LOAD, COMBINED_WAVEFORM_TABLE_SIZE,
	COMBINED_WAVEFORM_TRIANGLE_LOAD, BIT_COUNT, WAVEFORM_COUNT, WAVEFORM_MASK,
};

/* Combined-waveform tables are generated from an electrical line-coupling model rather than arithmetic mixing. Only selections with several active drivers need a settled twelve-line table; pure waveforms remain direct oscillator paths. */
pub struct GeneratedWaveShapes {
	/* None marks a pure waveform path; combined selections store the settled twelve-line network for every input code. */
	loaded: [Option<Box<[u16]>>; WAVEFORM_COUNT],
}

impl GeneratedWaveShapes {
	/* All combined-waveform lookup tables are generated once from the same line-coupling model, ensuring that runtime selection changes only which electrical network is sampled. */
	pub fn new() -> Self {
		Self {
			loaded: std::array::from_fn(|selection| {
				let selection = selection as u8;
				if selection.count_ones() < 2 {
					None
				} else {
					Some(LineArray::new(selection).generate().into_boxed_slice())
				}
			}),
		}
	}

	#[inline(always)]
	/* A single waveform passes through unchanged; several active drivers select the precomputed fixed point of their line-loading network. */
	pub fn apply_line_loading(&self, waveform: u8, value: u16) -> u16 {
		let selection = usize::from(waveform & 0x0f);
		match &self.loaded[selection] {
			Some(table) => table[usize::from(value & WAVEFORM_MASK)],
			None => value & WAVEFORM_MASK,
		}
	}

	#[inline(always)]
	/* Line loading is required only when at least two waveform drivers contend for the same twelve analogue lines. */
	pub const fn needs_line_loading(&self, waveform: u8) -> bool {
		(waveform & 0x0f).count_ones() > 1
	}
}

impl Default for GeneratedWaveShapes {
	fn default() -> Self { Self::new() }
}

/* LineArray iteratively settles the coupled waveform bus. Pulled-low neighbours reduce each line until the network reaches the fixed point represented in the lookup table. */
struct LineArray {
	selection: u8,
	coupling: [[f32; BIT_COUNT]; BIT_COUNT],
	static_load: f32,
}

impl LineArray {
	fn new(selection: u8) -> Self {
		let mut coupling = [[0.0; BIT_COUNT]; BIT_COUNT];
		for destination in 0..BIT_COUNT {
			for source in 0..BIT_COUNT {
				if destination == source {
					continue;
				}
				let signed_distance = source as isize - destination as isize;
				let distance = signed_distance.unsigned_abs() as i32;
				let direction = if signed_distance < 0 { -1.0 } else { 1.0 };
				coupling[destination][source] = COMBINED_WAVEFORM_NEIGHBOUR_COUPLING
					* COMBINED_WAVEFORM_DISTANCE_DECAY.powi(distance - 1)
					* (1.0 + direction * COMBINED_WAVEFORM_DIRECTIONAL_SKEW);
			}
		}

		let static_load = if selection & 0x01 != 0 { COMBINED_WAVEFORM_TRIANGLE_LOAD } else { 0.0 }
			+ if selection & 0x02 != 0 { COMBINED_WAVEFORM_SAW_LOAD } else { 0.0 }
			+ if selection & 0x04 != 0 { COMBINED_WAVEFORM_PULSE_LOAD } else { 0.0 }
			+ if selection & 0x08 != 0 { COMBINED_WAVEFORM_NOISE_LOAD } else { 0.0 };

		Self { selection, coupling, static_load }
	}

	fn generate(&self) -> Vec<u16> {
		(0..COMBINED_WAVEFORM_TABLE_SIZE)
			.map(|value| self.settle(value as u16))
			.collect()
	}

	/* Iteration stops only after the coupled lines reach a fixed point. The result is therefore independent of arbitrary driver-evaluation order. */
	fn settle(&self, value: u16) -> u16 {
		let mut charge = [0.0f32; BIT_COUNT];
		for bit in 0..BIT_COUNT {
			charge[bit] = f32::from((value >> bit) & 1);
		}

		let interaction_scale = 1.0
			+ COMBINED_WAVEFORM_INTERACTION_GAIN
				* (self.selection.count_ones() as f32 - 2.0);
		let loading_scale = COMBINED_WAVEFORM_LOADING_SCALE[self.selection as usize];
		for _ in 0..3 {
			let previous = charge;
			for destination in 0..BIT_COUNT {
				if previous[destination] <= 0.0 {
					continue;
				}
				let mut discharge = self.static_load;
				for source in 0..BIT_COUNT {
					discharge += (1.0 - previous[source]) * self.coupling[destination][source];
				}
				charge[destination] = (previous[destination]
					- discharge * interaction_scale * loading_scale)
					.clamp(0.0, 1.0);
			}
		}

		let mut output = 0u16;
		for (bit, level) in charge.iter().enumerate() {
			if *level >= COMBINED_WAVEFORM_RETENTION_THRESHOLD {
				output |= 1 << bit;
			}
		}
		output
	}
}

#[inline(always)]
/* Triangle folds the phase accumulator around bit 11 and shifts the result so both half-cycles span the full twelve-bit DAC range. */
pub const fn triangle_from_phase(phase: u16) -> u16 {
	let folded = if phase & 0x0800 == 0 { phase } else { !phase };
	(folded << 1) & WAVEFORM_MASK
}