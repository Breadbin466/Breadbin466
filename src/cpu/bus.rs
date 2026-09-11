// =======================================================
// src/cpu/bus.rs — CPU Bus Interface Definition
// =======================================================

/* Every CPU access carries the current processor cycle so memory-mapped devices can reproduce read and write timing rather than receiving timeless transactions. */
pub trait SystemBus {
	/* Boards with an address-enable input disconnect the processor during DMA.
	Standalone 6502 buses have no such gate. */
	fn address_enabled(&self) -> bool { true }
	fn read(&mut self, addr: u16, cycle: u64) -> u8;
	fn write(&mut self, addr: u16, value: u8, cycle: u64);
	/* Integrated-port writes still assert the external write strobe.
	 * Boards without an integrated port have no additional transaction. */
	fn write_port(&mut self, _addr: u16, _cycle: u64) {}
}