// =======================================================
// src/memory/memory.rs — Memory Management Unit and address space router
// =======================================================

use super::bus::BusState;
use super::color_ram::ColorRAM;
use super::constants::*;
use super::ram::RAMController;
use super::rom::ROMStorage;
use super::vic_access::VICMemoryController;

use crate::cartridge::Cartridge;
use crate::cia::{Cia1, Cia2};
use crate::iec::IecBus;
use crate::pla::cpu_map::{build_read_page_map, build_write_selection_map, CpuWriteSelection};
use crate::reu::{Reu, ReuC64Access};
use crate::sid::Mos6581;
use crate::vic::VicII;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/* Memory is the motherboard address-space router. The PLA-derived page tables choose the visible read source and the set of write destinations, while device-specific side effects remain in their owning components. Reads and writes deliberately use different maps because writes may reach RAM beneath a visible ROM and may also assert cartridge or I/O select lines (C64-PRG-1982, Memory Management; C64-PLA-DISSECTED-2012, memory maps). */
pub struct Memory {
	pub ram: RAMController,
	pub rom: ROMStorage,
	pub color_ram: ColorRAM,
	pub cartridge: Cartridge,
	pub bus_state: BusState,
	pub cia1: Cia1,
	pub cia2: Cia2,
	pub sid: Mos6581,
	pub iec: Rc<IecBus>,
	pub soft_reset_requested: bool,
	pub last_cpu_port_pins: u8,
	initial_crt_path: Option<PathBuf>,
	/* Reads resolve to one visible source per page. */
	read_map: [MapRegion; 256],
	read_map_port: u8,
	/* Writes retain every simultaneously selected destination, including hidden RAM. */
	write_map: [CpuWriteSelection; 256],
	/* Cartridge register writes can change GAME/EXROM asynchronously with respect to the cached tables. */
	map_dirty: bool,
	pub reu: Reu,
	pub c128_2mhz_debug_enabled: bool,
	pub c128_8502_control: u8,
}

impl Memory {
	/* The power-on map uses the normal $37 port value and the cartridge lines produced by the initially mounted image. */
	pub fn new(cia1: Cia1, cia2: Cia2, iec: Rc<IecBus>, active_crt: Option<PathBuf>) -> Self {
		let mut cartridge = Cartridge::new();
		let mounted_initial_crt = active_crt.filter(|path| path.exists()).and_then(|path| {
			if cartridge.mount(&path).is_ok() { Some(path) } else { None }
		});
		let initial_port = 0x37;
		let initial_game = cartridge.game;
		let initial_exrom = cartridge.exrom;
		let read_map = build_read_page_map(initial_port, initial_game, initial_exrom);
		let write_map = build_write_selection_map(initial_port, initial_game, initial_exrom);
		Self {
			ram: RAMController::new_with_deterministic_power_on_pattern(0xDEADBEEF),
			rom: ROMStorage::new(),
			color_ram: ColorRAM::new(),
			cartridge,
			bus_state: BusState::new(),
			cia1,
			cia2,
			sid: Mos6581::new(),
			iec,
			soft_reset_requested: false,
			last_cpu_port_pins: 0x37,
			initial_crt_path: mounted_initial_crt,
			read_map,
			read_map_port: initial_port & 0x07,
			write_map,
			map_dirty: false,
			reu: Reu::new(),
			c128_2mhz_debug_enabled: false,
			c128_8502_control: 0,
		}
	}

	/* Marking is separated from rebuilding so a cartridge can change banking during an access without recursively rebuilding the tables mid-operation. */
	#[inline(always)]
	pub fn mark_memory_map_dirty(&mut self) {
		self.map_dirty = true;
	}

	#[inline(always)]
	fn capture_cartridge_map_change(&mut self) {
		if self.cartridge.take_lines_changed() { self.map_dirty = true; }
	}

	/* Only LORAM, HIRAM, CHAREN, GAME and EXROM affect these cached CPU maps. Rebuilding by page keeps the hot read/write path free of repeated PLA evaluation. */
	#[inline(always)]
	pub fn sync_memory_map(&mut self, port: u8) {
		let pla_port = port & 0x07;
		if !self.map_dirty && pla_port == self.read_map_port {
			return;
		}
		let game = self.cartridge.game;
		let exrom = self.cartridge.exrom;
		self.read_map_port = pla_port;
		self.read_map = build_read_page_map(pla_port, game, exrom);
		self.write_map = build_write_selection_map(pla_port, game, exrom);
		self.map_dirty = false;
	}

