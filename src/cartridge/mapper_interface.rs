// =======================================================
// src/cartridge/mapper_interface.rs — Cartridge mapper contract
// =======================================================

use std::fmt;

use super::bus_configuration::{CartridgeMode, IoRead};

/* GAME and EXROM select the cartridge memory map, while NMI is an independent active-low output used by freeze cartridges. Keeping the three pins together lets a mapper publish one coherent electrical state after each bus access. */
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineState {
	pub game: bool,
	pub exrom: bool,
	pub nmi_low: bool,
}

/* CRT chip packets distinguish immutable ROM, writable RAM and flash storage even though simple mappers may load all three through the same bank-copy path. */
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChipType {
	Rom,
	Ram,
	Flash,
}

impl TryFrom<u16> for ChipType {
	type Error = ();

	fn try_from(value: u16) -> Result<Self, Self::Error> {
		match value {
			0 => Ok(Self::Rom),
			1 => Ok(Self::Ram),
			2 => Ok(Self::Flash),
			_ => Err(()),
		}
	}
}

/* MapperType is the stable identity shared by CRT loading, mapper construction, UI reporting and persistence. Separate Action Replay generations remain distinct because their registers and freeze timing are not interchangeable. */
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MapperType {
	Normal,
	ActionReplay,
	ActionReplay2,
	ActionReplay3,
	ActionReplay4,
	KCS,
	FinalCartridge3,
	SimonsBasic,
	Ocean,
	FunPlay,
	SuperGames,
	AtomicPower,
	EpyxFastLoad,
	C64GS,
	Dinamic,
	Zaxxon,
	MagicDesk,
	SuperSnapshot5,
	StructuredBasic,
	EasyFlash,
	EasyFlashXbank,
	EasyFlash3,
	RetroReplay,
	RGCD,
	GMod2,
	Pagefox,
}

impl MapperType {
	/* CRT type identifiers are format-level numbers, not internal enum ordinals. Keeping the translation explicit prevents later enum reordering from changing file compatibility. */
	pub fn from_crt_id(id: u16) -> Option<Self> {
		match id {
			0 => Some(Self::Normal),
			1 => Some(Self::ActionReplay),
			30 => Some(Self::ActionReplay4),
			35 => Some(Self::ActionReplay3),
			50 => Some(Self::ActionReplay2),
			2 => Some(Self::KCS),
			3 => Some(Self::FinalCartridge3),
			4 => Some(Self::SimonsBasic),
			5 => Some(Self::Ocean),
			7 => Some(Self::FunPlay),
			8 => Some(Self::SuperGames),
			9 => Some(Self::AtomicPower),
			10 => Some(Self::EpyxFastLoad),
			15 => Some(Self::C64GS),
			17 => Some(Self::Dinamic),
			18 => Some(Self::Zaxxon),
			19 => Some(Self::MagicDesk),
			20 => Some(Self::SuperSnapshot5),
			22 => Some(Self::StructuredBasic),
			32 => Some(Self::EasyFlash),
			33 => Some(Self::EasyFlashXbank),
			36 => Some(Self::RetroReplay),
			57 => Some(Self::RGCD),
			53 => Some(Self::Pagefox),
			60 => Some(Self::GMod2),
			_ => None,
		}
	}

	/* Serialisation uses the original CRT identifier even when several internal variants share related hardware ancestry. */
	pub fn crt_id(self) -> u16 {
		match self {
			Self::Normal => 0,
			Self::ActionReplay => 1,
			Self::ActionReplay2 => 50,
			Self::ActionReplay3 => 35,
			Self::ActionReplay4 => 30,
			Self::KCS => 2,
			Self::FinalCartridge3 => 3,
			Self::SimonsBasic => 4,
			Self::Ocean => 5,
			Self::FunPlay => 7,
			Self::SuperGames => 8,
			Self::AtomicPower => 9,
			Self::EpyxFastLoad => 10,
			Self::C64GS => 15,
			Self::Dinamic => 17,
			Self::Zaxxon => 18,
			Self::MagicDesk => 19,
			Self::SuperSnapshot5 => 20,
			Self::StructuredBasic => 22,
			Self::EasyFlash => 32,
			Self::EasyFlashXbank => 33,
			Self::EasyFlash3 => 33,
			Self::RetroReplay => 36,
			Self::RGCD => 57,
			Self::GMod2 => 60,
			Self::Pagefox => 53,
		}
	}

	/* Display names are stable user-facing identities and intentionally remain separate from filenames, CRT headers and Rust variant spelling. */
	pub fn display_name(self) -> &'static str {
		match self {
			Self::Normal => "Normal cartridge",
			Self::ActionReplay => "Action Replay",
			Self::ActionReplay2 => "Action Replay II",
			Self::ActionReplay3 => "Action Replay III",
			Self::ActionReplay4 => "Action Replay IV",
			Self::KCS => "KCS Power Cartridge",
			Self::FinalCartridge3 => "The Final Cartridge III",
			Self::SimonsBasic => "Simons' BASIC",
			Self::Ocean => "Ocean",
			Self::FunPlay => "Fun Play",
			Self::SuperGames => "Super Games",
			Self::AtomicPower => "Atomic Power",
			Self::EpyxFastLoad => "Epyx FastLoad",
			Self::C64GS => "C64GS/System 3",
			Self::Dinamic => "Dinamic",
			Self::Zaxxon => "Zaxxon",
			Self::MagicDesk => "Magic Desk",
			Self::SuperSnapshot5 => "Super Snapshot V5",
			Self::StructuredBasic => "Structured BASIC",
			Self::EasyFlash => "EasyFlash",
			Self::EasyFlashXbank => "EasyFlash Xbank",
			Self::EasyFlash3 => "EasyFlash 3",
			Self::RetroReplay => "Retro Replay",
			Self::RGCD => "RGCD",
			Self::GMod2 => "GMod2",
			Self::Pagefox => "Pagefox",
		}
	}
}

