// =======================================================
// src/cartridge/mapper_easyflash.rs — Unified EasyFlash emulation
// =======================================================

use super::bank_storage::BankStorage;
use super::mapper_interface::{CartridgeInfo, CartridgeMapper, LineState, MapperType};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/* FlashState follows the command parser of an individual programmable ROM chip. Unlock cycles, program setup, erase setup and busy status are kept explicit because ordinary ROM reads can advance or report those operations. */
enum FlashState {
	/* No command prefix is pending; array reads return normal contents. */
	Idle,
	/* The first AA unlock write was accepted and the parser expects the 55 cycle. */
	Command1,
	/* Both unlock writes were accepted and the following command byte selects the operation. */
	Command2,
	/* The next array write programs one byte when flash writes are enabled. */
	WriteMode,
	/* Erase setup was accepted and a second unlock sequence must follow. */
	SectorErase1,
	/* The first erase-confirm unlock write was accepted. */
	SectorErase2,
	/* The second erase-confirm unlock write was accepted and the next command chooses chip or sector erase. */
	SectorErase3,
	/* Reads return manufacturer and device identification until reset by command F0. */
	AutoSelect,
}

/* FlashChip keeps programmed bytes separate from transient command and timing state. Reads during a pending operation return status signalling rather than the final array contents. */
struct FlashChip {
	state: FlashState,
	write_enabled: bool,
	operation_start_cycle: u64,
	is_operating: bool,
	toggle_bit: bool,
	polling_byte: u8,
}

impl FlashChip {
	fn new() -> Self {
		Self {
			state: FlashState::Idle,
			write_enabled: false,
			operation_start_cycle: 0,
			is_operating: false,
			toggle_bit: false,
			polling_byte: 0xFF,
		}
	}

	fn reset(&mut self) {
		self.state = FlashState::Idle;
		self.write_enabled = false;
		self.is_operating = false;
		self.toggle_bit = false;
	}

	fn enable_writes(&mut self) {
		self.write_enabled = true;
	}

	fn disable_writes(&mut self) {
		self.write_enabled = false;
	}

	fn update_status(&mut self, current_cycle: u64) {
		if self.is_operating && current_cycle >= self.operation_start_cycle + 500000 {
			self.is_operating = false;
			self.state = FlashState::Idle;
		}
	}

	/* While a program or erase operation is pending, reads expose toggle and polling bits instead of stable array data; normal contents return only after the simulated operation completes. */
	fn read_status(&mut self, current_cycle: u64, real_byte: u8) -> u8 {
		self.update_status(current_cycle);
		if !self.is_operating {
			return real_byte;
		}
		self.toggle_bit = !self.toggle_bit;
		let mut status = 0x00;
		if self.toggle_bit {
			status |= 0x40;
		}
		status |= self.polling_byte & 0x80;
		status |= real_byte & 0x3F;
		status
	}

	/* Flash commands are recognised as an ordered unlock sequence. Any unexpected address or value returns the parser to Idle so ordinary cartridge writes cannot accidentally program the array. */
	fn process_write(&mut self, addr: u16, value: u8, current_cycle: u64) -> Option<u8> {
		let offset = addr & 0x1FFF;
		if value == 0xF0 {
			self.state = FlashState::Idle;
			self.is_operating = false;
			return None;
		}

		match self.state {
			FlashState::Idle => {
				if offset == 0x1555 && value == 0xAA {
					self.state = FlashState::Command1;
				}
				None
			}
			FlashState::Command1 => {
				if offset == 0x0AAA && value == 0x55 {
					self.state = FlashState::Command2;
				} else {
					self.state = FlashState::Idle;
				}
				None
			}
			FlashState::Command2 => {
				if offset == 0x1555 {
					match value {
						0xA0 => {
							self.state = FlashState::WriteMode;
							None
						}
						0x90 => {
							self.state = FlashState::AutoSelect;
							None
						}
						0x80 => {
							self.state = FlashState::SectorErase1;
							None
						}
						_ => {
							self.state = FlashState::Idle;
							None
						}
					}
				} else {
					self.state = FlashState::Idle;
					None
				}
			}
			FlashState::WriteMode => {
				self.state = FlashState::Idle;
				if self.write_enabled {
					self.is_operating = true;
					self.operation_start_cycle = current_cycle;
					self.polling_byte = value;
					Some(value)
				} else {
					None
				}
			}
			FlashState::SectorErase1 => {
				if offset == 0x1555 && value == 0xAA {
					self.state = FlashState::SectorErase2;
				} else {
					self.state = FlashState::Idle;
				}
				None
			}
			FlashState::SectorErase2 => {
				if offset == 0x0AAA && value == 0x55 {
					self.state = FlashState::SectorErase3;
				} else {
					self.state = FlashState::Idle;
				}
				None
			}
			FlashState::SectorErase3 => {
				self.state = FlashState::Idle;
				if self.write_enabled && value == 0x30 {
					self.is_operating = true;
					self.operation_start_cycle = current_cycle;
					self.polling_byte = 0xFF;
					Some(0xFF)
				} else {
					None
				}
			}
			FlashState::AutoSelect => None,
		}
	}
}

