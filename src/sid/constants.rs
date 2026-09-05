// =======================================================
// src/sid/constants.rs — SID hardware and model constants
// =======================================================

/* Fixed numeric profile and hardware constants for the MOS 6581 model. */

/* The SID is clocked from the PAL system clock used by the Assy 250466 machine. */
pub const CLOCK_FREQUENCY_HZ: f64 = 985_248.0;

/* The two on-chip DACs have different resolutions and are modelled independently throughout the voice path. */
pub const WAVEFORM_DAC_BITS: usize = 12;
pub const ENVELOPE_DAC_BITS: usize = 8;

/* The numeric profile is explicit and deterministic. Effective parameters are used where individual analogue components cannot be isolated from the observable chip response. */

/* DAC ladder mismatch, leakage and electrical equilibrium define the non-ideal transfer surfaces used by waveform and envelope conversion. */
/* Die reconstruction of the 6581 waveform and envelope ladders gives an effective R/2.02R ratio once the finite output impedance of the digital drivers is included (SID-SCHEMATICS-6581-DACS). The filter-frequency ladder is a separate 12-bit network and therefore has its own ratio. */
pub const SID_VOICE_DAC_TWO_R_OVER_R: f64 = 2.02;
/* The four most-significant waveform-ladder branches include small geometry trims. The envelope ladder retains the common 2.02R construction and does not use these waveform-only corrections. */
pub const SID_WAVEFORM_DAC_BIT_WEIGHT_TRIM: [f64; 12] = [
	1.0,
	1.0,
	1.0,
	1.0,
	1.0,
	1.0,
	1.0,
	1.077_000_000,
	1.080_000_000,
	1.057_717_820,
	1.047_111_500,
	1.034_347_076,
];
/* Source-follower curvature adds a small symmetric correction to the waveform DAC while preserving its endpoints. */
pub const SID_WAVEFORM_FOLLOWER_LIFT: f32 = 0.054_30;
pub const SID_WAVEFORM_FOLLOWER_COMPRESSION: f32 = 0.018;
/* The pure-triangle driver rounds a small fraction of its ideal folded ramp towards its analogue trajectory while keeping both endpoints fixed. */
pub const PURE_TRIANGLE_FUNDAMENTAL_ROUNDING: f32 = 0.069_2;
/* Endpoint-preserving even-order curvature models the residual asymmetry of the pure-triangle analogue transfer without altering saw or combined waveforms. */
pub const PURE_TRIANGLE_EVEN_CURVATURE_QUADRATIC: f32 = 0.002_75;
pub const PURE_TRIANGLE_EVEN_CURVATURE_QUARTIC: f32 = -0.006_05;
pub const SID_FILTER_DAC_TWO_R_OVER_R: f64 = 1.879;
pub const SID_DAC_LEAKAGE: f64 = 0.007_085_218;
pub const SID_WAVEFORM_EQUILIBRIUM: usize = 866;
/* Voice and mixer voltage ranges establish the analogue operating window before filter routing and output coupling. */
pub const SID_ANALOGUE_MIN_VOLTS: f32 = 0.822_775_6;
pub const SID_ANALOGUE_MAX_VOLTS: f32 = 10.412_254;
pub const SID_VOICE_BASE_VOLTS: f32 = 5.146_194;
pub const SID_VOICE_AC_VOLTS: f32 = 1.439_877_6;
pub const SID_VOICE_ENVELOPE_BIAS_VOLTS: f32 = 0.206_046_83;
pub const SID_EXTERNAL_INPUT_GAIN: f32 = 1.00;
pub const SID_MIXER_VOICE_AC_GAIN: f32 = 1.046_330_5;
/* Mixer conductance changes only weakly as additional voices become active, so one small per-input loading term is sufficient. */
pub const SID_MIXER_CONDUCTANCE_PER_INPUT: f32 = 1.09;
/* Two-voice AC loading is centred on the mixer quiescent point so it does not alter the DC operating point. */
pub const SID_MIXER_TWO_VOICE_ACTIVITY_LOAD: f32 = 0.0;
pub const SID_MIXER_TWO_VOICE_VOLUME_GAIN_AT_CODE_9: f32 = 1.006_400_7;
pub const SID_MIXER_TWO_VOICE_VOLUME_GAIN_PER_CODE: f32 = 0.0;
pub const SID_MIXER_THREE_VOICE_ACTIVITY_LIFT: f32 = 0.094_890_7;
/* This divisor maps the four volume-DAC branch conductances onto the loaded-amplifier conductance axis. */
pub const SID_VOLUME_CONDUCTANCE_DIVISOR: f32 = 12.280_097;

