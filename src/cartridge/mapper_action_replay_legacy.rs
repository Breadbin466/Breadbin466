// =======================================================
// src/cartridge/mapper_action_replay_legacy.rs — Early Action Replay hardware
// =======================================================

use super::constants::{ACTION_REPLAY_2_DISABLE_THRESHOLD, ACTION_REPLAY_2_ENABLE_THRESHOLD};

use super::mapper_interface::{CartridgeInfo, CartridgeMapper, LineState, MapperType};use super::bus_configuration::{CartridgeMode, IoRead};
use super::bank_storage::BankStorage;

/* The early Action Replay revisions share a mapper body but differ in bank selection, I/O decode and the capacitor-like enable/disable behaviour of revision II. */
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyActionReplayKind {
	/* Revision II uses access-driven charge and discharge thresholds to leave and re-enter the cartridge map. */
	ActionReplay2,
	/* Revision III selects its bank from the low control bit and has no revision-II charge model. */
	ActionReplay3,
	/* Revision IV extends the control register with an additional bank-select bit. */
	ActionReplay4,
}

/* LegacyActionReplayMapper keeps the selected generation, ROM image, freeze state and generation-specific charge behaviour in one device while exposing the common mapper contract. */
pub struct LegacyActionReplayMapper {
	kind: LegacyActionReplayKind,
	rom: BankStorage,
	control: u8,
	bank: usize,
	active: bool,
	freeze_mode: bool,
	cap_enable: u16,
	cap_disable: u16,
}

impl LegacyActionReplayMapper {
	/* Construction records the exact hardware revision while keeping the common power-on bank and active mapping shared. */
	pub fn new(kind: LegacyActionReplayKind) -> Self {
		let bank = match kind {
			LegacyActionReplayKind::ActionReplay2 | LegacyActionReplayKind::ActionReplay3 | LegacyActionReplayKind::ActionReplay4 => 1,
		};
		Self {
			kind,
			rom: BankStorage::new(),
			control: 0,
			bank,
			active: true,
			freeze_mode: false,
			cap_enable: 0,
			cap_disable: 0,
		}
	}

	fn mapper_type(&self) -> MapperType {
		match self.kind {
			LegacyActionReplayKind::ActionReplay2 => MapperType::ActionReplay2,
			LegacyActionReplayKind::ActionReplay3 => MapperType::ActionReplay3,
			LegacyActionReplayKind::ActionReplay4 => MapperType::ActionReplay4,
		}
	}

	fn selected_bank(&self) -> usize {
		match self.kind {
			LegacyActionReplayKind::ActionReplay2 => self.bank & 1,
			LegacyActionReplayKind::ActionReplay3 => (self.control & 1) as usize,
			LegacyActionReplayKind::ActionReplay4 => ((self.control & 1) | ((self.control >> 3) & 2)) as usize,
		}
	}

	fn rom_byte(&self, offset: u16) -> Option<u8> {
		self.rom.get_bank(self.selected_bank()).map(|data| data[(offset & 0x1fff) as usize])
	}

	fn ar2_charge(&mut self) {
		self.cap_disable = self.cap_disable.saturating_add(1);
		if self.cap_disable == ACTION_REPLAY_2_DISABLE_THRESHOLD {
			self.active = false;
			self.cap_enable = 0;
		}
	}

	fn ar2_discharge(&mut self) {
		self.cap_enable = self.cap_enable.saturating_add(1);
		if self.cap_enable == ACTION_REPLAY_2_ENABLE_THRESHOLD {
			self.bank = 1;
			self.active = true;
		}
		self.cap_disable = 0;
	}
}

impl CartridgeMapper for LegacyActionReplayMapper {
	fn reset(&mut self) {
		self.control = 0;
		self.bank = 1;
		self.active = true;
		self.freeze_mode = false;
		self.cap_enable = 0;
		self.cap_disable = 0;
	}

	fn read_roml(&mut self, offset: u16, _cycle: u64) -> Option<u8> {
		if !self.active {
			return None;
		}
		self.rom_byte(offset)
	}

	fn read_romh(&mut self, offset: u16, _cycle: u64) -> Option<u8> {
		if !self.active {
			return None;
		}
		self.rom_byte(offset)
	}

	fn peek_roml(&self, offset: u16, _cycle: u64) -> Option<u8> {
		if !self.active {
			return None;
		}
		self.rom_byte(offset)
	}

	fn peek_romh(&self, offset: u16, _cycle: u64) -> Option<u8> {
		if !self.active {
			return None;
		}
		self.rom_byte(offset)
	}

	fn read_io(&mut self, addr: u16, _cycle: u64) -> Option<u8> {
		match self.kind {
			LegacyActionReplayKind::ActionReplay2 => match addr {
				0xde00..=0xdeff => {
					self.ar2_discharge();
					Some(0)
				}
				0xdf00..=0xdfff => {
					self.ar2_charge();
					self.rom.get_bank(1).map(|data| data[0x1f00 + (addr & 0xff) as usize])
				}
				_ => None,
			},
			LegacyActionReplayKind::ActionReplay3 | LegacyActionReplayKind::ActionReplay4 => {
				if !self.active || !(0xdf00..=0xdfff).contains(&addr) {
					return None;
				}
				self.rom_byte(0x1f00 | (addr & 0xff))
			}
		}
	}

