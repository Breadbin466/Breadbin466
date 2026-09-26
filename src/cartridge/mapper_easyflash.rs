// =======================================================
// src/cartridge/mapper_easyflash.rs — Unified EasyFlash emulation
// =======================================================

use super::bank_storage::BankStorage;
use super::mapper_interface::{CartridgeInfo, CartridgeMapper, LineState, MapperType};

use super::flash_chip::{FlashChip, FlashState, FlashWrite};
use super::constants::{FLASH_BANKS_PER_SECTOR, FLASH_BANKS_PER_CHIP};

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
		self.control_reg = 0;
		self.pending_bank = None;
		self.pending_slot = None;
		self.pending_control = Some(0);
		self.pending_disable = Option::None;
	}

	fn read_roml(&mut self, offset: u16, cycle: u64) -> Option<u8> {
		if self.flash_lo.state == FlashState::AutoSelect {
			match offset & 0xFF {
				0x00 => return Some(0x01),
				0x01 => return Some(0xA4),
				0x02 => return Some(0x00),
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
				0x02 => Some(0x00),
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
				0x02 => return Some(0x00),
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
				0x02 => Some(0x00),
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
	fn write_rom(&mut self, addr: u16, value: u8, cycle: u64) {
		let bank = self.get_absolute_bank();
		let (chip, storage) = match addr {
			0x8000..=0x9FFF => (&mut self.flash_lo, &mut self.roml),
			0xA000..=0xBFFF | 0xE000..=0xFFFF => (&mut self.flash_hi, &mut self.romh),
			_ => return,
		};
		match chip.process_write(addr, value, cycle) {
			Some(FlashWrite::Program(data)) => {
				let offset = (addr & 0x1FFF) as usize;
				let previous = storage.get_bank(bank).map_or(0xFF, |bytes| bytes[offset]);
				storage.store_bank(bank, &[previous & data], offset);
			}
			Some(FlashWrite::EraseSector) => {
				let first = bank / FLASH_BANKS_PER_SECTOR * FLASH_BANKS_PER_SECTOR;
				for index in first..first + FLASH_BANKS_PER_SECTOR {
					if let Some(bytes) = storage.get_bank_mut(index) { bytes.fill(0xFF); }
				}
			}
			Some(FlashWrite::EraseChip) => {
				let first = bank / FLASH_BANKS_PER_CHIP * FLASH_BANKS_PER_CHIP;
				for index in first..first + FLASH_BANKS_PER_CHIP {
					if let Some(bytes) = storage.get_bank_mut(index) { bytes.fill(0xFF); }
				}
			}
			None => {}
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

			lines.exrom = (c & 0x02) == 0;
			/* With M clear, the boot jumper holds GAME low. */
			lines.game = (c & 0x04) != 0 && (c & 0x01) == 0;
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