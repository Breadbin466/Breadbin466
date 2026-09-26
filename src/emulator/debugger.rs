// =======================================================
// src/emulator/debugger.rs — Interactive debugger orchestration
// =======================================================

/*
 * Breadbin466 interactive debugger: orchestration and machine inspection.
 *
 * The debugger is an emulator service rather than an emulated device.  It does
 * not participate in the C64 bus, own hardware state or alter timing while the
 * machine is running.  A dedicated input thread blocks on standard input and
 * sends complete command lines through a channel; all parsing, inspection and
 * state changes occur on the emulator thread at cycle boundaries.
 *
 * CPU-visible inspection uses debug_peek, which follows the current PLA and
 * cartridge mapping without invoking destructive device reads.  Physical RAM
 * inspection bypasses that mapping.  This distinction is essential when code
 * is hidden beneath ROM or I/O and is exposed explicitly by the mem and ram
 * commands rather than inferred by the debugger.
 */

use std::collections::VecDeque;
use std::fs::File;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::thread;

use super::debugger_breakpoint::{AccessKind, Breakpoint};
use super::debugger_command::{self, DebugCommand};
use super::debugger_disassembly::{format_instruction, instruction_length};
use super::debugger_history::{DebugHistory, HistoryEntry};
use super::debugger_trace::{TraceEvent, TraceRecorder};
use crate::emulator::context::AppContext;
use crate::motherboard::DebugBusAccessKind;

/* Instruction history is intentionally bounded.  Two hundred thousand entries
 * retain useful lead-up context while preventing an unattended debugger from
 * becoming an unbounded trace recorder. */
const HISTORY_CAPACITY: usize = 200_000;

/* Debugger owns only observation policy and interactive control state.  paused
 * gates advancement in the emulator loop; the two step counters temporarily
 * permit bounded progress without changing the CPU's own state machine. */
#[derive(Debug, Clone, Copy)]
struct CycleBreakpoint {
	id: u32,
	cycle: u64,
	enabled: bool,
}

#[derive(Debug, Clone, Copy)]
struct RasterBreakpoint {
	id: u32,
	line: u16,
	cycle: Option<u16>,
	enabled: bool,
}

pub struct Debugger {
	receiver: Receiver<String>,
	startup_commands: VecDeque<String>,
	breakpoints: Vec<Breakpoint>,
	cycle_breakpoints: Vec<CycleBreakpoint>,
	raster_breakpoints: Vec<RasterBreakpoint>,
	next_breakpoint_id: u32,
	history: DebugHistory,
	history_auto: Option<(PathBuf, usize)>,
	trace: TraceRecorder,
	current_instruction_pc: u16,
	current_instruction_opcode: u8,
	pub paused: bool,
	step_cycles: u64,
	step_instructions: u64,
	last_instruction_boundary: bool,
	quit_requested: bool,
}

impl Debugger {
	/* Construction starts the console reader and leaves the machine paused before
	 * its first cycle.  The console thread never receives an AppContext reference,
	 * so it cannot race emulation or inspect partially updated hardware state. */
	pub fn new() -> Self {
		Self::new_with_commands(Vec::new())
	}

	/* Startup commands are deliberately fed through the ordinary command parser.
	 * This gives command-line automation exactly the same semantics and validation
	 * as an operator typing into the monitor, without a script/stub file. */
	pub fn new_with_commands(commands: Vec<String>) -> Self {
		let (sender, receiver) = mpsc::channel();
		thread::Builder::new()
			.name("breadbin-debugger-console".into())
			.spawn(move || {
				let stdin = io::stdin();
				for line in stdin.lock().lines() {
					match line {
						Ok(line) => {
							if sender.send(line).is_err() {
								break;
							}
						}
						Err(_) => break,
					}
				}
			})
			.expect("Unable to start debugger console thread");
		println!("Breadbin466 debugger ready. Type 'help'.");
		print_prompt();
		Self {
			receiver,
			startup_commands: commands.into(),
			breakpoints: Vec::new(),
			cycle_breakpoints: Vec::new(),
			raster_breakpoints: Vec::new(),
			next_breakpoint_id: 1,
			history: DebugHistory::new(HISTORY_CAPACITY),
			history_auto: None,
			trace: TraceRecorder::new(),
			current_instruction_pc: 0,
			current_instruction_opcode: 0,
			paused: true,
			step_cycles: 0,
			step_instructions: 0,
			last_instruction_boundary: false,
			quit_requested: false,
		}
	}