/* The four-bit volume DAC is not represented as four perfectly binary independent branches. Non-binary branch strengths and a shared-node compression term model the finite common conductance as more branches turn on. */
pub const SID_VOLUME_DAC_BRANCH_WEIGHTS: [f32; 4] = [1.0, 2.040_697_6, 4.179_677, 8.836_269];
pub const SID_VOLUME_DAC_CONDUCTANCE_SCALE: f32 = 1.076_082_2;
pub const SID_VOLUME_DAC_COMMON_COMPRESSION: f32 = 0.007_916_901;

/* A small high-level VCA compression is applied independently of the eight-bit envelope ladder. The response remains unity at full scale so the $ff envelope anchor is unchanged. */
pub const SID_ENVELOPE_VCA_COMPRESSION: f64 = 1.004_782_3;
pub const SID_ENVELOPE_VCA_COMPRESSION_POWER: f64 = 4.316_476_7;

/* The analogue model remains in internal voltage-like units until this scale maps it into the fixed-point motherboard audio domain, retaining headroom for bypass and resonance peaks. */
pub const SID_OUTPUT_SCALE: f32 = 268_904.72;

/* These values represent the external C64 output network after the SID pin: low-pass loading, AC coupling and the assumed following-stage impedance. */
pub const C64_AUDIO_LOW_PASS_RESISTANCE_OHMS: f32 = 1_000.0;
pub const C64_AUDIO_LOW_PASS_CAPACITANCE_FARADS: f32 = 1_000.0e-12;
pub const C64_AUDIO_COUPLING_CAPACITANCE_FARADS: f32 = 10.0e-6;

/* Together with the 10 uF coupling capacitor, this effective following-stage load sets the motherboard output high-pass time constant. */
pub const C64_AUDIO_ASSUMED_LOAD_RESISTANCE_OHMS: f32 = 963.0;
/* Separate fixed-point domains preserve precision in the low-pass, high-pass and accumulated output states without using floating point in the hot path. */
pub const LOW_PASS_FRACTION_BITS: u32 = 7;

pub const HIGH_PASS_FRACTION_BITS: u32 = 22;
pub const OUTPUT_STATE_FRACTION_BITS: u32 = 11;

/* MODE/VOL output is centred before gain is applied so volume-register changes can reproduce both normal mixing and digital sample playback. */
pub const SID_VOLUME_OUTPUT_CENTRE: f32 = 0.50;
pub const SID_VOLUME_OUTPUT_GAIN: f32 = 1.00;
/* The four-bit D418 volume DAC contributes a code-dependent DC bias independently of nonlinear signal transfer. Keeping this term additive preserves waveform-dependent operating-point shifts in the analogue path. */
pub const SID_VOLUME_DAC_BIAS_FULL: f32 = -0.047_231_26;

/* D418 conductance changes displace two charge reservoirs with different relaxation times and opposite output polarity. Their full-scale amplitudes are expressed in internal audio units; intermediate codes follow the non-ideal conductance law. */
pub const SID_VOLUME_TRANSITION_FAST_CHARGE_FULL_SCALE: f32 = 3_152.062_3;
pub const SID_VOLUME_TRANSITION_FAST_CHARGE_TIME_SECONDS: f32 = 0.001_834_690_7;
pub const SID_VOLUME_TRANSITION_SLOW_CHARGE_FULL_SCALE: f32 = 2_777.722_2;
pub const SID_VOLUME_TRANSITION_SLOW_CHARGE_TIME_SECONDS: f32 = 0.082_309_63;

