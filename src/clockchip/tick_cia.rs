// =======================================================
// src/clockchip/tick_cia.rs — CIA Tick Orchestration
// =======================================================

use crate::memory::Memory;

/* Both CIAs share the same system clock and TOD pulse, but only CIA2 receives the externally supplied CNT level used by the serial and timer logic. Returning both IRQ outputs lets the motherboard combine them after the same sampling boundary. */
#[inline(always)]
pub fn run_cia_cycle(memory: &mut Memory, tod_pulse: bool, cnt_in: bool) -> (bool, bool) {
	memory.cia1.tick(tod_pulse);
	memory.cia2.tick(tod_pulse, cnt_in);

	(memory.cia1.inner.irq_line, memory.cia2.inner.irq_line)
}