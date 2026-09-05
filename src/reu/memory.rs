// =======================================================
// src/reu/memory.rs — Commodore 1764 expansion DRAM
// =======================================================

/*
 * Commodore 1764 expansion DRAM.
 *
 * This module is the physical boundary between the MOS 8726 logical address
 * counter and the nineteen address lines connected by the 1764 hardware.
 */

pub const REU_1764_CAPACITY_BYTES: usize = 512 * 1024;
const REU_1764_ADDRESS_MASK: usize = REU_1764_CAPACITY_BYTES - 1;

/*
 * ReuMemory represents the eight 64 KiB DRAM banks fitted to a Commodore 1764.
 * The MOS 8726 retains a full twenty-four-bit logical address, but the 1764
 * connects only nineteen address lines to DRAM.  Address folding therefore
 * belongs at this physical-memory boundary rather than in the visible bank
 * register or in the DMA counters.
 *
 * The allocation is deliberately retained while the device is disabled or
 * reset.  A controller RESET does not erase DRAM, and the menu's enable switch
 * models reconnecting the expansion rather than manufacturing a new memory
 * image every time the user changes the setting.
 */
pub(crate) struct ReuMemory {
	bytes: Box<[u8]>,
}

impl ReuMemory {
	/* A newly attached 1764 begins with a deterministic zeroed image.  This is a
	host-side construction policy only; subsequent controller resets preserve
	the contents exactly. */
	pub(crate) fn new() -> Self {
		Self {
			bytes: vec![0; REU_1764_CAPACITY_BYTES].into_boxed_slice(),
		}
	}

	#[inline]
	fn physical_offset(logical_address: usize) -> usize {
		logical_address & REU_1764_ADDRESS_MASK
	}

	#[inline]
	pub(crate) fn read(&self, logical_address: usize) -> u8 {
		self.bytes[Self::physical_offset(logical_address)]
	}

	#[inline]
	pub(crate) fn write(&mut self, logical_address: usize, value: u8) {
		let offset = Self::physical_offset(logical_address);
		self.bytes[offset] = value;
	}

	pub(crate) fn capacity(&self) -> usize {
		REU_1764_CAPACITY_BYTES
	}
}

impl Default for ReuMemory {
	fn default() -> Self {
		Self::new()
	}
}