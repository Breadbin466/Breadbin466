// =======================================================
// src/sid/tables.rs — MOS 6581R4AR numeric model construction
// =======================================================

use super::constants::{
	CLOCK_FREQUENCY_HZ, ENVELOPE_DAC_BITS, FILTER_CUTOFF_COMPRESSION_POSITION,
	FILTER_CUTOFF_COMPRESSION_SLOPE, FILTER_CUTOFF_COMPRESSION_WIDTH, FILTER_CUTOFF_FLOOR_HZ,
	FILTER_CUTOFF_FREQUENCY_SCALE,
	FILTER_CUTOFF_HIGH_RESONANCE_DROP,
	FILTER_CUTOFF_OPENING_POSITION, FILTER_CUTOFF_OPENING_SLOPE, FILTER_CUTOFF_OPENING_WIDTH,
	FILTER_OPERATING_BINS, FILTER_SUBSTEPS, SID_ANALOGUE_FEEDBACK_CONDUCTANCE,
	FILTER_SOURCE_CUTOFF_CLOSING_END, FILTER_SOURCE_CUTOFF_CLOSING_START,
	FILTER_SOURCE_CUTOFF_OPENING_END, FILTER_SOURCE_CUTOFF_OPENING_START,
	FILTER_SOURCE_CUTOFF_SENSITIVITY_HZ, FILTER_SOURCE_CUTOFF_TRANSITION_WIDTH,
	SID_ANALOGUE_INVERTER_STEEPNESS, SID_ANALOGUE_LOAD_BINS, SID_ANALOGUE_MAX_CONDUCTANCE, SID_ANALOGUE_MAX_VOLTS,
	SID_ANALOGUE_MIN_VOLTS, SID_ANALOGUE_QUIESCENT_VOLTS, SID_ANALOGUE_SUBSTRATE_BIAS_VOLTS,
	SID_ANALOGUE_SUBSTRATE_LEAK_CONDUCTANCE, SID_DAC_LEAKAGE, SID_DAC_TWO_R_OVER_R,
	SID_RESONANCE_DAMPING_CURVE, SID_RESONANCE_DAMPING_MAX, SID_RESONANCE_DAMPING_MIN,
	SID_WAVEFORM_EQUILIBRIUM, WAVEFORM_DAC_BITS,
};

#[derive(Clone, Copy)]
pub(super) struct FilterCoefficients {
	pub(super) a1: [[f32; FILTER_OPERATING_BINS]; 16],
	pub(super) a2: [[f32; FILTER_OPERATING_BINS]; 16],
	pub(super) a3: [[f32; FILTER_OPERATING_BINS]; 16],
	pub(super) damping: [f32; 16],
	pub(super) input_drive: [f32; FILTER_OPERATING_BINS],
	pub(super) feedback_drive: [f32; FILTER_OPERATING_BINS],
	pub(super) g_source_gain: f32,
}

/* Solves the resistor-ladder network used by the DAC builders. Keeping the electrical solve here makes the generated transfer curves reproducible rather than embedding opaque measured tables. */
fn solve_tridiagonal<const N: usize>(
	lower: &[f64; N],
	diagonal: &[f64; N],
	upper: &[f64; N],
	rhs: &[f64; N],
) -> [f64; N] {
	let mut c = [0.0; N];
	let mut d = [0.0; N];
	let mut result = [0.0; N];
	c[0] = upper[0] / diagonal[0];
	d[0] = rhs[0] / diagonal[0];
	for index in 1..N {
		let denominator = diagonal[index] - lower[index] * c[index - 1];
		c[index] = if index + 1 < N { upper[index] / denominator } else { 0.0 };
		d[index] = (rhs[index] - lower[index] * d[index - 1]) / denominator;
	}
	result[N - 1] = d[N - 1];
	for index in (0..N - 1).rev() {
		result[index] = d[index] - c[index] * result[index + 1];
	}
	result
}

fn ladder_weights<const N: usize>(two_r_over_r: f64, terminated: bool) -> [f64; N] {
	let source = 1.0 / two_r_over_r;
	let link = 1.0;
	let mut weights = [0.0; N];
	for active in 0..N {
		let mut lower = [0.0; N];
		let mut diagonal = [0.0; N];
		let mut upper = [0.0; N];
		let mut rhs = [0.0; N];
		for node in 0..N {
			diagonal[node] += source;
			if node == active { rhs[node] += source; }
			if node > 0 { lower[node] = -link; diagonal[node] += link; }
			if node + 1 < N { upper[node] = -link; diagonal[node] += link; }
		}
		if terminated { diagonal[0] += source; }
		diagonal[N - 1] += 1.0;
		weights[active] = solve_tridiagonal(&lower, &diagonal, &upper, &rhs)[N - 1];
	}
	let total: f64 = weights.iter().sum();
	for weight in &mut weights { *weight /= total; }
	weights
}

