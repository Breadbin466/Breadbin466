// =======================================================
// src/cartridge/mapper_retro_replay.rs — Retro Replay cartridge
// =======================================================

use super::constants::{
	RETRO_REPLAY_NVRAM_MAGIC, RETRO_REPLAY_RAM_BANK_SIZE, RETRO_REPLAY_RAM_SIZE,
};

use super::bank_storage::BankStorage;
use super::bus_configuration::IoRead;
use super::mapper_interface::{CartridgeInfo, CartridgeMapper, LineState, MapperType};

/* RetroReplayMapper combines banked ROM, RAM, flash-style writes and freezer modes. Several IO regions are only partially decoded, so bus-drive semantics are kept explicit. */
pub struct RetroReplayMapper {
	nordic: bool,
	rom: BankStorage,
	ram: Box<[u8; RETRO_REPLAY_RAM_SIZE]>,
	bank: usize,
	mode: u8,
	ram_selected: bool,
	ram_at_a000: bool,
	allow_bank: bool,
	no_freeze: bool,
	reu_mapping: bool,
	clockport_enabled: bool,
	write_once: bool,
	active: bool,
	frozen: bool,
	freeze_button_pressed: bool,
}

impl RetroReplayMapper {
	/* Retro Replay powers up as an active ROM cartridge; RAM, clock-port, REU mapping and freezer latches are enabled only by later register writes. */
	pub fn new() -> Self {
		Self {
			nordic: false,
			rom: BankStorage::new(),
			ram: Box::new([0x00; RETRO_REPLAY_RAM_SIZE]),
			bank: 0,
			mode: 0,
			ram_selected: false,
			ram_at_a000: false,
			allow_bank: false,
			no_freeze: false,
			reu_mapping: false,
			clockport_enabled: false,
			write_once: false,
			active: true,
			frozen: false,
			freeze_button_pressed: false,
		}
	}

	#[inline(always)]
	fn ram_bank(&self) -> usize {
		self.bank & 0x03
	}

	#[inline(always)]
	/* RAM banking is optional hardware policy: before the allow-bank latch is set, both I/O windows always address RAM bank zero regardless of the ROM bank register. */
	fn io_ram_bank(&self) -> usize {
		if self.allow_bank { self.ram_bank() } else { 0 }
	}

	/* Status mirrors the bank latch into bits 3-4 and 7, then reports banked RAM enable in bit 1, the physical freeze button in bit 2 and REU-style I/O mapping in bit 6. */
	#[inline(always)]
	fn status(&self) -> u8 {
		(((self.bank & 0x03) as u8) << 3)
			| (((self.bank & 0x04) as u8) << 5)
			| if self.allow_bank { 0x02 } else { 0x00 }
			| if self.freeze_button_pressed {
				0x04
			} else {
				0x00
			}
			| if self.reu_mapping { 0x40 } else { 0x00 }
	}

	#[inline(always)]
	fn read_rom_bank(&self, bank: usize, offset: usize) -> Option<u8> {
		let resolved = self.rom.resolve_bank(bank)?;
		self.rom
			.get_bank(resolved)
			.map(|data| data[offset & 0x1fff])
	}

	#[inline(always)]
	/* IO1 and IO2 expose the final 512 bytes of the selected 8 KiB ROM or RAM bank at offsets $1E00 and $1F00 respectively. */
	fn io_window_read(&self, addr: u16, io1: bool) -> Option<u8> {
		let low = (addr & 0xff) as usize;
		let base = if io1 { 0x1e00 } else { 0x1f00 };
		if self.ram_selected || self.ram_at_a000 {
			return Some(self.ram[self.io_ram_bank() * RETRO_REPLAY_RAM_BANK_SIZE + base + low]);
		}
		self.read_rom_bank(self.bank, base + low)
	}

	#[inline(always)]
	fn io_window_write(&mut self, addr: u16, value: u8, io1: bool) {
		if !(self.ram_selected || self.ram_at_a000) {
			return;
		}
		let low = (addr & 0xff) as usize;
		let base = if io1 { 0x1e00 } else { 0x1f00 };
		let index = self.io_ram_bank() * RETRO_REPLAY_RAM_BANK_SIZE + base + low;
		self.ram[index] = value;
	}
}

