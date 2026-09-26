// =======================================================
// src/cartridge/cartridge_device.rs — Cartridge manager and lifecycle control
// =======================================================

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::bus_configuration::{CartridgeConfiguration, IoRead};
use super::constants::{CRT_MAGIC, MAX_CRT_SIZE, MAX_NVRAM_SIZE};
use super::crt_loader::CrtImage;
use super::mapper_creation::create_mapper;
use super::mapper_interface::{CartridgeMapper, LineState, MapperType};

static NVRAM_COUNTER: AtomicU64 = AtomicU64::new(0);

/* Cartridge owns the mounted image, mapper and expansion-port outputs. Every mapper access is followed by signal synchronisation, ensuring that bank-register side effects and freeze transitions become visible to the motherboard on the same emulated cycle. */
pub struct Cartridge {
	pub game: bool,
	pub exrom: bool,
	pub nmi_low: bool,
	pub configuration: CartridgeConfiguration,
	pub mapper: Box<dyn CartridgeMapper>,
	pub mapper_type: MapperType,
	pub crt_name: String,
	current_crt_path: Option<PathBuf>,
	reset_game: bool,
	reset_exrom: bool,
	reset_nmi_low: bool,
	present: bool,
	requires_cycle_tick: bool,
	lines_changed: bool,
	freeze_due_cycle: Option<u64>,
}

impl Cartridge {
	/* An empty expansion port starts with GAME and EXROM released, no interrupt asserted and a neutral mapper that cannot decode memory. */
	pub fn new() -> Self {
		Self {
			game: true,
			exrom: true,
			nmi_low: false,
			configuration: CartridgeConfiguration::from_lines(true, true, false),
			mapper: create_mapper(MapperType::Normal),
			mapper_type: MapperType::Normal,
			crt_name: String::new(),
			current_crt_path: None,
			reset_game: true,
			reset_exrom: true,
			reset_nmi_low: false,
			present: false,
			requires_cycle_tick: false,
			lines_changed: false,
			freeze_due_cycle: None,
		}
	}

	fn dispatch_load(&mut self, data: &[u8]) -> crate::emulator::Result<bool> {
		if data.len() >= 16 && &data[0..16] == CRT_MAGIC {
			self.load_crt(data)?;
			Ok(true)
		} else {
			self.load_raw(data)?;
			Ok(false)
		}
	}

	fn capture_reset_lines(&mut self) {
		self.reset_game = self.game;
		self.reset_exrom = self.exrom;
		self.reset_nmi_low = self.nmi_low;
	}

	/* Signal synchronisation is the single commit point between mapper-private registers and motherboard-visible state. It also keeps a sticky change flag so the PLA cache is rebuilt only when electrical configuration actually changes. */
	fn sync_configuration(&mut self, cycle: u64) -> bool {
		if !self.present {
			return false;
		}
		let previous = self.configuration;
		let previous_game = self.game;
		let previous_exrom = self.exrom;
		let previous_nmi = self.nmi_low;
		let mut lines = LineState {
			game: self.game,
			exrom: self.exrom,
			nmi_low: self.nmi_low,
		};
		self.mapper.update_signals(cycle, &mut lines);
		if self.freeze_due_cycle.is_some() {
			lines.nmi_low = true;
		}
		let (phi1_mode, phi2_mode) = self.mapper.phase_modes(lines);
		let bank = self.mapper.get_debug_bank();
		let (game, exrom) = phi2_mode.lines();
		self.game = game;
		self.exrom = exrom;
		self.nmi_low = lines.nmi_low;
		self.configuration = CartridgeConfiguration {
			phi1_mode,
			phi2_mode,
			roml_bank: bank,
			romh_bank: bank,
			phi2_ram: false,
			contended_ram_window: self.mapper.contended_ram_window(),
			exclusive_ram_window: self.mapper.exclusive_ram_window(),
			independent_write_window: self.mapper.independent_write_window(),
			irq_low: false,
			nmi_low: self.nmi_low,
		};
		let changed = previous != self.configuration
			|| previous_game != self.game
			|| previous_exrom != self.exrom
			|| previous_nmi != self.nmi_low;
		self.lines_changed |= changed;
		changed
	}