/* A stopped oscillator still loads the analogue mixer through its held DAC code. These coefficients approximate that code-dependent DC contribution. */
pub const SID_FROZEN_DAC_POLYNOMIAL: [[f32; 3]; 11] = [
	[0.083187386, 0.076594698, -0.006263331],
	[-0.094251172, -0.086904238, 0.007223559],
	[0.011220983, 0.010763546, -0.000589546],
	[-0.011110694, -0.010283382, 0.000734712],
	[-0.063138851, -0.058293584, 0.004630566],
	[0.004527714, 0.003662172, -0.000725594],
	[0.010390469, 0.009582484, -0.000785592],
	[0.006244834, 0.007575811, 0.001078388],
	[-0.002028794, -0.001875405, 0.000162112],
	[0.005621131, 0.004560351, -0.000916912],
	[0.006088577, 0.005621628, -0.000418789],
];
/* The filter tables discretise conductance and operating point while retaining nonlinear feedback, substrate leakage and inverter curvature. */
pub const SID_ANALOGUE_LOAD_BINS: usize = 145;
pub const SID_ANALOGUE_MAX_CONDUCTANCE: f32 = 9.0;
pub const SID_ANALOGUE_FEEDBACK_CONDUCTANCE: f32 = 1.0;
pub const SID_ANALOGUE_SUBSTRATE_LEAK_CONDUCTANCE: f32 = 0.0125;
pub const SID_ANALOGUE_QUIESCENT_VOLTS: f32 = 4.432_360_6;
pub const SID_ANALOGUE_SUBSTRATE_BIAS_VOLTS: f32 = SID_ANALOGUE_QUIESCENT_VOLTS;
pub const SID_ANALOGUE_INVERTER_STEEPNESS: f32 = 2.944_879_8;
/* Strong resonance rises gently in band-pass and high-pass operation instead of following an ideal state-variable response. */
pub const SID_RESONANCE_DAMPING_MIN: f32 = 0.39;
pub const SID_RESONANCE_DAMPING_MAX: f32 = 1.84;
pub const SID_RESONANCE_DAMPING_CURVE: f32 = 1.08;
pub const SID_RESONANCE_OUTPUT_LIFT: f32 = 0.12;
/* Resampling first converts cycle-domain output to a high intermediate rate, then performs a fixed four-to-one causal decimation to 44.1 kHz. */
pub const SID_RESAMPLER_PHASES: usize = 64;
pub const SID_RESAMPLER_FIRST_TAPS: usize = 95;
pub const SID_RESAMPLER_FIRST_CUTOFF_HZ: f64 = 20_000.0;
pub const SID_RESAMPLER_SECOND_CUTOFF_HZ: f64 = 19_000.0;
pub const SID_RESAMPLER_SECOND_DECIMATION: u64 = 4;
pub const SID_RESAMPLER_BUTTERWORTH_Q: [f64; 4] =
	[0.509_795_579, 0.601_344_887, 0.899_976_223, 2.562_915_448];

