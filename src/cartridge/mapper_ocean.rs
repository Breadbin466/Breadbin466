// =======================================================
// src/cartridge/mapper_ocean.rs — Ocean cartridge mapper
// =======================================================

use super::bank_storage::BankStorage;
use super::mapper_interface::{CartridgeInfo, CartridgeMapper, LineState, MapperType};

/* OceanMapper selects one 8 KiB bank through IO1 and maps the selected image into the window defined by the CRT layout. Bank bits and disable behaviour remain mapper state rather than motherboard policy. */
pub struct OceanMapper {
	rom: BankStorage,
	bank: usize,
	is_16k: bool,
	register: u8,
}

impl OceanMapper {
	/* Ocean cartridges write their bank number through IO1. Images up to 512 KiB use ROML only, while larger variants pair ROML and ROMH under the same bank latch. */
	pub fn new() -> Self {
		Self {
			rom: BankStorage::new(),
			bank: 0,
			is_16k: false,
			register: 0,
		}
	}
}

impl CartridgeMapper for OceanMapper {
	/* Reset selects the first image and clears the readable register mirror without changing whether the loaded cartridge uses the 8 KiB or 16 KiB Ocean layout. */
	fn reset(&mut self) {
		self.bank = 0;
		self.register = 0;
	}

	fn read_roml(&mut self, offset: u16, _cycle: u64) -> Option<u8> {
		self.rom
			.get_resolved(self.bank)
			.map(|data| data[(offset & 0x1FFF) as usize])
	}

	fn peek_roml(&self, offset: u16, _cycle: u64) -> Option<u8> {
		self.rom
			.get_resolved(self.bank)
			.map(|data| data[(offset & 0x1FFF) as usize])
	}

	fn read_romh(&mut self, offset: u16, _cycle: u64) -> Option<u8> {
		if !self.is_16k {
			return None;
		}
		self.rom
			.get_resolved(self.bank)
			.map(|data| data[(offset & 0x1FFF) as usize])
	}

	fn peek_romh(&self, offset: u16, _cycle: u64) -> Option<u8> {
		if !self.is_16k {
			return None;
		}
		self.rom
			.get_resolved(self.bank)
			.map(|data| data[(offset & 0x1FFF) as usize])
	}

	fn read_io(&mut self, _addr: u16, _cycle: u64) -> Option<u8> {
		None
	}

	/* IO1 exposes the last bank-register value for inspection; ordinary reads remain electrically undecoded on this implementation. */
	fn peek_io(&self, addr: u16, _cycle: u64) -> Option<u8> {
		if (addr & 0xFF00) == 0xDE00 {
			Some(self.register)
		} else {
			None
		}
	}

	/* Ocean decodes IO1 and masks the written value to the number of populated banks, with an upper limit of six bank bits. Small images use the selected bank in both ROM windows; larger images expose ROML only. */
	fn write_io(&mut self, addr: u16, value: u8, _cycle: u64) {
		if (addr & 0xFF00) != 0xDE00 {
			return;
		}
		self.register = value;
		let mask = self.rom.len().saturating_sub(1).min(0x3F);
		self.bank = value as usize & mask;
	}

	fn write_rom(&mut self, _addr: u16, _value: u8, _cycle: u64) {}

	fn add_bank(&mut self, bank: usize, _addr: u16, data: &[u8]) {
		self.rom.store_bank(bank, data, 0);
		self.is_16k = self.rom.len() < 64;
	}

	/* The image geometry fixes the electrical map: small Ocean sets use 16 KiB mode, while larger sets expose one 8 KiB bank at a time. */
	fn update_signals(&mut self, _cycle: u64, lines: &mut LineState) {
		if self.is_16k {
			lines.game = false;
			lines.exrom = false;
		} else {
			lines.game = true;
			lines.exrom = false;
		}
	}

	fn on_freeze(&mut self, _lines: &mut LineState) {}

	fn get_info(&self) -> CartridgeInfo {
		CartridgeInfo {
			name: "Ocean".to_string(),
			mapper_type: MapperType::Ocean,
			rom_size: self.rom.len() * 8192,
			bank_count: self.rom.len(),
			has_ram: false,
		}
	}

	fn get_debug_bank(&self) -> usize {
		self.bank
	}

	fn get_max_bank(&self) -> usize {
		self.rom.len().saturating_sub(1)
	}
}