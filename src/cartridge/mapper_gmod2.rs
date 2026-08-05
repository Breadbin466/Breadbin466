// =======================================================
// src/cartridge/mapper_gmod2.rs — GMod2 cartridge and serial EEPROM
// =======================================================

use super::mapper_interface::{CartridgeMapper, LineState, CartridgeInfo, MapperType};
use super::bus_configuration::IoRead;
use super::bank_storage::BankStorage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/* EepromMode follows the serial transaction from command reception through address capture and byte transfer. Mode changes occur only while chip select keeps one transaction active. */
enum EepromMode { Idle, ReadCommand, ReadAddress, ReadData, WriteAddress, WriteData }

/* Gmod2Eeprom models the small serial non-volatile memory as an edge-driven protocol machine. Chip select frames a transaction, rising clock edges shift command or data bits, and the output line is sampled through the cartridge I/O bus. */
struct Gmod2Eeprom {
	last_sck: bool, last_cs: bool, bit_counter: u8, command_byte: u8,
	address_byte: u16, mode: EepromMode, data_buffer: u8,
	pub storage: [u8; 512],
}

impl Gmod2Eeprom {
	fn new() -> Self {
		Self {
			last_sck: true, last_cs: true, bit_counter: 0, command_byte: 0,
			address_byte: 0, mode: EepromMode::Idle, data_buffer: 0,
			storage: [0xFF; 512],
		}
	}

	fn reset(&mut self) {
		self.last_sck = true; self.last_cs = true; self.bit_counter = 0;
		self.command_byte = 0; self.address_byte = 0; self.mode = EepromMode::Idle;
		self.data_buffer = 0;
	}

	fn process_pins(&mut self, ctrl: u8) -> u8 {
		let cs = (ctrl & 0x01) != 0;
		let sck = (ctrl & 0x02) != 0;
		let mosi = (ctrl & 0x04) != 0;

		if !cs {
			self.mode = EepromMode::Idle;
			self.bit_counter = 0;
			self.last_cs = cs;
			self.last_sck = sck;
			return 0x00;
		}

		if !self.last_cs && cs {
			self.mode = EepromMode::ReadCommand;
			self.bit_counter = 0;
			self.command_byte = 0;
		}

		let mut miso_out = 0x00;

		if !sck && self.last_sck {
			if self.mode == EepromMode::ReadData {
				let bit_idx = 7 - (self.bit_counter % 8);
				let byte_val = self.storage[(self.address_byte & 0x1FF) as usize];
				if (byte_val & (1 << bit_idx)) != 0 { miso_out = 0x08; }
				self.bit_counter += 1;
				if self.bit_counter % 8 == 0 { self.address_byte = (self.address_byte + 1) & 0x1FF; }
			}
		} else if sck && !self.last_sck {
			match self.mode {
				EepromMode::ReadCommand => {
					self.command_byte = (self.command_byte << 1) | (if mosi { 1 } else { 0 });
					self.bit_counter += 1;
					if self.bit_counter == 3 {
						let op = (self.command_byte >> 1) & 0x03;
						let high_bit = if (self.command_byte & 1) != 0 { 0x0100 } else { 0 };
						self.address_byte = high_bit;
						self.mode = if op == 0x02 { EepromMode::ReadAddress }
									else if op == 0x01 { EepromMode::WriteAddress }
									else { EepromMode::Idle };
						self.bit_counter = 0;
					}
				}
				EepromMode::ReadAddress => {
					self.address_byte = (self.address_byte & 0x0100) | ((self.address_byte & 0x00FF) << 1) | (if mosi { 1 } else { 0 });
					self.bit_counter += 1;
					if self.bit_counter == 8 { self.mode = EepromMode::ReadData; self.bit_counter = 0; }
				}
				EepromMode::WriteAddress => {
					self.address_byte = (self.address_byte & 0x0100) | ((self.address_byte & 0x00FF) << 1) | (if mosi { 1 } else { 0 });
					self.bit_counter += 1;
					if self.bit_counter == 8 { self.mode = EepromMode::WriteData; self.bit_counter = 0; self.data_buffer = 0; }
				}
				EepromMode::WriteData => {
					self.data_buffer = (self.data_buffer << 1) | (if mosi { 1 } else { 0 });
					self.bit_counter += 1;
					if self.bit_counter == 8 {
						self.storage[(self.address_byte & 0x1FF) as usize] = self.data_buffer;
						self.address_byte = (self.address_byte + 1) & 0x1FF;
						self.bit_counter = 0;
					}
				}
				_ => {}
			}
		}

		self.last_cs = cs;
		self.last_sck = sck;
		miso_out
	}
}