	/* All currently queued lines are consumed before the next machine cycle.  A
	 * command may resume execution, request bounded stepping or leave the machine
	 * paused for further inspection. */
	pub fn poll_commands(&mut self, context: &mut AppContext) {
		while let Some(line) = self.startup_commands.pop_front() {
			self.execute_line(&line, context, true);
		}
		while let Ok(line) = self.receiver.try_recv() {
			self.execute_line(&line, context, false);
			if self.paused {
				print_prompt();
			}
		}
	}

	fn execute_line(&mut self, line: &str, context: &mut AppContext, startup: bool) {
		match debugger_command::parse(line) {
			Ok(command) => self.execute(command, context),
			Err(error) if error.is_empty() => {}
			Err(error) => {
				if startup {
					println!("Monitor startup command failed: {line}: {error}");
				} else {
					println!("{error}");
				}
			}
		}
	}

	/* before_cycle is the sole execution gate.  Returning false means no emulated
	 * component advances.  Execute breakpoints are evaluated only when t_state is
	 * zero, before the opcode at PC begins, and history records that same boundary
	 * so the retained entry describes the instruction about to execute. */
	pub fn before_cycle(&mut self, context: &mut AppContext) -> bool {
		self.poll_commands(context);
		if self.quit_requested {
			context.input.close_requested = true;
			return false;
		}
		if self.paused && self.step_cycles == 0 && self.step_instructions == 0 {
			return false;
		}
		let machine = &context.machine;
		let boundary = machine.cpu.t_state == 0;
		if self
			.cycle_breakpoints
			.iter()
			.any(|b| b.enabled && machine.current_cycle() >= b.cycle)
		{
			self.paused = true;
			println!("Cycle breakpoint at cycle {}.", machine.current_cycle());
			self.auto_dump_history();
			self.print_registers(context);
			print_prompt();
			return false;
		}
		if self.raster_breakpoints.iter().any(|b| {
			b.enabled
				&& b.line == machine.vic.timing.raster_line
				&& b.cycle
					.map(|cycle| cycle == machine.vic.timing.cycle)
					.unwrap_or(true)
		}) {
			self.paused = true;
			println!(
				"Raster breakpoint at line {}, cycle {}.",
				machine.vic.timing.raster_line, machine.vic.timing.cycle
			);
			self.auto_dump_history();
			self.print_registers(context);
			print_prompt();
			return false;
		}
		if boundary {
			let opcode =
				machine
					.memory
					.debug_peek(machine.cpu.pc, machine.current_cycle(), &machine.vic);
			self.current_instruction_pc = machine.cpu.pc;
			self.current_instruction_opcode = opcode;
			let entry = HistoryEntry {
				cycle: machine.current_cycle(),
				raster_line: machine.vic.timing.raster_line,
				raster_cycle: machine.vic.timing.cycle,
				pc: machine.cpu.pc,
				opcode,
				a: machine.cpu.a,
				x: machine.cpu.x,
				y: machine.cpu.y,
				sp: machine.cpu.sp,
				p: machine.cpu.p,
				state: machine.cpu.state,
				port: machine.cpu.port.get_pins(),
				game: machine.memory.cartridge.game,
				exrom: machine.memory.cartridge.exrom,
				bank: machine.memory.cartridge.mapper.get_debug_bank(),
				irq: machine.cpu.irq_line,
				nmi: !machine.cpu.nmi_line,
				ba_low: machine.vic.ba_low,
				aec_low: machine.vic.aec_low,
				vic_irq_flags: machine.vic.irq.flags,
				vic_irq_enable: machine.vic.irq.enable,
				cia1_ta: machine.memory.cia1.inner.ta.counter,
				cia1_tb: machine.memory.cia1.inner.tb.counter,
				cia1_icr: machine.memory.cia1.inner.icr,
				cia1_mask: machine.memory.cia1.inner.icr_mask,
				cia1_irq: machine.memory.cia1.inner.irq_line,
				cia2_ta: machine.memory.cia2.inner.ta.counter,
				cia2_tb: machine.memory.cia2.inner.tb.counter,
				cia2_icr: machine.memory.cia2.inner.icr,
				cia2_mask: machine.memory.cia2.inner.icr_mask,
				cia2_irq: machine.memory.cia2.inner.irq_line,
			};
			self.history.push(entry);
			if let Err(error) = self.trace.record_execute(entry) {
				eprintln!("Trace write failed: {error}. Tracing stopped.");
				let _ = self.trace.stop();
			}
			if self
				.breakpoints
				.iter()
				.any(|b| b.matches(AccessKind::Execute, machine.cpu.pc, Some(opcode)))
			{
				self.paused = true;
				println!(
					"Breakpoint at ${:04X}, cycle {}.",
					machine.cpu.pc,
					machine.current_cycle()
				);
				self.auto_dump_history();
				self.print_registers(context);
				print_prompt();
				return false;
			}
		}
		self.last_instruction_boundary = boundary;
		true
	}

