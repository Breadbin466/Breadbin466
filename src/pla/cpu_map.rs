// =======================================================
// src/pla/cpu_map.rs — CPU address decoder using MOS 906114-01 PLA
// =======================================================

use crate::pla::constants::{RAM, ROML, ROMH, IO, COLOR_RAM};
use crate::memory::constants::MapRegion;
use super::pla::evaluate;
use super::signals::PlaInputSignals;

/* CPU accesses hold AEC and BA high in this reduced evaluator and derive the three banking inputs from the effective 6510 port pins. GAME and EXROM are passed as their physical active-low levels. */
#[inline(always)]
fn build_cpu_inputs(addr: u16, port: u8, game_n: bool, exrom_n: bool, rw: bool) -> PlaInputSignals {
	let loram = (port & 0x01) != 0;
	let hiram = (port & 0x02) != 0;
	let charen = (port & 0x04) != 0;

	PlaInputSignals {
		a15: (addr & 0x8000) != 0,
		a14: (addr & 0x4000) != 0,
		a13: (addr & 0x2000) != 0,
		a12: (addr & 0x1000) != 0,
		va14_n: true,
		charen,
		hiram,
		loram,
		game_n,
		exrom_n,
		rw,
		aec: true,
		ba: true,
		va13: false,
		va12: false,
		cas_n: true,
	}
}

/* A CPU write can assert more than one destination. The packed selection therefore records independent RAM, cartridge, I/O and colour-RAM enables rather than one mutually exclusive region. */
#[derive(Debug, Clone, Copy)]
pub struct CpuWriteSelection(u8);

impl CpuWriteSelection {

	#[inline(always)]
	fn new(
		ram_selected: bool,
		roml_selected: bool,
		romh_selected: bool,
		io_selected: bool,
		color_ram_selected: bool,
	) -> Self {
		Self(
			u8::from(ram_selected) * RAM
				| u8::from(roml_selected) * ROML
				| u8::from(romh_selected) * ROMH
				| u8::from(io_selected) * IO
				| u8::from(color_ram_selected) * COLOR_RAM,
		)
	}

	#[inline(always)]
	pub fn ram_selected(self) -> bool { self.0 & RAM != 0 }

	#[inline(always)]
	pub fn cartridge_selected(self) -> bool { self.0 & (ROML | ROMH) != 0 }

	#[inline(always)]
	pub fn io_selected(self) -> bool { self.0 & IO != 0 }

	#[inline(always)]
	pub fn color_ram_selected(self) -> bool { self.0 & COLOR_RAM != 0 }
}

/* Write decoding evaluates both the write-cycle outputs and the corresponding read window. The latter preserves ROML/ROMH cartridge chip selects even though internal ROMs themselves are read-only, while CASRAM can simultaneously expose hidden RAM (C64-PRG-1982, memory-map notes). */
#[inline(always)]
pub fn select_cpu_write(
	addr: u16,
	port: u8,
	game: bool,
	exrom: bool,
) -> CpuWriteSelection {
	/* Ultimax ignores the 6510 banking bits. Only low RAM, cartridge windows and I/O remain decoded; the other ranges are electrically unmapped (C64-PRG-1982, Ultimax memory map). */
	if !game && exrom {
		let io_visible = addr >= 0xD000;
		return CpuWriteSelection::new(
			addr <= 0x0FFF,
			(addr & 0xE000) == 0x8000,
			addr >= 0xE000,
			io_visible && (addr < 0xD800 || addr > 0xDBFF),
			io_visible && addr >= 0xD800 && addr <= 0xDBFF,
		);
	}
	let out = evaluate(build_cpu_inputs(addr, port, game, exrom, false));
	let window = evaluate(build_cpu_inputs(addr, port, game, exrom, true));
	let io_selected = !out.io_n && (addr < 0xD800 || addr > 0xDBFF);
	let color_ram_selected = !out.io_n && addr >= 0xD800 && addr <= 0xDBFF;

	CpuWriteSelection::new(
		!out.casram_n,
		!window.roml_n,
		!window.romh_n,
		io_selected,
		color_ram_selected,
	)
}

#[inline(always)]
pub fn map_cpu_read_addr(
	addr: u16,
	port: u8,
	game: bool,
	exrom: bool,
) -> MapRegion {
	/* In Ultimax mode the open ranges are represented explicitly rather than falling back to RAM. */
	if !game && exrom {
		return match addr {
			0x0000..=0x0FFF => MapRegion::Ram,
			0x1000..=0x7FFF => MapRegion::Ultimax,
			0x8000..=0x9FFF => MapRegion::RomL,
			0xA000..=0xCFFF => MapRegion::Ultimax,
			0xD000..=0xDFFF => decode_io_subrange(addr),
			0xE000..=0xFFFF => MapRegion::RomH,
		};
	}
	let out = evaluate(build_cpu_inputs(addr, port, game, exrom, true));

	if !out.roml_n { return MapRegion::RomL; }
	if !out.romh_n { return MapRegion::RomH; }
	if !out.basic_n { return MapRegion::Basic; }
	if !out.kernal_n { return MapRegion::Kernal; }
	if !out.charom_n { return MapRegion::Char; }

	if !out.io_n {
		return decode_io_subrange(addr);
	}

	if !out.casram_n { return MapRegion::Ram; }

	MapRegion::Floating
}

#[inline(always)]
fn decode_io_subrange(addr: u16) -> MapRegion {
	match addr {
		0xD000..=0xD3FF => MapRegion::Io,
		0xD400..=0xD7FF => MapRegion::Io,
		0xD800..=0xDBFF => MapRegion::ColorRam,
		0xDC00..=0xDCFF => MapRegion::Io,
		0xDD00..=0xDDFF => MapRegion::Io,
		0xDE00..=0xDEFF => MapRegion::Io,
		0xDF00..=0xDFFF => MapRegion::Io,
		_ => MapRegion::Floating,
	}
}

/* The PLA inputs used here are constant across each 256-byte page, so one entry per page is sufficient for the hot CPU read path. */
pub fn build_read_page_map(port: u8, game: bool, exrom: bool) -> [MapRegion; 256] {
	let mut table = [MapRegion::Ram; 256];
	for page in 0u16..=255 {
		let addr = page << 8;
		table[page as usize] = map_cpu_read_addr(addr, port, game, exrom);
	}
	table
}

/* Write selections are cached separately because they are not the inverse of the visible read source. */
pub fn build_write_selection_map(port: u8, game: bool, exrom: bool) -> [CpuWriteSelection; 256] {
	let empty = CpuWriteSelection::new(true, false, false, false, false);
	let mut table = [empty; 256];
	for page in 0u16..=255 {
		let addr = page << 8;
		table[page as usize] = select_cpu_write(addr, port, game, exrom);
	}
	table
}