/* Multiple substeps stabilise the nonlinear state-variable filter; operating bins and gains define the table domain and mode summation. */
pub const FILTER_SUBSTEPS: usize = 4;
pub const FILTER_OPERATING_BINS: usize = 16;
pub const FILTER_INPUT_GAIN: f32 = 1.00;
pub const FILTER_OUTPUT_GAIN: f32 = 0.88;
pub const FILTER_MODE_POLARITY: [f32; 3] = [1.00, -1.00, 1.00];
pub const FILTER_LOWPASS_GAIN_AT_MID_RESONANCE: f32 = 1.18;
pub const FILTER_LOWPASS_GAIN_RESONANCE_DROP: f32 = 0.18;
/* Mode-global output loading changes amplitude without altering the cutoff or resonance state evolution. */
pub const FILTER_LOWPASS_OUTPUT_TRIM: f32 = 0.910_872_7;
pub const FILTER_BANDPASS_GAIN_ZERO_CUTOFF: f32 = 2.169_565_7;
pub const FILTER_BANDPASS_GAIN_CUTOFF_SLOPE: f32 = 0.988_411_8;
pub const FILTER_BANDPASS_GAIN_RESONANCE_DROP: f32 = 0.612_764_4;
pub const FILTER_BANDPASS_GAIN_TRIM_ZERO: f32 = 0.92;
pub const FILTER_BANDPASS_GAIN_TRIM_CUTOFF_SLOPE: f32 = 0.16;
pub const FILTER_BANDPASS_OUTPUT_TRIM: f32 = 0.846_128_7;
/* Residual single-mode loading is represented by smooth cutoff-indexed curves. Low-pass remains the unity reference. */
pub const FILTER_SINGLE_MODE_CUTOFF_GAIN: [[f32; 32]; 3] = [
	[
		1.0, 1.0, 1.23353623, 0.89895064, 0.86740986, 0.85526656, 0.84718767, 0.84884046,
		0.86227679, 0.87671523, 0.88607646, 0.89402053, 0.90257817, 0.91483444, 0.92907219,
		0.94157819, 0.95622251, 0.97426037, 0.99266103, 1.01210112, 1.02934943, 1.03989924,
		1.04441322, 1.04383719, 1.04116640, 1.03644809, 1.02943435, 1.02353474, 1.05295808, 1.0,
		1.0, 1.0,
	],
	[
		0.89093639, 1.13634142, 0.97812194, 0.86039058, 0.91946334, 0.96311392, 1.00244342,
		1.04029696, 1.06731510, 1.06559903, 1.05455488, 1.06080772, 1.08175473, 1.09638590,
		1.09767025, 1.11775615, 1.17183848, 1.23149359, 1.27929831, 1.30239330, 1.30120409,
		1.29489753, 1.27214687, 1.22549025, 1.17642266, 1.14213297, 1.10644345, 1.01442249,
		1.10323332, 1.68230621, 1.37379690, 0.49436831,
	],
	[
		1.31723193, 1.14871818, 1.05818941, 1.03192606, 1.00364194, 0.97844439, 0.96093557,
		0.95107789, 0.94767043, 0.95145714, 0.96612163, 1.00017911, 1.03907290, 1.07079645,
		1.13330182, 1.25638278, 1.41911372, 1.56058533, 1.64288642, 1.68253238, 1.69442833,
		1.70359570, 1.72694245, 1.74100566, 1.77025222, 1.84478685, 1.90582228, 1.85591364,
		1.66984516, 2.55944267, 2.51802873, 0.52698360,
	],
];
pub const FILTER_SINGLE_MODE_RESONANCE_GAIN: [[f32; 16]; 3] = [
	[
		1.00630351, 1.00073388, 0.99603216, 0.98889116, 0.99926666, 0.99028775, 0.99058415,
		0.98036540, 0.99781395, 1.01537258, 1.02951897, 1.03220971, 1.04246099, 1.03675808,
		1.02646305, 0.99761809,
	],
	[
		1.05848653, 1.03617474, 1.02286838, 1.00757282, 0.98742304, 0.96558247, 0.94568571,
		0.92456103, 0.92323363, 0.94834975, 0.97143255, 1.00321903, 1.02699835, 1.02721612,
		1.04865660, 1.04748858,
	],
	[
		0.99266444, 0.97056585, 0.96428306, 0.96636973, 0.97260598, 0.98380531, 0.98886633,
		0.99618476, 1.01294612, 1.01406545, 1.01395639, 1.00681010, 1.00532142, 0.99928534,
		1.00667762, 1.04021323,
	],
];
/* Output-node loading caused by cutoff-DAC transition charge. Each
 * row is the gain for a saturated increasing FC transition; decreasing
 * transitions use its inverse and smaller steps scale logarithmically. */