	/* Reset restores the line levels captured when the image was mounted, then lets the mapper apply any hardware-specific reset wiring before publishing a fresh configuration. */
	pub fn reset(&mut self) {
		self.freeze_due_cycle = None;
		self.mapper.reset();
		self.game = self.reset_game;
		self.exrom = self.reset_exrom;
		self.nmi_low = self.reset_nmi_low;
		let mut lines = LineState {
			game: self.game,
			exrom: self.exrom,
			nmi_low: self.nmi_low,
		};
		self.mapper.reset_lines(&mut lines);
		self.game = lines.game;
		self.exrom = lines.exrom;
		self.nmi_low = lines.nmi_low;
		self.configuration =
			CartridgeConfiguration::from_lines(self.game, self.exrom, self.nmi_low);
		self.sync_configuration(0);
	}

	/* The mounted path is retained solely for persistence and UI reporting; mapper behaviour never depends on the host filename. */
	pub fn get_path(&self) -> Option<&Path> {
		self.current_crt_path.as_deref()
	}

	/* Detaching persists mapper-owned non-volatile storage before replacing the complete device with the electrically disconnected state. A failed host write leaves the cartridge mounted. */
	pub fn detach(&mut self) -> std::io::Result<()> {
		self.save_associated_nvram()?;
		*self = Self::new();
		Ok(())
	}

	#[inline(always)]
	pub fn is_present(&self) -> bool {
		self.present
	}

	#[inline(always)]
	/* Most cartridges are purely access-driven. This fast path clocks only mappers with time-dependent outputs or a pending delayed freeze transition. */
	pub fn tick_if_needed(&mut self, cycle: u64) -> bool {
		if !self.present || (!self.requires_cycle_tick && self.freeze_due_cycle.is_none()) {
			return false;
		}
		self.tick(cycle)
	}

	#[inline(always)]
	/* The motherboard consumes this edge-triggered flag after rebuilding its PLA routing; clearing it here prevents repeated cache invalidation. */
	pub fn take_lines_changed(&mut self) -> bool {
		let changed = self.lines_changed;
		self.lines_changed = false;
		changed
	}

	/* The physical cartridge button is mapper-specific: EasyFlash 3 performs a reset-style action, while KCS enters the common delayed freeze path. */
	pub fn trigger_menu_button(&mut self, cycle: u64) -> bool {
		match self.mapper_type {
			MapperType::EasyFlash3 => {
				self.mapper.reset();
				self.sync_configuration(cycle);
				self.lines_changed = true;
				true
			}
			MapperType::KCS => {
				self.trigger_freeze_button(cycle);
				false
			}
			_ => false,
		}
	}

	/* A freeze button asserts NMI immediately, then delays mapper reconfiguration by three CPU cycles. This separates the asynchronous interrupt edge from the later ROM/bank takeover performed by freezer hardware. */
	pub fn trigger_freeze_button(&mut self, cycle: u64) {
		if !self.present || self.freeze_due_cycle.is_some() {
			return;
		}
		self.nmi_low = true;
		self.configuration.nmi_low = true;
		self.freeze_due_cycle = Some(cycle.saturating_add(3));
		self.lines_changed = true;
	}

	#[inline(always)]
	/* ROM-window reads are followed by signal publication because some hardware changes banks or mode as a side effect of the access itself. */
	pub fn read_roml(&mut self, addr: u16, cycle: u64) -> Option<u8> {
		if !self.present {
			return None;
		}
		let value = self.mapper.read_roml(addr, cycle);
		self.sync_configuration(cycle);
		value
	}