	/* The 6510 port pins, not merely its output latch, feed LORAM, HIRAM and CHAREN into the PLA. Pull-ups and data-direction changes can therefore alter the map. */
	#[inline(always)]
	pub fn update_cpu_port_pins(&mut self, pins: u8) {
		let port_changed = pins != self.last_cpu_port_pins;
		self.last_cpu_port_pins = pins;
		if port_changed || self.map_dirty {
			self.sync_memory_map(pins);
		}
	}

	#[cold]
	fn c128_adjust_vic_read(&self, addr: u16, value: u8, vic: &VicII) -> u8 {
		if self.c128_8502_control & 1 != 0 && vic.c128_2mhz_allowed() && addr == 0xD012 && vic.timing.cycle == 1 {
			value.wrapping_sub(1)
		} else { value }
	}

	#[cold]
	fn c128_adjust_cia_read(&self, addr: u16, value: u8, vic: &VicII) -> u8 {
		if self.c128_8502_control & 1 != 0 && vic.c128_2mhz_allowed() && matches!(addr & 0x0F, 0x04 | 0x05) && vic.timing.cycle == 1 {
			value.wrapping_sub(1)
		} else { value }
	}

	pub fn load_system_roms(&mut self) -> crate::emulator::Result<()> {
		self.rom.load_system_roms()
	}

	pub fn mount_cartridge(&mut self, path: &Path) -> crate::emulator::Result<()> {
		self.cartridge.mount(path)?;
		self.initial_crt_path = Some(path.to_path_buf());
		self.mark_memory_map_dirty();
		Ok(())
	}

	pub fn detach_cartridge(&mut self) {
		self.initial_crt_path = None;
		self.cartridge.detach();
		self.mark_memory_map_dirty();
	}

	#[inline(always)]
	pub fn tick_sid(&mut self) -> Option<i32> {
		self.sid.tick()
	}

	/* REU DMA owns the physical C64 DRAM bus. It yields only while VIC-II AEC is low;
	   BA is an advance warning for the CPU and does not itself remove the bus. Cartridge
	   ROM, system ROM, I/O and Color RAM do not replace the underlying 64 KiB DRAM. */
	#[inline(always)]
	pub fn run_reu_cycle(&mut self, aec_high: bool) -> bool {
		if !self.reu.dma_active() {
			return false;
		}

		if !matches!(self.reu.c64_access(), ReuC64Access::None) && !aec_high {
			return true;
		}

		self.reu.tick_dma(&mut self.ram);
		true
	}

	/* A CPU read resolves one PLA-selected source, applies device side effects, then refreshes the shared data-bus latch with the value actually observed. Colour RAM contributes only its low nibble; unmapped and unclaimed cartridge reads retain the floating bus. */
	#[inline(always)]
	pub fn cpu_read(&mut self, addr: u16, cycle: u64, vic: &mut VicII) -> u8 {
		let cpu_port_pins = self.last_cpu_port_pins;
		let region = self.read_map[(addr >> 8) as usize];
		let value = match region {
			MapRegion::Ram => self.ram.read(addr),
			MapRegion::Basic => self.rom.read_basic(addr - BASIC_ROM_START),
			MapRegion::Kernal => self.rom.read_kernal(addr - KERNAL_ROM_START),
			MapRegion::Char => self.rom.read_char((addr - CHAR_ROM_START) & 0x0FFF),
			MapRegion::ColorRam => self.color_ram.read_low_nibble(addr) | (self.bus_state.get_floating(cycle) & 0xF0),
			MapRegion::Io => match addr {
				0xD000..=0xD3FF => {
					if self.c128_2mhz_debug_enabled {
						if addr == 0xD030 { self.c128_8502_control }
						else { let value = vic.read_register(addr); self.c128_adjust_vic_read(addr, value, vic) }
					} else { vic.read_register(addr) }
				}
				0xD505 => if self.c128_2mhz_debug_enabled { self.c128_8502_control } else { 0 },
				0xD400..=0xD7FF => self.sid.read(addr),
				0xDC00..=0xDCFF => {
					let value = self.cia1.read(addr);
					if self.c128_2mhz_debug_enabled { self.c128_adjust_cia_read(addr, value, vic) } else { value }
				}
				0xDD00..=0xDDFF => {
					let value = self.cia2.read(addr);
					if self.c128_2mhz_debug_enabled { self.c128_adjust_cia_read(addr, value, vic) } else { value }
				}
				0xDE00..=0xDFFF => {
					let floating = if self.c128_2mhz_debug_enabled { 0xFF } else { self.bus_state.get_floating(cycle) };
					let cartridge_value = self.cartridge.read_io_bus(addr, cycle).resolve(floating);
					self.capture_cartridge_map_change();
					if self.reu.enabled && (0xDF00..=0xDFFF).contains(&addr) {
						let reu_value = self.reu.read(addr);
						cartridge_value.map_or(reu_value, |value| value & reu_value)
					} else {
						cartridge_value.unwrap_or(floating)
					}
				}
				_ => if self.c128_2mhz_debug_enabled { 0xFF } else { self.bus_state.get_floating(cycle) },
			},
			MapRegion::RomL => {
				let value = self.cartridge.read_roml(addr & 0x1FFF, cycle).unwrap_or(self.bus_state.get_floating(cycle));
				self.capture_cartridge_map_change();
				self.sync_memory_map(cpu_port_pins);
				value
			},
			MapRegion::RomH => {
				let value = self.cartridge.read_romh(addr & 0x1FFF, cycle).unwrap_or(self.bus_state.get_floating(cycle));
				self.capture_cartridge_map_change();
				self.sync_memory_map(cpu_port_pins);
				value
			},
			MapRegion::Floating | MapRegion::Ultimax => if self.c128_2mhz_debug_enabled { 0xFF } else { self.bus_state.get_floating(cycle) },
		};
		self.bus_state.update(value, cycle);
		value
	}