#[inline(always)]
fn ladder_value(code: usize, weights: &[f64], leakage: f64) -> f64 {
	weights.iter().enumerate().map(|(bit, weight)| {
		let level = if code & (1usize << bit) != 0 { 1.0 } else { leakage };
		level * weight
	}).sum()
}

/* Builds the eight-bit envelope ladder response, including resistor mismatch and termination rather than assuming a perfectly linear code-to-level mapping. */
pub(super) fn build_envelope_dac_table() -> [f32; 256] {
	let weights = ladder_weights::<ENVELOPE_DAC_BITS>(SID_DAC_TWO_R_OVER_R, false);
	let full_scale = ladder_value(255, &weights, SID_DAC_LEAKAGE);
	std::array::from_fn(|code| {
		(ladder_value(code, &weights, SID_DAC_LEAKAGE) / full_scale) as f32
	})
}

#[inline(always)]
fn follower_response(input: f32) -> f32 {
	let headroom = 1.0 - input;
	let lift = 0.092 * input * headroom;
	let compression = 0.018 * input * input * headroom;
	input + lift - compression
}

/* Builds the twelve-bit waveform ladder and source-follower response used by every voice conversion. */
pub(super) fn build_waveform_dac_table() -> Box<[f32; 4096]> {
	let weights = ladder_weights::<WAVEFORM_DAC_BITS>(SID_DAC_TWO_R_OVER_R, false);
	let mut table = Box::new([0.0; 4096]);
	for (code, entry) in table.iter_mut().enumerate() {
		let ladder = ladder_value(code, &weights, SID_DAC_LEAKAGE) as f32;
		*entry = follower_response(ladder);
	}
	let equilibrium = table[SID_WAVEFORM_EQUILIBRIUM];
	for entry in table.iter_mut() { *entry -= equilibrium; }
	table
}

fn smooth_conduction(position: f32, boundary: f32, width: f32) -> f32 {
	let displacement = position - boundary;
	0.5 * (displacement + displacement.mul_add(displacement, width * width).sqrt())
}

/* The cutoff control is intentionally nonuniform: a small code change can move across a very different frequency span depending on the local control-voltage region. */
fn cutoff_frequency(position: f32) -> f32 {
	let opening = smooth_conduction(
		position,
		FILTER_CUTOFF_OPENING_POSITION,
		FILTER_CUTOFF_OPENING_WIDTH,
	);
	let compression = smooth_conduction(
		position,
		FILTER_CUTOFF_COMPRESSION_POSITION,
		FILTER_CUTOFF_COMPRESSION_WIDTH,
	);
	FILTER_CUTOFF_FLOOR_HZ + FILTER_CUTOFF_OPENING_SLOPE * opening
		- FILTER_CUTOFF_COMPRESSION_SLOPE * compression
}

fn smooth_window(position: f32, opening: f32, closing: f32, width: f32) -> f32 {
	(smooth_conduction(position, opening, width)
		- smooth_conduction(position, closing, width))
		/ (closing - opening)
}

fn source_frequency_sensitivity(position: f32) -> f32 {
	let opening = smooth_window(
		position,
		FILTER_SOURCE_CUTOFF_OPENING_START,
		FILTER_SOURCE_CUTOFF_OPENING_END,
		FILTER_SOURCE_CUTOFF_TRANSITION_WIDTH,
	);
	let closing = 1.0 - smooth_window(
		position,
		FILTER_SOURCE_CUTOFF_CLOSING_START,
		FILTER_SOURCE_CUTOFF_CLOSING_END,
		FILTER_SOURCE_CUTOFF_TRANSITION_WIDTH,
	);
	FILTER_SOURCE_CUTOFF_SENSITIVITY_HZ * opening * closing
}

fn inverter_midpoint() -> f32 {
	let span = SID_ANALOGUE_MAX_VOLTS - SID_ANALOGUE_MIN_VOLTS;
	let resting_above_floor = SID_ANALOGUE_QUIESCENT_VOLTS - SID_ANALOGUE_MIN_VOLTS;
	let logistic_odds = span / resting_above_floor - 1.0;
	SID_ANALOGUE_QUIESCENT_VOLTS
		- logistic_odds.ln() / SID_ANALOGUE_INVERTER_STEEPNESS
}