#[derive(Debug, Clone)]
/* CartridgeInfo is a detached reporting snapshot. It deliberately contains no live mapper references, so UI and diagnostics can inspect cartridge identity without participating in bus ownership. */
pub struct CartridgeInfo {
	pub name: String,
	pub mapper_type: MapperType,
	pub rom_size: usize,
	pub bank_count: usize,
	pub has_ram: bool,
}

impl fmt::Display for CartridgeInfo {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(
			f,
			"{} ({}) | ROM: {}KB | Banks: {}",
			self.name,
			self.mapper_type.display_name(),
			self.rom_size / 1024,
			self.bank_count,
		)
	}
}

/* CartridgeMapper describes the signals and address windows that real expansion-port hardware can influence. Reads may mutate mapper state, peeks must not; update_signals publishes GAME, EXROM and NMI after those side effects; phase_modes allows cartridges whose mapping differs between PHI1 and PHI2. */
pub trait CartridgeMapper {
	/* Image revisions select physical cartridge variants before CHIP packets
	 * are loaded. Most cartridge families have only their original revision. */
	fn set_hardware_revision(&mut self, _revision: u8) {}
	/* Some freezer RAM overlays inhibit motherboard DRAM writes while
	 * their cartridge window is selected. The range is inclusive. */
	fn exclusive_ram_window(&self) -> Option<(u16, u16)> {
		None
	}
	/* Address-decoded RAM writes can remain active independently of GAME,
	 * EXROM and the CPU port. The range is inclusive. */
	fn independent_write_window(&self) -> Option<(u16, u16)> {
		None
	}
	fn contended_ram_window(&self) -> Option<(u16, u16)> { None }
	fn reset(&mut self);
	fn read_roml(&mut self, offset: u16, cycle: u64) -> Option<u8>;
	fn read_romh(&mut self, offset: u16, cycle: u64) -> Option<u8>;
	fn peek_roml(&self, offset: u16, cycle: u64) -> Option<u8>;
	fn peek_romh(&self, offset: u16, cycle: u64) -> Option<u8>;
	fn peek_io(&self, _addr: u16, _cycle: u64) -> Option<u8> {
		None
	}
	fn read_io(&mut self, addr: u16, cycle: u64) -> Option<u8>;
	fn write_io(&mut self, addr: u16, value: u8, cycle: u64);
	fn write_rom(&mut self, addr: u16, value: u8, cycle: u64);
	fn add_bank(&mut self, bank: usize, addr: u16, data: &[u8]);
	fn update_signals(&mut self, cycle: u64, lines: &mut LineState);
	fn on_freeze(&mut self, lines: &mut LineState);
	fn get_info(&self) -> CartridgeInfo;
	fn get_debug_bank(&self) -> usize;
	fn get_max_bank(&self) -> usize;

	/* The default I/O bridge treats a returned byte as actively driven and absence as no decode. Mappers with open-bus or partially driven registers override this method. */
	fn read_io_bus(&mut self, addr: u16, cycle: u64) -> IoRead {
		match self.read_io(addr, cycle) {
			Some(value) => IoRead::Driven(value),
			None => IoRead::NotDecoded,
		}
	}

	/* The default loader preserves chip classification at the CRT layer but presents a uniform bank-loading contract to simple mappers. Flash- and RAM-aware implementations may override storage behaviour elsewhere. */
	fn add_chip(&mut self, chip_type: ChipType, bank: usize, addr: u16, data: &[u8]) {
		match chip_type {
			ChipType::Rom | ChipType::Ram | ChipType::Flash => self.add_bank(bank, addr, data),
		}
	}

	/* Cycle-aware freeze entry defaults to the timeless hook. Mappers whose NMI pulse or mapping depends on the exact host cycle override this method. */
	fn on_freeze_at(&mut self, cycle: u64, lines: &mut LineState) {
		let _ = cycle;
		self.on_freeze(lines);
	}

	/* Most cartridges expose the same GAME/EXROM map during both clock phases. Freeze and accelerator hardware can override this when PHI1 and PHI2 must decode differently. */
	fn phase_modes(&self, lines: LineState) -> (CartridgeMode, CartridgeMode) {
		let mode = CartridgeMode::from_lines(lines.game, lines.exrom);
		(mode, mode)
	}

	/* Most cartridges release NMI after the motherboard has accepted the freeze request. Hardware with a level-held monitor entry overrides this contract. */
	fn freeze_keeps_nmi(&self) -> bool {
		false
	}

	fn reset_lines(&self, _lines: &mut LineState) {}

	/* NVRAM hooks are intentionally optional because most cartridges contain only immutable ROM. Implementations with EEPROM or flash expose persistence without widening the core mapper API. */
	fn load_nvram(&mut self, _data: &[u8]) {}

	fn save_nvram(&self) -> Option<Vec<u8>> {
		None
	}
}