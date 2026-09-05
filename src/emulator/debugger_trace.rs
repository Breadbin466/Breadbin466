// =======================================================
// src/emulator/debugger_trace.rs — Non-blocking debugger trace recorder
// =======================================================

/*
 * Breadbin466 tracing is a non-blocking observation service.  Rules may select
 * instruction execution, reads or writes.  Bus events and instruction events
 * share one chronological file but keep distinct record types, so a trace can
 * correlate control flow with the hardware accesses that caused it without
 * pretending that an instruction boundary is a bus transfer.
 */

use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use super::debugger_breakpoint::AccessKind;
use super::debugger_history::HistoryEntry;
use crate::motherboard::DebugBusAccessKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraceRule {
	pub id: u32,
	pub kind: AccessKind,
	pub start: u16,
	pub end: u16,
	pub value: Option<u8>,
	pub enabled: bool,
}

impl TraceRule {
	#[inline]
	pub fn matches_bus(&self, kind: DebugBusAccessKind, addr: u16, value: u8) -> bool {
		let access_kind = match kind {
			DebugBusAccessKind::Read => AccessKind::Read,
			DebugBusAccessKind::Write => AccessKind::Write,
		};
		self.enabled
			&& self.kind == access_kind
			&& (self.start..=self.end).contains(&addr)
			&& self.value.map(|expected| expected == value).unwrap_or(true)
	}
	#[inline]
	pub fn matches_execute(&self, pc: u16, opcode: u8) -> bool {
		self.enabled
			&& self.kind == AccessKind::Execute
			&& (self.start..=self.end).contains(&pc)
			&& self
				.value
				.map(|expected| expected == opcode)
				.unwrap_or(true)
	}
}

#[derive(Debug, Clone, Copy)]
pub struct TraceEvent {
	pub kind: DebugBusAccessKind,
	pub addr: u16,
	pub value: u8,
	pub cycle: u64,
	pub raster_line: u16,
	pub raster_cycle: u16,
	pub instruction_pc: u16,
	pub opcode: u8,
	pub a: u8,
	pub x: u8,
	pub y: u8,
	pub sp: u8,
	pub p: u8,
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

pub struct TraceRecorder {
	rules: Vec<TraceRule>,
	next_rule_id: u32,
	path: Option<PathBuf>,
	writer: Option<BufWriter<File>>,
	enabled: bool,
	sequence: u64,
	last_event_cycle: Option<u64>,
}

impl TraceRecorder {
	pub fn new() -> Self {
		Self {
			rules: Vec::new(),
			next_rule_id: 1,
			path: None,
			writer: None,
			enabled: false,
			sequence: 0,
			last_event_cycle: None,
		}
	}

	pub fn set_path(&mut self, path: PathBuf) -> io::Result<()> {
		self.close()?;
		self.path = Some(path);
		Ok(())
	}
	pub fn path(&self) -> Option<&Path> {
		self.path.as_deref()
	}
	pub fn is_enabled(&self) -> bool {
		self.enabled
	}
	pub fn rules(&self) -> &[TraceRule] {
		&self.rules
	}
	pub fn event_count(&self) -> u64 {
		self.sequence
	}

	pub fn add_rule(&mut self, kind: AccessKind, start: u16, end: u16, value: Option<u8>) -> u32 {
		let id = self.next_rule_id;
		self.next_rule_id = self.next_rule_id.saturating_add(1);
		self.rules.push(TraceRule {
			id,
			kind,
			start,
			end,
			value,
			enabled: true,
		});
		id
	}
	pub fn delete_rule(&mut self, id: u32) -> bool {
		let old = self.rules.len();
		self.rules.retain(|rule| rule.id != id);
		old != self.rules.len()
	}
	pub fn set_rule_enabled(&mut self, id: u32, enabled: bool) -> bool {
		if let Some(rule) = self.rules.iter_mut().find(|rule| rule.id == id) {
			rule.enabled = enabled;
			true
		} else {
			false
		}
	}
	pub fn clear_rules(&mut self) {
		self.rules.clear();
	}

