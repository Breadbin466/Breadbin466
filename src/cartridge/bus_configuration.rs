// =======================================================
// src/cartridge/bus_configuration.rs — Cartridge bus and phase configuration
// =======================================================

/* The four expansion-port maps are the electrical combinations of GAME and EXROM. Ram means no cartridge overlay; the other modes select 8 KiB, 16 KiB or Ultimax visibility through the PLA (C64-PRG-1982, expansion-port memory configurations). */
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CartridgeMode {
	Ram,
	Game8K,
	Game16K,
	Ultimax,
}

impl CartridgeMode {
	#[inline(always)]
	/* GAME and EXROM are represented as their released/high Boolean levels, so the truth table is written in electrical rather than active-low register notation. */
	pub fn from_lines(game: bool, exrom: bool) -> Self {
		match (game, exrom) {
			(true, true) => Self::Ram,
			(true, false) => Self::Game8K,
			(false, false) => Self::Game16K,
			(false, true) => Self::Ultimax,
		}
	}

	#[inline(always)]
	/* Converting back to pin levels keeps the mapping truth table centralised rather than duplicating active-low combinations in each mapper. */
	pub fn lines(self) -> (bool, bool) {
		match self {
			Self::Ram => (true, true),
			Self::Game8K => (true, false),
			Self::Game16K => (false, false),
			Self::Ultimax => (false, true),
		}
	}
}

/* CartridgeConfiguration is the motherboard-facing snapshot of one mapper cycle. It separates phase-specific maps, selected banks and interrupt outputs so memory routing does not need to know the mapper implementation. */
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CartridgeConfiguration {
	pub phi1_mode: CartridgeMode,
	pub phi2_mode: CartridgeMode,
	pub roml_bank: usize,
	pub romh_bank: usize,
	pub phi2_ram: bool,
	pub irq_low: bool,
	pub nmi_low: bool,
}

impl CartridgeConfiguration {
	/* A line-only snapshot is used at reset and detach boundaries, before a mapper has published phase-specific banks or RAM overlays. */
	pub fn from_lines(game: bool, exrom: bool, nmi_low: bool) -> Self {
		let mode = CartridgeMode::from_lines(game, exrom);
		Self {
			phi1_mode: mode,
			phi2_mode: mode,
			roml_bank: 0,
			romh_bank: 0,
			phi2_ram: false,
			irq_low: false,
			nmi_low,
		}
	}
}

/* An expansion-port I/O read can be undecoded, leave the bus floating, drive all eight bits or drive only selected lines. The distinction preserves open-bus behaviour that Option<u8> alone cannot represent. */
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoRead {
	NotDecoded,
	OpenBus,
	Driven(u8),
	PartiallyDriven { value: u8, mask: u8 },
}

impl IoRead {
	#[inline(always)]
	/* Resolution combines only the lines actively driven by the cartridge with the motherboard data-bus latch; an undecoded access remains distinguishable from a decoded open-bus read. */
	pub fn resolve(self, floating: u8) -> Option<u8> {
		match self {
			Self::NotDecoded => None,
			Self::OpenBus => Some(floating),
			Self::Driven(value) => Some(value),
			Self::PartiallyDriven { value, mask } => Some((value & mask) | (floating & !mask)),
		}
	}
}