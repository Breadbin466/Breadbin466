// =======================================================
// src/pla/vic_map.rs — VIC-II cartridge and character ROM routing
// =======================================================

use crate::cartridge::bus_configuration::CartridgeMode;
use crate::memory::constants::MapRegion;

/* The VIC-II normally sees physical RAM, with character ROM substituted at $1000-$1FFF of banks 0 and 2. Ultimax removes that substitution and exposes ROMH at VIC addresses $3000-$3FFF (C64-PRG-1982, memory maps; C64-PLA-DISSECTED-2012, VIC maps). */
#[inline(always)]
pub fn map_vic_addr(va: u16, bank: usize, mode: CartridgeMode) -> MapRegion {
	debug_assert!(va < 0x4000, "VIC address must be 14-bit");
	debug_assert!(bank < 4, "VIC bank must be 0-3");

	if mode == CartridgeMode::Ultimax && va >= 0x3000 {
		return MapRegion::RomH;
	}

	if mode != CartridgeMode::Ultimax && bank & 1 == 0 && (0x1000..=0x1FFF).contains(&va) {
		return MapRegion::Char;
	}

	MapRegion::Ram
}