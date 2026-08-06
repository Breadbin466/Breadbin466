/*
 * Breadbin466 interactive debugger: bounded instruction history.
 *
 * History is sampled only at 6510/8502 instruction boundaries.  An entry is a
 * compact observation of the state from which the next opcode will execute;
 * it is not a save state and cannot be used to restore the machine.  The
 * fixed-capacity queue guarantees bounded memory use during arbitrarily long
 * debugging sessions and avoids continuous trace files.
 */

use std::collections::VecDeque;
use crate::cpu::CpuState;

/* Cartridge lines and the CPU port are retained with the register set because
 * the same program counter can resolve to different bytes under different PLA
 * configurations.  The selected cartridge bank completes the minimum context
 * needed to interpret an historical instruction fetch. */
#[derive(Debug, Clone, Copy)]
pub struct HistoryEntry {
	pub cycle: u64,
	pub pc: u16,
	pub opcode: u8,
	pub a: u8,
	pub x: u8,
	pub y: u8,
	pub sp: u8,
	pub p: u8,
	pub state: CpuState,
	pub port: u8,
	pub game: bool,
	pub exrom: bool,
	pub bank: usize,
}

/* VecDeque is used as a circular history rather than a growing log.  Once full,
 * the oldest instruction is discarded before the newest one is appended. */
pub struct DebugHistory {
	entries: VecDeque<HistoryEntry>,
	capacity: usize,
}

impl DebugHistory {
	/* A zero capacity remains valid: every pushed entry is immediately discarded.
	 * The production debugger supplies a non-zero fixed capacity. */
	pub fn new(capacity: usize) -> Self { Self { entries: VecDeque::with_capacity(capacity), capacity } }

	/* Retaining the newest observations is more useful than refusing additions
	 * after saturation because a breakpoint normally concerns the events that
	 * immediately precede it. */
	pub fn push(&mut self, entry: HistoryEntry) {
		if self.capacity == 0 { return; }
		if self.entries.len() == self.capacity { self.entries.pop_front(); }
		self.entries.push_back(entry);
	}

	/* recent preserves chronological order.  Requesting more entries than are
	 * available simply starts at the oldest retained observation. */
	pub fn recent(&self, count: usize) -> impl Iterator<Item = &HistoryEntry> {
		let skip = self.entries.len().saturating_sub(count);
		self.entries.iter().skip(skip)
	}

	/* Clearing history does not change its allocation or configured capacity, so
	 * a new investigation can begin without reallocating the ring. */
	pub fn clear(&mut self) { self.entries.clear(); }
}