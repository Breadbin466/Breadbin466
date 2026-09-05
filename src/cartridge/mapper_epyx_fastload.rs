// =======================================================
// src/cartridge/mapper_epyx_fastload.rs — Epyx FastLoad cartridge
// =======================================================

use super::bank_storage::BankStorage;
use super::mapper_interface::{CartridgeInfo, CartridgeMapper, LineState, MapperType};

/* EpyxFastloadMapper models a capacitor-timed cartridge enable. Relevant accesses recharge the active period; when the deadline expires, the cartridge releases its ROM mapping automatically. */
pub struct EpyxMapper {
	rom: BankStorage,
	idle_counter: u32,
	just_accessed: bool,
	rom_active: bool,
}
impl EpyxMapper {
	pub fn new() -> Self {
		Self {
			rom: BankStorage::new(),
			idle_counter: 0,
			just_accessed: false,
			rom_active: true,
		}
	}
}
impl CartridgeMapper for EpyxMapper {
	fn reset(&mut self) {
		self.idle_counter = 0;
		self.just_accessed = false;
		self.rom_active = true;
	}
	/* Any ROML access refreshes the activity timeout, keeping the cartridge mapped while its fastloader is executing. */
	fn read_roml(&mut self, offset: u16, _cycle: u64) -> Option<u8> {
		self.just_accessed = true;
		self.rom_active = true;
		self.rom.get_bank(0).map(|b| b[(offset & 0x1FFF) as usize])
	}
	fn peek_roml(&self, offset: u16, _cycle: u64) -> Option<u8> {
		if !self.rom_active {
			return None;
		}
		self.rom.get_bank(0).map(|b| b[(offset & 0x1FFF) as usize])
	}
	fn read_romh(&mut self, _offset: u16, _cycle: u64) -> Option<u8> {
		None
	}
	fn peek_romh(&self, _offset: u16, _cycle: u64) -> Option<u8> {
		None
	}
	/* IO2 accesses also refresh the timeout and expose the ROM tail used by the loader entry code. */
	fn read_io(&mut self, addr: u16, _cycle: u64) -> Option<u8> {
		if addr >= 0xDE00 && addr <= 0xDEFF {
			self.just_accessed = true;
			self.rom_active = true;
			return None;
		}
		if addr >= 0xDF00 && addr <= 0xDFFF {
			return self
				.rom
				.get_bank(0)
				.map(|b| b[0x1F00 + (addr & 0xFF) as usize]);
		}
		None
	}
	fn peek_io(&self, addr: u16, _cycle: u64) -> Option<u8> {
		match addr {
			0xDE00..=0xDEFF => Some(0),
			0xDF00..=0xDFFF => self
				.rom
				.get_bank(0)
				.map(|data| data[0x1F00 + (addr & 0xFF) as usize]),
			_ => None,
		}
	}
	fn write_io(&mut self, _addr: u16, _val: u8, _cycle: u64) {}
	fn write_rom(&mut self, _addr: u16, _val: u8, _cycle: u64) {}
	/* Inactivity ages the monostable-style visibility window; expiry releases GAME and EXROM until another decoded access reactivates the cartridge. */
	fn update_signals(&mut self, _cycle: u64, lines: &mut LineState) {
		if self.just_accessed {
			self.just_accessed = false;
			self.idle_counter = 0;
		} else if self.rom_active {
			self.idle_counter += 1;
			if self.idle_counter >= 512 {
				self.rom_active = false;
			}
		}
		lines.exrom = !self.rom_active;
		lines.game = true;
	}
	fn on_freeze(&mut self, _lines: &mut LineState) {}
	fn add_bank(&mut self, bank: usize, addr: u16, data: &[u8]) {
		self.rom.store_bank(bank, data, (addr & 0x1FFF) as usize);
	}
	fn get_max_bank(&self) -> usize {
		0
	}
	fn get_debug_bank(&self) -> usize {
		0
	}
	fn get_info(&self) -> CartridgeInfo {
		CartridgeInfo {
			name: "Epyx FastLoad".to_string(),
			mapper_type: MapperType::EpyxFastLoad,
			rom_size: self.rom.len() * 8192,
			bank_count: 1,
			has_ram: false,
		}
	}
}