	/* A write can reach several destinations in the same cycle. Cartridge selects and I/O are handled first, hidden DRAM is updated when CASRAM is asserted, and the written byte becomes the new shared-bus value. */
	#[inline(always)]
	pub fn cpu_write(&mut self, addr: u16, value: u8, cycle: u64, vic: &mut VicII) {
		if addr == 0xFF00 {
			self.reu.trigger_ff00();
		}
		let selection = self.write_map[(addr >> 8) as usize];
		if selection.cartridge_selected() {
			self.cartridge.write_rom(addr, value, cycle);
			self.capture_cartridge_map_change();
		}
		if selection.color_ram_selected() {
			self.color_ram.write(addr, value);
		}
		if selection.io_selected() {
			match addr {
				0xD000..=0xD3FF => {
					if self.c128_2mhz_debug_enabled && addr == 0xD030 {
						self.c128_8502_control = value;
					} else {
						vic.write_register(addr, value, self);
					}
				}
				0xD505 => {
					if self.c128_2mhz_debug_enabled {
						self.c128_8502_control = value;
					}
				}
				0xD400..=0xD7FF => self.sid.write(addr, value),
				0xDC00..=0xDCFF => self.cia1.write(addr, value),
				0xDD00..=0xDDFF => self.cia2.write(addr, value, cycle),
				0xDE00..=0xDFFF => {
					self.cartridge.write_io(addr, value, cycle);
					self.capture_cartridge_map_change();
					self.sync_memory_map(self.last_cpu_port_pins);
					if self.reu.enabled && (0xDF00..=0xDFFF).contains(&addr) {
						self.reu.write(addr, value);
					}
				}
				_ => {}
			}
		}
		if selection.ram_selected() {
			self.ram.write(addr, value);
		}
		self.bus_state.update(value, cycle);
	}

	/* VIC-II reads use its 14-bit address plus the CIA2-selected bank and do not share the CPU overlay map. The resulting byte still drives the common data bus. */
	#[inline(always)]
	pub fn vic_read(&mut self, va: u16, bank: u8, cycle: u64) -> u8 {
		let value = VICMemoryController::read(
			va,
			bank,
			cycle,
			&self.ram,
			&self.rom,
			&mut self.cartridge,
			self.bus_state.get_floating(cycle),
		);
		self.bus_state.update(value, cycle);
		value
	}

	#[inline(always)]
	pub fn read_ram(&self, addr: u16) -> u8 { self.ram.read(addr) }
	#[inline(always)]
	pub fn read_color_ram(&self, addr: u16) -> u8 { self.color_ram.read_low_nibble(addr) }
	pub fn get_vic_bank(&self) -> u8 { self.cia2.vic_bank() }

	pub fn reset(&mut self, hard_reset: bool) {
		self.bus_state.reset();
		self.cartridge.save_associated_nvram();
		self.reu.reset(hard_reset);
		if hard_reset {
			self.ram = RAMController::new_with_deterministic_power_on_pattern(0xDEADBEEF);
			self.color_ram.clear();
			self.cartridge.reset();
		}
		self.sid.reset();
		self.soft_reset_requested = false;
		self.last_cpu_port_pins = 0x37;
		self.mark_memory_map_dirty();
		self.sync_memory_map(0x37);
	}
}