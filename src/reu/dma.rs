// =======================================================
// src/reu/dma.rs — MOS 8726 DMA transfer state
// =======================================================

/*
 * MOS 8726 DMA transfer state.
 *
 * A programmed command is expanded into an explicit byte-transfer state when
 * EXECUTE is accepted.  Keeping the decoded transfer type and bus phase here
 * avoids repeatedly interpreting command bits while the motherboard grants
 * individual bus cycles.
 */

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransferType {
	Store,
	Recall,
	Swap,
	Verify,
}

impl TransferType {
	pub(crate) fn from_command(command: u8) -> Self {
		match command & 0x03 {
			0 => Self::Store,
			1 => Self::Recall,
			2 => Self::Swap,
			_ => Self::Verify,
		}
	}
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SwapPhase {
	ReadC64,
	WriteC64,
}

/*
 * Swap needs two C64 bus grants for each byte.  The latch retains the displaced
 * REU byte between those grants.  Other transfer types always remain in the
 * ReadC64 phase, whose name reflects the first phase of a swap rather than a
 * universal read operation.
 */
#[derive(Clone, Copy)]
pub(crate) struct DmaState {
	pub(crate) transfer_type: TransferType,
	pub(crate) c64_addr: u16,
	pub(crate) reu_addr: usize,
	pub(crate) remaining: usize,
	pub(crate) fix_c64: bool,
	pub(crate) fix_reu: bool,
	pub(crate) autoload: bool,
	pub(crate) swap_phase: SwapPhase,
	pub(crate) latch: u8,
}

impl DmaState {
	/*
	 * Completing one byte is the only point at which the externally visible
	 * counters advance.  Keeping the transition here keeps the runtime
	 * controller on one authoritative implementation of
	 * fixed-address handling, sixteen-bit C64 wrapping, twenty-four-bit REU
	 * wrapping and the terminal-length rule.
	 */
	pub(crate) fn complete_byte(&mut self) -> bool {
		self.swap_phase = SwapPhase::ReadC64;
		if !self.fix_c64 {
			self.c64_addr = self.c64_addr.wrapping_add(1);
		}
		if !self.fix_reu {
			self.reu_addr = (self.reu_addr + 1) & 0x00FF_FFFF;
		}

		let transfer_complete = self.remaining == 1;
		if !transfer_complete {
			self.remaining -= 1;
		}
		transfer_complete
	}
}