/* EasyFlashMapper owns two independently addressed flash chips, cartridge RAM and control registers. Its flash state machines preserve command sequences, busy status and programmable contents across accesses. */
pub struct EasyFlashMapper {
	roml: BankStorage,
	romh: BankStorage,
	bank: usize,
	active_slot: usize,
	ram: Box<[u8; 256]>,
	flash_lo: FlashChip,
	flash_hi: FlashChip,
	register_enabled: bool,
	mapper_type: MapperType,
	is_ef3: bool,
	control_reg: u8,
	pending_bank: Option<usize>,
	pending_slot: Option<usize>,
	pending_control: Option<u8>,
	pending_disable: Option<bool>,
}

impl EasyFlashMapper {
	/* Construction selects the EasyFlash wiring variant while initialising both flash chips to read-array mode and releasing all command sequences. */
	pub fn new(mapper_type: MapperType) -> Self {
		Self {
			roml: BankStorage::new(),
			romh: BankStorage::new(),
			bank: 0,
			active_slot: 0,
			ram: Box::new([0x00; 256]),
			flash_lo: FlashChip::new(),
			flash_hi: FlashChip::new(),
			register_enabled: true,
			mapper_type,
			is_ef3: mapper_type == MapperType::EasyFlash3,
			control_reg: 0x02,
			pending_bank: None,
			pending_slot: None,
			pending_control: Option::None,
			pending_disable: Option::None,
		}
	}

	fn get_absolute_bank(&self) -> usize {
		if self.is_ef3 {
			(self.active_slot * 64) + self.bank
		} else {
			self.bank
		}
	}
}

impl CartridgeMapper for EasyFlashMapper {
	fn reset(&mut self) {
		self.bank = 0;
		self.active_slot = 0;
		self.register_enabled = true;
		self.flash_lo.reset();
		self.flash_hi.reset();
		self.ram.fill(0x00);
		self.control_reg = 0x02;
		self.pending_bank = None;
		self.pending_slot = None;
		self.pending_control = Option::None;
		self.pending_disable = Option::None;
	}

	fn read_roml(&mut self, offset: u16, cycle: u64) -> Option<u8> {
		if self.flash_lo.state == FlashState::AutoSelect {
			match offset & 0xFF {
				0x00 => return Some(0x01),
				0x01 => return Some(0xA4),
				_ => {}
			}
		}
		let b = self.get_absolute_bank();
		let real_byte = self
			.roml
			.get_bank(b)
			.map(|bank| bank[(offset & 0x1FFF) as usize])
			.unwrap_or(0xFF);
		Some(self.flash_lo.read_status(cycle, real_byte))
	}

	fn peek_roml(&self, offset: u16, _cycle: u64) -> Option<u8> {
		if self.flash_lo.state == FlashState::AutoSelect {
			return match offset & 0xFF {
				0x00 => Some(0x01),
				0x01 => Some(0xA4),
				_ => Some(0xFF),
			};
		}
		let bank = self.get_absolute_bank();
		Some(
			self.roml
				.get_bank(bank)
				.map(|data| data[(offset & 0x1FFF) as usize])
				.unwrap_or(0xFF),
		)
	}

	fn read_romh(&mut self, offset: u16, cycle: u64) -> Option<u8> {
		if self.flash_hi.state == FlashState::AutoSelect {
			match offset & 0xFF {
				0x00 => return Some(0x01),
				0x01 => return Some(0xA4),
				_ => {}
			}
		}
		let b = self.get_absolute_bank();
		let real_byte = self
			.romh
			.get_bank(b)
			.map(|bank| bank[(offset & 0x1FFF) as usize])
			.unwrap_or(0xFF);
		Some(self.flash_hi.read_status(cycle, real_byte))
	}

	fn peek_romh(&self, offset: u16, _cycle: u64) -> Option<u8> {
		if self.flash_hi.state == FlashState::AutoSelect {
			return match offset & 0xFF {
				0x00 => Some(0x01),
				0x01 => Some(0xA4),
				_ => Some(0xFF),
			};
		}
		let bank = self.get_absolute_bank();
		Some(
			self.romh
				.get_bank(bank)
				.map(|data| data[(offset & 0x1FFF) as usize])
				.unwrap_or(0xFF),
		)
	}

	fn peek_io(&self, addr: u16, _cycle: u64) -> Option<u8> {
		if (0xDF00..=0xDFFF).contains(&addr) {
			return Some(self.ram[(addr & 0xFF) as usize]);
		}
		if !self.register_enabled {
			return None;
		}
		match addr {
			0xDE00 => Some(self.bank as u8),
			0xDE01 if self.is_ef3 => Some(self.active_slot as u8),
			0xDE02 => Some(self.control_reg),
			_ => None,
		}
	}

