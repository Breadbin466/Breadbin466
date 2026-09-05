// =======================================================
// src/cartridge/mapper_final_cartridge_3.rs — Final Cartridge III
// =======================================================

use super::bank_storage::BankStorage;
use super::bus_configuration::CartridgeMode;
use super::crt_layout::add_bank_split;
use super::mapper_interface::{CartridgeInfo, CartridgeMapper, LineState, MapperType};

/* FinalCartridge3Mapper models banked ROM, control-register line outputs and delayed freeze behaviour. Its mapping can differ by clock phase, matching hardware that decodes cartridge signals around PHI2. */
pub struct FinalCartridge3Mapper {
	roml: BankStorage,
	romh: BankStorage,
	bank: usize,
	mode: CartridgeMode,
	register_enabled: bool,
	register_value: u8,
	freeze_phase: bool,
	nmi_low: bool,
}

impl FinalCartridge3Mapper {
	/* The FCIII powers up enabled in its boot bank, with the freezer NMI and special monitor mapping inactive. */
	pub fn new() -> Self {
		Self {
			roml: BankStorage::new(),
			romh: BankStorage::new(),
			bank: 0,
			mode: CartridgeMode::Game16K,
			register_enabled: true,
			register_value: 0,
			freeze_phase: false,
			nmi_low: false,
		}
	}

	fn apply_mode_lines(&self, lines: &mut LineState) {
		let (game, exrom) = self.mode.lines();
		lines.game = game;
		lines.exrom = exrom;
		lines.nmi_low = self.nmi_low;
	}

	fn decode_mode(value: u8) -> CartridgeMode {
		let encoded = ((value >> 3) & 2) | (((value >> 5) & 1) ^ 1);
		match encoded {
			0 => CartridgeMode::Game8K,
			1 => CartridgeMode::Game16K,
			2 => CartridgeMode::Ram,
			_ => CartridgeMode::Ultimax,
		}
	}
}

impl CartridgeMapper for FinalCartridge3Mapper {
	fn reset(&mut self) {
		self.bank = 0;
		self.mode = CartridgeMode::Game16K;
		self.register_enabled = true;
		self.register_value = 0;
		self.freeze_phase = false;
		self.nmi_low = false;
	}

	/* Freeze enters the monitor mapping, asserts NMI and records that the line must remain active until the cartridge control logic releases it. */
	fn on_freeze(&mut self, lines: &mut LineState) {
		self.on_freeze_at(0, lines);
	}

	/* Freeze immediately restores the monitor mapping and asserts NMI; later register activity decides when the level-held request is released. */
	fn on_freeze_at(&mut self, _cycle: u64, lines: &mut LineState) {
		self.register_enabled = true;
		self.freeze_phase = true;
		self.nmi_low = true;
		lines.nmi_low = true;
	}

	fn freeze_keeps_nmi(&self) -> bool {
		true
	}

	fn phase_modes(&self, lines: LineState) -> (CartridgeMode, CartridgeMode) {
		if self.freeze_phase {
			(CartridgeMode::Ram, CartridgeMode::Ultimax)
		} else {
			let mode = CartridgeMode::from_lines(lines.game, lines.exrom);
			(mode, mode)
		}
	}

	/* Reset publishes the cartridge startup lines independently of mutable register state so the motherboard sees a coherent map before the first CPU cycle. */
	fn reset_lines(&self, lines: &mut LineState) {
		self.apply_mode_lines(lines);
	}

	fn read_roml(&mut self, offset: u16, _cycle: u64) -> Option<u8> {
		self.roml
			.get_resolved(self.bank)
			.map(|data| data[(offset & 0x1FFF) as usize])
	}
	fn peek_roml(&self, offset: u16, _cycle: u64) -> Option<u8> {
		self.roml
			.get_resolved(self.bank)
			.map(|data| data[(offset & 0x1FFF) as usize])
	}

	fn read_romh(&mut self, offset: u16, _cycle: u64) -> Option<u8> {
		self.romh
			.get_resolved(self.bank)
			.map(|data| data[(offset & 0x1FFF) as usize])
	}
	fn peek_romh(&self, offset: u16, _cycle: u64) -> Option<u8> {
		self.romh
			.get_resolved(self.bank)
			.map(|data| data[(offset & 0x1FFF) as usize])
	}

	fn read_io(&mut self, addr: u16, _cycle: u64) -> Option<u8> {
		match addr {
			0xDE00..=0xDEFF => self
				.roml
				.get_resolved(self.bank)
				.map(|data| data[0x1E00 + (addr & 0xFF) as usize]),
			0xDF00..=0xDFFF => self
				.roml
				.get_resolved(self.bank)
				.map(|data| data[0x1F00 + (addr & 0xFF) as usize]),
			_ => None,
		}
	}

	/* The FC3 control register couples bank selection, ROM mode, freeze acknowledgement and cartridge disable; the next cycle observes all changes together. */
	fn write_io(&mut self, addr: u16, value: u8, _cycle: u64) {
		if !(0xDF00..=0xDFFF).contains(&addr) {
			return;
		}
		self.register_value = value;
		if addr != 0xDFFF || !self.register_enabled {
			return;
		}
		let bank_count = self.roml.len().max(1);
		self.bank = (value as usize & (bank_count - 1)).min(bank_count - 1);
		self.mode = Self::decode_mode(value);
		self.register_enabled = (value & 0x80) == 0;
		self.freeze_phase = false;
		self.nmi_low = (value & 0x40) == 0;
	}

	fn write_rom(&mut self, _addr: u16, _value: u8, _cycle: u64) {}

	/* Normal mapping and freeze mapping use different GAME/EXROM combinations, while the NMI line remains independent until firmware acknowledgement. */
	fn update_signals(&mut self, _cycle: u64, lines: &mut LineState) {
		self.apply_mode_lines(lines);
	}

	fn add_bank(&mut self, bank: usize, addr: u16, data: &[u8]) {
		add_bank_split(&mut self.roml, &mut self.romh, bank, addr, data);
	}

	fn get_max_bank(&self) -> usize {
		self.roml.len().saturating_sub(1)
	}

	fn get_debug_bank(&self) -> usize {
		self.bank
	}

	fn get_info(&self) -> CartridgeInfo {
		CartridgeInfo {
			name: "Final Cartridge III".to_string(),
			mapper_type: MapperType::FinalCartridge3,
			rom_size: (self.roml.len() + self.romh.len()) * 8192,
			bank_count: self.roml.len(),
			has_ram: false,
		}
	}
}