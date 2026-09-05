// =======================================================
// src/cartridge/mapper_simons_basic.rs — Simons' BASIC cartridge mapper
// =======================================================

use super::bank_storage::BankStorage;
use super::mapper_interface::{CartridgeInfo, CartridgeMapper, LineState, MapperType};

/* SimonsBasicMapper starts as a 16 KiB cartridge and uses IO1 access to remove or restore the ROMH window, leaving the lower ROM visible while BASIC expansion code changes the map. */
pub struct SimonsBasicMapper {
	roml: BankStorage,
	romh: BankStorage,
	a000_enabled: bool,
}
impl SimonsBasicMapper {
	/* Simons' BASIC boots as an 8 KiB ROML cartridge. Accessing IO1 enables the additional ROMH window, while IO2 disables it again without changing the ROM contents. */
	pub fn new() -> Self {
		Self {
			roml: BankStorage::new(),
			romh: BankStorage::new(),
			a000_enabled: true,
		}
	}
}
impl CartridgeMapper for SimonsBasicMapper {
	fn reset(&mut self) {
		self.a000_enabled = true;
	}
	fn read_roml(&mut self, offset: u16, _cycle: u64) -> Option<u8> {
		self.roml.get_bank(0).map(|b| b[(offset & 0x1FFF) as usize])
	}
	fn peek_roml(&self, offset: u16, _cycle: u64) -> Option<u8> {
		self.roml.get_bank(0).map(|b| b[(offset & 0x1FFF) as usize])
	}
	fn read_romh(&mut self, offset: u16, _cycle: u64) -> Option<u8> {
		self.romh.get_bank(0).map(|b| b[(offset & 0x1FFF) as usize])
	}
	fn peek_romh(&self, offset: u16, _cycle: u64) -> Option<u8> {
		self.romh.get_bank(0).map(|b| b[(offset & 0x1FFF) as usize])
	}
	/* Any IO1 access enables the ROMH extension at $A000-$BFFF, while IO2 clears the latch. The data value is irrelevant because the address decode itself controls the mapping. */
	fn read_io(&mut self, addr: u16, _cycle: u64) -> Option<u8> {
		if addr >= 0xDE00 && addr <= 0xDEFF {
			self.a000_enabled = false;
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
	/* Writes follow the same address-decoded protocol as reads: IO1 enables ROMH and IO2 returns to the ROML-only map. */
	fn write_io(&mut self, addr: u16, _val: u8, _cycle: u64) {
		if addr >= 0xDE00 && addr <= 0xDEFF {
			self.a000_enabled = true;
		}
	}
	fn write_rom(&mut self, _addr: u16, _val: u8, _cycle: u64) {}
	fn add_bank(&mut self, bank: usize, addr: u16, data: &[u8]) {
		let offset = (addr & 0x1FFF) as usize;
		if addr < 0xA000 {
			self.roml.store_bank(bank, data, offset);
		} else {
			self.romh.store_bank(bank, data, offset);
		}
	}
	fn update_signals(&mut self, _cycle: u64, lines: &mut LineState) {
		if self.a000_enabled {
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
			name: "Simons Basic".to_string(),
			mapper_type: MapperType::SimonsBasic,
			rom_size: (self.roml.len() + self.romh.len()) * 8192,
			bank_count: 1,
			has_ram: false,
		}
	}
	fn get_debug_bank(&self) -> usize {
		0
	}
	fn get_max_bank(&self) -> usize {
		0
	}
}