// =======================================================
// src/cartridge/mapper_rgcd.rs — RGCD cartridge
// =======================================================

use super::mapper_interface::{CartridgeMapper, LineState, MapperType, CartridgeInfo};
use super::bank_storage::BankStorage;

/* RgcdMapper provides banked ROML with a latched disable bit. The register retains its written value for debug visibility while GAME and EXROM are derived from the enabled state. */
pub struct RgcdMapper { roml: BankStorage, bank: usize, disabled: bool, regval: u8, max_bank: usize }

impl RgcdMapper {
	/* RGCD decodes a single IO1 register: low bits select the ROML bank and the high control bit releases the cartridge, allowing menu firmware to hand control back to the underlying machine. */
	pub fn new() -> Self { Self { roml: BankStorage::new(), bank: 0, disabled: false, regval: 0, max_bank: 7 } }
}

impl CartridgeMapper for RgcdMapper {
	fn reset(&mut self) { self.bank = 0; self.disabled = false; self.regval = 0; }

	fn read_roml(&mut self, offset: u16, _cycle: u64) -> Option<u8> {
		if self.disabled { return None; }
		self.roml.get_resolved(self.bank).map(|b| b[(offset & 0x1FFF) as usize])
	}

	fn peek_roml(&self, offset: u16, _cycle: u64) -> Option<u8> {
		if self.disabled { return None; }
		self.roml.get_resolved(self.bank).map(|b| b[(offset & 0x1FFF) as usize])
	}

	fn read_romh(&mut self, _offset: u16, _cycle: u64) -> Option<u8> { None }

	fn peek_romh(&self, _offset: u16, _cycle: u64) -> Option<u8> { None }

	fn read_io(&mut self, _addr: u16, _cycle: u64) -> Option<u8> { None }

	fn peek_io(&self, addr: u16, _cycle: u64) -> Option<u8> {
		if (addr & 0xFF00) == 0xDE00 { Some(self.regval) } else { None }
	}

	/* IO1 bits 0-2 select one of eight ROML banks. Bit 3 is a one-way disable latch for the current reset interval and releases both cartridge lines when set. */
	fn write_io(&mut self, addr: u16, val: u8, _cycle: u64) {
		if (addr & 0xFF00) != 0xDE00 { return; }
		self.regval = val;
		self.bank = (val & 0x07) as usize;
		self.disabled |= (val & 0x08) != 0;
	}

	fn write_rom(&mut self, _addr: u16, _val: u8, _cycle: u64) {}

	fn add_bank(&mut self, bank: usize, addr: u16, data: &[u8]) {
		self.roml.store_bank(bank, data, (addr & 0x1FFF) as usize);
		self.max_bank = self.roml.populated_len().saturating_sub(1).min(7);
	}

	/* Only ROML participates in the map, so enabled operation is always the 8 KiB cartridge configuration. */
	fn update_signals(&mut self, _cycle: u64, lines: &mut LineState) {
		if self.disabled { lines.game = true; lines.exrom = true; } else { lines.game = true; lines.exrom = false; }
	}

	fn on_freeze(&mut self, _lines: &mut LineState) {}

	fn get_info(&self) -> CartridgeInfo {
		CartridgeInfo { name: "RGCD".to_string(), mapper_type: MapperType::RGCD, rom_size: self.roml.populated_len() * 8192, bank_count: self.max_bank + 1, has_ram: false }
	}

	fn get_debug_bank(&self) -> usize { self.bank }

	fn get_max_bank(&self) -> usize { self.max_bank }
}