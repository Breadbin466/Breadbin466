// =======================================================
// src/emulator/debugger_history.rs — Debugger instruction history
// =======================================================

/*
 * Breadbin466 interactive debugger: bounded, exportable instruction history.
 *
 * History is sampled only at CPU instruction boundaries.  Entries are compact
 * observations, not save states.  Besides the CPU and mapping context needed to
 * interpret an opcode fetch, the history retains the interrupt, raster and CIA
 * state that most often explains why execution took a different path.  The
 * capacity is configurable at run time and remains bounded.
 */

use crate::cpu::CpuState;
use std::collections::VecDeque;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

#[derive(Debug, Clone, Copy)]
pub struct HistoryEntry {
	pub cycle: u64,
	pub raster_line: u16,
	pub raster_cycle: u16,
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
	pub irq: bool,
	pub nmi: bool,
	pub ba_low: bool,
	pub aec_low: bool,
	pub vic_irq_flags: u8,
	pub vic_irq_enable: u8,
	pub cia1_ta: u16,
	pub cia1_tb: u16,
	pub cia1_icr: u8,
	pub cia1_mask: u8,
	pub cia1_irq: bool,
	pub cia2_ta: u16,
	pub cia2_tb: u16,
	pub cia2_icr: u8,
	pub cia2_mask: u8,
	pub cia2_irq: bool,
}

pub struct DebugHistory {
	entries: VecDeque<HistoryEntry>,
	capacity: usize,
}

impl DebugHistory {
	pub fn new(capacity: usize) -> Self {
		Self {
			entries: VecDeque::with_capacity(capacity),
			capacity,
		}
	}

	pub fn push(&mut self, entry: HistoryEntry) {
		if self.capacity == 0 {
			return;
		}
		if self.entries.len() == self.capacity {
			self.entries.pop_front();
		}
		self.entries.push_back(entry);
	}

	pub fn recent(&self, count: usize) -> impl Iterator<Item = &HistoryEntry> {
		let skip = self.entries.len().saturating_sub(count);
		self.entries.iter().skip(skip)
	}

	pub fn len(&self) -> usize {
		self.entries.len()
	}
	pub fn capacity(&self) -> usize {
		self.capacity
	}
	pub fn is_empty(&self) -> bool {
		self.entries.is_empty()
	}
	pub fn clear(&mut self) {
		self.entries.clear();
	}

	pub fn set_capacity(&mut self, capacity: usize) {
		if capacity < self.entries.len() {
			let remove = self.entries.len() - capacity;
			for _ in 0..remove {
				self.entries.pop_front();
			}
		}
		self.capacity = capacity;
		self.entries.shrink_to_fit();
		if capacity > self.entries.capacity() {
			self.entries.reserve(capacity - self.entries.capacity());
		}
	}

	pub fn dump(&self, path: &Path, count: usize) -> io::Result<usize> {
		let file = File::create(path)?;
		let mut writer = BufWriter::with_capacity(256 * 1024, file);
		writeln!(writer, "# Breadbin466 instruction history v2")?;
		writeln!(
			writer,
			"# fields: seq cycle delta raster_line raster_cycle pc opcode a x y sp p state port game exrom bank irq nmi ba aec vic_irq_flags vic_irq_enable cia1_ta cia1_tb cia1_icr cia1_mask cia1_irq cia2_ta cia2_tb cia2_icr cia2_mask cia2_irq"
		)?;
		let available = self.entries.len();
		let wanted = if count == 0 {
			available
		} else {
			count.min(available)
		};
		let skip = available.saturating_sub(wanted);
		let mut previous_cycle = None;
		for (index, entry) in self.entries.iter().skip(skip).enumerate() {
			let delta = previous_cycle
				.map(|previous: u64| entry.cycle.saturating_sub(previous))
				.unwrap_or(0);
			previous_cycle = Some(entry.cycle);
			writeln!(
				writer,
				"{} {} {} {} {} ${:04X} ${:02X} ${:02X} ${:02X} ${:02X} ${:02X} ${:02X} {:?} ${:02X} {} {} {} {} {} {} {} ${:02X} ${:02X} ${:04X} ${:04X} ${:02X} ${:02X} {} ${:04X} ${:04X} ${:02X} ${:02X} {}",
				index + 1,
				entry.cycle,
				delta,
				entry.raster_line,
				entry.raster_cycle,
				entry.pc,
				entry.opcode,
				entry.a,
				entry.x,
				entry.y,
				entry.sp,
				entry.p,
				entry.state,
				entry.port,
				entry.game as u8,
				entry.exrom as u8,
				entry.bank,
				entry.irq as u8,
				entry.nmi as u8,
				entry.ba_low as u8,
				entry.aec_low as u8,
				entry.vic_irq_flags,
				entry.vic_irq_enable,
				entry.cia1_ta,
				entry.cia1_tb,
				entry.cia1_icr,
				entry.cia1_mask,
				entry.cia1_irq as u8,
				entry.cia2_ta,
				entry.cia2_tb,
				entry.cia2_icr,
				entry.cia2_mask,
				entry.cia2_irq as u8
			)?;
		}
		writer.flush()?;
		Ok(wanted)
	}
}