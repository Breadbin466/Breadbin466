// =======================================================
// src/memory/rom.rs — Replaceable system ROM storage
// =======================================================

use crate::memory::constants::{BASIC_ROM, KERNAL_ROM, CHAR_ROM};
use std::path::Path;
use crate::memory::constants::*;

/* ROMStorage owns replaceable copies of the three system ROM images. Address visibility remains a PLA concern; this type only stores and indexes their contents. */
pub struct ROMStorage {
	pub basic: Box<[u8; BASIC_ROM_SIZE]>,
	pub kernal: Box<[u8; KERNAL_ROM_SIZE]>,
	pub char_rom: Box<[u8; CHAR_ROM_SIZE]>,
}

impl ROMStorage {
/* Construction begins with embedded reference images for all three sockets, providing a complete firmware set before any optional replacement is applied. */
	pub fn new() -> Self {
		Self {
			basic: Box::new(*BASIC_ROM),
			kernal: Box::new(*KERNAL_ROM),
			char_rom: Box::new(*CHAR_ROM),
		}
	}

	/* Reloading restores the embedded reference images without changing the current memory-map configuration. */
	pub fn load_system_roms(&mut self) -> crate::emulator::Result<()> {
		self.basic.copy_from_slice(BASIC_ROM);
		self.kernal.copy_from_slice(KERNAL_ROM);
		self.char_rom.copy_from_slice(CHAR_ROM);
		Ok(())
	}

/* A custom character image is validated against the physical 4 KiB socket before replacing the active contents. */
	pub fn load_custom_char_rom(&mut self, path: &Path) -> crate::emulator::Result<()> {
		let data = std::fs::read(path)?;
		if data.len() != CHAR_ROM_SIZE {
			return Err(format!("Character ROM must be exactly {} bytes", CHAR_ROM_SIZE).into());
		}
		self.char_rom.copy_from_slice(&data);
		Ok(())
	}

/* A custom BASIC image replaces only its 8 KiB socket, preserving the other two system ROMs. */
	pub fn load_custom_basic_rom(&mut self, path: &Path) -> crate::emulator::Result<()> {
		let data = std::fs::read(path)?;
		if data.len() != BASIC_ROM_SIZE {
			return Err(format!("BASIC ROM must be exactly {} bytes", BASIC_ROM_SIZE).into());
		}
		self.basic.copy_from_slice(&data);
		Ok(())
	}

/* A custom KERNAL image replaces only its 8 KiB socket, preserving BASIC and character ROM contents. */
	pub fn load_custom_kernal_rom(&mut self, path: &Path) -> crate::emulator::Result<()> {
		let data = std::fs::read(path)?;
		if data.len() != KERNAL_ROM_SIZE {
			return Err(format!("KERNAL ROM must be exactly {} bytes", KERNAL_ROM_SIZE).into());
		}
		self.kernal.copy_from_slice(&data);
		Ok(())
	}

	pub fn reset_char_rom(&mut self) {
		self.char_rom.copy_from_slice(CHAR_ROM);
	}

	pub fn reset_basic_rom(&mut self) {
		self.basic.copy_from_slice(BASIC_ROM);
	}

	pub fn reset_kernal_rom(&mut self) {
		self.kernal.copy_from_slice(KERNAL_ROM);
	}

	/* Masking the offset mirrors the physical ROM capacity and keeps callers independent of the CPU address at which the image is currently mapped. */
	#[inline(always)]
	pub fn read_basic(&self, offset: u16) -> u8 {
		let idx = (offset & 0x1FFF) as usize;
		self.basic[idx]
	}

	#[inline(always)]
	pub fn read_kernal(&self, offset: u16) -> u8 {
		let idx = (offset & 0x1FFF) as usize;
		self.kernal[idx]
	}

	#[inline(always)]
	pub fn read_char(&self, offset: u16) -> u8 {
		let idx = (offset & 0x0FFF) as usize;
		self.char_rom[idx]
	}
}

impl Default for ROMStorage {
	fn default() -> Self {
		Self::new()
	}
}