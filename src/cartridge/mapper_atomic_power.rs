// =======================================================
// src/cartridge/mapper_atomic_power.rs — Atomic Power (Nordic Power) mapper
// =======================================================

use super::bank_storage::BankStorage;
use super::bus_configuration::{CartridgeMode, IoRead};
use super::mapper_interface::{CartridgeInfo, CartridgeMapper, LineState, MapperType};

/* AtomicPowerMapper is a freezer cartridge with banked ROM, RAM overlays, mode bits and phase-dependent mapping. IO reads may expose open or partially driven bus values as well as stored data. */
pub struct AtomicPowerMapper {
	roml: BankStorage,
	romh: BankStorage,
	ram: Box<[u8; 8192]>,
	control: u8,
	bank: usize,
	active: bool,
	export_ram: bool,
	export_ram_at_a000: bool,
	phi1: CartridgeMode,
	phi2: CartridgeMode,
}

impl AtomicPowerMapper {
	/* Atomic Power starts in its boot mapping with ROM selected; later control writes may substitute RAM or disconnect the cartridge. */
	pub fn new() -> Self {
		Self {
			roml: BankStorage::new(),
			romh: BankStorage::new(),
			ram: Box::new([0x00; 8192]),
			control: 0,
			bank: 0,
			active: true,
			export_ram: false,
			export_ram_at_a000: false,
			phi1: CartridgeMode::Game8K,
			phi2: CartridgeMode::Game8K,
		}
	}

	fn mode_from_bits(bits: u8) -> CartridgeMode {
		match bits & 0x03 {
			0 => CartridgeMode::Game8K,
			1 => CartridgeMode::Game16K,
			2 => CartridgeMode::Ram,
			_ => CartridgeMode::Ultimax,
		}
	}

	fn io2_byte(&self, addr: u16) -> Option<u8> {
		if !self.active {
			return None;
		}
		let offset = 0x1F00 + (addr & 0xFF) as usize;
		if self.export_ram || self.export_ram_at_a000 {
			return Some(self.ram[offset]);
		}
		self.roml.get_bank(self.bank).map(|data| data[offset])
	}
}

impl CartridgeMapper for AtomicPowerMapper {
	fn exclusive_ram_window(&self) -> Option<(u16, u16)> {
		self.export_ram_at_a000.then_some((0xA000, 0xBFFF))
	}

	fn reset(&mut self) {
		self.control = 0;
		self.bank = 0;
		self.active = true;
		self.export_ram = false;
		self.export_ram_at_a000 = false;
		self.phi1 = CartridgeMode::Game8K;
		self.phi2 = CartridgeMode::Game8K;
	}

	/* Freeze re-enables the mapper, selects the monitor mapping and keeps NMI active until firmware acknowledges entry through the control register. */
	fn on_freeze(&mut self, _lines: &mut LineState) {
		self.active = true;
		self.export_ram = true;
		self.phi1 = CartridgeMode::Ultimax;
		self.phi2 = CartridgeMode::Ultimax;
	}

	fn read_roml(&mut self, offset: u16, _cycle: u64) -> Option<u8> {
		self.peek_roml(offset, 0)
	}

	fn peek_roml(&self, offset: u16, _cycle: u64) -> Option<u8> {
		if self.export_ram {
			return Some(self.ram[(offset & 0x1FFF) as usize]);
		}
		self.roml
			.get_bank(self.bank)
			.map(|data| data[(offset & 0x1FFF) as usize])
	}

	fn read_romh(&mut self, offset: u16, _cycle: u64) -> Option<u8> {
		self.peek_romh(offset, 0)
	}

	fn peek_romh(&self, offset: u16, _cycle: u64) -> Option<u8> {
		if self.export_ram_at_a000 {
			return Some(self.ram[(offset & 0x1FFF) as usize]);
		}
		self.romh
			.get_bank(self.bank)
			.map(|data| data[(offset & 0x1FFF) as usize])
	}

	fn read_io(&mut self, addr: u16, _cycle: u64) -> Option<u8> {
		if (0xDF00..=0xDFFF).contains(&addr) {
			return self.io2_byte(addr);
		}
		None
	}