	/* The I/O read result carries an explicit drive mask because some legacy revisions expose only part of the data bus while the remaining bits retain the motherboard latch. */
	fn read_io_bus(&mut self, addr: u16, cycle: u64) -> IoRead {
		match self.kind {
			LegacyActionReplayKind::ActionReplay3 | LegacyActionReplayKind::ActionReplay4 if (0xde00..=0xdeff).contains(&addr) => IoRead::OpenBus,
			_ => self.read_io(addr, cycle).map_or(IoRead::NotDecoded, IoRead::Driven),
		}
	}

	fn peek_io(&self, addr: u16, _cycle: u64) -> Option<u8> {
		match self.kind {
			LegacyActionReplayKind::ActionReplay2 => match addr {
				0xde00..=0xdeff => Some(0),
				0xdf00..=0xdfff => self.rom.get_bank(1).map(|data| data[0x1f00 + (addr & 0xff) as usize]),
				_ => None,
			},
			LegacyActionReplayKind::ActionReplay3 => match addr {
				0xde00..=0xdeff => Some(self.control),
				0xdf00..=0xdfff if self.active => self.rom_byte(0x1f00 | (addr & 0xff)),
				_ => None,
			},
			LegacyActionReplayKind::ActionReplay4 => {
				if self.active && (0xdf00..=0xdfff).contains(&addr) {
					self.rom_byte(0x1f00 | (addr & 0xff))
				} else {
					None
				}
			}
		}
	}

	/* Control-register writes combine bank selection, RAM visibility, freeze release and cartridge disable. Keeping those effects together preserves the hardware ordering seen by the following bus cycle. */
	fn write_io(&mut self, addr: u16, value: u8, _cycle: u64) {
		match self.kind {
			LegacyActionReplayKind::ActionReplay2 => match addr {
				0xde00..=0xdeff => self.ar2_discharge(),
				0xdf00..=0xdfff => self.ar2_charge(),
				_ => {}
			},
			LegacyActionReplayKind::ActionReplay3 => {
				if (0xde00..=0xdeff).contains(&addr) && self.active {
					self.control = value;
					self.freeze_mode = false;
					if value & 0x04 != 0 {
						self.active = false;
					}
				}
			},
			LegacyActionReplayKind::ActionReplay4 => {
				if (0xde00..=0xdeff).contains(&addr) && self.active {
					self.control = value;
					self.freeze_mode = false;
					if value & 0x04 != 0 {
						self.active = false;
					}
				}
			},
		}
	}

	fn write_rom(&mut self, _addr: u16, _value: u8, _cycle: u64) {}

	fn add_bank(&mut self, bank: usize, _addr: u16, data: &[u8]) {
		if bank == 0 && (data.len() == 0x4000 || data.len() == 0x8000) {
			for (index, chunk) in data.chunks(0x2000).enumerate() {
				self.rom.store_bank(index, chunk, 0);
			}
		} else {
			self.rom.store_bank(bank, data, 0);
		}
	}

	fn update_signals(&mut self, _cycle: u64, lines: &mut LineState) {
		if !self.active {
			lines.game = true;
			lines.exrom = true;
			return;
		}
		if self.freeze_mode {
			lines.game = false;
			lines.exrom = true;
			return;
		}
		match self.kind {
			LegacyActionReplayKind::ActionReplay2 => {
				lines.game = true;
				lines.exrom = false;
			}
			LegacyActionReplayKind::ActionReplay3 => {
				lines.game = true;
				lines.exrom = self.control & 0x08 == 0;
			}
			LegacyActionReplayKind::ActionReplay4 => {
				lines.game = self.control & 0x02 != 0;
				lines.exrom = self.control & 0x08 == 0;
			}
		}
	}

	fn phase_modes(&self, lines: LineState) -> (CartridgeMode, CartridgeMode) {
		let mode = CartridgeMode::from_lines(lines.game, lines.exrom);
		(mode, mode)
	}

	fn on_freeze(&mut self, _lines: &mut LineState) {
		self.active = true;
		self.freeze_mode = true;
		self.cap_enable = 0;
		self.cap_disable = 0;
		if self.kind == LegacyActionReplayKind::ActionReplay2 {
			self.bank = 0;
		}
	}

	fn get_info(&self) -> CartridgeInfo {
		CartridgeInfo {
			name: self.mapper_type().display_name().to_string(),
			mapper_type: self.mapper_type(),
			rom_size: self.rom.len() * 8192,
			bank_count: self.rom.len(),
			has_ram: false,
		}
	}

	fn get_debug_bank(&self) -> usize {
		self.selected_bank()
	}

	fn get_max_bank(&self) -> usize {
		self.rom.len()
	}
}