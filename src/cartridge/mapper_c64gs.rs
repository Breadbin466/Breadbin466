// =======================================================
// src/cartridge/mapper_c64gs.rs — C64 Games System cartridge mapper
// =======================================================

use super::mapper_interface::{CartridgeInfo, CartridgeMapper, LineState, MapperType};
use super::bank_storage::BankStorage;

/* C64GSMapper exposes a banked ROML image intended for cartridge-only systems. IO1 writes select a bank while the expansion port remains in the 8 KiB game configuration. */
pub struct C64GSMapper {
	roml: BankStorage,
	bank: usize,
	register: u8,
}

impl C64GSMapper {
	/* The C64GS hardware decodes IO1 addresses rather than the data byte: A0 through A5 select one of 64 ROML banks, while GAME released and EXROM asserted retain the 8 KiB cartridge map. */
	pub fn new() -> Self {
		Self {
			roml: BankStorage::new(),
			bank: 0,
			register: 0,
		}
	}
}

impl CartridgeMapper for C64GSMapper {
	fn reset(&mut self) {
		self.bank = 0;
		self.register = 0;
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

	/* Accessing IO1 uses address bits 0-5 as the bank selector. The data byte is not part of the decode, so both reads and writes can switch banks through the accessed address alone. */
	fn read_io(&mut self, addr: u16, _cycle: u64) -> Option<u8> {
		if (addr & 0xFF00) == 0xDE00 {
			self.bank = (addr & 0x003F) as usize;
		}
		None
	}

	/* Peeking reports the currently selected bank without performing the address-decoded switch used by a real read. */
	fn peek_io(&self, addr: u16, _cycle: u64) -> Option<u8> {
		if (addr & 0xFF00) == 0xDE00 {
			Some(self.register)
		} else {
			None
		}
	}

	/* Writes use the same address-decoded bank selection as reads, allowing software to switch banks without depending on the written data byte. */
	fn write_io(&mut self, addr: u16, value: u8, _cycle: u64) {
		if (addr & 0xFF00) == 0xDE00 {
			self.bank = (addr & 0x003F) as usize;
			self.register = value;
		}
	}

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
			name: "C64GS".to_string(),
			mapper_type: MapperType::C64GS,
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