	/* Read and write watchpoints are evaluated after a completed motherboard cycle.
	 * This ordering preserves the hardware access and its side effects, then pauses
	 * before another cycle can begin.  Cycle and instruction stepping use the same
	 * post-cycle boundary, avoiding partial rollback of an executed bus phase. */
	pub fn after_cycle(&mut self, context: &mut AppContext) {
		if let Some(access) = context.machine.take_debug_bus_access() {
			let machine = &context.machine;
			let trace_event = TraceEvent {
				kind: access.kind,
				addr: access.addr,
				value: access.value,
				cycle: access.cycle,
				raster_line: machine.vic.timing.raster_line,
				raster_cycle: machine.vic.timing.cycle,
				instruction_pc: self.current_instruction_pc,
				opcode: self.current_instruction_opcode,
				a: machine.cpu.a,
				x: machine.cpu.x,
				y: machine.cpu.y,
				sp: machine.cpu.sp,
				p: machine.cpu.p,
				irq: machine.cpu.irq_line,
				nmi: !machine.cpu.nmi_line,
				ba_low: machine.vic.ba_low,
				aec_low: machine.vic.aec_low,
				vic_irq_flags: machine.vic.irq.flags,
				vic_irq_enable: machine.vic.irq.enable,
				cia1_ta: machine.memory.cia1.inner.ta.counter,
				cia1_tb: machine.memory.cia1.inner.tb.counter,
				cia1_icr: machine.memory.cia1.inner.icr,
				cia1_mask: machine.memory.cia1.inner.icr_mask,
				cia1_irq: machine.memory.cia1.inner.irq_line,
				cia2_ta: machine.memory.cia2.inner.ta.counter,
				cia2_tb: machine.memory.cia2.inner.tb.counter,
				cia2_icr: machine.memory.cia2.inner.icr,
				cia2_mask: machine.memory.cia2.inner.icr_mask,
				cia2_irq: machine.memory.cia2.inner.irq_line,
			};
			if let Err(error) = self.trace.record(trace_event) {
				eprintln!("Trace write failed: {error}. Tracing stopped.");
				let _ = self.trace.stop();
			}

			let kind = match access.kind {
				DebugBusAccessKind::Read => AccessKind::Read,
				DebugBusAccessKind::Write => AccessKind::Write,
			};
			if self
				.breakpoints
				.iter()
				.any(|b| b.matches(kind, access.addr, Some(access.value)))
			{
				self.paused = true;
				println!(
					"{:?} watchpoint at ${:04X}, value ${:02X}, cycle {}.",
					kind, access.addr, access.value, access.cycle
				);
				self.auto_dump_history();
				self.print_registers(context);
				print_prompt();
			}
		}
		if self.step_cycles > 0 {
			self.step_cycles -= 1;
			if self.step_cycles == 0 {
				self.paused = true;
				self.print_registers(context);
				print_prompt();
			}
		}
		if self.step_instructions > 0
			&& !self.last_instruction_boundary
			&& context.machine.cpu.t_state == 0
		{
			self.step_instructions -= 1;
			if self.step_instructions == 0 {
				self.paused = true;
				self.print_registers(context);
				print_prompt();
			}
		}
	}