fn opamp_output_analytic(input: f32) -> f32 {
	let span = SID_ANALOGUE_MAX_VOLTS - SID_ANALOGUE_MIN_VOLTS;
	let exponent = SID_ANALOGUE_INVERTER_STEEPNESS * (input - inverter_midpoint());
	SID_ANALOGUE_MIN_VOLTS + span / (1.0 + exponent.exp())
}

fn local_opamp_slope(voltage: f32) -> f32 {
	let response = opamp_output_analytic(voltage);
	let upper_distance = SID_ANALOGUE_MAX_VOLTS - response;
	let lower_distance = response - SID_ANALOGUE_MIN_VOLTS;
	SID_ANALOGUE_INVERTER_STEEPNESS * upper_distance * lower_distance
		/ (SID_ANALOGUE_MAX_VOLTS - SID_ANALOGUE_MIN_VOLTS)
}

fn build_cutoff_control_positions() -> [f32; 2048] {
	let weights = ladder_weights::<11>(SID_DAC_TWO_R_OVER_R, false);
	let zero = ladder_value(0, &weights, SID_DAC_LEAKAGE);
	let full = ladder_value(2047, &weights, SID_DAC_LEAKAGE);
	let span = full - zero;

	std::array::from_fn(|code| {
		let ladder = ladder_value(code, &weights, SID_DAC_LEAKAGE);
		let normal = ((ladder - zero) / span) as f32;
		(normal * 2047.0).clamp(0.0, 2047.0)
	})
}

/* Converts the eleven-bit cutoff code into a complete set of nonlinear integrator coefficients. The table preserves the strongly nonuniform 6581 control curve while keeping filter clocking branch-light. */
pub(super) fn build_filter_coefficients() -> Box<[FilterCoefficients; 2048]> {
	let cutoff_control = build_cutoff_control_positions();
	let sample_rate = CLOCK_FREQUENCY_HZ as f32 * FILTER_SUBSTEPS as f32;
	let damping = std::array::from_fn(|code| {
		let inverse_resonance = 1.0 - code as f32 / 15.0;
		SID_RESONANCE_DAMPING_MIN
			+ (SID_RESONANCE_DAMPING_MAX - SID_RESONANCE_DAMPING_MIN)
				* inverse_resonance.powf(SID_RESONANCE_DAMPING_CURVE)
	});
	let empty = FilterCoefficients {
		a1: [[0.0; FILTER_OPERATING_BINS]; 16],
		a2: [[0.0; FILTER_OPERATING_BINS]; 16],
		a3: [[0.0; FILTER_OPERATING_BINS]; 16],
		damping,
		input_drive: [0.0; FILTER_OPERATING_BINS],
		feedback_drive: [0.0; FILTER_OPERATING_BINS],
		g_source_gain: 0.0,
	};
	let mut table: Box<[FilterCoefficients; 2048]> = vec![empty; 2048]
		.into_boxed_slice()
		.try_into()
		.unwrap_or_else(|_| unreachable!("filter coefficient table has a fixed length"));
	for (code, entry) in table.iter_mut().enumerate() {
		let base_frequency = cutoff_frequency(cutoff_control[code]);
		let frequency_sensitivity = source_frequency_sensitivity(cutoff_control[code]);
		entry.g_source_gain = std::f32::consts::PI * frequency_sensitivity / sample_rate;
		for bin in 0..FILTER_OPERATING_BINS {
			let operating = bin as f32 / (FILTER_OPERATING_BINS - 1) as f32;
			let available_headroom = SID_ANALOGUE_MAX_VOLTS - SID_ANALOGUE_QUIESCENT_VOLTS;
			let bias_voltage = SID_ANALOGUE_QUIESCENT_VOLTS
				+ available_headroom * 0.246 * operating;
			let slope = local_opamp_slope(bias_voltage).clamp(0.08, 6.0);
			let compression = 1.0 - 0.105 * operating - 0.025 * operating * operating;
			let mobility = (compression + 0.016 * slope).clamp(0.76, 1.12);
			let frequency = (base_frequency * mobility * FILTER_CUTOFF_FREQUENCY_SCALE)
				.clamp(18.0, 19_800.0);
			for resonance in 0..16 {
				let high_resonance = ((resonance as f32 - 8.0) / 7.0).clamp(0.0, 1.0);
				let low_cutoff_weight = ((1024.0 - code as f32) / 384.0).clamp(0.0, 1.0);
				let resonance_frequency = frequency
					* (1.0 - FILTER_CUTOFF_HIGH_RESONANCE_DROP
						* high_resonance * low_cutoff_weight);
				let g = (std::f32::consts::PI * resonance_frequency / sample_rate)
					.tan().min(0.94);
				let k = damping[resonance];
				let a1 = 1.0 / (1.0 + g * (g + k));
				entry.a1[resonance][bin] = a1;
				entry.a2[resonance][bin] = g * a1;
				entry.a3[resonance][bin] = g * g * a1;
			}
			entry.input_drive[bin] = 0.72 + 0.88 * operating;
			entry.feedback_drive[bin] = 0.48 + 1.62 * operating;
		}
	}
	table
}

