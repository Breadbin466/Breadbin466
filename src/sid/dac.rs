// =======================================================
// src/sid/dac.rs — MOS 6581R4AR voice DAC conversion
// =======================================================

use super::constants::WAVEFORM_MASK;
use super::tables::{build_envelope_dac_table, build_waveform_dac_table};

#[derive(Clone, Copy, Default)]
/* A voice enters the filter as separate waveform and envelope analogue factors. The frozen flag preserves the distinct DC operating point produced when TEST or no waveform prevents the oscillator DAC from updating. */
pub struct VoiceSignal {
	pub waveform: f32,
	pub envelope: f32,
	pub frozen: bool,
}

/* The 6581 waveform and envelope DACs are nonlinear ladders. Precomputed transfer tables keep that analogue conversion out of the per-cycle hot path while preserving the original digital codes. */
pub struct VoiceLevelConverter {
	waveform: Box<[f32; 4096]>,
	envelope: [f32; 256],
}

impl VoiceLevelConverter {
	/* The transfer tables are immutable model data shared by all voices, while each output call remains stateless and therefore cannot introduce hidden inter-voice coupling. */
	pub fn new() -> Self {
		Self {
			waveform: build_waveform_dac_table(),
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
	) -> VoiceSignal {
		VoiceSignal {
			waveform: self.waveform[usize::from(waveform_code & WAVEFORM_MASK)],
			envelope: self.envelope[usize::from(envelope_code)],
			frozen,
		}
	}
}

impl Default for VoiceLevelConverter {
	fn default() -> Self { Self::new() }
}