	/* Typed commands are executed synchronously against a stable machine state.
	 * Inspection commands are observational; run and step commands only change the
	 * debugger's execution gate, while quit requests an orderly termination at the
	 * next polling point. */
	fn execute(&mut self, command: DebugCommand, context: &mut AppContext) {
		match command {
			DebugCommand::Help => print_help(),
			DebugCommand::Run => {
				self.paused = false;
				self.step_cycles = 0;
				self.step_instructions = 0;
				println!("Running.");
			}
			DebugCommand::Pause => {
				self.paused = true;
				self.print_registers(context);
			}
			DebugCommand::StepCycle(count) => {
				self.paused = false;
				self.step_cycles = count.max(1);
				self.step_instructions = 0;
			}
			DebugCommand::StepInstruction(count) => {
				self.paused = false;
				self.step_instructions = count.max(1);
				self.step_cycles = 0;
			}
			DebugCommand::Registers => self.print_registers(context),
			DebugCommand::SetRegister { name, value } => self.set_register(context, &name, value),
			DebugCommand::Memory {
				start,
				end,
				physical,
			} => self.print_memory(context, start, end, physical),
			DebugCommand::PokeRam { start, values } => self.poke_ram(context, start, &values),
			DebugCommand::Disassemble { start, count } => self.disassemble(context, start, count),
			DebugCommand::AddBreakpoint {
				kind,
				start,
				end,
				value,
			} => {
				let id = self.next_breakpoint_id;
				self.next_breakpoint_id += 1;
				self.breakpoints.push(Breakpoint {
					id,
					kind,
					start,
					end,
					value,
					enabled: true,
				});
				println!(
					"Breakpoint {id}: {kind:?} ${start:04X}-${end:04X}{}",
					value.map(|v| format!(" = ${v:02X}")).unwrap_or_default()
				);
			}
			DebugCommand::AddCycleBreakpoint(cycle) => {
				let id = self.next_breakpoint_id;
				self.next_breakpoint_id += 1;
				self.cycle_breakpoints.push(CycleBreakpoint {
					id,
					cycle,
					enabled: true,
				});
				println!("Breakpoint {id}: absolute cycle {cycle}");
			}
			DebugCommand::AddCycleAfter(delta) => {
				let cycle = context.machine.current_cycle().saturating_add(delta);
				let id = self.next_breakpoint_id;
				self.next_breakpoint_id += 1;
				self.cycle_breakpoints.push(CycleBreakpoint {
					id,
					cycle,
					enabled: true,
				});
				println!("Breakpoint {id}: cycle {cycle} (+{delta})");
			}
			DebugCommand::AddRasterBreakpoint { line, cycle } => {
				let id = self.next_breakpoint_id;
				self.next_breakpoint_id += 1;
				self.raster_breakpoints.push(RasterBreakpoint {
					id,
					line,
					cycle,
					enabled: true,
				});
				println!(
					"Breakpoint {id}: raster {line}{}",
					cycle.map(|v| format!(":{v}")).unwrap_or_default()
				);
			}
			DebugCommand::DeleteBreakpoint(id) => {
				self.breakpoints.retain(|b| b.id != id);
				self.cycle_breakpoints.retain(|b| b.id != id);
				self.raster_breakpoints.retain(|b| b.id != id);
			}
			DebugCommand::EnableBreakpoint { id, enabled } => {
				let mut found = false;
				for b in &mut self.breakpoints {
					if b.id == id {
						b.enabled = enabled;
						found = true;
					}
				}
				for b in &mut self.cycle_breakpoints {
					if b.id == id {
						b.enabled = enabled;
						found = true;
					}
				}
				for b in &mut self.raster_breakpoints {
					if b.id == id {
						b.enabled = enabled;
						found = true;
					}
				}
				if !found {
					println!("No breakpoint {id}.");
				}
			}
			DebugCommand::ClearBreakpoints => {
				self.breakpoints.clear();
				self.cycle_breakpoints.clear();
				self.raster_breakpoints.clear();
				println!("Breakpoints cleared.");
			}
			DebugCommand::ListBreakpoints => self.print_breakpoints(),
			DebugCommand::History(count) => self.print_history(count),
			DebugCommand::HistoryCapacity(capacity) => {
				self.history.set_capacity(capacity);
				println!("History capacity: {capacity} instructions.");
			}
			DebugCommand::HistoryClear => {
				self.history.clear();
				println!("History cleared.");
			}
			DebugCommand::HistoryDump { path, count } => self.dump_history(&path, count),
			DebugCommand::HistoryAuto { path, count } => {
				self.history_auto = Some((path.clone(), count));
				println!(
					"Automatic history dump: {} ({} entries; 0=all retained).",
					path.display(),
					count
				);
			}
			DebugCommand::HistoryAutoOff => {
				self.history_auto = None;
				println!("Automatic history dump disabled.");
			}
			DebugCommand::HistoryStatus => self.print_history_status(),
			DebugCommand::Reu => self.print_reu(context),
			DebugCommand::Cartridge => self.print_cartridge(context),
			DebugCommand::Pla => self.print_pla(context),
			DebugCommand::Vic => self.print_vic(context),
			DebugCommand::Cia(index) => self.print_cia(context, index),
			DebugCommand::Sid => self.print_sid(context),
			DebugCommand::TraceFile(path) => match self.trace.set_path(path.clone()) {
				Ok(()) => println!("Trace file: {}", path.display()),
				Err(error) => println!("Unable to select trace file: {error}"),
			},
			DebugCommand::AddTrace {
				kind,
				start,
				end,
				value,
			} => {
				let id = self.trace.add_rule(kind, start, end, value);
				println!(
					"Trace rule {id}: {kind:?} ${start:04X}-${end:04X}{}",
					value.map(|v| format!(" = ${v:02X}")).unwrap_or_default()
				);
			}
			DebugCommand::DeleteTrace(id) => {
				if !self.trace.delete_rule(id) {
					println!("No trace rule {id}.");
				}
			}
			DebugCommand::EnableTrace { id, enabled } => {
				if !self.trace.set_rule_enabled(id, enabled) {
					println!("No trace rule {id}.");
				}
			}
			DebugCommand::ListTraces => self.print_traces(),
			DebugCommand::TraceStart => match self.trace.start() {
				Ok(()) => println!("Tracing started."),
				Err(error) => println!("Unable to start tracing: {error}"),
			},
			DebugCommand::TraceStop => match self.trace.stop() {
				Ok(()) => println!("Tracing stopped after {} events.", self.trace.event_count()),
				Err(error) => println!("Unable to stop tracing cleanly: {error}"),
			},
			DebugCommand::TraceFlush => match self.trace.flush() {
				Ok(()) => println!("Trace file flushed."),
				Err(error) => println!("Unable to flush trace file: {error}"),
			},
			DebugCommand::TraceClear => {
				self.trace.clear_rules();
				println!("Trace rules cleared.");
			}
			DebugCommand::TraceStatus => self.print_trace_status(),
			DebugCommand::Save {
				path,
				start,
				end,
				physical,
			} => match self.save_memory(context, &path, start, end, physical) {
				Ok(()) => println!(
					"Saved {} bytes to {}.",
					usize::from(end.wrapping_sub(start)) + 1,
					path.display()
				),
				Err(e) => println!("Save failed: {e}"),
			},
			DebugCommand::Quit => {
				self.auto_dump_history();
				let _ = self.trace.stop();
				self.quit_requested = true;
			}
		}
	}