pub const FILTER_DIRECTIONAL_LOADING_GAIN: [[f32; 32]; 3] = [
	[
		1.00000000, 1.00000000, 0.93765405, 0.94081614, 0.99449308, 1.00244643, 0.95505805,
		0.93066433, 0.93696096, 0.95613242, 0.97951331, 0.97899309, 0.96035296, 0.94564853,
		0.94146301, 0.94814680, 0.96259405, 0.98077592, 0.99445758, 0.99666390, 0.99269815,
		0.98984601, 0.98941140, 0.99007803, 0.99221018, 0.99852738, 1.00511773, 1.00695487,
		0.98709561, 1.00000000, 1.00000000, 1.00000000,
	],
	[
		1.27847607, 0.93319057, 1.10955689, 0.99774913, 0.91342853, 0.92918657, 0.93319293,
		0.95360476, 0.95059116, 0.97433434, 0.97005751, 0.99825384, 1.00189309, 0.99971844,
		0.99470656, 1.00254850, 1.01009023, 1.00238117, 1.02629542, 1.03472767, 1.03563478,
		1.03140479, 1.05261832, 1.06349830, 1.05430574, 1.05401898, 1.08311857, 1.03183261,
		0.82135649, 0.93798117, 1.67687657, 1.26060993,
	],
	[
		1.04859041, 1.07546298, 1.03340730, 1.00812060, 1.01630685, 1.01212524, 1.00673030,
		1.01586716, 1.01943717, 1.01488782, 1.01866306, 1.03333549, 1.04897587, 1.00919125,
		0.99459604, 1.04383455, 1.05054944, 1.04520286, 1.05583242, 1.07213932, 1.07807940,
		1.08217416, 1.09811072, 1.09974413, 1.09055189, 1.08028820, 1.09888608, 1.14626051,
		0.91357403, 0.88178420, 1.85312496, 1.43249691,
	],
];
pub const FILTER_SYMMETRIC_LOADING_GAIN: [[f32; 32]; 3] = [
	[
		1.00000000, 1.00000000, 1.00000000, 1.00000000, 1.00000000, 1.00000000, 1.00000000,
		1.00000000, 1.00000000, 1.00000000, 1.00000000, 1.00000000, 1.00000000, 1.00000000,
		1.00000000, 1.00000000, 1.00000000, 1.00000000, 1.00000000, 1.00000000, 1.00000000,
		1.00000000, 1.00000000, 1.00000000, 1.00000000, 1.00000000, 1.00000000, 1.00000000,
		1.00000000, 1.00000000, 1.00000000, 1.00000000,
	],
	[
		1.09330756, 0.82283497, 0.83008611, 0.91884152, 0.98863140, 0.99929709, 1.02952338,
		1.03804137, 1.01706013, 1.01167829, 1.01613454, 1.02907269, 1.03903550, 1.01683951,
		0.98966821, 0.98175867, 0.98936190, 1.02742924, 1.03887273, 1.04408989, 1.04965008,
		1.03288924, 1.01529653, 0.99038672, 1.02531015, 1.02002558, 1.04102769, 0.99527514,
		0.83526745, 0.81144426, 1.05536789, 1.63472715,
	],
	[
		0.91209361, 0.94250347, 0.99888706, 1.02097301, 1.01384441, 1.01338901, 1.00833940,
		1.00595227, 1.00686148, 1.00283746, 1.00495545, 1.01693755, 1.02628255, 0.99278781,
		0.97194577, 0.98594703, 0.99127781, 1.04047916, 1.04668138, 1.04619147, 1.04717928,
		1.03226866, 1.02158493, 0.99095460, 1.03283741, 1.03252721, 1.04036051, 1.08427591,
		0.94379028, 0.93381396, 1.39023005, 2.55917735,
	],
];

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

/* These coefficients describe the low- and mid-cutoff resonance dependence of high-pass output loading. */
pub const FILTER_HIGHPASS_GAIN_ZERO: f32 = 0.675_766_37;
pub const FILTER_HIGHPASS_GAIN_CUTOFF_SLOPE: f32 = -0.650_767_5;
pub const FILTER_HIGHPASS_GAIN_RESONANCE_DROP: f32 = 0.026_494_04;
pub const FILTER_HIGHPASS_GAIN_CUTOFF_RESONANCE_DROP: f32 = 0.833_295_6;
/* A single saturating loading term is used near the high end of FC, avoiding a per-code or per-resonance correction table. */
pub const FILTER_HIGHPASS_HIGH_CUTOFF_LOAD: f32 = 3.110_968;
pub const FILTER_HIGHPASS_HIGH_CUTOFF_KNEE: f32 = 0.863_042_5;
pub const FILTER_HIGHPASS_HIGH_CUTOFF_EXPONENT: f32 = 10.211_241;
/* High-pass output loading depends smoothly on resonance and grows with FC. Multiplication by normalised FC anchors the correction to unity at FC=$000. */
pub const FILTER_HIGHPASS_RESONANCE_TRIM_C0: f32 = -0.118_600_82;
pub const FILTER_HIGHPASS_RESONANCE_TRIM_C1: f32 = -0.962_186_8;
pub const FILTER_HIGHPASS_RESONANCE_TRIM_C2: f32 = 1.492_663_4;
pub const FILTER_HIGHPASS_RESONANCE_TRIM_C3: f32 = 2.835_841_4;
pub const FILTER_SOURCE_ENERGY_TRACKING: f32 = 0.001;
pub const FILTER_SOURCE_ENERGY_KNEE: f32 = 0.005;
pub const FILTER_SOURCE_RESONANCE_FLOOR: f32 = 0.20;
pub const FILTER_SOURCE_CUTOFF_SENSITIVITY_HZ: f32 = 500.0;
pub const FILTER_SOURCE_CUTOFF_OPENING_START: f32 = 0.0;
pub const FILTER_SOURCE_CUTOFF_OPENING_END: f32 = 200.0;
pub const FILTER_SOURCE_CUTOFF_CLOSING_START: f32 = 500.0;
pub const FILTER_SOURCE_CUTOFF_CLOSING_END: f32 = 1100.0;
pub const FILTER_SOURCE_CUTOFF_TRANSITION_WIDTH: f32 = 160.0;
pub const FILTER_CUTOFF_FLOOR_HZ: f32 = 257.116_12;
pub const FILTER_CUTOFF_SATURATION_HZ: f32 = 20_199.891;
pub const FILTER_CUTOFF_SIGMOID_CENTRE: f32 = 908.0;
pub const FILTER_CUTOFF_SIGMOID_WIDTH: f32 = 145.0;
pub const FILTER_CUTOFF_FREQUENCY_SCALE: f32 = 0.88;
/* FC transitions inject charge into the cutoff DAC. The displacement scales with the written-code step and saturates before it can move the effective cutoff outside the valid range. */
pub const FILTER_CUTOFF_STEP_CHARGE_GAIN: f32 = 0.8592;
pub const FILTER_CUTOFF_STEP_CHARGE_LIMIT_CODES: i16 = 57;
pub const FILTER_CUTOFF_HIGH_RESONANCE_DROP: f32 = 0.10;
pub const REGISTER_MASK: u16 = 0x001f;
/* The eight SID data lines retain charge for different intervals. Each entry is the effective survival threshold for one physical bit before a write-only register read loses that retained high level. */
pub const DATA_BUS_HOLD_CYCLES: [u32; 8] = [
	12,     /* bit 0: < minimum observable read latency */
	26_624, /* bit 1: 24_576 .. 28_672 */
	34_816, /* bit 2: 32_768 .. 36_864 */
	38_912, /* bit 3: 36_864 .. 40_960 */
	7_680,  /* bit 4: 7_168 .. 8_192 */
	11_264, /* bit 5: 10_240 .. 12_288 */
	15_360, /* bit 6: 14_336 .. 16_384 */
	43_008, /* bit 7: 40_960 .. 45_056 */
];

