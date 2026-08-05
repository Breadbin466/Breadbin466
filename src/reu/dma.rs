// =======================================================
// src/reu/dma.rs — MOS 8726 DMA transfer state
// =======================================================

/* A DMA command is expanded into an explicit byte-transfer state. Swap needs two bus phases and uses latch to retain the displaced REU byte while the C64 write completes. */
#[derive(Clone, Copy)]
pub(crate) struct DmaState {
	pub(crate) transfer_type: u8,
	pub(crate) c64_addr: u16,
	pub(crate) reu_addr: usize,
	pub(crate) remaining: usize,
	pub(crate) fix_c64: bool,
	pub(crate) fix_reu: bool,
	pub(crate) autoload: bool,
	pub(crate) phase: u8,
	pub(crate) latch: u8,
}