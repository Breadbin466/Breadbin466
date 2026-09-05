// =======================================================
// src/cartridge/mapper_gmod2.rs — GMod2 cartridge and serial EEPROM
// =======================================================

use super::bank_storage::BankStorage;
use super::bus_configuration::IoRead;
use super::mapper_interface::{CartridgeInfo, CartridgeMapper, ChipType, LineState, MapperType};

const EEPROM_BYTES: usize = 2048;
const EEPROM_WORDS: u16 = 1024;
const EEPROM_ADDRESS_BITS: u8 = 10;
const EEPROM_WORD_BITS: u8 = 16;
const EEPROM_PROGRAM_BUSY_CYCLES: u64 = 3_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/* The M93C86 is wired for x16 organisation on GMod2, so transactions address 1024 words and transfer one 16-bit word at a time. */
enum EepromMode {
	Idle,
	Command,
	Read,
	WriteData,
	WriteAllData,
	Status,
}

/*
GMod2 connects an M93C86 EEPROM in x16 organisation to three bits of the $DE00 control register. CS frames a MICROWIRE transaction, DI is sampled on rising clock edges, and DO changes during reads so software can sample it through bit 7 of $DE00. The write-enable latch survives CS transitions and is changed only by EWEN/EWDS commands.

The byte array remains the persistence format because CRT and sidecar data are byte-oriented. Word addresses are converted explicitly to big-endian byte pairs at the EEPROM boundary; serial transfers themselves remain MSB-first as specified by the device protocol.
*/
struct Gmod2Eeprom {
	storage: [u8; EEPROM_BYTES],
	mode: EepromMode,
	last_cs: bool,
	last_clock: bool,
	started: bool,
	opcode: u8,
	address: u16,
	input_bits: u8,
	input_word: u16,
	output_word: u16,
	output_bits: u8,
	data_out: bool,
	write_enabled: bool,
	programming_until: Option<u64>,
}

impl Gmod2Eeprom {
	fn new() -> Self {
		Self {
			storage: [0xFF; EEPROM_BYTES],
			mode: EepromMode::Idle,
			last_cs: false,
			last_clock: false,
			started: false,
			opcode: 0,
			address: 0,
			input_bits: 0,
			input_word: 0,
			output_word: 0,
			output_bits: 0,
			data_out: false,
			write_enabled: false,
			programming_until: None,
		}
	}

	fn reset(&mut self) {
		self.mode = EepromMode::Idle;
		self.last_cs = false;
		self.last_clock = false;
		self.started = false;
		self.opcode = 0;
		self.address = 0;
		self.input_bits = 0;
		self.input_word = 0;
		self.output_word = 0;
		self.output_bits = 0;
		self.data_out = false;
		self.write_enabled = false;
		self.programming_until = None;
	}

	#[inline(always)]
	fn read_word(&self, address: u16) -> u16 {
		let index = ((address % EEPROM_WORDS) as usize) * 2;
		u16::from_be_bytes([self.storage[index], self.storage[index + 1]])
	}

	#[inline(always)]
	fn write_word(&mut self, address: u16, value: u16) {
		let index = ((address % EEPROM_WORDS) as usize) * 2;
		let [high, low] = value.to_be_bytes();
		self.storage[index] = high;
		self.storage[index + 1] = low;
	}

	fn begin_transaction(&mut self, cycle: u64) {
		self.started = false;
		self.opcode = 0;
		self.address = 0;
		self.input_bits = 0;
		self.input_word = 0;
		self.output_word = 0;
		self.output_bits = 0;

		if self.programming_until.is_some() {
			self.mode = EepromMode::Status;
			self.data_out = self.programming_ready(cycle);
		} else {
			self.mode = EepromMode::Command;
			self.data_out = false;
		}
	}

	fn end_transaction(&mut self) {
		self.mode = EepromMode::Idle;
		self.started = false;
		self.input_bits = 0;
		self.output_bits = 0;
		self.data_out = false;
	}

	#[inline(always)]
	fn programming_ready(&self, cycle: u64) -> bool {
		self.programming_until.is_some_and(|until| cycle >= until)
	}

	fn begin_programming(&mut self, cycle: u64) {
		self.programming_until = Some(cycle.saturating_add(EEPROM_PROGRAM_BUSY_CYCLES));
		self.mode = EepromMode::Idle;
		self.data_out = false;
	}

	fn begin_command_from_status(&mut self) {
		self.programming_until = None;
		self.mode = EepromMode::Command;
		self.started = true;
		self.opcode = 0;
		self.address = 0;
		self.input_bits = 0;
		self.input_word = 0;
		self.output_word = 0;
		self.output_bits = 0;
		self.data_out = false;
	}