	/* Register output includes the CPU micro-state and external interrupt levels so
	 * a stop in the middle of an instruction remains interpretable. */
	fn print_registers(&self, context: &AppContext) {
		let m = &context.machine;
		println!(
			"CYCLE={} PC=${:04X} IR=${:02X} T={} A=${:02X} X=${:02X} Y=${:02X} SP=${:02X} P=${:02X} STATE={:?} IRQ={} NMI={} $01=${:02X}",
			m.current_cycle(),
			m.cpu.pc,
			m.cpu.ir,
			m.cpu.t_state,
			m.cpu.a,
			m.cpu.x,
			m.cpu.y,
			m.cpu.sp,
			m.cpu.p,
			m.cpu.state,
			m.cpu.irq_line as u8,
			(!m.cpu.nmi_line) as u8,
			m.cpu.port.get_pins()
		);
	}
	/* Memory ranges are inclusive and may wrap through $FFFF.  CPU-visible reads use
	 * non-destructive inspection; physical reads expose the underlying DRAM even
	 * where ROM, cartridge or I/O is currently selected. */
	fn print_memory(&self, context: &AppContext, start: u16, end: u16, physical: bool) {
		let mut addr = start;
		loop {
			print!("${addr:04X}: ");
			for offset in 0..16u16 {
				let current = addr.wrapping_add(offset);
				if current > end {
					break;
				}
				let value = if physical {
					context.machine.memory.read_ram(current)
				} else {
					context.machine.memory.debug_peek(
						current,
						context.machine.current_cycle(),
						&context.machine.vic,
					)
				};
				print!("{value:02X} ");
			}
			println!();
			if end.wrapping_sub(addr) < 16 {
				break;
			}
			addr = addr.wrapping_add(16);
		}
	}
	/* Disassembly follows current CPU visibility and never performs ordinary device
	 * reads.  It is therefore a view of what the processor would fetch under the
	 * present PLA state, not a scan of physical RAM. */
	fn disassemble(&self, context: &AppContext, mut pc: u16, count: usize) {
		for _ in 0..count {
			let op = context.machine.memory.debug_peek(
				pc,
				context.machine.current_cycle(),
				&context.machine.vic,
			);
			let len = instruction_length(op);
			let mut bytes = [0u8; 3];
			for i in 0..len {
				bytes[i as usize] = context.machine.memory.debug_peek(
					pc.wrapping_add(i),
					context.machine.current_cycle(),
					&context.machine.vic,
				);
			}
			println!(
				"${pc:04X}: {:02X} {:02X} {:02X}  {}",
				bytes[0],
				bytes[1],
				bytes[2],
				format_instruction(pc, &bytes)
			);
			pc = pc.wrapping_add(len);
		}
	}
	/* REU output uses explicit debug accessors so register display does not clear
	 * status flags or otherwise emulate a CPU register read. */
	fn print_reu(&self, context: &AppContext) {
		let r = &context.machine.memory.reu;
		print!(
			"REU enabled={} dma={} irq={} regs:",
			r.enabled,
			r.dma_active(),
			r.irq_pending
		);
		for i in 0..=10 {
			print!(" {:02X}", r.debug_register(i));
		}
		println!();
	}
	/* Cartridge state reports the mapper's logical bank and expansion-port lines;
	 * it does not force a ROM read or advance flash command state. */
	fn print_cartridge(&self, context: &AppContext) {
		let c = &context.machine.memory.cartridge;
		let info = c.mapper.get_info();
		println!(
			"Cartridge {:?} '{}' GAME={} EXROM={} NMI={} bank={}/{}",
			info.mapper_type,
			c.crt_name,
			c.game as u8,
			c.exrom as u8,
			c.nmi_low as u8,
			c.mapper.get_debug_bank(),
			c.mapper.get_max_bank()
		);
	}
	/* The PLA summary samples representative windows rather than claiming a static
	 * whole-address-space map.  It is intended to explain the current CPU view of
	 * the principal ROM, cartridge and I/O regions. */
	fn print_pla(&self, context: &AppContext) {
		let m = &context.machine;
		println!(
			"PLA $01=${:02X} GAME={} EXROM={} bank={}",
			m.cpu.port.get_pins(),
			m.memory.cartridge.game as u8,
			m.memory.cartridge.exrom as u8,
			m.memory.cartridge.mapper.get_debug_bank()
		);
		for addr in [0x8000u16, 0xA000, 0xD000, 0xE000] {
			println!("${addr:04X}: {:?}", m.memory.debug_region(addr));
		}
	}
	fn print_traces(&self) {
		if self.trace.rules().is_empty() {
			println!("No trace rules.");
			return;
		}
		for rule in self.trace.rules() {
			println!(
				"{}: {:?} ${:04X}-${:04X}{} enabled={}",
				rule.id,
				rule.kind,
				rule.start,
				rule.end,
				rule.value
					.map(|v| format!(" = ${v:02X}"))
					.unwrap_or_default(),
				rule.enabled
			);
		}
	}

