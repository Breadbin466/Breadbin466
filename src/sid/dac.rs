// =======================================================
// src/sid/dac.rs — SID voice DAC conversion
// =======================================================

/* Voice waveform and envelope DAC conversion. */

use super::constants::{
	COMBINED_WAVEFORM_AC_CENTRE_BY_SELECTION, COMBINED_WAVEFORM_AC_GAIN_BY_SELECTION,
	PURE_TRIANGLE_EVEN_CURVATURE_QUADRATIC, PURE_TRIANGLE_EVEN_CURVATURE_QUARTIC,
	PURE_TRIANGLE_FUNDAMENTAL_ROUNDING, WAVEFORM_MASK,
};
use super::tables::{build_envelope_dac_table, build_waveform_dac_table};

#[derive(Clone, Copy, Default)]
/* A voice enters the filter as separate waveform and envelope analogue factors. The frozen flag preserves the distinct DC operating point produced when TEST or no waveform prevents the oscillator DAC from updating. */
pub struct VoiceSignal {
	pub waveform: f32,
	pub envelope: f32,
	/* VCA drive above the envelope ladder's zero-code leakage equilibrium. */
	pub envelope_activity: f32,
	pub frozen: bool,
	pub combined: bool,
	pub waveform_selection: u8,
}

/* The 6581 waveform and envelope DACs are nonlinear ladders. Precomputed transfer tables keep that analogue conversion out of the per-cycle hot path while preserving the original digital codes. */
pub struct VoiceLevelConverter {
	waveform: Box<[f32; 4096]>,
	triangle_waveform: Box<[f32; 4096]>,
	envelope: [f32; 256],
}

impl VoiceLevelConverter {
	/* The transfer tables are immutable model data shared by all voices, while each output call remains stateless and therefore cannot introduce hidden inter-voice coupling. */
	pub fn new() -> Self {
		let waveform = build_waveform_dac_table();
		let lower = waveform[0];
		let upper = waveform[4095];
		let midpoint = 0.5 * (lower + upper);
		let half_span = 0.5 * (upper - lower);
		let triangle_waveform = Box::new(std::array::from_fn(|code| {
			let position = ((waveform[code] - midpoint) / half_span).clamp(-1.0, 1.0);
			let rounded = (1.0 - PURE_TRIANGLE_FUNDAMENTAL_ROUNDING) * position
				+ PURE_TRIANGLE_FUNDAMENTAL_ROUNDING
					* (0.5 * std::f32::consts::PI * position).sin();
			let squared = position * position;
			let even_curvature = PURE_TRIANGLE_EVEN_CURVATURE_QUADRATIC * (squared - 1.0)
				+ PURE_TRIANGLE_EVEN_CURVATURE_QUARTIC * (squared * squared - 1.0);
			midpoint + half_span * (rounded + even_curvature)
		}));
		Self {
			waveform,
			triangle_waveform,
			envelope: build_envelope_dac_table(),
		}
	}

	#[inline(always)]
	/* Resolves the twelve-bit waveform ladder and eight-bit envelope ladder as cascaded non-ideal DACs, producing the normalised analogue factors consumed by the shared filter. */
	pub fn output(
		&self,
		waveform_code: u16,
		envelope_code: u8,
		frozen: bool,
		waveform_selection: u8,
	) -> VoiceSignal {
		let selection = usize::from(waveform_selection & 0x0f);
		let waveform_table = if selection == 1 {
			&self.triangle_waveform
		} else {
			&self.waveform
		};
		let raw_waveform = waveform_table[usize::from(waveform_code & WAVEFORM_MASK)];
		let centre = COMBINED_WAVEFORM_AC_CENTRE_BY_SELECTION[selection];
		let waveform =
			centre + (raw_waveform - centre) * COMBINED_WAVEFORM_AC_GAIN_BY_SELECTION[selection];
		VoiceSignal {
			waveform,
			envelope: self.envelope[usize::from(envelope_code)],
			envelope_activity: (self.envelope[usize::from(envelope_code)] - self.envelope[0]).max(0.0),
			frozen,
			combined: waveform_selection.count_ones() > 1,
			waveform_selection: waveform_selection & 0x0f,
		}
	}
}

impl Default for VoiceLevelConverter {
	fn default() -> Self {
		Self::new()
	}
}