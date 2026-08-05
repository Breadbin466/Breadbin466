// =======================================================
// src/pla/constants.rs — PLA decoding constants
// =======================================================

/* PLA lookup results are bit sets rather than exclusive enum values because a write can select hidden DRAM at the same time as cartridge or I/O hardware. */
pub(crate) const RAM: u8 = 1 << 0;
pub(crate) const ROML: u8 = 1 << 1;
pub(crate) const ROMH: u8 = 1 << 2;
pub(crate) const IO: u8 = 1 << 3;
pub(crate) const COLOR_RAM: u8 = 1 << 4;