	fn print_trace_status(&self) {
		let path = self
			.trace
			.path()
			.map(|path| path.display().to_string())
			.unwrap_or_else(|| "<not set>".into());
		println!(
			"Trace enabled={} file={} rules={} events={}",
			self.trace.is_enabled(),
			path,
			self.trace.rules().len(),
			self.trace.event_count()
		);
	}

	fn set_register(&mut self, context: &mut AppContext, name: &str, value: u64) {
		let cpu = &mut context.machine.cpu;
		match name {
			"a" if value <= 0xFF => cpu.a = value as u8,
			"x" if value <= 0xFF => cpu.x = value as u8,
			"y" if value <= 0xFF => cpu.y = value as u8,
			"sp" if value <= 0xFF => cpu.sp = value as u8,
			"p" | "flags" if value <= 0xFF => cpu.p = value as u8,
			"pc" if value <= 0xFFFF => cpu.pc = value as u16,
			_ => {
				println!("Unknown register or value out of range: {name}");
				return;
			}
		}
		self.print_registers(context);
	}

	fn poke_ram(&mut self, context: &mut AppContext, start: u16, values: &[u8]) {
		for (offset, value) in values.iter().copied().enumerate() {
			context
				.machine
				.memory
				.ram
				.write(start.wrapping_add(offset as u16), value);
		}
		println!(
			"Wrote {} byte(s) to physical RAM starting at ${start:04X}.",
			values.len()
		);
	}

