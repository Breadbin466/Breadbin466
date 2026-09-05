// =======================================================
// src/cartridge/mapper_action_replay.rs — Action Replay and Retro Replay
// =======================================================

use super::bank_storage::BankStorage;
use super::mapper_interface::{CartridgeInfo, CartridgeMapper, LineState, MapperType};

/* ActionReplayMapper combines banked ROM, writable RAM, mode control and freeze entry. Its register changes both memory visibility and which backing store answers the ROM windows. */
pub struct ActionReplayMapper {
	roml: BankStorage,
	romh: BankStorage,
	ram: Box<[u8; 8192]>,
	reg_control: u8,
	bank: usize,
	freeze_mode: bool,
	enabled: bool,
}
impl ActionReplayMapper {
	/* Power-on exposes the monitor ROM with bank zero selected and leaves the freezer RAM and NMI latches inactive. */
	pub fn new() -> Self {
		Self {
			roml: BankStorage::new(),
			romh: BankStorage::new(),
			ram: Box::new([0x00; 8192]),
			reg_control: 0,
			bank: 0,
			freeze_mode: false,
			enabled: true,
		}
	}
}
impl CartridgeMapper for ActionReplayMapper {
	/* Reset restores bank zero, enables the cartridge and leaves freeze RAM hidden until software or the freeze button selects it. */
	fn reset(&mut self) {
		self.reg_control = 0;
		self.bank = 0;
		self.freeze_mode = false;
		self.enabled = true;
	}
	/* Freeze re-enables the cartridge, selects bank zero and exposes the writable monitor workspace before the motherboard releases the NMI entry sequence. */
	fn on_freeze(&mut self, _lines: &mut LineState) {
		self.bank = 0;
		self.freeze_mode = true;
		self.enabled = true;
	}
	fn read_roml(&mut self, offset: u16, _cycle: u64) -> Option<u8> {
		if !self.enabled {
			return None;
		}
		let bank = self.bank;
		let ram_mode = self.freeze_mode || (self.reg_control & 0x20) != 0;
		if ram_mode {
			Some(self.ram[(offset & 0x1FFF) as usize])
		} else {
			self.roml
				.get_bank(bank)
				.map(|b| b[(offset & 0x1FFF) as usize])
		}
	}
	fn peek_roml(&self, offset: u16, _cycle: u64) -> Option<u8> {
		if !self.enabled {
			return None;
		}
		let bank = self.bank;
		let ram_mode = self.freeze_mode || (self.reg_control & 0x20) != 0;
		if ram_mode {
			Some(self.ram[(offset & 0x1FFF) as usize])
		} else {
			self.roml
				.get_bank(bank)
				.map(|b| b[(offset & 0x1FFF) as usize])
		}
	}
	fn read_romh(&mut self, offset: u16, _cycle: u64) -> Option<u8> {
		if !self.enabled {
			return None;
		}
		let bank = self.bank;
		self.romh
			.get_bank(bank)
			.map(|b| b[(offset & 0x1FFF) as usize])
	}
	fn peek_romh(&self, offset: u16, _cycle: u64) -> Option<u8> {
		if !self.enabled {
			return None;
		}
		let bank = self.bank;
		self.romh
			.get_bank(bank)
			.map(|b| b[(offset & 0x1FFF) as usize])
	}
	/* IO2 is both the Action Replay control aperture and a partially decoded RAM window, so reads can expose state that ordinary ROM accesses cannot. */
	fn read_io(&mut self, addr: u16, _cycle: u64) -> Option<u8> {
		if !self.enabled {
			return None;
		}
		if addr >= 0xDF00 && addr <= 0xDFFF {
			let ram_mode = self.freeze_mode || (self.reg_control & 0x20) != 0;
			if ram_mode {
				return Some(self.ram[0x1F00 + (addr & 0xFF) as usize]);
			}
			let bank = self.bank;
			return self
				.roml
				.get_bank(bank)
				.map(|b| b[0x1F00 + (addr & 0xFF) as usize]);
		}
		None
	}
	/* The DE00 control register changes bank, memory mode and cartridge visibility in one write. Bits 3-4 select one of four banks, bit 5 exposes cartridge RAM, bit 6 acknowledges freeze, and bit 2 disconnects the cartridge; DF00 remains a window onto the selected RAM or ROM tail. */
	fn write_io(&mut self, addr: u16, val: u8, _cycle: u64) {
		if !self.enabled {
			return;
		}
		if (0xDE00..=0xDEFF).contains(&addr) {
			self.reg_control = val;
			self.bank = ((val >> 3) & 0x03) as usize;
			if (val & 0x40) != 0 {
				self.freeze_mode = false;
			}
			if (val & 0x04) != 0 {
				self.enabled = false;
			}
		} else if addr >= 0xDF00 && addr <= 0xDFFF {
			let ram_mode = self.freeze_mode || (self.reg_control & 0x20) != 0;
			if ram_mode {
				self.ram[0x1F00 + (addr & 0xFF) as usize] = val;
			}
		}
	}
	fn write_rom(&mut self, addr: u16, val: u8, _cycle: u64) {
		if !self.enabled {
			return;
		}
		let ram_mode = self.freeze_mode || (self.reg_control & 0x20) != 0;

		if ram_mode && (0x8000..=0x9FFF).contains(&addr) {
			self.ram[(addr & 0x1FFF) as usize] = val;
		}
	}
	/* GAME and EXROM are derived from the same control state that selects ROM or RAM, so the PLA view changes atomically with the mapper register. */
	fn update_signals(&mut self, _cycle: u64, lines: &mut LineState) {
		lines.nmi_low = false;
		if !self.enabled {
			lines.game = true;
			lines.exrom = true;
		} else if self.freeze_mode {
			lines.game = false;
			lines.exrom = true;
		} else {
			/* The special $22 combination requests RAM at $A000 while retaining the 8 KiB cartridge line state; all other values use bits 0-1 directly as the GAME/EXROM mode. */
			let mode = if (self.reg_control & 0x23) == 0x22 {
				0
			} else {
				self.reg_control & 0x03
			};
			lines.game = (mode & 0x01) == 0;
			lines.exrom = (mode & 0x02) != 0;
		}
	}
	fn add_bank(&mut self, bank: usize, _addr: u16, data: &[u8]) {
		if bank == 0 && data.len() == 0x8000 {
			for i in 0..4 {
				let chunk = &data[i * 0x2000..(i + 1) * 0x2000];
				self.roml.store_bank(i, chunk, 0);
				self.romh.store_bank(i, chunk, 0);
			}
		} else if bank < 4 {
			self.roml.store_bank(bank, data, 0);
			self.romh.store_bank(bank, data, 0);
		}
	}
	fn get_max_bank(&self) -> usize {
		self.roml.len()
	}
	fn get_debug_bank(&self) -> usize {
		self.bank
	}
	fn load_nvram(&mut self, data: &[u8]) {
		let n = data.len().min(self.ram.len());
		self.ram[..n].copy_from_slice(&data[..n]);
	}
	fn save_nvram(&self) -> Option<Vec<u8>> {
		Some(self.ram.to_vec())
	}
	fn get_info(&self) -> CartridgeInfo {
		CartridgeInfo {
			name: "Action Replay".to_string(),
			mapper_type: MapperType::ActionReplay,
			rom_size: self.roml.len() * 8192,
			bank_count: self.roml.len(),
			has_ram: true,
		}
	}
}