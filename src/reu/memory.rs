// =======================================================
// src/reu/memory.rs — REU and C64 memory transfer helpers
// =======================================================

use crate::memory::ram::RAMController;

/* The MOS 8726 masters the C64 DRAM bus directly. Cartridge ROM, system ROM,
   I/O devices and the separate four-bit Color RAM do not replace the 64 KiB
   DRAM address selected by the controller, so the full $0000-$FFFF range maps
   to physical main RAM during a REU transfer. */
#[inline(always)]
pub(crate) fn read_c64(ram: &RAMController, addr: u16) -> u8 {
	ram.read(addr)
}

/* REU writes target physical C64 DRAM, including the RAM hidden beneath the
   $D000-$DFFF I/O area. Color RAM is a separate device and is therefore not a
   REU DMA destination. */
#[inline(always)]
pub(crate) fn write_c64(ram: &mut RAMController, addr: u16, value: u8) {
	ram.write(addr, value);
}