	/* A complete x16 command consists of one start bit, a two-bit opcode and ten address/control bits. */
	fn clock_command_bit(&mut self, data_in: bool, cycle: u64) {
		if !self.started {
			if data_in {
				self.started = true;
				self.opcode = 0;
				self.address = 0;
				self.input_bits = 0;
			}
			return;
		}

		if self.input_bits < 2 {
			self.opcode = (self.opcode << 1) | u8::from(data_in);
			self.input_bits += 1;
			return;
		}

		self.address = (self.address << 1) | u16::from(data_in);
		self.input_bits += 1;
		if self.input_bits != 2 + EEPROM_ADDRESS_BITS {
			return;
		}

		self.address &= EEPROM_WORDS - 1;
		match self.opcode {
			0b10 => {
				self.output_word = self.read_word(self.address);
				self.output_bits = EEPROM_WORD_BITS;
				self.mode = EepromMode::Read;
			}
			0b01 => {
				self.input_word = 0;
				self.input_bits = 0;
				self.mode = EepromMode::WriteData;
			}
			0b11 => {
				if self.write_enabled {
					self.write_word(self.address, 0xFFFF);
					self.begin_programming(cycle);
				} else {
					self.mode = EepromMode::Idle;
				}
			}
			0b00 => {
				match (self.address >> 8) & 0x03 {
					0b00 => self.write_enabled = false,
					0b01 => {
						self.input_word = 0;
						self.input_bits = 0;
						self.mode = EepromMode::WriteAllData;
						return;
					}
					0b10 => {
						if self.write_enabled {
							self.storage.fill(0xFF);
							self.begin_programming(cycle);
							return;
						}
					}
					0b11 => self.write_enabled = true,
					_ => {}
				}
				self.mode = EepromMode::Idle;
			}
			_ => {}
		}
	}

	fn clock_write_bit(&mut self, data_in: bool, write_all: bool, cycle: u64) {
		self.input_word = (self.input_word << 1) | u16::from(data_in);
		self.input_bits += 1;
		if self.input_bits != EEPROM_WORD_BITS {
			return;
		}

		if self.write_enabled {
			if write_all {
				for address in 0..EEPROM_WORDS {
					self.write_word(address, self.input_word);
				}
			} else {
				self.write_word(self.address, self.input_word);
			}
			self.begin_programming(cycle);
		} else {
			self.mode = EepromMode::Idle;
		}
		self.input_bits = 0;
	}

	/* During sequential READ, each falling clock edge presents the next MSB-first data bit and advances to the next word after bit zero. */
	fn clock_read_output(&mut self) {
		if self.output_bits == 0 {
			self.address = (self.address + 1) % EEPROM_WORDS;
			self.output_word = self.read_word(self.address);
			self.output_bits = EEPROM_WORD_BITS;
		}
		let bit = self.output_bits - 1;
		self.data_out = (self.output_word & (1u16 << bit)) != 0;
		self.output_bits -= 1;
	}

	/* Register writes directly drive CS, CLK and DI. Input is sampled on CLK rising edges; READ data is advanced on falling edges. During a self-timed write or erase, reselecting the EEPROM exposes READY/BUSY on DO until the next START bit. */
	fn drive(&mut self, cs: bool, clock: bool, data_in: bool, cycle: u64) {
		if !self.last_cs && cs {
			self.begin_transaction(cycle);
		} else if self.last_cs && !cs {
			self.end_transaction();
		}

		if cs {
			if self.mode == EepromMode::Status {
				self.data_out = self.programming_ready(cycle);
			}

			if !self.last_clock && clock {
				match self.mode {
					EepromMode::Command => self.clock_command_bit(data_in, cycle),
					EepromMode::WriteData => self.clock_write_bit(data_in, false, cycle),
					EepromMode::WriteAllData => self.clock_write_bit(data_in, true, cycle),
					EepromMode::Status if data_in && self.programming_ready(cycle) => {
						self.begin_command_from_status()
					}
					EepromMode::Idle | EepromMode::Read | EepromMode::Status => {}
				}
			} else if self.last_clock && !clock && self.mode == EepromMode::Read {
				self.clock_read_output();
			}
		}

		self.last_cs = cs;
		self.last_clock = clock;
	}

	#[inline(always)]
	fn sample_data_out(&mut self, cycle: u64) -> bool {
		if self.mode == EepromMode::Status {
			self.data_out = self.programming_ready(cycle);
		}
		self.data_out
	}
}

/*
GMod2 is a fixed 8K GAME cartridge with a 512 KiB flash device and a 2 KiB serial EEPROM sharing the $DE00 register. Bits 0-5 select the flash bank. Bit 6 deasserts EXROM while selecting the EEPROM. Bit 7 is the flash write-enable control and the EEPROM data-output bit on reads; it is not a cartridge mapping mode bit.
*/
pub struct GMod2Mapper {
	roml: BankStorage,
	bank: usize,
	control: u8,
	eeprom: Gmod2Eeprom,
}

impl GMod2Mapper {
	pub fn new() -> Self {
		Self {
			roml: BankStorage::new(),
			bank: 0,
			control: 0,
			eeprom: Gmod2Eeprom::new(),
		}
	}

	#[inline(always)]
	fn eeprom_selected(&self) -> bool {
		self.control & 0x40 != 0
	}