	/*
	 * Debugger inspection must observe the currently selected cartridge bytes
	 * without advancing flash command state, changing a bank latch or publishing
	 * new GAME/EXROM levels. These three accessors therefore use the mapper's
	 * side-effect-free peek path and deliberately avoid sync_configuration().
	 */
	#[inline(always)]
	pub fn debug_peek_roml(&self, addr: u16, cycle: u64) -> Option<u8> {
		if self.present {
			self.mapper.peek_roml(addr, cycle)
		} else {
			None
		}
	}

	#[inline(always)]
	pub fn debug_peek_romh(&self, addr: u16, cycle: u64) -> Option<u8> {
		if self.present {
			self.mapper.peek_romh(addr, cycle)
		} else {
			None
		}
	}

	#[inline(always)]
	pub fn debug_peek_io(&self, addr: u16, cycle: u64) -> Option<u8> {
		if self.present {
			self.mapper.peek_io(addr, cycle)
		} else {
			None
		}
	}

	#[inline(always)]
	pub fn read_romh(&mut self, addr: u16, cycle: u64) -> Option<u8> {
		if !self.present {
			return None;
		}
		let value = self.mapper.read_romh(addr, cycle);
		self.sync_configuration(cycle);
		value
	}

	/* I/O reads preserve drive masks and open-bus semantics until the motherboard combines them with its data-bus latch. */
	pub fn read_io_bus(&mut self, addr: u16, cycle: u64) -> IoRead {
		if !self.present {
			return IoRead::NotDecoded;
		}
		let value = self.mapper.read_io_bus(addr, cycle);
		self.sync_configuration(cycle);
		value
	}

	pub fn read_io(&mut self, addr: u16, cycle: u64) -> Option<u8> {
		match self.read_io_bus(addr, cycle) {
			IoRead::Driven(value) => Some(value),
			_ => None,
		}
	}

	pub fn write_io(&mut self, addr: u16, val: u8, cycle: u64) {
		if !self.present {
			return;
		}
		self.mapper.write_io(addr, val, cycle);
		self.sync_configuration(cycle);
	}

	/* Writes in ROM space are still forwarded because flash cartridges and RAM overlays decode write cycles even when reads appear ROM-like. */
	pub fn write_rom(&mut self, addr: u16, val: u8, cycle: u64) {
		if !self.present {
			return;
		}
		self.mapper.write_rom(addr, val, cycle);
		self.sync_configuration(cycle);
	}

	pub fn get_info(&self) -> String {
		self.mapper.get_info().to_string()
	}

	#[inline(always)]
	/* Per-cycle service completes delayed freeze entry and services mappers whose electrical outputs decay or change with time even when the CPU performs no cartridge access. */
	pub fn tick(&mut self, cycle: u64) -> bool {
		if !self.present {
			return false;
		}
		if let Some(due_cycle) = self.freeze_due_cycle {
			if cycle >= due_cycle {
				let mut lines = LineState {
					game: self.game,
					exrom: self.exrom,
					nmi_low: true,
				};
				self.mapper.on_freeze_at(cycle, &mut lines);
				if !self.mapper.freeze_keeps_nmi() {
					lines.nmi_low = false;
				}
				self.game = lines.game;
				self.exrom = lines.exrom;
				self.nmi_low = lines.nmi_low;
				self.freeze_due_cycle = None;
				self.lines_changed = true;
			}
		}
		self.sync_configuration(cycle)
	}

	/* Mounting loads and validates the replacement image before committing it, then records reset lines and associated NVRAM only after mapper construction succeeds. */
	pub fn mount(&mut self, path: &Path) -> crate::emulator::Result<()> {
		if !path.is_file() {
			return Err("File not found".into());
		}
		let data = crate::host_files::read(path, MAX_CRT_SIZE)?;

		let mut replacement = Self::new();
		replacement.current_crt_path = Some(path.to_path_buf());
		let is_crt = replacement.dispatch_load(&data)?;
		if is_crt {
			replacement.load_associated_nvram();
		}
		replacement.present = true;
		replacement.requires_cycle_tick =
			matches!(replacement.mapper_type, MapperType::EpyxFastLoad);
		replacement.sync_configuration(0);
		replacement.capture_reset_lines();
		replacement.lines_changed = false;

		self.save_associated_nvram()?;
		*self = replacement;
		Ok(())
	}

