// =======================================================
// src/cartridge/mapper_standard.rs — Standard and Ultimax mappers
// =======================================================

use super::mapper_interface::{CartridgeMapper, LineState, MapperType, CartridgeInfo};
use super::bank_storage::BankStorage;

/* StandardMapper represents fixed ROML/ROMH cartridges with no banking register or runtime line changes. Packet load addresses decide which 8 KiB expansion-port window is populated. */
pub struct StandardMapper { roml: BankStorage, romh: BankStorage, kind: MapperType }

impl StandardMapper {
	/* Construction retains the exact fixed-map cartridge identity so diagnostics can distinguish Normal and Ultimax images even though their storage mechanics are shared. */
	pub fn new(kind: MapperType) -> Self { Self { roml: BankStorage::new(), romh: BankStorage::new(), kind } }
}

impl CartridgeMapper for StandardMapper {
	/* Fixed cartridges have no writable mapper state; reset intentionally leaves the CRT-defined ROM placement and static line levels unchanged. */
	fn reset(&mut self) {}
	fn read_roml(&mut self, offset: u16, _cycle: u64) -> Option<u8> { self.roml.get_bank(0).map(|b| b[(offset & 0x1FFF) as usize]) }
	fn peek_roml(&self, offset: u16, _cycle: u64) -> Option<u8> { self.roml.get_bank(0).map(|b| b[(offset & 0x1FFF) as usize]) }
	fn read_romh(&mut self, offset: u16, _cycle: u64) -> Option<u8> { self.romh.get_bank(0).map(|b| b[(offset & 0x1FFF) as usize]) }
	fn peek_romh(&self, offset: u16, _cycle: u64) -> Option<u8> { self.romh.get_bank(0).map(|b| b[(offset & 0x1FFF) as usize]) }
	fn read_io(&mut self, _addr: u16, _cycle: u64) -> Option<u8> { None }
	fn write_io(&mut self, _addr: u16, _val: u8, _cycle: u64) {}
	fn write_rom(&mut self, _addr: u16, _val: u8, _cycle: u64) {}
	/* Packet load address, rather than bank number, determines whether bytes feed ROML, ROMH or both halves of a 16 KiB image. */
	fn add_bank(&mut self, _bank: usize, addr: u16, data: &[u8]) {
		let offset = (addr & 0x1FFF) as usize;
		if addr == 0x8000 {
			let len_l = data.len().min(8192);
			self.roml.store_bank(0, &data[0..len_l], offset);
			if data.len() > 8192 { self.romh.store_bank(0, &data[8192..], 0); }
		} else if addr == 0xA000 || addr == 0xE000 {
			self.romh.store_bank(0, data, offset);
		} else if addr < 0xA000 {
			self.roml.store_bank(0, data, offset);
		} else {
			self.romh.store_bank(0, data, offset);
		}
	}
	/* Standard cartridges keep the static GAME/EXROM levels supplied by the CRT header; the mapper therefore has no runtime signal state to publish. */
	fn update_signals(&mut self, _cycle: u64, _lines: &mut LineState) {}
	fn on_freeze(&mut self, _lines: &mut LineState) {}
	fn get_max_bank(&self) -> usize { 0 }
	fn get_debug_bank(&self) -> usize { 0 }
	fn get_info(&self) -> CartridgeInfo {
		CartridgeInfo { name: "Standard/Ultimax".to_string(), mapper_type: self.kind, rom_size: (self.roml.len() + self.romh.len()) * 8192, bank_count: 1, has_ram: false }
	}
}