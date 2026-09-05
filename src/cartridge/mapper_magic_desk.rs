// =======================================================
// src/cartridge/mapper_magic_desk.rs — Magic Desk cartridge mapper
// =======================================================

use super::bank_storage::BankStorage;
use super::mapper_interface::{CartridgeInfo, CartridgeMapper, LineState, MapperType};

/* MagicDeskMapper combines a bank number with a cartridge-disable bit in its IO1 register. Disabling releases GAME and EXROM so the underlying C64 map returns without detaching the image. */
pub struct MagicDeskMapper {
	roml: BankStorage,
	bank: usize,
	off: bool,
	register: u8,
}

impl MagicDeskMapper {
	/* The IO1 register uses bits 0 through 6 as the ROML bank number and bit 7 as a permanent-looking disconnect latch. Releasing both GAME and EXROM hides the cartridge without discarding its selected bank. */
	pub fn new() -> Self {
		Self {
			roml: BankStorage::new(),
			bank: 0,
			off: false,
			register: 0,
		}
	}
}

impl CartridgeMapper for MagicDeskMapper {
	/* Reset re-enables the cartridge and selects bank zero, matching the power-on state expected by menu-driven Magic Desk images. */
	fn reset(&mut self) {
		self.bank = 0;
		self.off = false;
		self.register = 0;
	}

	fn read_roml(&mut self, offset: u16, _cycle: u64) -> Option<u8> {
		if self.off {
			return None;
		}
		self.roml
			.get_bank(self.bank)
			.map(|data| data[(offset & 0x1FFF) as usize])
	}

	fn peek_roml(&self, offset: u16, _cycle: u64) -> Option<u8> {
		if self.off {
			return None;
		}
		self.roml
			.get_bank(self.bank)
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

	fn peek_io(&self, addr: u16, _cycle: u64) -> Option<u8> {
		if (addr & 0xFF00) == 0xDE00 {
			Some(self.register)
		} else {
			None
		}
	}

	/* $DE00 bits 0-6 select the visible 8 KiB ROML bank. Bit 7 disconnects the cartridge and releases both GAME and EXROM until reset. */
	fn write_io(&mut self, addr: u16, value: u8, _cycle: u64) {
		if (addr & 0xFF00) != 0xDE00 {
			return;
		}
		self.register = value;
		self.bank = (value & 0x7F) as usize;
		self.off = (value & 0x80) != 0;
	}

	fn write_rom(&mut self, _addr: u16, _value: u8, _cycle: u64) {}

	fn add_bank(&mut self, bank: usize, addr: u16, data: &[u8]) {
		self.roml.store_bank(bank, data, (addr & 0x1FFF) as usize);
	}

	/* Disabling releases both cartridge lines, while an enabled mapper publishes the normal 8 KiB configuration. */
	fn update_signals(&mut self, _cycle: u64, lines: &mut LineState) {
		if self.off {
			lines.game = true;
			lines.exrom = true;
		} else {
			lines.game = true;
			lines.exrom = false;
		}
	}

	fn on_freeze(&mut self, _lines: &mut LineState) {}

	fn get_info(&self) -> CartridgeInfo {
		CartridgeInfo {
			name: "Magic Desk".to_string(),
			mapper_type: MapperType::MagicDesk,
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