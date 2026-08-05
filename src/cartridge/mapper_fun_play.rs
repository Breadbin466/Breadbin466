// =======================================================
// src/cartridge/mapper_fun_play.rs — Fun Play cartridge mapper
// =======================================================

use super::mapper_interface::{CartridgeInfo, CartridgeMapper, LineState, MapperType};
use super::bank_storage::BankStorage;

/* FunPlayMapper decodes a sparse IO1 register layout into bank bits and a disable latch. The mapper preserves the original bit wiring instead of flattening it into a sequential bank index. */
pub struct FunPlayMapper {
	roml: BankStorage,
	bank: usize,
	off: bool,
	register: u8,
}

impl FunPlayMapper {
	/* Fun Play folds a sparse control-byte encoding into a physical ROML bank number. The unusual bit permutation mirrors the cartridge's address-line wiring rather than a linear software register. */
	pub fn new() -> Self {
		Self {
			roml: BankStorage::new(),
			bank: 0,
			off: false,
			register: 0,
		}
	}
}

impl CartridgeMapper for FunPlayMapper {
	fn reset(&mut self) {
		self.bank = 0;
		self.off = false;
		self.register = 0;
	}

	fn read_roml(&mut self, offset: u16, _cycle: u64) -> Option<u8> {
		if self.off {
			return None;
		}
		self.roml.get_resolved(self.bank).map(|data| data[(offset & 0x1FFF) as usize])
	}

	fn peek_roml(&self, offset: u16, _cycle: u64) -> Option<u8> {
		if self.off {
			return None;
		}
		self.roml.get_resolved(self.bank).map(|data| data[(offset & 0x1FFF) as usize])
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

	/* IO1 bits 3-5 become bank bits 0-2 and bit 0 becomes bank bit 3. The $86 pattern disables the cartridge, while $00 re-enables it. */
	fn write_io(&mut self, addr: u16, value: u8, _cycle: u64) {
		if (addr & 0xFF00) != 0xDE00 {
			return;
		}
		self.register = value;
		self.bank = (((value >> 3) & 7) | ((value & 1) << 3)) as usize;
		match value & 0xC6 {
			0x00 => self.off = false,
			0x86 => self.off = true,
			_ => {}
		}
	}

	fn write_rom(&mut self, _addr: u16, _value: u8, _cycle: u64) {}

	fn add_bank(&mut self, bank: usize, addr: u16, data: &[u8]) {

		let logical_bank = ((bank >> 3) & 7) | ((bank & 1) << 3);
		self.roml.store_bank(logical_bank, data, (addr & 0x1FFF) as usize);
	}

	/* The register can electrically detach the cartridge as well as select a bank, so line publication follows the decoded disable state. */
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
			name: "Fun Play".to_string(),
			mapper_type: MapperType::FunPlay,
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