	fn print_breakpoints(&self) {
		if self.breakpoints.is_empty()
			&& self.cycle_breakpoints.is_empty()
			&& self.raster_breakpoints.is_empty()
		{
			println!("No breakpoints.");
			return;
		}
		for b in &self.breakpoints {
			println!(
				"{}: {:?} ${:04X}-${:04X}{} enabled={}",
				b.id,
				b.kind,
				b.start,
				b.end,
				b.value.map(|v| format!(" = ${v:02X}")).unwrap_or_default(),
				b.enabled
			);
		}
		for b in &self.cycle_breakpoints {
			println!("{}: Cycle {} enabled={}", b.id, b.cycle, b.enabled);
		}
		for b in &self.raster_breakpoints {
			println!(
				"{}: Raster {}{} enabled={}",
				b.id,
				b.line,
				b.cycle.map(|v| format!(":{v}")).unwrap_or_default(),
				b.enabled
			);
		}
	}

	fn print_history(&self, count: usize) {
		for e in self.history.recent(count) {
			println!(
				"{:>12} R={:03}:{:02} PC=${:04X} OP=${:02X} A={:02X} X={:02X} Y={:02X} SP={:02X} P={:02X} {:?} IRQ={} NMI={} BA={} AEC={} $01={:02X} G={} E={} B={} C1={:04X}/{:04X}/{:02X}/{:02X}/{} C2={:04X}/{:04X}/{:02X}/{:02X}/{}",
				e.cycle,
				e.raster_line,
				e.raster_cycle,
				e.pc,
				e.opcode,
				e.a,
				e.x,
				e.y,
				e.sp,
				e.p,
				e.state,
				e.irq as u8,
				e.nmi as u8,
				e.ba_low as u8,
				e.aec_low as u8,
				e.port,
				e.game as u8,
				e.exrom as u8,
				e.bank,
				e.cia1_ta,
				e.cia1_tb,
				e.cia1_icr,
				e.cia1_mask,
				e.cia1_irq as u8,
				e.cia2_ta,
				e.cia2_tb,
				e.cia2_icr,
				e.cia2_mask,
				e.cia2_irq as u8
			);
		}
	}

	fn dump_history(&self, path: &std::path::Path, count: usize) {
		match self.history.dump(path, count) {
			Ok(written) => println!("Dumped {written} history entries to {}.", path.display()),
			Err(error) => println!("History dump failed: {error}"),
		}
	}

	fn auto_dump_history(&self) {
		if let Some((path, count)) = &self.history_auto {
			if let Err(error) = self.history.dump(path, *count) {
				eprintln!("Automatic history dump failed: {error}");
			}
		}
	}

	fn print_history_status(&self) {
		let auto = self
			.history_auto
			.as_ref()
			.map(|(path, count)| format!("{} ({count})", path.display()))
			.unwrap_or_else(|| "off".into());
		println!(
			"History retained={}/{} auto={}",
			self.history.len(),
			self.history.capacity(),
			auto
		);
	}

	fn print_vic(&self, context: &AppContext) {
		let v = &context.machine.vic;
		println!(
			"VIC raster={}:{} BA={} AEC={} IRQ flags=${:02X} enable=${:02X} sprite_dma=${:02X} sprite_display=${:02X}",
			v.timing.raster_line,
			v.timing.cycle,
			v.ba_low as u8,
			v.aec_low as u8,
			v.irq.flags,
			v.irq.enable,
			v.sprites.sprite_dma,
			v.sprites.sprite_display
		);
	}

	fn print_cia(&self, context: &AppContext, index: u8) {
		let cia = if index == 1 {
			&context.machine.memory.cia1.inner
		} else {
			&context.machine.memory.cia2.inner
		};
		println!(
			"CIA{} TA=${:04X} latch=${:04X} TB=${:04X} latch=${:04X} ICR=${:02X} mask=${:02X} IRQ={} stages=${:02X}/${:02X} PRA=${:02X} PRB=${:02X} DDRA=${:02X} DDRB=${:02X} CNT={} SP={} FLAG={}",
			index,
			cia.ta.counter,
			cia.ta.latch,
			cia.tb.counter,
			cia.tb.latch,
			cia.icr,
			cia.icr_mask,
			cia.irq_line as u8,
			cia.irq_stage_now,
			cia.irq_stage_next,
			cia.pra,
			cia.prb,
			cia.ddra,
			cia.ddrb,
			cia.cnt_pin as u8,
			cia.sp_pin as u8,
			cia.flag_pin as u8
		);
	}

	fn print_sid(&self, context: &AppContext) {
		let sid = &context.machine.memory.sid;
		println!(
			"SID POTX=${:02X} POTY=${:02X}",
			sid.pot_x, sid.pot_y
		);
		for i in 0..3 {
			println!(
				"  voice{} accumulator=${:06X} envelope=${:02X}",
				i + 1,
				sid.oscillators[i].accumulator,
				sid.envelopes[i].volume
			);
		}
	}

