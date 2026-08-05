// =======================================================
// src/sid/constants.rs — MOS 6581R4AR numeric profiles
// =======================================================

/* The reference SID is clocked from the PAL system clock used by the Assy 250466 machine. */
pub const CLOCK_FREQUENCY_HZ: f64 = 985_248.0;

/* The two on-chip DACs have different resolutions and are modelled independently throughout the voice path. */
pub const WAVEFORM_DAC_BITS: usize = 12;
pub const ENVELOPE_DAC_BITS: usize = 8;

/* One fixed seed derives a coherent set of chip-specific analogue parameters. It is not runtime randomness: every build reproduces the same reference 6581R4AR profile. */
const SYNTHETIC_CHIP_SEED: u64 = 0x0466_6581_1986_2286;

const fn profile_sample(tag: u64) -> f32 {
	let mut bits = SYNTHETIC_CHIP_SEED.wrapping_add(tag.wrapping_mul(0x9e37_79b9_7f4a_7c15));
	bits = (bits ^ (bits >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
	bits = (bits ^ (bits >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
	bits ^= bits >> 31;
	((bits >> 40) as u32) as f32 / 16_777_215.0
}

const fn profile_range(tag: u64, minimum: f32, maximum: f32) -> f32 {
	minimum + (maximum - minimum) * profile_sample(tag)
}

/* DAC ladder mismatch, leakage and electrical equilibrium define the non-ideal transfer surfaces used by waveform and envelope conversion. */
pub const SID_DAC_TWO_R_OVER_R: f64 = profile_range(1, 2.12, 2.28) as f64;
pub const SID_DAC_LEAKAGE: f64 = profile_range(2, 0.0055, 0.0090) as f64;
pub const SID_WAVEFORM_EQUILIBRIUM: usize =
	(profile_range(13, 0.20, 0.24) * ((1 << WAVEFORM_DAC_BITS) - 1) as f32) as usize;
/* Voice and mixer voltage ranges establish the analogue operating window before filter routing and output coupling. */
pub const SID_ANALOGUE_MIN_VOLTS: f32 = profile_range(3, 0.76, 0.90);
pub const SID_ANALOGUE_MAX_VOLTS: f32 = profile_range(4, 10.15, 10.48);
pub const SID_VOICE_BASE_VOLTS: f32 = profile_range(5, 4.98, 5.16);
pub const SID_VOICE_AC_VOLTS: f32 = profile_range(6, 1.42, 1.58);
pub const SID_VOICE_ENVELOPE_BIAS_VOLTS: f32 = profile_range(7, 0.19, 0.235);
pub const SID_EXTERNAL_INPUT_GAIN: f32 = 1.00;
pub const SID_MIXER_VOICE_AC_GAIN: f32 = profile_range(8, 1.04, 1.095);
pub const SID_MIXER_CONDUCTANCE_PER_INPUT: f32 = profile_range(9, 1.28, 1.38);
pub const SID_VOLUME_CONDUCTANCE_DIVISOR: f32 = profile_range(10, 11.7, 12.4);

/* The analogue model remains in internal voltage-like units until this final scale maps it into the fixed-point motherboard audio domain. */
pub const SID_OUTPUT_SCALE: f32 = 115_400.0;

/* These values represent the external C64 output network after the SID pin: low-pass loading, AC coupling and the assumed following-stage impedance. */
pub const C64_AUDIO_LOW_PASS_RESISTANCE_OHMS: f32 = 10_000.0;
pub const C64_AUDIO_LOW_PASS_CAPACITANCE_FARADS: f32 = 1_000.0e-12;
pub const C64_AUDIO_COUPLING_CAPACITANCE_FARADS: f32 = 10.0e-6;

pub const C64_AUDIO_ASSUMED_LOAD_RESISTANCE_OHMS: f32 = 47_000.0;
/* Separate fixed-point domains preserve precision in the low-pass, high-pass and accumulated output states without using floating point in the hot path. */
pub const LOW_PASS_FRACTION_BITS: u32 = 7;

pub const HIGH_PASS_FRACTION_BITS: u32 = 22;
pub const OUTPUT_STATE_FRACTION_BITS: u32 = 11;

/* MODE/VOL output is centred before gain is applied so volume-register changes can reproduce both normal mixing and digital sample playback. */
pub const SID_VOLUME_OUTPUT_CENTRE: f32 = 0.50;pub const SID_VOLUME_OUTPUT_GAIN: f32 = 1.00;

/* A stopped oscillator still loads the analogue mixer through its held DAC code. These coefficients approximate that code-dependent DC contribution. */
pub const SID_FROZEN_DAC_POLYNOMIAL: [[f32; 3]; 11] = [
	[ 0.083187386,  0.076594698, -0.006263331],
	[-0.094251172, -0.086904238,  0.007223559],
	[ 0.011220983,  0.010763546, -0.000589546],
	[-0.011110694, -0.010283382,  0.000734712],
	[-0.063138851, -0.058293584,  0.004630566],
	[ 0.004527714,  0.003662172, -0.000725594],
	[ 0.010390469,  0.009582484, -0.000785592],
	[ 0.006244834,  0.007575811,  0.001078388],
	[-0.002028794, -0.001875405,  0.000162112],
	[ 0.005621131,  0.004560351, -0.000916912],
	[ 0.006088577,  0.005621628, -0.000418789],
];
/* The filter tables discretise conductance and operating point while retaining nonlinear feedback, substrate leakage and inverter curvature. */
pub const SID_ANALOGUE_LOAD_BINS: usize = 145;
pub const SID_ANALOGUE_MAX_CONDUCTANCE: f32 = 9.0;
pub const SID_ANALOGUE_FEEDBACK_CONDUCTANCE: f32 = 1.0;
pub const SID_ANALOGUE_SUBSTRATE_LEAK_CONDUCTANCE: f32 = 0.0125;
pub const SID_ANALOGUE_QUIESCENT_VOLTS: f32 = profile_range(11, 4.43, 4.66);
pub const SID_ANALOGUE_SUBSTRATE_BIAS_VOLTS: f32 = SID_ANALOGUE_QUIESCENT_VOLTS;
pub const SID_ANALOGUE_INVERTER_STEEPNESS: f32 = profile_range(12, 2.8, 3.4);
pub const SID_RESONANCE_DAMPING_MIN: f32 = profile_range(14, 0.32, 0.42);
pub const SID_RESONANCE_DAMPING_MAX: f32 = profile_range(15, 1.72, 1.90);
pub const SID_RESONANCE_DAMPING_CURVE: f32 = profile_range(16, 0.90, 1.18);
pub const SID_RESONANCE_OUTPUT_LIFT: f32 = profile_range(20, 0.21, 0.25);
/* Resampling first converts cycle-domain output to a high intermediate rate, then performs a fixed four-to-one causal decimation to 44.1 kHz. */
pub const SID_RESAMPLER_INTERMEDIATE_HZ: f64 = 176_400.0;
pub const SID_RESAMPLER_PHASES: usize = 64;
pub const SID_RESAMPLER_FIRST_TAPS: usize = 95;
pub const SID_RESAMPLER_FIRST_CUTOFF_HZ: f64 = 20_000.0;
pub const SID_RESAMPLER_SECOND_CUTOFF_HZ: f64 = 19_000.0;
pub const SID_RESAMPLER_SECOND_DECIMATION: u64 = 4;
pub const SID_RESAMPLER_BUTTERWORTH_Q: [f64; 4] = [
	0.509_795_579, 0.601_344_887, 0.899_976_223, 2.562_915_448,
];

/* Multiple substeps stabilise the nonlinear state-variable filter; operating bins and gains define the table domain and mode summation. */
pub const FILTER_SUBSTEPS: usize = 4;
pub const FILTER_OPERATING_BINS: usize = 16;
pub const FILTER_INPUT_GAIN: f32 = 1.00;
pub const FILTER_OUTPUT_GAIN: f32 = 0.88;
pub const FILTER_MODE_POLARITY: [f32; 3] = [1.00, -1.00, 1.00];
pub const FILTER_LOWPASS_GAIN_AT_MID_RESONANCE: f32 = 1.18;
pub const FILTER_LOWPASS_GAIN_RESONANCE_DROP: f32 = 0.18;
pub const FILTER_BANDPASS_GAIN_ZERO_CUTOFF: f32 = 1.58;
pub const FILTER_BANDPASS_GAIN_CUTOFF_SLOPE: f32 = 0.61;
pub const FILTER_BANDPASS_GAIN_RESONANCE_DROP: f32 = 0.22;
pub const FILTER_BANDPASS_GAIN_TRIM_ZERO: f32 = 0.96;
pub const FILTER_BANDPASS_GAIN_TRIM_CUTOFF_SLOPE: f32 = 0.20;
pub const FILTER_LOW_BAND_GAIN_ZERO_CUTOFF: f32 = 1.378;
pub const FILTER_LOW_BAND_GAIN_CUTOFF_SLOPE: f32 = 0.407;
pub const FILTER_LOW_BAND_GAIN_RESONANCE_DROP: f32 = 0.214;
pub const FILTER_LOW_BAND_LOW_RESONANCE_LIFT: f32 = 0.20;
pub const FILTER_BYPASS_ACTIVITY_LINEAR_GAIN: f32 = 0.2756;
pub const FILTER_BYPASS_ACTIVITY_QUADRATIC_GAIN: f32 = 0.0476;
pub const FILTER_BANDPASS_LOW_COUPLING_MAX: f32 = 0.26;
pub const FILTER_BANDPASS_LOW_COUPLING_KNEE: f32 = 600.0;
pub const FILTER_HIGHPASS_LOW_COUPLING_ZERO: f32 = 0.07;
pub const FILTER_HIGHPASS_LOW_COUPLING_DECAY: f32 = 700.0;
pub const FILTER_SOURCE_ENERGY_TRACKING: f32 = 0.001;
pub const FILTER_SOURCE_ENERGY_KNEE: f32 = 0.005;
pub const FILTER_SOURCE_RESONANCE_FLOOR: f32 = 0.20;
pub const FILTER_SOURCE_CUTOFF_SENSITIVITY_HZ: f32 = 500.0;
pub const FILTER_SOURCE_CUTOFF_OPENING_START: f32 = 0.0;
pub const FILTER_SOURCE_CUTOFF_OPENING_END: f32 = 200.0;
pub const FILTER_SOURCE_CUTOFF_CLOSING_START: f32 = 500.0;
pub const FILTER_SOURCE_CUTOFF_CLOSING_END: f32 = 1100.0;
pub const FILTER_SOURCE_CUTOFF_TRANSITION_WIDTH: f32 = 160.0;
pub const FILTER_CUTOFF_FLOOR_HZ: f32 = 256.83;
pub const FILTER_CUTOFF_OPENING_SLOPE: f32 = 12.081;
pub const FILTER_CUTOFF_COMPRESSION_SLOPE: f32 = 4.410;
pub const FILTER_CUTOFF_OPENING_POSITION: f32 = 353.11;
pub const FILTER_CUTOFF_COMPRESSION_POSITION: f32 = 1_521.98;
pub const FILTER_CUTOFF_OPENING_WIDTH: f32 = 34.83;
pub const FILTER_CUTOFF_COMPRESSION_WIDTH: f32 = 36.96;
pub const FILTER_CUTOFF_FREQUENCY_SCALE: f32 = 0.90;
pub const FILTER_CUTOFF_HIGH_RESONANCE_DROP: f32 = 0.10;
pub const REGISTER_MASK: u16 = 0x001f;
pub const DATA_BUS_HOLD_CYCLES: u32 = profile_range(17, 6_000.0, 9_000.0) as u32;
pub const DATA_BUS_CHARGE_MAX: u16 = 65_535;

pub const PHASE_MASK: u32 = 0x00ff_ffff;
pub const PHASE_RESET_VALUE: u32 = ((profile_sample(18) * 16_777_215.0) as u32) & PHASE_MASK;
pub const WAVEFORM_MASK: u16 = 0x0fff;

pub const COMBINED_WAVEFORM_TABLE_SIZE: usize = 4096;
pub const BIT_COUNT: usize = 12;
pub const WAVEFORM_COUNT: usize = 16;

pub const COMBINED_WAVEFORM_NEIGHBOUR_COUPLING: f32 = 0.05931141;pub const COMBINED_WAVEFORM_DIRECTIONAL_SKEW: f32 = 0.09584916;
pub const COMBINED_WAVEFORM_DISTANCE_DECAY: f32 = 0.8333164;
pub const COMBINED_WAVEFORM_RETENTION_THRESHOLD: f32 = 0.23039357;
pub const COMBINED_WAVEFORM_PULSE_LOAD: f32 = 0.04016286;
pub const COMBINED_WAVEFORM_NOISE_LOAD: f32 = 0.00355965;
pub const COMBINED_WAVEFORM_TRIANGLE_LOAD: f32 = 0.08020278;
pub const COMBINED_WAVEFORM_SAW_LOAD: f32 = 0.02190812;
pub const COMBINED_WAVEFORM_INTERACTION_GAIN: f32 = 0.0672637;
pub const COMBINED_WAVEFORM_LOADING_SCALE: [f32; 16] = [
	1.0, 1.0, 1.0, 0.92,
	1.0, 0.89, 1.0, 0.82,
	1.0, 1.0, 1.0, 1.0,
	1.0, 1.0, 1.0, 1.0,
];

pub const WAVEFORM_PIPELINE_RESET_VALUE: u16 = 0x0000;
pub const SYNC_EVENT_MASK: u32 = 0x0080_0000;
pub const NOISE_EVENT_MASK: u32 = 0x0008_0000;
pub const NOISE_MASK: u32 = 0x007f_ffff;
pub const NOISE_RESET_VALUE: u32 = 0x007f_ffff;
pub const COMBINED_WAVEFORM_MSB_CLEAR_MASK: u32 = 0x007f_ffff;
pub const NOISE_OUTPUT_TAPS: [u8; 8] = [2, 4, 8, 11, 13, 17, 20, 22];

pub const WAVEFORM_FLOAT_HOLD_CYCLES: u32 =
	profile_range(19, 850_000.0, 1_200_000.0) as u32;
pub const WAVEFORM_FLOAT_LEAK_INTERVAL_CYCLES: u32 = 8_192;
pub const WAVEFORM_FLOAT_CHARGE_MAX: u16 = 65_535;
pub const WAVEFORM_FLOAT_THRESHOLD: u16 = 18_000;
pub const NOISE_TEST_LEAK_INTERVAL_CYCLES: u32 = 65_536;
pub const NOISE_TEST_CHARGE_MAX: u16 = 65_535;
pub const NOISE_TEST_LOGIC_THRESHOLD: u16 = 32_768;
pub const NEVER: u64 = u64::MAX;

const ENVELOPE_ATTACK_DURATION_US: [u32; 16] = [    2_000, 8_000, 16_000, 24_000,
	38_000, 56_000, 68_000, 80_000,
	100_000, 250_000, 500_000, 800_000,
	1_000_000, 3_000_000, 5_000_000, 8_000_000,
];

const fn envelope_rate_period(rate: u8) -> u32 {
	const ENVELOPE_STEPS: u64 = 255;
	const MICROSECONDS_PER_SECOND: u64 = 1_000_000;
	let duration = ENVELOPE_ATTACK_DURATION_US[(rate & 0x0f) as usize] as u64;
	let numerator = duration * CLOCK_FREQUENCY_HZ as u64;
	let denominator = ENVELOPE_STEPS * MICROSECONDS_PER_SECOND;
	let period = (numerator + denominator / 2) / denominator;
	if period == 0 { 1 } else { period as u32 }
}

const fn derive_envelope_rate_comparator(rate: u8) -> u16 {
	let mut state = 0x7fffu16;
	let mut clocks = envelope_rate_period(rate) - 1;
	while clocks != 0 {
		let feedback = ((state << 14) ^ (state << 13)) & 0x4000;
		state = (state >> 1) | feedback;
		clocks -= 1;
	}
	state
}

const fn build_envelope_rate_comparators() -> [u16; 16] {
	let mut comparators = [0u16; 16];
	let mut rate = 0usize;
	while rate < comparators.len() {
		comparators[rate] = derive_envelope_rate_comparator(rate as u8);
		rate += 1;
	}
	comparators
}

const ENVELOPE_RATE_COMPARATORS: [u16; 16] = build_envelope_rate_comparators();

#[inline(always)]
pub const fn envelope_rate_comparator(rate: u8) -> u16 {
	ENVELOPE_RATE_COMPARATORS[(rate & 0x0f) as usize]
}