	/* Atomic Power RAM and control windows do not always drive all eight data lines, so the mapper returns both value and mask instead of fabricating a complete byte. */
	fn read_io_bus(&mut self, addr: u16, _cycle: u64) -> IoRead {
		if (0xDE00..=0xDEFF).contains(&addr) {
			return IoRead::OpenBus;
		}
		if (0xDF00..=0xDFFF).contains(&addr) {
			return self.io2_byte(addr).map_or(IoRead::OpenBus, IoRead::Driven);
		}
		IoRead::NotDecoded
	}

	fn peek_io(&self, addr: u16, _cycle: u64) -> Option<u8> {
		if (0xDF00..=0xDFFF).contains(&addr) {
			return self.io2_byte(addr);
		}
		None
	}

	/* The control register simultaneously selects the ROM bank, chooses RAM overlay and determines the GAME/EXROM mode used by the next PLA evaluation. */
	fn write_io(&mut self, addr: u16, value: u8, _cycle: u64) {
		if (0xDE00..=0xDEFF).contains(&addr) {
			if !self.active {
				return;
			}
			self.control = value;
			self.bank = ((value >> 3) & 0x03) as usize;
			let mode = if (value & 0xE7) == 0x22 {
				self.export_ram_at_a000 = true;
				self.export_ram = false;
				CartridgeMode::Game16K
			} else {
				self.export_ram_at_a000 = false;
				self.export_ram = (value & 0x20) != 0;
				Self::mode_from_bits(value)
			};
			if (value & 0x04) != 0 {
				self.active = false;
			}
			self.phi1 = CartridgeMode::Ram;
			self.phi2 = mode;
		} else if (0xDF00..=0xDFFF).contains(&addr)
			&& self.active
			&& (self.export_ram || self.export_ram_at_a000)
		{
			self.ram[0x1F00 + (addr & 0xFF) as usize] = value;
		}
	}

	fn write_rom(&mut self, addr: u16, value: u8, _cycle: u64) {
		if self.export_ram && (0x8000..=0x9FFF).contains(&addr) {
			self.ram[(addr & 0x1FFF) as usize] = value;
		} else if self.export_ram_at_a000 && (0xA000..=0xBFFF).contains(&addr) {
			self.ram[(addr & 0x1FFF) as usize] = value;
		}
	}

	/* GAME and EXROM are decoded from the control latch only while the cartridge is enabled; disabling releases the expansion port completely. */
	fn update_signals(&mut self, _cycle: u64, lines: &mut LineState) {
		let (game, exrom) = self.phi2.lines();
		lines.game = game;
		lines.exrom = exrom;
	}

	fn phase_modes(&self, lines: LineState) -> (CartridgeMode, CartridgeMode) {
		(
			self.phi1,
			CartridgeMode::from_lines(lines.game, lines.exrom),
		)
	}

	fn add_bank(&mut self, bank: usize, _addr: u16, data: &[u8]) {
		if bank == 0 && data.len() == 0x8000 {
			for index in 0..4 {
				let chunk = &data[index * 0x2000..(index + 1) * 0x2000];
				self.roml.store_bank(index, chunk, 0);
				self.romh.store_bank(index, chunk, 0);
			}
		} else if bank < 4 {
			self.roml.store_bank(bank, data, 0);
			self.romh.store_bank(bank, data, 0);
		}
	}

	fn get_max_bank(&self) -> usize {
		self.roml.len()
	}

	fn get_debug_bank(&self) -> usize {
		self.bank
	}

	fn load_nvram(&mut self, data: &[u8]) {
		let count = data.len().min(self.ram.len());
		self.ram[..count].copy_from_slice(&data[..count]);
	}

	fn save_nvram(&self) -> Option<Vec<u8>> {
		Some(self.ram.to_vec())
	}

	fn get_info(&self) -> CartridgeInfo {
		CartridgeInfo {
			name: "Atomic Power".to_string(),
			mapper_type: MapperType::AtomicPower,
			rom_size: (self.roml.len() + self.romh.len()) * 8192,
			bank_count: self.roml.len(),
			has_ram: true,
		}
	}
}