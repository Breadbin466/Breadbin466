// =======================================================
// src/memory/vic_access.rs — VIC-II Memory Controller
// =======================================================

use super::ram::RAMController;
use super::rom::ROMStorage;
use crate::cartridge::Cartridge;
use crate::memory::constants::MapRegion;
use crate::pla::map_vic_addr;

/* VICMemoryController translates the VIC-II's 14-bit address into a physical DRAM address, then applies the two exceptions visible to the video chip: character ROM in banks 0/2 and ROMH in Ultimax mode. */
pub struct VICMemoryController;

impl VICMemoryController {
	/* CIA2 selects one of four 16 KiB banks. Within the bank, the PLA may substitute character ROM or cartridge ROMH; otherwise the VIC reads physical RAM directly (C64-PRG-1982, VIC memory organisation; C64-PLA-DISSECTED-2012, VIC maps). */
	#[inline(always)]
	pub fn read<const PHI2: bool>(
		va: u16,
		bank: u8,
		cycle: u64,
		ram: &RAMController,
		rom: &ROMStorage,
		cartridge: &mut Cartridge,
		floating_byte: u8,
	) -> u8 {
		let bank = bank & 0x03;
		let va14 = va & 0x3FFF;
		let phys_addr = ((bank as u16) << 14) | va14;

		let mode = if PHI2 { cartridge.configuration.phi2_mode } else { cartridge.configuration.phi1_mode };
		let region = map_vic_addr(va14, bank as usize, mode);

		match region {
			MapRegion::Char => rom.read_char(va14 & 0x0FFF),
			MapRegion::RomH => cartridge
				.read_romh(va14 & 0x1FFF, cycle)
				.unwrap_or_else(|| ram.read(phys_addr)),
			MapRegion::Ram => ram.read(phys_addr),
			_ => floating_byte,
		}
	}
}