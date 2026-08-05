// =======================================================
// src/cartridge/mapper_super_games.rs — Super Games cartridge mapper
// =======================================================

use super::crt_layout::add_bank_split;
use super::mapper_interface::{CartridgeInfo, CartridgeMapper, LineState, MapperType};
use super::bank_storage::BankStorage;

/* SuperGamesMapper controls bank selection, cartridge mode and a permanent disable latch through IO2. Once disabled, the mapper releases the expansion-port lines until reset. */
pub struct SuperGamesMapper {
	roml: BankStorage,
	romh: BankStorage,
	bank: usize,
	mode_16k: bool,
	latched: bool,
	register: u8,
}

impl SuperGamesMapper {
	/* Super Games uses an IO2 register whose low two bits select one of four 16 KiB banks. The control byte can also switch to Ultimax-style mapping or disable the cartridge entirely. */
	pub fn new() -> Self {
		Self {
			roml: BankStorage::new(),
			romh: BankStorage::new(),
			bank: 0,
			mode_16k: true,
			latched: false,
			register: 0,
		}
	}
}

impl CartridgeMapper for SuperGamesMapper {
	fn reset(&mut self) {
		self.bank = 0;
		self.mode_16k = true;
		self.latched = false;
		self.register = 0;
	}

	fn read_roml(&mut self, offset: u16, _cycle: u64) -> Option<u8> {
		self.roml.get_resolved(self.bank).map(|data| data[(offset & 0x1FFF) as usize])
	}

	fn peek_roml(&self, offset: u16, _cycle: u64) -> Option<u8> {
		self.roml.get_resolved(self.bank).map(|data| data[(offset & 0x1FFF) as usize])
	}

	fn read_romh(&mut self, offset: u16, _cycle: u64) -> Option<u8> {
		self.romh.get_resolved(self.bank).map(|data| data[(offset & 0x1FFF) as usize])
	}

	fn peek_romh(&self, offset: u16, _cycle: u64) -> Option<u8> {
		self.romh.get_resolved(self.bank).map(|data| data[(offset & 0x1FFF) as usize])
	}

	fn read_io(&mut self, _addr: u16, _cycle: u64) -> Option<u8> {
		None
	}

	fn peek_io(&self, addr: u16, _cycle: u64) -> Option<u8> {
		if (addr & 0xFF00) == 0xDF00 {
			Some(self.register)
		} else {
			None
		}
	}

	/* IO2 bits 0-1 select one paired ROML/ROMH bank. Bit 2 keeps the 16 KiB map when clear and releases the cartridge when set; bit 3 locks the register against later writes until reset. */
	fn write_io(&mut self, addr: u16, value: u8, _cycle: u64) {
		if (addr & 0xFF00) != 0xDF00 || self.latched {
			return;
		}
		self.register = value;
		self.bank = (value & 3) as usize;
		self.mode_16k = (value & 0x04) == 0;
		self.latched = (value & 0x08) != 0;
	}

	fn write_rom(&mut self, _addr: u16, _value: u8, _cycle: u64) {}

	fn add_bank(&mut self, bank: usize, addr: u16, data: &[u8]) {
		add_bank_split(&mut self.roml, &mut self.romh, bank, addr, data);
	}

	/* Enabled Super Games hardware uses the 16 KiB map; the disable latch releases GAME and EXROM together. */
	fn update_signals(&mut self, _cycle: u64, lines: &mut LineState) {
		if self.mode_16k {
			lines.game = false;
			lines.exrom = false;
		} else {
			lines.game = true;
			lines.exrom = true;
		}
	}

	fn on_freeze(&mut self, _lines: &mut LineState) {}

	fn get_info(&self) -> CartridgeInfo {
		CartridgeInfo {
			name: "Super Games".to_string(),
			mapper_type: MapperType::SuperGames,
			rom_size: (self.roml.populated_len() + self.romh.populated_len()) * 8192,
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