impl CartridgeMapper for RetroReplayMapper {
	fn set_hardware_revision(&mut self, revision: u8) {
		self.nordic = revision == 1;
	}
	fn reset(&mut self) {
		self.bank = 0;
		self.mode = 0;
		self.ram_selected = false;
		self.ram_at_a000 = false;
		self.active = true;
		self.frozen = false;
		self.freeze_button_pressed = false;
		self.clockport_enabled = false;
		self.allow_bank = false;
		self.no_freeze = false;
		self.reu_mapping = false;
		self.write_once = false;
	}

	/* Freeze re-enables the monitor bank, exposes cartridge RAM and latches NMI until firmware acknowledges the request, while preserving the registers needed for controlled exit. */
	fn on_freeze(&mut self, _lines: &mut LineState) {
		self.freeze_button_pressed = true;
		if !self.no_freeze {
			self.active = true;
			self.frozen = true;
			self.bank = 0;
			self.mode = 3;
		}
	}

	fn read_roml(&mut self, offset: u16, _cycle: u64) -> Option<u8> {
		self.peek_roml(offset, 0)
	}

	fn peek_roml(&self, offset: u16, _cycle: u64) -> Option<u8> {
		if !self.active || self.frozen {
			return None;
		}
		let index = (offset & 0x1fff) as usize;
		if self.ram_selected {
			return Some(self.ram[self.ram_bank() * RETRO_REPLAY_RAM_BANK_SIZE + index]);
		}
		self.read_rom_bank(self.bank, index)
	}

	fn read_romh(&mut self, offset: u16, _cycle: u64) -> Option<u8> {
		self.peek_romh(offset, 0)
	}

	fn peek_romh(&self, offset: u16, _cycle: u64) -> Option<u8> {
		if !self.active {
			return None;
		}
		let index = (offset & 0x1fff) as usize;
		if self.ram_at_a000 {
			return Some(self.ram[index]);
		}
		self.read_rom_bank(self.bank, index)
	}

	fn read_io(&mut self, addr: u16, _cycle: u64) -> Option<u8> {
		match self.read_io_bus(addr, 0) {
			IoRead::Driven(value) => Some(value),
			_ => None,
		}
	}

	/* Retro Replay combines control registers, freezer RAM, clock-port space and flash status in the same I/O pages; explicit drive masks preserve the data lines each source actually controls. */
	fn read_io_bus(&mut self, addr: u16, _cycle: u64) -> IoRead {
		if !self.active {
			return IoRead::NotDecoded;
		}
		match addr {
			0xde00 | 0xde01 => IoRead::Driven(self.status()),
			0xde02..=0xde0f if self.clockport_enabled => IoRead::OpenBus,
			0xde02..=0xdeff if self.reu_mapping => self
				.io_window_read(addr, true)
				.map_or(IoRead::NotDecoded, IoRead::Driven),
			0xdf00..=0xdfff if !self.reu_mapping => self
				.io_window_read(addr, false)
				.map_or(IoRead::NotDecoded, IoRead::Driven),
			_ => IoRead::NotDecoded,
		}
	}

	fn peek_io(&self, addr: u16, _cycle: u64) -> Option<u8> {
		if !self.active {
			return None;
		}
		match addr {
			0xde00 | 0xde01 => Some(self.status()),
			0xde02..=0xde0f if self.clockport_enabled => None,
			0xde02..=0xdeff if self.reu_mapping => self.io_window_read(addr, true),
			0xdf00..=0xdfff if !self.reu_mapping => self.io_window_read(addr, false),
			_ => None,
		}
	}

	/* $DE00 combines bank bits 3-4 and 7 with RAM select (bit 5), freeze acknowledge (bit 6), disable (bit 2) and mapping mode (bits 0-1). $DE01 latches one-time hardware options: banked RAM (bit 1), freeze inhibit (bit 2), REU-style I/O placement (bit 6), plus the live clock-port enable in bit 0. */
	fn write_io(&mut self, addr: u16, value: u8, _cycle: u64) {
		if !self.active {
			return;
		}
		match addr {
			0xde00 => {
				self.bank = ((value >> 3) & 0x03) as usize | (((value >> 7) & 0x01) as usize) << 2;
				self.ram_selected = (value & 0x20) != 0;
				/* Only Nordic Replay implements the additional RAM aperture;
				 * Retro Replay releases both ROM windows in mode $22
				 * (REPLAY-HARDWARE-MAPS). */
				self.ram_at_a000 = self.nordic && (value & 0x27) == 0x22;
				if self.ram_at_a000 {
					self.ram_selected = false;
				}
				if (value & 0x40) != 0 {
					self.frozen = false;
					self.freeze_button_pressed = false;
				}
				if self.frozen {
					self.mode = 3;
				} else if self.ram_at_a000 {
					self.mode = 1;
				} else {
					self.mode = value & 0x03;
				}
				if (value & 0x04) != 0 {
					self.active = false;
					self.mode = 2;
				}
			}
			0xde01 => {
				self.bank = ((value >> 3) & 0x03) as usize | (((value >> 7) & 0x01) as usize) << 2;
				if !self.write_once {
					self.allow_bank = (value & 0x02) != 0;
					self.no_freeze = (value & 0x04) != 0;
					self.reu_mapping = (value & 0x40) != 0;
					self.write_once = true;
				}
				self.clockport_enabled = (value & 0x01) != 0;
			}
			0xde02..=0xde0f if self.clockport_enabled => {}
			0xde02..=0xdeff if self.reu_mapping => self.io_window_write(addr, value, true),
			0xdf00..=0xdfff if !self.reu_mapping => self.io_window_write(addr, value, false),
			_ => {}
		}
	}

