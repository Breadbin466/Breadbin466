// =======================================================
// src/cartridge/mapper_kcs_power.rs — KCS Power Cartridge mapper
// =======================================================

use super::mapper_interface::{CartridgeMapper, LineState, MapperType, CartridgeInfo};
use super::bus_configuration::IoRead;
use super::bank_storage::BankStorage;
use super::crt_layout::add_bank_split;

/* KcsPowerMapper couples ROM, RAM and freeze logic with IO registers that can change the expansion-port mode. Freeze entry and normal banking remain separate state transitions. */
pub struct KCSMapper { roml: BankStorage, romh: BankStorage, ram: Box<[u8; 128]>, config: u8 }
impl KCSMapper {
	pub fn new() -> Self { Self { roml: BankStorage::new(), romh: BankStorage::new(), ram: Box::new([0x00; 128]), config: 1 } }
}
impl CartridgeMapper for KCSMapper {
	/* Reset selects the normal startup map; freeze changes the same configuration latch to expose the monitor and scratch RAM. */
	fn reset(&mut self) { self.config = 1; }
	fn on_freeze(&mut self, _lines: &mut LineState) { self.config = 3; }
	fn read_roml(&mut self, offset: u16, _cycle: u64) -> Option<u8> { self.roml.get_bank(0).map(|b| b[(offset & 0x1FFF) as usize]) }
	fn peek_roml(&self, offset: u16, _cycle: u64) -> Option<u8> { self.roml.get_bank(0).map(|b| b[(offset & 0x1FFF) as usize]) }
	fn read_romh(&mut self, offset: u16, _cycle: u64) -> Option<u8> { self.romh.get_bank(0).map(|b| b[(offset & 0x1FFF) as usize]) }
	fn peek_romh(&self, offset: u16, _cycle: u64) -> Option<u8> { self.romh.get_bank(0).map(|b| b[(offset & 0x1FFF) as usize]) }
	fn read_io(&mut self, addr: u16, _cycle: u64) -> Option<u8> {
		if addr >= 0xDE00 && addr <= 0xDEFF {
			self.config = if (addr & 2) != 0 { 2 } else { 0 };
			return self.roml.get_bank(0).map(|b| b[0x1E00 + (addr & 0xFF) as usize]);
		}
		if addr >= 0xDF00 && addr <= 0xDFFF {
			if (addr & 0x80) != 0 {
				let mut ret = 0u8;
				if (self.config & 2) != 0 { ret |= 0x80; }
				if (self.config & 1) == 0 { ret |= 0x40; }
				return Some(ret);
			}
			return Some(self.ram[(addr & 0x7F) as usize]);
		}
		None
	}
	/* KCS I/O mixes control registers with 128 bytes of mirrored scratch RAM; only implemented data lines are driven, while the remaining bits retain the floating-bus value. */
	fn read_io_bus(&mut self, addr: u16, cycle: u64) -> IoRead {
		if addr >= 0xDF80 && addr <= 0xDFFF {
			let mut value = 0u8;
			if (self.config & 2) != 0 { value |= 0x80; }
			if (self.config & 1) == 0 { value |= 0x40; }
			return IoRead::PartiallyDriven { value, mask: 0xC0 };
		}
		match self.read_io(addr, cycle) {
			Some(value) => IoRead::Driven(value),
			None => IoRead::NotDecoded,
		}
	}
	/* IO1 and IO2 accesses update the configuration and scratch RAM independently, allowing the monitor to change mapping while preserving its workspace. */
	fn write_io(&mut self, addr: u16, val: u8, _cycle: u64) {
		if addr >= 0xDE00 && addr <= 0xDEFF {
			self.config = if (addr & 2) != 0 { 3 } else { 1 };
		} else if addr >= 0xDF00 && addr <= 0xDFFF && (addr & 0x80) == 0 {
			self.ram[(addr & 0x7F) as usize] = val;
		}
	}
	fn write_rom(&mut self, _addr: u16, _val: u8, _cycle: u64) {}
	fn add_bank(&mut self, bank: usize, addr: u16, data: &[u8]) { add_bank_split(&mut self.roml, &mut self.romh, bank, addr, data); }
	fn update_signals(&mut self, _cycle: u64, lines: &mut LineState) {
		match self.config {
			0 => { lines.game = true; lines.exrom = false; }
			1 => { lines.game = false; lines.exrom = false; }
			2 => { lines.game = true; lines.exrom = true; }
			_ => { lines.game = false; lines.exrom = true; }
		}
	}
	fn get_max_bank(&self) -> usize { self.roml.len() }
	fn get_debug_bank(&self) -> usize { 0 }
	fn load_nvram(&mut self, data: &[u8]) { let n = data.len().min(self.ram.len()); self.ram[..n].copy_from_slice(&data[..n]); }
	fn save_nvram(&self) -> Option<Vec<u8>> { Some(self.ram.to_vec()) }
	fn get_info(&self) -> CartridgeInfo {
		CartridgeInfo { name: "KCS Power Cartridge".to_string(), mapper_type: MapperType::KCS, rom_size: (self.roml.len() + self.romh.len()) * 8192, bank_count: 1, has_ram: true }
	}
}