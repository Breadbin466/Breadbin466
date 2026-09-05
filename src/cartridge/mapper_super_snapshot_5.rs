// =======================================================
// src/cartridge/mapper_super_snapshot_5.rs — Super Snapshot V5 cartridge mapper
// =======================================================

use super::bank_storage::BankStorage;
use super::crt_layout::add_bank_split;
use super::mapper_interface::{CartridgeInfo, CartridgeMapper, LineState, MapperType};

/* SuperSnapshot5Mapper combines banked ROM, RAM overlays and a freezer-controlled memory map. Register writes select both the active bank and whether ROM or RAM drives each cartridge window. */
pub struct SuperSnapshot5Mapper {
	roml: BankStorage,
	romh: BankStorage,
	ram: Box<[u8; 0x8000]>,
	bank: usize,
	ram_bank: usize,
	cmode: u8,
	ram_enabled: bool,
	disabled: bool,
}
impl SuperSnapshot5Mapper {
	/* Super Snapshot starts in its boot mapping with writable monitor RAM exposed, matching the firmware state expected immediately after reset. */
	pub fn new() -> Self {
		Self {
			roml: BankStorage::new(),
			romh: BankStorage::new(),
			ram: Box::new([0x00; 0x8000]),
			bank: 0,
			ram_bank: 0,
			cmode: 3,
			ram_enabled: true,
			disabled: false,
		}
	}
}
impl CartridgeMapper for SuperSnapshot5Mapper {
	fn reset(&mut self) {
		self.bank = 0;
		self.ram_bank = 0;
		self.cmode = 3;
		self.ram_enabled = true;
		self.disabled = false;
	}
	/* Freeze selects the monitor bank, re-enables the cartridge and exposes writable working RAM before NMI execution begins, allowing firmware to save machine state immediately. */
	fn on_freeze(&mut self, _lines: &mut LineState) {
		self.cmode = 3;
		self.ram_enabled = true;
		self.disabled = false;
	}
	fn read_roml(&mut self, offset: u16, _cycle: u64) -> Option<u8> {
		if self.ram_enabled {
			let index = self.ram_bank * 0x2000 + (offset & 0x1FFF) as usize;
			return Some(self.ram[index]);
		}
		if self.disabled {
			return None;
		}
		self.roml
			.get_resolved(self.bank)
			.map(|data| data[(offset & 0x1FFF) as usize])
	}
	fn peek_roml(&self, offset: u16, _cycle: u64) -> Option<u8> {
		if self.ram_enabled {
			let index = self.ram_bank * 0x2000 + (offset & 0x1FFF) as usize;
			return Some(self.ram[index]);
		}
		if self.disabled {
			return None;
		}
		self.roml
			.get_resolved(self.bank)
			.map(|data| data[(offset & 0x1FFF) as usize])
	}
	fn read_romh(&mut self, offset: u16, _cycle: u64) -> Option<u8> {
		if self.disabled {
			return None;
		}
		self.romh
			.get_resolved(self.bank)
			.map(|data| data[(offset & 0x1FFF) as usize])
	}
	fn peek_romh(&self, offset: u16, _cycle: u64) -> Option<u8> {
		if self.disabled {
			return None;
		}
		self.romh
			.get_resolved(self.bank)
			.map(|data| data[(offset & 0x1FFF) as usize])
	}
	fn read_io(&mut self, addr: u16, _cycle: u64) -> Option<u8> {
		if self.disabled || !(0xDE00..=0xDEFF).contains(&addr) {
			return None;
		}
		self.roml
			.get_resolved(self.bank)
			.map(|data| data[0x1E00 + (addr & 0xFF) as usize])
	}
	/* The IO1 control byte inverts bits 0-1 into the four cartridge modes, uses bits 2 and 4 as bank bits zero and one, optionally bit 5 as bank bit two, enables the RAM overlay when bit 1 is clear, and disconnects the cartridge when bit 3 is set. */
	fn write_io(&mut self, addr: u16, val: u8, _cycle: u64) {
		if self.disabled || !(0xDE00..=0xDEFF).contains(&addr) {
			return;
		}
		self.cmode = ((!val) & 0x03) as u8;
		self.bank = (((val >> 2) & 0x01) | (((val >> 4) & 0x01) << 1)) as usize;
		if self.roml.len() > 4 {
			self.bank |= (((val >> 5) & 0x01) << 2) as usize;
		}
		self.ram_bank = self.bank & 0x03;
		self.ram_enabled = (val & 0x02) == 0;
		self.disabled = (val & 0x08) != 0;
	}
	fn write_rom(&mut self, addr: u16, val: u8, _cycle: u64) {
		if self.ram_enabled && (0x8000..=0x9FFF).contains(&addr) {
			let index = self.ram_bank * 0x2000 + (addr & 0x1FFF) as usize;
			self.ram[index] = val;
		}
	}
	fn add_bank(&mut self, bank: usize, addr: u16, data: &[u8]) {
		add_bank_split(&mut self.roml, &mut self.romh, bank, addr, data);
	}
	/* Signal publication derives from the enable and mode latches after every access, keeping memory routing consistent with the selected ROM/RAM bank. */
	fn update_signals(&mut self, _cycle: u64, lines: &mut LineState) {
		if self.disabled {
			lines.game = true;
			lines.exrom = true;
			return;
		}
		/* cmode 0 selects 8 KiB, 1 selects 16 KiB, 2 releases both lines, and 3 selects Ultimax. */
		match self.cmode {
			0 => {
				lines.game = true;
				lines.exrom = false;
			}
			1 => {
				lines.game = false;
				lines.exrom = false;
			}
			2 => {
				lines.game = true;
				lines.exrom = true;
			}
			_ => {
				lines.game = false;
				lines.exrom = true;
			}
		}
	}
	fn get_max_bank(&self) -> usize {
		self.roml.len()
	}
	fn get_debug_bank(&self) -> usize {
		self.bank
	}
	fn load_nvram(&mut self, data: &[u8]) {
		let count = data.len().min(self.ram.len());
		self.ram[..count].copy_from_slice(&data[..count]);
	}
	fn save_nvram(&self) -> Option<Vec<u8>> {
		Some(self.ram.to_vec())
	}
	fn get_info(&self) -> CartridgeInfo {
		CartridgeInfo {
			name: "Super Snapshot V5".to_string(),
			mapper_type: MapperType::SuperSnapshot5,
			rom_size: (self.roml.len() + self.romh.len()) * 8192,
			bank_count: self.roml.len(),
			has_ram: true,
		}
	}
}