	pub fn start(&mut self) -> io::Result<()> {
		if self.enabled {
			return Ok(());
		}
		let path = self
			.path
			.as_ref()
			.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "No trace file selected"))?;
		let file = OpenOptions::new()
			.create(true)
			.truncate(true)
			.write(true)
			.open(path)?;
		let mut writer = BufWriter::with_capacity(512 * 1024, file);
		writeln!(writer, "# Breadbin466 unified trace v2")?;
		writeln!(
			writer,
			"# E = instruction boundary, B = completed CPU bus access"
		)?;
		writeln!(
			writer,
			"# Every record carries cycle/raster/CPU/interrupt context; B additionally carries address/value and device state."
		)?;
		writer.flush()?;
		self.writer = Some(writer);
		self.enabled = true;
		self.sequence = 0;
		self.last_event_cycle = None;
		Ok(())
	}
	pub fn stop(&mut self) -> io::Result<()> {
		self.enabled = false;
		self.flush()
	}
	pub fn flush(&mut self) -> io::Result<()> {
		if let Some(writer) = self.writer.as_mut() {
			writer.flush()?;
		}
		Ok(())
	}
	pub fn close(&mut self) -> io::Result<()> {
		self.enabled = false;
		if let Some(mut writer) = self.writer.take() {
			writer.flush()?;
		}
		Ok(())
	}

	#[inline]
	fn next_header(&mut self, cycle: u64) -> (u64, u64) {
		let delta = self
			.last_event_cycle
			.map(|previous| cycle.saturating_sub(previous))
			.unwrap_or(0);
		self.sequence = self.sequence.saturating_add(1);
		self.last_event_cycle = Some(cycle);
		(self.sequence, delta)
	}

	#[inline]
	pub fn record_execute(&mut self, entry: HistoryEntry) -> io::Result<bool> {
		if !self.enabled
			|| !self
				.rules
				.iter()
				.any(|rule| rule.matches_execute(entry.pc, entry.opcode))
		{
			return Ok(false);
		}
		let (sequence, delta) = self.next_header(entry.cycle);
		if let Some(writer) = self.writer.as_mut() {
			writeln!(
				writer,
				"E {} {} {} {} {} PC=${:04X} OP=${:02X} A=${:02X} X=${:02X} Y=${:02X} SP=${:02X} P=${:02X} STATE={:?} PORT=${:02X} G={} E={} BANK={} IRQ={} NMI={} BA={} AEC={} VICIF=${:02X} VICIE=${:02X} C1TA=${:04X} C1TB=${:04X} C1ICR=${:02X} C1MASK=${:02X} C1IRQ={} C2TA=${:04X} C2TB=${:04X} C2ICR=${:02X} C2MASK=${:02X} C2IRQ={}",
				sequence,
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
		Ok(true)
	}

	#[inline]
	pub fn record(&mut self, event: TraceEvent) -> io::Result<bool> {
		if !self.enabled
			|| !self
				.rules
				.iter()
				.any(|rule| rule.matches_bus(event.kind, event.addr, event.value))
		{
			return Ok(false);
		}
		let (sequence, delta) = self.next_header(event.cycle);
		let kind = match event.kind {
			DebugBusAccessKind::Read => 'R',
			DebugBusAccessKind::Write => 'W',
		};
		let region = region_name(event.addr);
		if let Some(writer) = self.writer.as_mut() {
			writeln!(
				writer,
				"B {} {} {} {} {} {} ADDR=${:04X} VAL=${:02X} PC=${:04X} OP=${:02X} A=${:02X} X=${:02X} Y=${:02X} SP=${:02X} P=${:02X} IRQ={} NMI={} BA={} AEC={} VICIF=${:02X} VICIE=${:02X} C1TA=${:04X} C1TB=${:04X} C1ICR=${:02X} C1MASK=${:02X} C1IRQ={} C2TA=${:04X} C2TB=${:04X} C2ICR=${:02X} C2MASK=${:02X} C2IRQ={} REGION={}",
				sequence,
				event.cycle,
				delta,
				event.raster_line,
				event.raster_cycle,
				kind,
				event.addr,
				event.value,
				event.instruction_pc,
				event.opcode,
				event.a,
				event.x,
				event.y,
				event.sp,
				event.p,
				event.irq as u8,
				event.nmi as u8,
				event.ba_low as u8,
				event.aec_low as u8,
				event.vic_irq_flags,
				event.vic_irq_enable,
				event.cia1_ta,
				event.cia1_tb,
				event.cia1_icr,
				event.cia1_mask,
				event.cia1_irq as u8,
				event.cia2_ta,
				event.cia2_tb,
				event.cia2_icr,
				event.cia2_mask,
				event.cia2_irq as u8,
				region
			)?;
		}
		Ok(true)
	}
}

impl Drop for TraceRecorder {
	fn drop(&mut self) {
		let _ = self.close();
	}
}

fn region_name(addr: u16) -> &'static str {
	match addr {
		0xD000..=0xD02E => "VIC",
		0xD400..=0xD41C => "SID",
		0xDC00..=0xDC0F => "CIA1",
		0xDD00..=0xDD0F => "CIA2",
		0xDE00..=0xDEFF => "IO1",
		0xDF00..=0xDFFF => "IO2",
		0x0000..=0x0001 => "CPU_PORT",
		_ => "MEM",
	}
}