	fn read_io(&mut self, addr: u16, _cycle: u64) -> Option<u8> {
		if addr >= 0xDF00 && addr <= 0xDFFF {
			return Some(self.ram[(addr & 0xFF) as usize]);
		}
		if !self.register_enabled {
			return None;
		}
		match addr {
			0xDE00 => Some(self.bank as u8),
			0xDE01 => {
				if self.is_ef3 {
					Some(self.active_slot as u8)
				} else {
					None
				}
			}
			0xDE02 => Some(self.control_reg),
			_ => None,
		}
	}

	/* EasyFlash separates bank selection at DE00 from cartridge mode and LED control at DE02. Both registers affect subsequent ROM-window resolution without rewriting flash contents. */
	fn write_io(&mut self, addr: u16, val: u8, _cycle: u64) {
		if addr >= 0xDF00 && addr <= 0xDFFF {
			self.ram[(addr & 0xFF) as usize] = val;
			return;
		}
		if !self.register_enabled {
			return;
		}
		match addr {
			0xDE00 => {
				self.pending_bank = Some((val & 0x3F) as usize);
			}
			0xDE01 => {
				if self.is_ef3 {
					self.pending_slot = Some((val & 0x07) as usize);
				}
			}
			0xDE02 => {
				self.pending_control = Some(val);
			}
			0xDE0F => {
				if self.is_ef3 && (val & 0x07) == 0x07 {
					self.pending_disable = Some(true);
				}
			}
			_ => {}
		}
	}

	/* Writes to ROML or ROMH are routed through the command parser of the selected flash chip. Array bytes change only when a completed command authorises programming or erase. */
	fn write_rom(&mut self, addr: u16, val: u8, cycle: u64) {
		let b = self.get_absolute_bank();
		let offset = (addr & 0x1FFF) as usize;
		if addr >= 0x8000 && addr <= 0x9FFF {
			if let Some(mask) = self.flash_lo.process_write(addr, val, cycle) {
				if let Some(bank) = self.roml.get_bank_mut(b) {
					if mask == 0xFF {
						bank[offset] = 0xFF;
					} else {
						bank[offset] &= mask;
					}
				}
			}
		} else if addr >= 0xA000 && addr <= 0xBFFF {
			if let Some(mask) = self.flash_hi.process_write(addr, val, cycle) {
				if let Some(bank) = self.romh.get_bank_mut(b) {
					if mask == 0xFF {
						bank[offset] = 0xFF;
					} else {
						bank[offset] &= mask;
					}
				}
			}
		}
	}

	fn add_bank(&mut self, bank: usize, addr: u16, data: &[u8]) {
		let offset = (addr & 0x1FFF) as usize;
		if addr < 0xA000 {
			self.roml.store_bank(bank, data, offset);
		} else {
			self.romh.store_bank(bank, data, offset);
		}
	}

	fn update_signals(&mut self, _cycle: u64, lines: &mut LineState) {
		if let Some(b) = self.pending_bank {
			self.bank = b;
			self.pending_bank = None;
		}
		if let Some(s) = self.pending_slot {
			self.active_slot = s;
			self.pending_slot = None;
		}
		if let Some(c) = self.pending_control {
			self.control_reg = c;
			let write_enable = (c & 0x10) != 0;
			if write_enable {
				self.flash_lo.enable_writes();
				self.flash_hi.enable_writes();
			} else {
				self.flash_lo.disable_writes();
				self.flash_hi.disable_writes();
			}
			lines.exrom = (c & 0x02) == 0;
			lines.game = (c & 0x01) == 0;
			self.pending_control = None;
		}
		if let Some(d) = self.pending_disable {
			self.register_enabled = !d;
			lines.game = true;
			lines.exrom = true;
			self.pending_disable = None;
		}
	}

	fn on_freeze(&mut self, _lines: &mut LineState) {}

	fn get_max_bank(&self) -> usize {
		self.roml.len().max(self.romh.len())
	}

	fn get_debug_bank(&self) -> usize {
		self.bank
	}

	fn load_nvram(&mut self, data: &[u8]) {
		let len = data.len().min(self.ram.len());
		self.ram[..len].copy_from_slice(&data[..len]);
	}

	fn save_nvram(&self) -> Option<Vec<u8>> {
		Some(self.ram.to_vec())
	}

	fn get_info(&self) -> CartridgeInfo {
		CartridgeInfo {
			name: if self.is_ef3 {
				"EasyFlash 3".to_string()
			} else {
				"EasyFlash".to_string()
			},
			mapper_type: self.mapper_type,
			rom_size: (self.roml.len() + self.romh.len()) * 8192,
			bank_count: self.get_max_bank(),
			has_ram: true,
		}
	}
}