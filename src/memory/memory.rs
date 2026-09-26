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
use crate::mouse1351::Mouse1351;
use crate::pla::cpu_map::{CpuWriteSelection, build_read_page_map, build_write_selection_map, map_cpu_read_addr_with_ba};
use crate::reu::{Reu, ReuBusAction};
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
	pub mouse1351: Mouse1351,
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
			if cartridge.mount(&path).is_ok() {
				Some(path)
			} else {
				None
			}
		});
		let initial_port = 0x37;
		let initial_game = cartridge.game;
		let initial_exrom = cartridge.exrom;
		let read_map = build_read_page_map(initial_port, initial_game, initial_exrom);
		let write_map = build_write_selection_map(initial_port, initial_game, initial_exrom);
		Self {
			ram: RAMController::new(),
			rom: ROMStorage::new(),
			color_ram: ColorRAM::new(),
			cartridge,
			bus_state: BusState::new(),
			cia1,
			cia2,
			sid: Mos6581::new(),
			mouse1351: Mouse1351::new(),
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
		if self.cartridge.take_lines_changed() {
			self.map_dirty = true;
		}
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
		/* Nordic freezer RAM takes exclusive ownership of its overlay;
		 * writes must not leak into the motherboard RAM underneath. */
		if let Some((start, end)) = self.cartridge.configuration.exclusive_ram_window {
			for page in (start >> 8)..=(end >> 8) {
				let selection = &mut self.write_map[page as usize];
				if selection.cartridge_selected() {
					*selection = selection.without_ram();
				}
			}
		}
		if let Some((start, end)) = self.cartridge.configuration.independent_write_window {
			for page in (start >> 8)..=(end >> 8) {
				let selection = &mut self.write_map[page as usize];
				*selection = selection.with_cartridge();
			}
		}
		if let Some((start, end)) = self.cartridge.configuration.contended_ram_window {
			for page in (start >> 8)..=(end >> 8) {
				if self.read_map[page as usize] == MapRegion::Ram {
					self.read_map[page as usize] = MapRegion::ContendedCartridgeRam;
				}
			}
		}
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
		if self.c128_8502_control & 1 != 0
			&& vic.c128_2mhz_allowed()
			&& addr == 0xD012
			&& vic.timing.cycle == 1
		{
			value.wrapping_sub(1)
		} else {
			value
		}
	}

	#[cold]
	fn c128_adjust_cia_read(&self, addr: u16, value: u8, vic: &VicII) -> u8 {
		if self.c128_8502_control & 1 != 0
			&& vic.c128_2mhz_allowed()
			&& matches!(addr & 0x0F, 0x04 | 0x05)
			&& vic.timing.cycle == 1
		{
			value.wrapping_sub(1)
		} else {
			value
		}
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

	pub fn detach_cartridge(&mut self) -> std::io::Result<()> {
		self.cartridge.detach()?;
		self.initial_crt_path = None;
		self.mark_memory_map_dirty();
		Ok(())
	}

	#[inline(always)]
	pub fn tick_sid(&mut self) -> i32 {
		self.sid.tick()
	}

	/*
	 * REU DMA keeps the processor stopped for the complete command.  BA decides
	 * whether the current one-megahertz phase advances the REC or is consumed by
	 * VIC-II arbitration.  A paused phase still returns true so the motherboard
	 * never lets the CPU execute in the middle of an active command.
	 *
	 * The controller drives ordinary C64 addresses.  Reads and writes therefore
	 * use the normal PLA and device dispatch.  Temporarily moving the REU out of
	 * Memory prevents its own I/O2 decoder from recursively answering a DMA access.
	 */
	#[inline(always)]
	pub fn run_reu_cycle(&mut self, ba_high: bool, cycle: u64, vic: &mut VicII) -> bool {
		match self.reu.bus_action(ba_high, vic.ba_high_at_phi1(), cycle) {
			ReuBusAction::Cpu => false,
			ReuBusAction::Hold => true,
			ReuBusAction::Transfer => {
				let mut reu = std::mem::take(&mut self.reu);
				reu.tick_dma(self, cycle, vic);
				self.reu = reu;
				true
			}
		}
	}

	/* A CPU read resolves one PLA-selected source, applies device side effects, then refreshes the shared data-bus latch with the value actually observed. Colour RAM contributes only its low nibble; unmapped and unclaimed cartridge reads retain the floating bus. */
	#[inline(always)]
	pub fn cpu_read(&mut self, addr: u16, cycle: u64, vic: &mut VicII) -> u8 {
		self.read_with_ba(addr, cycle, vic, !vic.ba_low)
	}

	/* The REC calls this only for an accepted read phase. Preserve that phase's
	 * I/O selection through the BA warning window, rather than decoding it from
	 * the VIC's later pin state. The old-VIC REU timing recordings distinguish
	 * this window from the newer motherboard's RAM-under-I/O read glitch. */
	pub fn reu_read(&mut self, addr: u16, cycle: u64, vic: &mut VicII) -> u8 {
		self.read_with_ba(addr, cycle, vic, true)
	}

	fn read_with_ba(&mut self, addr: u16, cycle: u64, vic: &mut VicII, ba_high: bool) -> u8 {
		let cpu_port_pins = self.last_cpu_port_pins;
		/* The cached map describes BA high. During the VIC warning interval,
		 * evaluate the same PLA with the live BA pin before issuing a read.
		 * (C64-PLA-DISSECTED-2012, section 2.7) */
		let region = if !ba_high {
			let decoded = map_cpu_read_addr_with_ba(addr, cpu_port_pins, self.cartridge.game, self.cartridge.exrom, false);
			if decoded == MapRegion::Ram && self.read_map[(addr >> 8) as usize] == MapRegion::ContendedCartridgeRam {
				MapRegion::ContendedCartridgeRam
			} else { decoded }
		} else {
			self.read_map[(addr >> 8) as usize]
		};
		let value = match region {
			MapRegion::Ram => self.ram.read(addr),
			MapRegion::ContendedCartridgeRam => {
				let value = self.ram.read(addr) | self.cartridge.read_roml(addr & 0x1FFF, cycle).unwrap_or(0);
				self.capture_cartridge_map_change();
				value
			},
			MapRegion::Basic => self.rom.read_basic(addr - BASIC_ROM_START),
			MapRegion::Kernal => self.rom.read_kernal(addr - KERNAL_ROM_START),
			MapRegion::Char => self.rom.read_char((addr - CHAR_ROM_START) & 0x0FFF),
			MapRegion::ColorRam => {
				self.color_ram.read_low_nibble(addr) | (self.bus_state.get_floating(cycle) & 0xF0)
			}
			MapRegion::Io => match addr {
				0xD000..=0xD3FF => {
					if self.c128_2mhz_debug_enabled {
						if addr == 0xD030 {
							self.c128_8502_control
						} else {
							let value = vic.read_register(addr);
							self.c128_adjust_vic_read(addr, value, vic)
						}
					} else {
						vic.read_register(addr)
					}
				}
				0xD505 => {
					if self.c128_2mhz_debug_enabled {
						self.c128_8502_control
					} else {
						0
					}
				}
				0xD400..=0xD7FF => {
					if matches!(addr & 0x001F, 0x19 | 0x1A) {
						if let Some((pot_x, pot_y)) = self.mouse1351.pot_values(cycle) {
							self.sid.pot_x = pot_x;
							self.sid.pot_y = pot_y;
						}
					}
					self.sid.read(addr)
				}
				0xDC00..=0xDCFF => {
					let value = self.cia1.read(addr);
					if self.c128_2mhz_debug_enabled {
						self.c128_adjust_cia_read(addr, value, vic)
					} else {
						value
					}
				}
				0xDD00..=0xDDFF => {
					let value = self.cia2.read(addr);
					if self.c128_2mhz_debug_enabled {
						self.c128_adjust_cia_read(addr, value, vic)
					} else {
						value
					}
				}
				0xDE00..=0xDFFF => {
					if self.reu.enabled && addr >= 0xDF00 {
						self.reu.read(addr)
					} else {
						let floating = if self.c128_2mhz_debug_enabled {
							0xFF
						} else {
							self.bus_state.get_floating(cycle)
						};
						let value = self
							.cartridge
							.read_io_bus(addr, cycle)
							.resolve(floating)
							.unwrap_or(floating);
						self.capture_cartridge_map_change();
						value
					}
				}
				_ => {
					if self.c128_2mhz_debug_enabled {
						0xFF
					} else {
						self.bus_state.get_floating(cycle)
					}
				}
			},
			MapRegion::RomL => {
				let value = self
					.cartridge
					.read_roml(addr & 0x1FFF, cycle)
					.unwrap_or(self.bus_state.get_floating(cycle));
				self.capture_cartridge_map_change();
				self.sync_memory_map(cpu_port_pins);
				value
			}
			MapRegion::RomH => {
				let value = self
					.cartridge
					.read_romh(addr & 0x1FFF, cycle)
					.unwrap_or(self.bus_state.get_floating(cycle));
				self.capture_cartridge_map_change();
				self.sync_memory_map(cpu_port_pins);
				value
			}
			MapRegion::Floating | MapRegion::Ultimax => {
				if self.c128_2mhz_debug_enabled {
					0xFF
				} else {
					self.bus_state.get_floating(cycle)
				}
			}
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
				0xDC00..=0xDCFF => {
					self.cia1.write(addr, value);
					self.mouse1351
						.observe_port_selection(self.cia1.port_a_pin_levels(), cycle);
				}
				0xDD00..=0xDDFF => self.cia2.write(addr, value, cycle),
				0xDE00..=0xDFFF => {
					if self.reu.enabled && addr >= 0xDF00 {
						self.reu.write(addr, value);
					} else {
						self.cartridge.write_io(addr, value, cycle);
						self.capture_cartridge_map_change();
						self.sync_memory_map(self.last_cpu_port_pins);
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
		let value = VICMemoryController::read::<false>(
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

	/* Matrix reads and two of each sprite’s three data slots occupy PHI2,
	 * where cartridges may expose a different mapping from PHI1. */
	#[inline(always)]
	pub fn vic_read_phi2(&mut self, va: u16, bank: u8, cycle: u64) -> u8 {
		let value = VICMemoryController::read::<true>(
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

	/*
	 * Debugger memory inspection is intentionally distinct from an emulated CPU
	 * access. It follows the current PLA-visible map but suppresses destructive
	 * register reads, flash status progression and cartridge line transitions.
	 * The separate physical-RAM accessor lets the debugger inspect storage hidden
	 * beneath ROM or I/O without pretending that the CPU could currently see it.
	 */
	pub fn debug_region(&self, addr: u16) -> MapRegion {
		self.read_map[(addr >> 8) as usize]
	}

	pub fn debug_peek(&self, addr: u16, cycle: u64, _vic: &VicII) -> u8 {
		match self.debug_region(addr) {
			MapRegion::Ram => self.ram.read(addr),
			MapRegion::ContendedCartridgeRam => self.ram.read(addr) | self.cartridge.debug_peek_roml(addr & 0x1FFF, cycle).unwrap_or(0),
			MapRegion::Basic => self.rom.read_basic(addr - BASIC_ROM_START),
			MapRegion::Kernal => self.rom.read_kernal(addr - KERNAL_ROM_START),
			MapRegion::Char => self.rom.read_char((addr - CHAR_ROM_START) & 0x0FFF),
			MapRegion::ColorRam => self.color_ram.read_low_nibble(addr),
			MapRegion::Io => match addr {
				0xD000..=0xD3FF => 0xFF,
				0xD400..=0xD7FF => 0xFF,
				0xDC00..=0xDCFF => self.cia1.peek(addr),
				0xDD00..=0xDDFF => self.cia2.peek(addr),
				0xDF00..=0xDFFF if self.reu.enabled => {
					self.reu.debug_register((addr & 0x1F) as usize)
				}
				0xDE00..=0xDFFF => self.cartridge.debug_peek_io(addr, cycle).unwrap_or(0xFF),
				_ => 0xFF,
			},
			MapRegion::RomL => self
				.cartridge
				.debug_peek_roml(addr & 0x1FFF, cycle)
				.unwrap_or(0xFF),
			MapRegion::RomH => self
				.cartridge
				.debug_peek_romh(addr & 0x1FFF, cycle)
				.unwrap_or(0xFF),
			MapRegion::Floating | MapRegion::Ultimax => 0xFF,
		}
	}

	#[inline(always)]
	pub fn read_ram(&self, addr: u16) -> u8 {
		self.ram.read(addr)
	}
	#[inline(always)]
	pub fn read_color_ram(&self, addr: u16) -> u8 {
		self.color_ram.read_low_nibble(addr)
	}
	pub fn get_vic_bank(&self) -> u8 {
		self.cia2.vic_bank()
	}

	pub fn reset(&mut self, hard_reset: bool) {
		/* Reset releases the software-selected fast clock without changing
		 * the optional processor model selected by the host. */
		self.c128_8502_control = 0;
		self.bus_state.reset();
		if let Err(error) = self.cartridge.save_associated_nvram() {
			eprintln!("[CARTRIDGE] Failed to persist cartridge NVRAM before reset: {error}");
		}
		self.reu.reset(hard_reset);
		if hard_reset {
			self.ram.clear();
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