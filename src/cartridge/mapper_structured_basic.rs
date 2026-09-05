// =======================================================
// src/cartridge/mapper_structured_basic.rs — Structured BASIC cartridge mapper
// =======================================================

use super::bank_storage::BankStorage;
use super::mapper_interface::{CartridgeInfo, CartridgeMapper, LineState, MapperType};

/* StructuredBasicMapper selects ROML banks through its IO register while keeping a fixed game-mode mapping. Sparse packet placement is preserved through BankStorage. */
pub struct StructuredBasicMapper {
	roml: BankStorage,
	bank: usize,
}

impl StructuredBasicMapper {
	/* Structured BASIC exposes one of two ROML banks in the normal 8 KiB cartridge map. The low bit written through IO1 selects which bank drives $8000-$9FFF. */
	pub fn new() -> Self {
		Self {
			roml: BankStorage::new(),
			bank: 0,
		}
	}
}

impl CartridgeMapper for StructuredBasicMapper {
	fn reset(&mut self) {
		self.bank = 0;
	}

	fn read_roml(&mut self, offset: u16, _cycle: u64) -> Option<u8> {
		self.roml
			.get_resolved(self.bank)
			.map(|data| data[(offset & 0x1FFF) as usize])
	}

	fn peek_roml(&self, offset: u16, _cycle: u64) -> Option<u8> {
		self.roml
			.get_resolved(self.bank)
			.map(|data| data[(offset & 0x1FFF) as usize])
	}

	fn read_romh(&mut self, _offset: u16, _cycle: u64) -> Option<u8> {
		None
	}

	fn peek_romh(&self, _offset: u16, _cycle: u64) -> Option<u8> {
		None
	}

	fn read_io(&mut self, _addr: u16, _cycle: u64) -> Option<u8> {
		None
	}

	fn peek_io(&self, _addr: u16, _cycle: u64) -> Option<u8> {
		None
	}

	/* IO1 bit 0 selects one of the two ROML banks; no other written bits affect the cartridge map. */
	fn write_io(&mut self, addr: u16, value: u8, _cycle: u64) {
		if (addr & 0xFF00) == 0xDE00 {
			self.bank = usize::from(value & 1);
		}
	}

	fn write_rom(&mut self, _addr: u16, _value: u8, _cycle: u64) {}

	fn add_bank(&mut self, bank: usize, addr: u16, data: &[u8]) {
		if addr == 0x8000 {
			self.roml.store_bank(bank, data, 0);
		}
	}

	/* GAME released and EXROM asserted keep the cartridge in the fixed 8 KiB map with ROML at $8000-$9FFF. */
	fn update_signals(&mut self, _cycle: u64, lines: &mut LineState) {
		lines.game = true;
		lines.exrom = false;
	}

	fn on_freeze(&mut self, _lines: &mut LineState) {}

	fn get_info(&self) -> CartridgeInfo {
		CartridgeInfo {
			name: "Structured BASIC".to_string(),
			mapper_type: MapperType::StructuredBasic,
			rom_size: self.roml.populated_len() * 8192,
			bank_count: self.roml.len(),
			has_ram: false,
		}
	}

	fn get_debug_bank(&self) -> usize {
		self.bank
	}

	fn get_max_bank(&self) -> usize {
		self.roml.len().saturating_sub(1)
	}
}