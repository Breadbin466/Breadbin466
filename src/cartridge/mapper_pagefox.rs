// =======================================================
// src/cartridge/mapper_pagefox.rs — Pagefox cartridge mapper
// =======================================================

use super::mapper_interface::{CartridgeInfo, CartridgeMapper, LineState, MapperType};

/* PAGEFOX-HARDWARE: two 32 KiB EPROMs and one 32 KiB RAM share a
 * 16 KiB window. Register bit 1 supplies A14 and bits 2-3 select the chip.
 * Bit 4 releases GAME/EXROM but does not disable RAM writes. */
pub struct PagefoxMapper {
	rom: Vec<u8>,
	ram: Vec<u8>,
	control: u8,
}

impl PagefoxMapper {
	pub fn new() -> Self {
		Self { rom: vec![0xFF; 65536], ram: vec![0; 32768], control: 0 }
	}

	fn read_window(&self, offset: usize) -> Option<u8> {
		let bank_offset = usize::from(self.control & 2) << 13;
		match (self.control >> 2) & 3 {
			0 => Some(self.rom[bank_offset + offset]),
			1 => Some(self.rom[32768 + bank_offset + offset]),
			2 => Some(self.ram[bank_offset + offset]),
			_ => None,
		}
	}
}

impl CartridgeMapper for PagefoxMapper {
	fn reset(&mut self) { self.control = 0; }
	fn independent_write_window(&self) -> Option<(u16, u16)> {
		(self.control & 0x0C == 0x08).then_some((0x8000, 0xBFFF))
	}
	fn read_roml(&mut self, offset: u16, cycle: u64) -> Option<u8> { self.peek_roml(offset, cycle) }
	fn read_romh(&mut self, offset: u16, cycle: u64) -> Option<u8> { self.peek_romh(offset, cycle) }
	fn peek_roml(&self, offset: u16, _cycle: u64) -> Option<u8> { self.read_window(usize::from(offset & 0x1FFF)) }
	fn peek_romh(&self, offset: u16, _cycle: u64) -> Option<u8> { self.read_window(0x2000 + usize::from(offset & 0x1FFF)) }
	fn read_io(&mut self, _addr: u16, _cycle: u64) -> Option<u8> { None }
	fn write_io(&mut self, addr: u16, value: u8, _cycle: u64) {
		if (0xDE80..=0xDEFF).contains(&addr) { self.control = value; }
	}
	fn write_rom(&mut self, addr: u16, value: u8, _cycle: u64) {
		if self.control & 0x0C == 8 && (0x8000..=0xBFFF).contains(&addr) {
			let index = (usize::from(self.control & 2) << 13) + usize::from(addr - 0x8000);
			self.ram[index] = value;
		}
	}
	fn add_bank(&mut self, bank: usize, addr: u16, data: &[u8]) {
		if bank < 4 && (0x8000..=0xBFFF).contains(&addr) {
			let offset = usize::from(addr - 0x8000);
			let length = data.len().min(16384 - offset);
			let start = bank * 16384 + offset;
			self.rom[start..start + length].copy_from_slice(&data[..length]);
		}
	}
	fn update_signals(&mut self, _cycle: u64, lines: &mut LineState) {
		lines.game = self.control & 0x10 != 0;
		lines.exrom = lines.game;
	}
	fn on_freeze(&mut self, _lines: &mut LineState) {}
	fn get_info(&self) -> CartridgeInfo {
		CartridgeInfo { name: "Pagefox".to_string(), mapper_type: MapperType::Pagefox, rom_size: self.rom.len(), bank_count: 4, has_ram: true }
	}
	fn get_debug_bank(&self) -> usize { usize::from((self.control >> 1) & 7) }
	fn get_max_bank(&self) -> usize { 7 }
}