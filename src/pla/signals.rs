// =======================================================
// src/pla/signals.rs — MOS 906114-01 external signal levels
// =======================================================

/* These fields represent the external levels sampled by the 906114-01. Names ending in _n are active-low physical signals; the remaining booleans are true when the corresponding logical input is high. */
#[derive(Debug, Clone, Copy)]
pub struct PlaInputSignals {
	pub a15: bool,
	pub a14: bool,
	pub a13: bool,
	pub a12: bool,
	pub va14_n: bool,
	pub charen: bool,
	pub hiram: bool,
	pub loram: bool,
	pub game_n: bool,
	pub exrom_n: bool,
	pub rw: bool,
	pub aec: bool,
	pub ba: bool,
	pub va13: bool,
	pub va12: bool,
	pub cas_n: bool,
}

impl Default for PlaInputSignals {
	fn default() -> Self {
		Self {
			a15: false,
			a14: false,
			a13: false,
			a12: false,
			va14_n: true,
			charen: true,
			hiram: true,
			loram: true,
			game_n: true,
			exrom_n: true,
			rw: true,
			aec: true,
			ba: true,
			va13: false,
			va12: false,
			cas_n: true,
		}
	}
}

/* All PLA outputs are active low. A false value therefore means that the corresponding chip-select or write-enable line is asserted. */
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaOutputSignals {
	pub casram_n: bool,
	pub basic_n: bool,
	pub kernal_n: bool,
	pub charom_n: bool,
	pub io_n: bool,
	pub roml_n: bool,
	pub romh_n: bool,
	pub grw_n: bool,
}

impl Default for PlaOutputSignals {
	fn default() -> Self {
		Self {
			casram_n: true,
			basic_n: true,
			kernal_n: true,
			charom_n: true,
			io_n: true,
			roml_n: true,
			romh_n: true,
			grw_n: true,
		}
	}
}