/* GMod2Mapper combines a large banked ROM image with serial EEPROM storage. Bank control and EEPROM line signalling share the cartridge IO space but remain separate internal devices. */
pub struct GMod2Mapper {
	roml: BankStorage, romh: BankStorage, bank: usize, cmode: u8,
	eeprom: Gmod2Eeprom, eeprom_cs: bool, romh_enabled: bool,
}
impl GMod2Mapper {
	/* GMod2 starts with ROM bank zero visible and the serial EEPROM deselected, leaving its data output electrically idle. */
	pub fn new() -> Self {
		Self { roml: BankStorage::new(), romh: BankStorage::new(), bank: 0, cmode: 0,
			eeprom: Gmod2Eeprom::new(), eeprom_cs: false, romh_enabled: false }
	}
}
impl CartridgeMapper for GMod2Mapper {
	fn reset(&mut self) {
		self.bank = 0;
		self.cmode = 0;
		self.eeprom_cs = false;
		self.romh_enabled = false;
		self.eeprom.reset();
	}
	fn read_roml(&mut self, offset: u16, _cycle: u64) -> Option<u8> {
		if self.cmode != 0 { return None; }
		self.roml.get_resolved(self.bank).map(|b| b[(offset & 0x1FFF) as usize])
	}
	fn peek_roml(&self, offset: u16, _cycle: u64) -> Option<u8> {
		if self.cmode != 0 { return None; }
		self.roml.get_resolved(self.bank).map(|b| b[(offset & 0x1FFF) as usize])
	}
	fn read_romh(&mut self, offset: u16, _cycle: u64) -> Option<u8> {
		if !self.romh_enabled { return None; }
		self.romh.get_resolved(self.bank).map(|b| b[(offset & 0x1FFF) as usize])
	}
	fn peek_romh(&self, offset: u16, _cycle: u64) -> Option<u8> {
		if !self.romh_enabled { return None; }
		self.romh.get_resolved(self.bank).map(|b| b[(offset & 0x1FFF) as usize])
	}
	fn read_io(&mut self, addr: u16, _cycle: u64) -> Option<u8> {
		if (addr & 0xFF00) != 0xDE00 || !self.eeprom_cs { return None; }
		let bit = self.eeprom.process_pins(0x01) & 0x08;
		Some(if bit != 0 { 0x80 } else { 0x00 })
	}
	/* The serial EEPROM contributes only its data-output line to the I/O byte; the remaining seven bits retain the motherboard bus value through the drive mask. */
	fn read_io_bus(&mut self, addr: u16, _cycle: u64) -> IoRead {
		if (addr & 0xFF00) != 0xDE00 || !self.eeprom_cs { return IoRead::NotDecoded; }
		let bit = self.eeprom.process_pins(0x01) & 0x08;
		IoRead::PartiallyDriven { value: if bit != 0 { 0x80 } else { 0x00 }, mask: 0x80 }
	}
	fn peek_io(&self, addr: u16, _cycle: u64) -> Option<u8> {
		if (addr & 0xFF00) != 0xDE00 || !self.eeprom_cs { return None; }
		Some(0)
	}
	/* IO1 writes update the ROM bank and EEPROM chip-select, clock and data pins together; the serial engine advances only on the relevant signal edges. */
	fn write_io(&mut self, addr: u16, val: u8, _cycle: u64) {
		if !(addr >= 0xDE00 && addr <= 0xDEFF) { return; }
		self.bank = (val & 0x3F) as usize;
		self.cmode = if (val & 0xC0) == 0xC0 { 2 } else if (val & 0x40) == 0 { 0 } else { 1 };
		self.romh_enabled = self.cmode == 2;
		self.eeprom_cs = (val & 0x40) != 0;
		let cs = (val >> 6) & 1;
		let clock = (val >> 5) & 1;
		let data = (val >> 4) & 1;
		let ctrl = cs | (clock << 1) | (data << 2);
		self.eeprom.process_pins(ctrl);
	}
	/* Writes in the mapped ROM area are redirected to cartridge RAM when the active mode exposes writable storage. */
	fn write_rom(&mut self, addr: u16, val: u8, _cycle: u64) {
		if self.cmode != 2 || addr < 0xE000 { return; }
		if let Some(bank) = self.romh.get_resolved_mut(self.bank) {
			let index = (addr & 0x1FFF) as usize;
			bank[index] &= val;
		}
	}
	fn add_bank(&mut self, bank: usize, addr: u16, data: &[u8]) {
		let offset = (addr & 0x1FFF) as usize;
		if addr < 0xA000 { self.roml.store_bank(bank, data, offset); } else { self.romh.store_bank(bank, data, offset); }
	}
	fn update_signals(&mut self, _cycle: u64, lines: &mut LineState) {
		match self.cmode {
			0 => { lines.game = true; lines.exrom = false; }
			1 => { lines.game = true; lines.exrom = true; }
			_ => { lines.game = false; lines.exrom = true; }
		}
	}
	fn on_freeze(&mut self, _lines: &mut LineState) {}
	fn get_info(&self) -> CartridgeInfo {
		CartridgeInfo { name: "GMod2".to_string(), mapper_type: MapperType::GMod2, rom_size: (self.roml.len() + self.romh.len()) * 8192, bank_count: self.roml.len().max(1), has_ram: true }
	}
	fn get_debug_bank(&self) -> usize { self.bank }
	fn get_max_bank(&self) -> usize { self.roml.len().saturating_sub(1) }
	fn load_nvram(&mut self, data: &[u8]) {
		let n = data.len().min(self.eeprom.storage.len()); self.eeprom.storage[..n].copy_from_slice(&data[..n]);
	}
	fn save_nvram(&self) -> Option<Vec<u8>> { Some(self.eeprom.storage.to_vec()) }
}