/* The loaded amplifier is solved to its local operating point for each table cell. This captures source loading and output compression before runtime interpolation. */
fn solve_loaded_amplifier(conductance: f32, input: f32) -> f32 {
	let minimum = SID_ANALOGUE_MIN_VOLTS;
	let maximum = SID_ANALOGUE_MAX_VOLTS;
	let source_conductance = conductance.max(0.0);
	let residual = |node: f32| {
		let feedback_voltage = opamp_output_analytic(node);
		source_conductance * (node - input)
			+ SID_ANALOGUE_FEEDBACK_CONDUCTANCE * (node - feedback_voltage)
			+ SID_ANALOGUE_SUBSTRATE_LEAK_CONDUCTANCE * (node - SID_ANALOGUE_SUBSTRATE_BIAS_VOLTS)
	};

	let mut lower = minimum;
	let mut upper = maximum;
	let mut lower_residual = residual(lower);
	for _ in 0..32 {
		let middle = 0.5 * (lower + upper);
		let middle_residual = residual(middle);
		if (middle_residual >= 0.0) == (lower_residual >= 0.0) {
			lower = middle;
			lower_residual = middle_residual;
		} else {
			upper = middle;
		}
	}

	opamp_output_analytic(0.5 * (lower + upper))
}

/* The transfer surface tabulates the loaded analogue amplifier over conductance and input voltage. Bilinear sampling lets mixer and volume stages move continuously between the solved operating points. */
pub(super) struct AnalogueTransferSurface {
	table: Box<[f32]>,
}

impl AnalogueTransferSurface {
	#[inline(always)]
	pub(super) fn sample(&self, conductance: f32, input: f32) -> f32 {
		let load_position = conductance.clamp(0.0, SID_ANALOGUE_MAX_CONDUCTANCE)
			* (SID_ANALOGUE_LOAD_BINS - 1) as f32
			/ SID_ANALOGUE_MAX_CONDUCTANCE;
		let load_lower = load_position as usize;
		let load_upper = (load_lower + 1).min(SID_ANALOGUE_LOAD_BINS - 1);
		let load_fraction = load_position - load_lower as f32;

		let input_position = input.clamp(0.0, 1.0) * 2047.0;
		let input_lower = input_position as usize;
		let input_upper = (input_lower + 1).min(2047);
		let input_fraction = input_position - input_lower as f32;

		let lower_base = load_lower * 2048;
		let upper_base = load_upper * 2048;
		let lower_left = self.table[lower_base + input_lower];
		let lower_right = self.table[lower_base + input_upper];
		let upper_left = self.table[upper_base + input_lower];
		let upper_right = self.table[upper_base + input_upper];
		let lower_value = lower_left + (lower_right - lower_left) * input_fraction;
		let upper_value = upper_left + (upper_right - upper_left) * input_fraction;
		lower_value + (upper_value - lower_value) * load_fraction
	}
}

pub(super) fn build_analogue_transfer_surface() -> AnalogueTransferSurface {
	let minimum = SID_ANALOGUE_MIN_VOLTS;
	let maximum = SID_ANALOGUE_MAX_VOLTS;
	let span = maximum - minimum;
	let mut table = vec![0.0f32; SID_ANALOGUE_LOAD_BINS * 2048];
	for load_bin in 0..SID_ANALOGUE_LOAD_BINS {
		let conductance = load_bin as f32 * SID_ANALOGUE_MAX_CONDUCTANCE
			/ (SID_ANALOGUE_LOAD_BINS - 1) as f32;
		let base = load_bin * 2048;
		for code in 0..2048 {
			let normalised_input = code as f32 / 2047.0;
			let voltage = minimum + normalised_input * span;
			let output = solve_loaded_amplifier(conductance, voltage);
			table[base + code] = (output - minimum) / span;
		}
	}
	AnalogueTransferSurface { table: table.into_boxed_slice() }
}