pub const PHASE_MASK: u32 = 0x00ff_ffff;
pub const PHASE_RESET_VALUE: u32 = 10_977_228;
pub const WAVEFORM_MASK: u16 = 0x0fff;

pub const COMBINED_WAVEFORM_TABLE_SIZE: usize = 4096;
pub const BIT_COUNT: usize = 12;
pub const WAVEFORM_COUNT: usize = 16;

pub const COMBINED_WAVEFORM_NEIGHBOUR_COUPLING: f32 = 0.05931141;
pub const COMBINED_WAVEFORM_DIRECTIONAL_SKEW: f32 = 0.09584916;

/* Triangle+saw uses the opposite line-direction asymmetry from the other combined-driver arrangements. */
pub const COMBINED_WAVEFORM_DIRECTIONAL_SKEW_BY_SELECTION: [f32; 16] = [
	0.09584916, 0.09584916, 0.09584916, -0.40, 0.09584916, -0.0385, 0.09584916, 0.09584916,
	0.09584916, 0.09584916, 0.09584916, 0.09584916, 0.09584916, 0.09584916, 0.09584916, 0.09584916,
];
pub const COMBINED_WAVEFORM_DISTANCE_DECAY: f32 = 0.8333164;

/* Triangle+saw uses a shorter effective coupling reach once combined-waveform feedback is included; other selections retain the common distance law. */
pub const COMBINED_WAVEFORM_DISTANCE_DECAY_BY_SELECTION: [f32; 16] = [
	0.8333164, 0.8333164, 0.8333164, 0.37161743, 0.8333164, 0.83625, 0.8333164, 0.8333164,
	0.8333164, 0.8333164, 0.8333164, 0.8333164, 0.8333164, 0.8333164, 0.8333164, 0.8333164,
];
pub const COMBINED_WAVEFORM_RETENTION_THRESHOLD: f32 = 0.23039357;

/* Combined-driver topologies use different digital retention thresholds on the twelve resolved waveform lines before DAC conversion. Pure-waveform entries are inert because they bypass line loading. */
pub const COMBINED_WAVEFORM_RETENTION_THRESHOLD_BY_SELECTION: [f32; 16] = [
	0.23039357, 0.23039357, 0.23039357, 0.39408336, 0.23039357, 0.23039357, 0.29368378, 0.28520789,
	0.23039357, 0.29799863, 0.27780770, 0.28648792, 0.33473923, 0.32701850, 0.31776808, 0.32491357,
];
pub const COMBINED_WAVEFORM_PULSE_LOAD: f32 = 0.04016286;
pub const COMBINED_WAVEFORM_NOISE_LOAD: f32 = 0.00355965;
pub const COMBINED_WAVEFORM_TRIANGLE_LOAD: f32 = 0.08020278;
pub const COMBINED_WAVEFORM_SAW_LOAD: f32 = 0.02190812;
/* Combined-driver displacement at the SID output operating point. The board
 * coupling capacitor exposes changes in this otherwise inaudible DC level. */