	/* Binary dumps contain exactly the inclusive address interval and no load-address
	 * prefix.  The selected visibility mode is identical to mem or ram, allowing a
	 * displayed range and its saved representation to be compared byte for byte. */
	fn save_memory(
		&self,
		context: &AppContext,
		path: &std::path::Path,
		start: u16,
		end: u16,
		physical: bool,
	) -> io::Result<()> {
		let mut file = File::create(path)?;
		let mut addr = start;
		loop {
			let value = if physical {
				context.machine.memory.read_ram(addr)
			} else {
				context.machine.memory.debug_peek(
					addr,
					context.machine.current_cycle(),
					&context.machine.vic,
				)
			};
			file.write_all(&[value])?;
			if addr == end {
				break;
			}
			addr = addr.wrapping_add(1);
		}
		file.flush()
	}
}

impl Drop for Debugger {
	fn drop(&mut self) {
		self.auto_dump_history();
		let _ = self.trace.stop();
	}
}

/* Prompt flushing is explicit because standard output is not guaranteed to be
 * line-buffered when Breadbin466 is launched from a build script. */
fn print_prompt() {
	print!("(breadbin) ");
	let _ = io::stdout().flush();
}

/* Help describes the stable command surface only; implementation-specific
 * aliases remain accepted by the parser but do not obscure the primary syntax. */
fn print_help() {
	println!(
		"Breadbin466 monitor commands:\n\
EXECUTION\n\
  run|c                         Continue execution\n\
  pause                         Pause execution\n\
  step [N]                      Step N instructions\n\
  step-cycle [N]                Step N machine cycles\n\
  regs                          CPU registers and lines\n\
  set REG VALUE                 Set A/X/Y/SP/P/PC while paused\n\
\nMEMORY / CODE\n\
  mem START [END]               CPU-visible, non-destructive memory\n\
  ram START [END]               Physical RAM\n\
  poke START BYTE [BYTE ...]    Write physical RAM\n\
  disasm START [COUNT]          Disassemble current CPU mapping\n\
  save FILE START END [ram]     Binary dump, CPU-visible or physical RAM\n\
\nBREAKPOINTS / WATCHPOINTS\n\
  break START [END]             Execute breakpoint\n\
  watchr START [END]            Blocking read watchpoint\n\
  watchw START [END] [=VALUE]   Blocking write watchpoint\n\
  break-cycle CYCLE             Stop at an absolute machine cycle\n\
  break-after CYCLES            Stop after a relative cycle count\n\
  break-raster LINE [CYCLE]     Stop at PAL raster position\n\
  breaks                        List all stopping rules\n\
  enable ID | disable ID        Toggle a stopping rule\n\
  delete ID | clear-breaks      Delete stopping rules\n\
\nINSTRUCTION HISTORY\n\
  history [N]                   Show recent instructions\n\
  history capacity N            Resize the bounded ring buffer\n\
  history clear                 Clear retained history\n\
  history dump FILE [N]         Export N entries, 0/all if omitted\n\
  history auto FILE [N]         Auto-export on breakpoint, quit or exit\n\
  history auto-off              Disable automatic export\n\
  history status                Ring and auto-dump status\n\
\nNON-BLOCKING UNIFIED TRACE\n\
  trace file FILE               Select output file\n\
  trace exec START [END] [=OP]  Trace instruction execution\n\
  trace read START [END] [=V]   Trace CPU reads\n\
  trace write START [END] [=V]  Trace CPU writes\n\
  tracee / tracer / tracew ...  Short forms for exec/read/write\n\
  trace list                    List trace rules\n\
  trace enable ID               Enable a trace rule\n\
  trace disable ID              Disable a trace rule\n\
  trace delete ID               Delete a trace rule\n\
  trace clear                   Clear trace rules\n\
  trace on|off|flush|status     Control recording\n\
\nHARDWARE INSPECTION\n\
  vic                           VIC-II timing/DMA/IRQ state\n\
  cia1 | cia2                   CIA timers, IRQ pipeline and pins\n\
  sid                           SID clock/voice state\n\
  reu                           REU state\n\
  cart                          Cartridge mapper/lines/bank\n\
  pla|map                       Current CPU memory mapping\n\
\nAUTOMATION\n\
  --monitor-commands 'A; B; C'  Run monitor commands from command-line\n\
  --monitor-command 'A'         Repeatable single startup command\n\
\n  quit                          Flush trace/history and exit\n\
\nNumbers accept decimal, $hex or 0xhex. Address ranges are inclusive."
	);
}