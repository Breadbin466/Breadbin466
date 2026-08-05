// =======================================================
// src/cartridge/mapper_dinamic.rs — Dinamic cartridge mapper
// =======================================================

use super::mapper_interface::{CartridgeInfo, CartridgeMapper, LineState, MapperType};
use super::bank_storage::BankStorage;

/* DinamicMapper selects ROML banks through reads in the IO1 window. The address, rather than the returned data, acts as the bank register input. */
pub struct DinamicMapper {
	roml: BankStorage,
	bank: usize,
}

impl DinamicMapper {
	/* Dinamic uses the low four IO1 address lines as a 16-way bank selector. The access is a strobe only, so the cartridge need not drive a meaningful data byte during the switch. */
	pub fn new() -> Self {
		Self {
			roml: BankStorage::new(),
			bank: 0,
		}
	}
}

impl CartridgeMapper for DinamicMapper {
	fn reset(&mut self) {
		self.bank = 0;
	}

	fn read_roml(&mut self, offset: u16, _cycle: u64) -> Option<u8> {
		self.roml.get_resolved(self.bank).map(|data| data[(offset & 0x1FFF) as usize])
	}

	fn peek_roml(&self, offset: u16, _cycle: u64) -> Option<u8> {
		self.roml.get_resolved(self.bank).map(|data| data[(offset & 0x1FFF) as usize])
	}

	fn read_romh(&mut self, _offset: u16, _cycle: u64) -> Option<u8> {
		None
	}

	fn peek_romh(&self, _offset: u16, _cycle: u64) -> Option<u8> {
		None
	}

	/* Reading $DE00-$DE0F uses address bits 0-3 as the bank latch. The cartridge does not need to drive a useful data byte; the decoded read itself changes subsequent ROML visibility. */
	fn read_io(&mut self, addr: u16, _cycle: u64) -> Option<u8> {
		if (addr & 0xFFF0) == 0xDE00 {
			self.bank = (addr & 0x000F) as usize;
		}
		None
	}

	fn peek_io(&self, addr: u16, _cycle: u64) -> Option<u8> {
		if (addr & 0xFF00) == 0xDE00 {
			Some(0)
		} else {
			None
		}
	}

	fn write_io(&mut self, _addr: u16, _value: u8, _cycle: u64) {}

	fn write_rom(&mut self, _addr: u16, _value: u8, _cycle: u64) {}

	fn add_bank(&mut self, bank: usize, addr: u16, data: &[u8]) {
		self.roml.store_bank(bank, data, (addr & 0x1FFF) as usize);
	}

	fn update_signals(&mut self, _cycle: u64, lines: &mut LineState) {
		lines.game = true;
		lines.exrom = false;
	}

	fn on_freeze(&mut self, _lines: &mut LineState) {}

	fn get_info(&self) -> CartridgeInfo {
		CartridgeInfo {
			name: "Dinamic".to_string(),
			mapper_type: MapperType::Dinamic,
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