pub const COMBINED_OUTPUT_COMMON_MODE: f32 = 0.014_051_97;
/* A pure waveform contributes a separate VCA operating-point displacement while connected to the unfiltered mixer. The motherboard coupling capacitor exposes changes in this otherwise static level at GATE attack and release. Combined and routed voices use their own operating-point paths. */
pub const PURE_BYPASS_OUTPUT_COMMON_MODE: f32 = 0.018_140_94;
/* Active filter branches have distinct output-node operating points. The two cutoff endpoints are interpolated to form one continuous common-mode law for each branch. */
pub const FILTER_LOWPASS_COMMON_MODE_FC_0: f32 = -0.034_882_37;
pub const FILTER_LOWPASS_COMMON_MODE_FC_2047: f32 = -0.036_773_43;
pub const FILTER_BANDPASS_COMMON_MODE_FC_0: f32 = -0.005_140_44;
pub const FILTER_BANDPASS_COMMON_MODE_FC_2047: f32 = -0.005_218_12;
pub const FILTER_HIGHPASS_COMMON_MODE_FC_0: f32 = -0.005_109_45;
pub const FILTER_HIGHPASS_COMMON_MODE_FC_2047: f32 = -0.011_489_80;
/* A routed internal voice also shifts the filter input node DC operating point. The motherboard coupling capacitor makes the routing transition observable even though the offset disappears from settled AC output. */
pub const FILTER_INPUT_COMMON_MODE_ONE_VOICE: f32 = 0.013_197_48;
/* A rising pure-bypass VCA current briefly displaces the shared mixer node. Smooth activity weights cover one, two and three voices, and the charge relaxes without being refreshed by falling envelope activity. */
pub const PURE_BYPASS_ATTACK_COMMON_MODE_ONE_VOICE: f32 = 0.224_63;
pub const PURE_BYPASS_ATTACK_COMMON_MODE_TWO_VOICE_DELTA: f32 = -0.109_01;
pub const PURE_BYPASS_ATTACK_COMMON_MODE_THREE_VOICE_DELTA: f32 = -0.134_59;
/* This common scale balances attack displacement against the established bypass loading law. */
pub const PURE_BYPASS_ATTACK_COMMON_MODE_SCALE: f32 = 0.715;
/* Pure waveform drivers present different dynamic impedances to the shared bypass node; combined and otherwise unspecified selections retain unity scaling. */
pub const PURE_BYPASS_ATTACK_SCALE_BY_SELECTION: [f32; 16] = [
	1.0, 1.490, 0.0, 1.0, 0.865, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0,
];
/* Instantaneous waveform-DAC displacement changes the dynamic impedance seen by the bypass node. A bounded topology-global linear term models that coupling. */
pub const PURE_BYPASS_ATTACK_WAVEFORM_STATE_COUPLING: f32 = 0.065;
pub const PURE_BYPASS_ATTACK_CHARGE_TIME_SECONDS: f32 = 0.009_633;
/* With pulse width at zero, the oscillator DAC becomes effectively constant and exposes a GATE-specific relaxation. Equal initial charges in two time constants produce no instantaneous step while allowing the later opposite-polarity drift. */
pub const PURE_PULSE_GATE_RELAXATION_CHARGE: f32 = 1_291.601_0;
pub const PURE_PULSE_GATE_RELAXATION_FAST_SECONDS: f32 = 0.015_026_569;
pub const PURE_PULSE_GATE_RELAXATION_SLOW_SECONDS: f32 = 0.102_022_365;
/* Combined-waveform selections use small topology-dependent AC loading corrections after the waveform DAC. Scaling occurs around each topology settled analogue mean so line codes and common-mode transitions are unchanged. */
pub const COMBINED_WAVEFORM_AC_GAIN_BY_SELECTION: [f32; 16] = [
	1.0,
	0.996_400_0,
	1.024_160_0,
	1.763_412_6,
	1.0,
	1.062_242_5,
	1.0,
	1.0,
	1.0,
	1.0,
	1.0,
	1.0,
	1.0,
	1.0,
	1.0,
	1.0,
];
pub const COMBINED_WAVEFORM_AC_CENTRE_BY_SELECTION: [f32; 16] = [
	0.0,
	0.286_975_47,
	0.0,
	-0.213_779_55,
	0.0,
	-0.153_224_69,
	0.0,
	0.0,
	0.0,
	0.0,
	0.0,
	0.0,
	0.0,
	0.0,
	0.0,
	0.0,
];
/* Settled pure-saw bypass output uses a small asymmetric mixer transfer. Its settling state is reset by the logical saw GATE edge so attack-charge behaviour remains independent. */
pub const PURE_SAW_MIXER_LINEAR: f32 = 0.812_330;
pub const PURE_SAW_MIXER_QUADRATIC: f32 = 0.324_400;
pub const PURE_SAW_MIXER_SETTLING_SECONDS: f32 = 0.050;
/* Higher-order line interaction strengthens loading only when more than two waveform sources contend; two-source combinations are unaffected by construction. */
pub const COMBINED_WAVEFORM_INTERACTION_GAIN: f32 = 1.10;
/* Loading scale varies by waveform selection: some two-source combinations need stronger discharge, while three-or-more-source combinations are generally more heavily loaded. */
pub const COMBINED_WAVEFORM_LOADING_SCALE: [f32; 16] = [
	1.0, 1.0, 1.0, 1.31175965, 1.0, 0.8125, 1.36, 2.75, 1.0, 2.30, 2.05, 2.55, 2.20, 2.60, 2.45,
	2.70,
];

