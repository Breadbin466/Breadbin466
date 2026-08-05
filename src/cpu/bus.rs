// =======================================================
// src/cpu/bus.rs — CPU Bus Interface Definition
// =======================================================

/* Every CPU access carries the current processor cycle so memory-mapped devices can reproduce read and write timing rather than receiving timeless transactions. */
pub trait SystemBus {
	fn read(&mut self, addr: u16, cycle: u64) -> u8;
	fn write(&mut self, addr: u16, value: u8, cycle: u64);
}