	/* Mapped ROM writes target RAM or flash according to the active control mode; ordinary ROM operation remains read-only. */
	fn write_rom(&mut self, addr: u16, value: u8, _cycle: u64) {
		if !self.active {
			return;
		}
		let offset = (addr & 0x1fff) as usize;
		/* Original Retro Replay SRAM is read-only in game modes;
		 * Nordic Replay enables writes there as well as in Ultimax
		 * (REPLAY-HARDWARE-MAPS). */
		if self.ram_selected
			&& (self.nordic || self.mode == 3)
			&& (0x8000..=0x9fff).contains(&addr)
		{
			self.ram[self.ram_bank() * RETRO_REPLAY_RAM_BANK_SIZE + offset] = value;
		} else if self.ram_at_a000 && (0xa000..=0xbfff).contains(&addr) {
			self.ram[offset] = value;
		}
	}

	fn add_bank(&mut self, bank: usize, _addr: u16, data: &[u8]) {
		if bank == 0 && data.len() == 0x10000 {
			for index in 0..8 {
				let start = index * 0x2000;
				self.rom.store_bank(index, &data[start..start + 0x2000], 0);
			}
		} else if bank < 16 {
			self.rom.store_bank(bank, data, 0);
		}
	}

	fn update_signals(&mut self, _cycle: u64, lines: &mut LineState) {
		if !self.active {
			lines.game = true;
			lines.exrom = true;
		} else if self.frozen {
			lines.game = false;
			lines.exrom = true;
		} else {
			match self.mode {
				0 => {
					lines.game = true;
					lines.exrom = false;
				}
				1 => {
					lines.game = false;
					lines.exrom = false;
				}
				2 => {
					lines.game = true;
					lines.exrom = true;
				}
				_ => {
					lines.game = false;
					lines.exrom = true;
				}
			}
		}
	}

	fn freeze_keeps_nmi(&self) -> bool {
		false
	}

	fn add_chip(
		&mut self,
		_chip_type: super::mapper_interface::ChipType,
		bank: usize,
		addr: u16,
		data: &[u8],
	) {
		self.add_bank(bank, addr, data);
	}

	fn load_nvram(&mut self, data: &[u8]) {
		if data.len() == RETRO_REPLAY_RAM_SIZE {
			self.ram.copy_from_slice(data);
		} else if data.len() == RETRO_REPLAY_NVRAM_MAGIC.len() + RETRO_REPLAY_RAM_SIZE
			&& &data[..4] == RETRO_REPLAY_NVRAM_MAGIC
		{
			self.ram.copy_from_slice(&data[4..]);
		}
	}

	fn save_nvram(&self) -> Option<Vec<u8>> {
		let mut data = Vec::with_capacity(RETRO_REPLAY_NVRAM_MAGIC.len() + RETRO_REPLAY_RAM_SIZE);
		data.extend_from_slice(RETRO_REPLAY_NVRAM_MAGIC);
		data.extend_from_slice(self.ram.as_ref());
		Some(data)
	}

	fn get_max_bank(&self) -> usize {
		self.rom.populated_len()
	}
	fn get_debug_bank(&self) -> usize {
		self.bank
	}

	fn get_info(&self) -> CartridgeInfo {
		CartridgeInfo {
			name: if self.nordic {
				"Nordic Replay"
			} else {
				"Retro Replay"
			}
			.to_string(),
			mapper_type: MapperType::RetroReplay,
			rom_size: self.rom.populated_len() * 8192,
			bank_count: self.rom.populated_len(),
			has_ram: true,
		}
	}
}