pub const WAVEFORM_PIPELINE_RESET_VALUE: u16 = 0x0000;
pub const SYNC_EVENT_MASK: u32 = 0x0080_0000;
pub const NOISE_EVENT_MASK: u32 = 0x0008_0000;
pub const NOISE_MASK: u32 = 0x007f_ffff;
pub const NOISE_RESET_VALUE: u32 = 0x007f_ffff;
pub const COMBINED_WAVEFORM_MSB_CLEAR_MASK: u32 = 0x007f_ffff;
pub const NOISE_OUTPUT_TAPS: [u8; 8] = [2, 4, 8, 11, 13, 17, 20, 22];

pub const WAVEFORM_FLOAT_HOLD_CYCLES: u32 = 1_137_140;
pub const WAVEFORM_FLOAT_LEAK_INTERVAL_CYCLES: u32 = 8_192;
pub const WAVEFORM_FLOAT_CHARGE_MAX: u16 = 65_535;
pub const WAVEFORM_FLOAT_THRESHOLD: u16 = 18_000;
pub const NOISE_TEST_LEAK_INTERVAL_CYCLES: u32 = 65_536;
pub const NOISE_TEST_CHARGE_MAX: u16 = 65_535;
pub const NOISE_TEST_LOGIC_THRESHOLD: u16 = 32_768;
pub const NEVER: u64 = u64::MAX;

/* The rate divider is a fifteen-bit LFSR clocked by PHI2. The sixteen programmed rates select fixed cycle periods; they are properties of the SID divider itself rather than PAL-time values reconstructed from the rounded millisecond figures in the data sheet. The nominal data-sheet times therefore scale naturally with the actual SID clock (SID-SCHEMATICS-ENVELOPE). */
pub const ENVELOPE_RATE_PERIODS: [u32; 16] = [
	9, 32, 63, 95, 149, 220, 267, 313, 392, 977, 1_954, 3_126, 3_907, 11_720, 19_532, 31_251,
];

#[inline(always)]
pub const fn envelope_rate_period(rate: u8) -> u32 {
	ENVELOPE_RATE_PERIODS[(rate & 0x0f) as usize]
}

#[inline(always)]
pub const fn envelope_rate_sequence_next(state: u16) -> u16 {
	let low_pair = state & 0x0003;
	let incoming = ((low_pair ^ (low_pair >> 1)) & 0x0001) << 14;
	(state >> 1) | incoming
}

const fn derive_envelope_rate_comparator(rate: u8) -> u16 {
	let mut state = 0x7fffu16;
	let mut clocks = envelope_rate_period(rate) - 1;
	while clocks != 0 {
		state = envelope_rate_sequence_next(state);
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