	fn load_raw(&mut self, data: &[u8]) -> crate::emulator::Result<()> {
		if data.is_empty() || data.len() > 0x4000 {
			return Err("Raw cartridge must contain between 1 and 16384 bytes".into());
		}
		self.mapper = create_mapper(MapperType::Normal);
		self.mapper_type = MapperType::Normal;
		self.crt_name = String::new();
		if data.len() <= 0x2000 {
			self.mapper.add_bank(0, 0x8000, data);
			self.game = true;
			self.exrom = false;
		} else {
			let (l, h) = data.split_at(0x2000);
			self.mapper.add_bank(0, 0x8000, l);
			if !h.is_empty() {
				self.mapper.add_bank(0, 0xA000, h);
			}
			self.game = false;
			self.exrom = false;
		}
		Ok(())
	}

	/* CRT loading creates the declared mapper first and then delivers CHIP packets in file order. Mapper-specific add_chip implementations retain control over ROM, RAM and flash placement. */
	fn load_crt(&mut self, data: &[u8]) -> crate::emulator::Result<()> {
		let image = CrtImage::parse(data)?;
		self.mapper_type = image.mapper_type;
		self.mapper = create_mapper(image.mapper_type);
		self.mapper.set_hardware_revision(image.hardware_revision);
		self.crt_name = image.name;
		self.game = image.game;
		self.exrom = image.exrom;
		for chip in image.chips {
			self.mapper
				.add_chip(chip.chip_type, chip.bank, chip.address, chip.data);
		}
		Ok(())
	}

	/* NVRAM loading is deliberately optional and fail-soft: a missing or unreadable sidecar leaves the cartridge in its mapper-defined erased state. */
	pub fn load_associated_nvram(&mut self) {
		if let Some(ref path) = self.current_crt_path {
			let nvram_path = path.with_extension("sav");
			if let Ok(buf) = crate::host_files::read(&nvram_path, MAX_NVRAM_SIZE) {
				self.mapper.load_nvram(&buf);
			}
		}
	}

	/* Mapper-owned persistent bytes are written only when a cartridge supplies them, avoiding empty sidecars for ordinary ROM cartridges. */
	pub fn save_associated_nvram(&self) -> std::io::Result<()> {
		let Some(path) = self.current_crt_path.as_ref() else {
			return Ok(());
		};
		let Some(bytes) = self.mapper.save_nvram() else {
			return Ok(());
		};
		let nvram_path = path.with_extension("sav");
		let counter = NVRAM_COUNTER.fetch_add(1, Ordering::Relaxed);
		let temporary = nvram_path.with_extension(format!("sav.{}.tmp", counter));
		let backup = nvram_path.with_extension(format!("sav.{}.bak", counter));
		/* An existing temporary path is never opened or removed: it may be a
		 * symlink supplied alongside an untrusted cartridge image. */
		let mut file = OpenOptions::new()
			.write(true)
			.create_new(true)
			.open(&temporary)?;
		let result = (|| -> std::io::Result<()> {
			file.write_all(&bytes)?;
			file.sync_all()?;
			drop(file);
			if std::fs::rename(&temporary, &nvram_path).is_ok() {
				return Ok(());
			}
			let had_original = nvram_path.exists();
			if had_original {
				std::fs::rename(&nvram_path, &backup)?;
			}
			if let Err(error) = std::fs::rename(&temporary, &nvram_path) {
				if had_original {
					let _ = std::fs::rename(&backup, &nvram_path);
				}
				return Err(error);
			}
			if had_original {
				let _ = std::fs::remove_file(&backup);
			}
			Ok(())
		})();
		if result.is_err() {
			let _ = std::fs::remove_file(temporary);
		}
		result
	}
}