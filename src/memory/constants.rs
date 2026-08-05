// =======================================================
// src/memory/constants.rs — Memory Layout Constants & Enums
// =======================================================

/* The CPU presents a 16-bit address space, while the PLA decides which physical device responds to each address. The full 64 KiB DRAM therefore remains present beneath ROM and I/O overlays (C64-PRG-1982, Memory Management). */
pub const RAM_SIZE: usize = 65536;
pub const BASIC_ROM_START: u16 = 0xA000;
pub const BASIC_ROM_SIZE: usize = 8192;
pub const KERNAL_ROM_START: u16 = 0xE000;
pub const KERNAL_ROM_SIZE: usize = 8192;
pub const CHAR_ROM_START: u16 = 0xD000;
pub const CHAR_ROM_SIZE: usize = 4096;
pub const COLOR_RAM_SIZE: usize = 1024;
pub const IRQ_VECTOR: u16 = 0xFFFE;
pub const NMI_VECTOR: u16 = 0xFFFA;
pub const RESET_VECTOR: u16 = 0xFFFC;

/* MapRegion records the device visible to a read after PLA decoding. Writes use a separate selection because RAM can remain writable beneath a visible ROM or cartridge window. */
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapRegion {
	Ram,
	Basic,
	Kernal,
	Char,
	Io,
	ColorRam,
	RomL,
	RomH,
	Ultimax,
	Floating,
}

/* The shared data-bus latch is retained briefly before undriven reads relax to all ones. This is an emulator model of residual bus charge rather than a PLA output. */
pub(crate) const FLOAT_HOLD_CYCLES: u64 = 8;
pub(crate) const BASIC_ROM: &[u8; BASIC_ROM_SIZE] = include_bytes!("../../roms/basic_901226-01.bin");
pub(crate) const KERNAL_ROM: &[u8; KERNAL_ROM_SIZE] = include_bytes!("../../roms/kernal_901227-03.bin");
pub(crate) const CHAR_ROM: &[u8; CHAR_ROM_SIZE] = include_bytes!("../../roms/characters_901225-01.bin");