// =======================================================
// src/reu/timing.rs — MOS 8726 cartridge-port bus timing
// =======================================================

/*
 * MOS 8726 cartridge-port bus timing.
 *
 * The REC asserts DMA for the complete command, so the processor remains held
 * even while the VIC-II temporarily owns the bus.  BA is sampled separately to
 * decide whether the current one-megahertz bus phase may advance the transfer.
 * A low BA therefore pauses the byte stream without releasing the CPU.
 */

/*
 * ReuBusAction is the complete decision required by the motherboard for one
 * master cycle.  Cpu means that no command owns the bus.  Hold means that DMA
 * remains asserted while the VIC-II has announced a bus request.  Transfer
 * grants exactly one external C64 bus phase to the REC.
 */
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReuBusAction {
	Cpu,
	Hold,
	Transfer,
}

#[inline]
pub(crate) fn action_for_cycle(dma_active: bool, ba_high: bool) -> ReuBusAction {
	match (dma_active, ba_high) {
		(false, _) => ReuBusAction::Cpu,
		(true, false) => ReuBusAction::Hold,
		(true, true) => ReuBusAction::Transfer,
	}
}