	#[inline(always)]
	fn flash_write_enabled(&self) -> bool {
		self.control & 0xC0 == 0xC0
	}
}

impl CartridgeMapper for GMod2Mapper {
	fn reset(&mut self) {
		self.bank = 0;
		self.control = 0;
		self.eeprom.reset();
	}

	fn read_roml(&mut self, offset: u16, _cycle: u64) -> Option<u8> {
		if self.eeprom_selected() {
			return None;
		}
		self.roml
			.get_resolved(self.bank)
			.map(|data| data[(offset & 0x1FFF) as usize])
	}

	fn peek_roml(&self, offset: u16, _cycle: u64) -> Option<u8> {
		if self.eeprom_selected() {
			return None;
		}
		self.roml
			.get_resolved(self.bank)
			.map(|data| data[(offset & 0x1FFF) as usize])
	}

	fn read_romh(&mut self, _offset: u16, _cycle: u64) -> Option<u8> {
		None
	}

	fn peek_romh(&self, _offset: u16, _cycle: u64) -> Option<u8> {
		None
	}

	fn read_io(&mut self, addr: u16, cycle: u64) -> Option<u8> {
		if (addr & 0xFF00) != 0xDE00 {
			return None;
		}
		Some(if self.eeprom.sample_data_out(cycle) {
			0x80
		} else {
			0x00
		})
	}

	/* Only bit 7 is driven by the EEPROM data-output path; the rest of the byte remains the shared motherboard bus value. */
	fn read_io_bus(&mut self, addr: u16, cycle: u64) -> IoRead {
		if (addr & 0xFF00) != 0xDE00 {
			return IoRead::NotDecoded;
		}
		IoRead::PartiallyDriven {
			value: if self.eeprom.sample_data_out(cycle) {
				0x80
			} else {
				0x00
			},
			mask: 0x80,
		}
	}

	fn peek_io(&self, addr: u16, cycle: u64) -> Option<u8> {
		if (addr & 0xFF00) != 0xDE00 {
			return None;
		}
		let data_out = if self.eeprom.mode == EepromMode::Status {
			self.eeprom.programming_ready(cycle)
		} else {
			self.eeprom.data_out
		};
		Some(if data_out { 0x80 } else { 0x00 })
	}

	/* Every address in the IO1 page mirrors the single GMod2 register. Banking and EEPROM pins therefore change from the same written byte. */
	fn write_io(&mut self, addr: u16, value: u8, cycle: u64) {
		if (addr & 0xFF00) != 0xDE00 {
			return;
		}
		self.control = value;
		self.bank = (value & 0x3F) as usize;
		self.eeprom.drive(
			value & 0x40 != 0,
			value & 0x20 != 0,
			value & 0x10 != 0,
			cycle,
		);
	}

	/* GMod2 flash programming is enabled only by bits 7 and 6 together; normal game operation never treats bit 7 as a ROMH/Ultimax mapping selector. */
	fn write_rom(&mut self, addr: u16, value: u8, _cycle: u64) {
		if !self.flash_write_enabled() || addr < 0xE000 {
			return;
		}
		if let Some(bank) = self.roml.get_resolved_mut(self.bank) {
			let index = (addr & 0x1FFF) as usize;
			bank[index] &= value;
		}
	}

	fn add_bank(&mut self, bank: usize, addr: u16, data: &[u8]) {
		if addr == 0x8000 {
			self.roml.store_bank(bank, data, 0);
		}
	}

	/* Some GMod2 CRT images carry the initial 2 KiB EEPROM image as a CHIP packet at $DE00. It is device state, not ROMH. */
	fn add_chip(&mut self, _chip_type: ChipType, bank: usize, addr: u16, data: &[u8]) {
		if addr == 0xDE00 && data.len() <= EEPROM_BYTES {
			let count = data.len();
			self.eeprom.storage[..count].copy_from_slice(data);
			return;
		}
		self.add_bank(bank, addr, data);
	}

	fn update_signals(&mut self, _cycle: u64, lines: &mut LineState) {
		lines.game = true;
		lines.exrom = self.eeprom_selected();
	}

	fn on_freeze(&mut self, _lines: &mut LineState) {}

	fn get_info(&self) -> CartridgeInfo {
		CartridgeInfo {
			name: "GMod2".to_string(),
			mapper_type: MapperType::GMod2,
			rom_size: self.roml.len() * 8192,
			bank_count: self.roml.len().max(1),
			has_ram: true,
		}
	}

	fn get_debug_bank(&self) -> usize {
		self.bank
	}

	fn get_max_bank(&self) -> usize {
		self.roml.len().saturating_sub(1)
	}

	fn load_nvram(&mut self, data: &[u8]) {
		let count = data.len().min(self.eeprom.storage.len());
		self.eeprom.storage[..count].copy_from_slice(&data[..count]);
	}

	fn save_nvram(&self) -> Option<Vec<u8>> {
		Some(self.eeprom.storage.to_vec())
	}
}