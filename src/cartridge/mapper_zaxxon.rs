// =======================================================
// src/cartridge/mapper_zaxxon.rs — Zaxxon cartridge mapper
// =======================================================

use super::bank_storage::BankStorage;
use super::mapper_interface::{CartridgeInfo, CartridgeMapper, LineState, MapperType};

/* ZaxxonMapper uses reads from its smaller ROML image to select which ROMH bank becomes visible. This models address-decoded banking as a side effect of the lower-window access. */
pub struct ZaxxonMapper {
	roml: BankStorage,
	romh: BankStorage,
	bank: usize,
}
impl ZaxxonMapper {
	/* ROML occupies two 4 KiB halves. Address bit A12 selects ROMH bank zero or one as a side effect of the read, so reset merely chooses the initial bank before the first ROML access. */
	pub fn new() -> Self {
		Self {
			roml: BankStorage::new(),
			romh: BankStorage::new(),
			bank: 0,
		}
	}
}
impl CartridgeMapper for ZaxxonMapper {
	fn reset(&mut self) {
		self.bank = 0;
	}
	/* ROML is split into two 4 KiB halves. Reading either half selects the corresponding ROMH bank before returning the ROML byte. */
	fn read_roml(&mut self, offset: u16, _cycle: u64) -> Option<u8> {
		self.bank = usize::from((offset & 0x1000) != 0);
		self.roml
			.get_bank(0)
			.map(|bank| bank[(offset & 0x0FFF) as usize])
	}
	/* A debugger peek observes the ROML byte without reproducing the hardware read side effect that changes the active ROMH bank. */
	fn peek_roml(&self, offset: u16, _cycle: u64) -> Option<u8> {
		self.roml
			.get_bank(0)
			.map(|bank| bank[(offset & 0x0FFF) as usize])
	}
	fn read_romh(&mut self, offset: u16, _cycle: u64) -> Option<u8> {
		self.romh
			.get_resolved(self.bank)
			.map(|b| b[(offset & 0x1FFF) as usize])
	}
	fn peek_romh(&self, offset: u16, _cycle: u64) -> Option<u8> {
		self.romh
			.get_resolved(self.bank)
			.map(|b| b[(offset & 0x1FFF) as usize])
	}
	fn read_io(&mut self, _addr: u16, _cycle: u64) -> Option<u8> {
		None
	}
	fn write_io(&mut self, _addr: u16, _val: u8, _cycle: u64) {}
	fn write_rom(&mut self, _addr: u16, _val: u8, _cycle: u64) {}
	fn add_bank(&mut self, bank: usize, addr: u16, data: &[u8]) {
		let offset = (addr & 0x1FFF) as usize;
		if addr < 0xA000 {
			self.roml.store_bank(0, data, offset);
		} else {
			self.romh.store_bank(bank, data, offset);
		}
	}
	/* Zaxxon remains in the 16 KiB GAME/EXROM configuration while its internal address decode changes only the ROMH source. */
	fn update_signals(&mut self, _cycle: u64, lines: &mut LineState) {
		lines.game = false;
		lines.exrom = false;
	}
	fn on_freeze(&mut self, _lines: &mut LineState) {}
	fn get_info(&self) -> CartridgeInfo {
		CartridgeInfo {
			name: "Zaxxon".to_string(),
			mapper_type: MapperType::Zaxxon,
			rom_size: 4096 + self.romh.len() * 8192,
			bank_count: self.romh.len(),
			has_ram: false,
		}
	}
	fn get_debug_bank(&self) -> usize {
		self.bank
	}
	fn get_max_bank(&self) -> usize {
		self.romh.len().saturating_sub(1)
	}
}