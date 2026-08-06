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

use std::fs::File;
use std::io::{self, BufRead, Write};
use std::sync::mpsc::{self, Receiver};
use std::thread;

use crate::emulator::context::AppContext;
use crate::motherboard::DebugBusAccessKind;
use super::debugger_breakpoint::{AccessKind, Breakpoint};
use super::debugger_command::{self, DebugCommand};
use super::debugger_disassembly::{format_instruction, instruction_length};
use super::debugger_history::{DebugHistory, HistoryEntry};

/* Instruction history is intentionally bounded.  Two hundred thousand entries
 * retain useful lead-up context while preventing an unattended debugger from
 * becoming an unbounded trace recorder. */
const HISTORY_CAPACITY: usize = 200_000;

/* Debugger owns only observation policy and interactive control state.  paused
 * gates advancement in the emulator loop; the two step counters temporarily
 * permit bounded progress without changing the CPU's own state machine. */
pub struct Debugger {
	receiver: Receiver<String>,
	breakpoints: Vec<Breakpoint>,
	next_breakpoint_id: u32,
	history: DebugHistory,
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
		let (sender, receiver) = mpsc::channel();
		thread::Builder::new().name("breadbin-debugger-console".into()).spawn(move || {
			let stdin = io::stdin();
			for line in stdin.lock().lines() {
				match line {
					Ok(line) => {
						if sender.send(line).is_err() { break; }
					}
					Err(_) => break,
				}
			}
		}).expect("Unable to start debugger console thread");
		println!("Breadbin466 debugger ready. Type 'help'.");
		print_prompt();
		Self {
			receiver,
			breakpoints: Vec::new(),
			next_breakpoint_id: 1,
			history: DebugHistory::new(HISTORY_CAPACITY),
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
		while let Ok(line) = self.receiver.try_recv() {
			match debugger_command::parse(&line) {
				Ok(command) => self.execute(command, context),
				Err(error) if error.is_empty() => {}
				Err(error) => println!("{error}"),
			}
			if self.paused { print_prompt(); }
		}
	}

	/* before_cycle is the sole execution gate.  Returning false means no emulated
	 * component advances.  Execute breakpoints are evaluated only when t_state is
	 * zero, before the opcode at PC begins, and history records that same boundary
	 * so the retained entry describes the instruction about to execute. */
	pub fn before_cycle(&mut self, context: &mut AppContext) -> bool {
		self.poll_commands(context);
		if self.quit_requested { std::process::exit(0); }
		if self.paused && self.step_cycles == 0 && self.step_instructions == 0 { return false; }
		let machine = &context.machine;
		let boundary = machine.cpu.t_state == 0;
		if boundary {
			let opcode = machine.memory.debug_peek(machine.cpu.pc, machine.current_cycle(), &machine.vic);
			self.history.push(HistoryEntry {
				cycle: machine.current_cycle(), pc: machine.cpu.pc, opcode,
				a: machine.cpu.a, x: machine.cpu.x, y: machine.cpu.y, sp: machine.cpu.sp, p: machine.cpu.p,
				state: machine.cpu.state, port: machine.cpu.port.get_pins(),
				game: machine.memory.cartridge.game, exrom: machine.memory.cartridge.exrom,
				bank: machine.memory.cartridge.mapper.get_debug_bank(),
			});
			if self.breakpoints.iter().any(|b| b.matches(AccessKind::Execute, machine.cpu.pc, Some(opcode))) {
				self.paused = true;
				println!("Breakpoint at ${:04X}, cycle {}.", machine.cpu.pc, machine.current_cycle());
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
			let kind = match access.kind { DebugBusAccessKind::Read => AccessKind::Read, DebugBusAccessKind::Write => AccessKind::Write };
			if self.breakpoints.iter().any(|b| b.matches(kind, access.addr, Some(access.value))) {
				self.paused = true;
				println!("{:?} watchpoint at ${:04X}, value ${:02X}, cycle {}.", kind, access.addr, access.value, access.cycle);
				self.print_registers(context);
				print_prompt();
			}
		}
		if self.step_cycles > 0 {
			self.step_cycles -= 1;
			if self.step_cycles == 0 { self.paused = true; self.print_registers(context); print_prompt(); }
		}
		if self.step_instructions > 0 && !self.last_instruction_boundary && context.machine.cpu.t_state == 0 {
			self.step_instructions -= 1;
			if self.step_instructions == 0 { self.paused = true; self.print_registers(context); print_prompt(); }
		}
	}

	/* Typed commands are executed synchronously against a stable machine state.
	 * Inspection commands are observational; run and step commands only change the
	 * debugger's execution gate, while quit requests an orderly termination at the
	 * next polling point. */
	fn execute(&mut self, command: DebugCommand, context: &mut AppContext) {
		match command {
			DebugCommand::Help => print_help(),
			DebugCommand::Run => { self.paused = false; self.step_cycles = 0; self.step_instructions = 0; println!("Running."); },
			DebugCommand::Pause => { self.paused = true; self.print_registers(context); },
			DebugCommand::StepCycle(count) => { self.paused = false; self.step_cycles = count.max(1); self.step_instructions = 0; },
			DebugCommand::StepInstruction(count) => { self.paused = false; self.step_instructions = count.max(1); self.step_cycles = 0; },
			DebugCommand::Registers => self.print_registers(context),
			DebugCommand::Memory { start, end, physical } => self.print_memory(context, start, end, physical),
			DebugCommand::Disassemble { start, count } => self.disassemble(context, start, count),
			DebugCommand::AddBreakpoint { kind, start, end, value } => {
				let id = self.next_breakpoint_id; self.next_breakpoint_id += 1;
				self.breakpoints.push(Breakpoint { id, kind, start, end, value, enabled: true });
				println!("Breakpoint {id}: {kind:?} ${start:04X}-${end:04X}{}", value.map(|v| format!(" = ${v:02X}")).unwrap_or_default());
			}
			DebugCommand::DeleteBreakpoint(id) => { self.breakpoints.retain(|b| b.id != id); },
			DebugCommand::ListBreakpoints => for b in &self.breakpoints { println!("{}: {:?} ${:04X}-${:04X} value={:?} enabled={}", b.id, b.kind, b.start, b.end, b.value, b.enabled); },
			DebugCommand::History(count) => for e in self.history.recent(count) { println!("{:>12} PC=${:04X} OP=${:02X} A={:02X} X={:02X} Y={:02X} SP={:02X} P={:02X} {:?} $01={:02X} G={} E={} B={}", e.cycle, e.pc, e.opcode, e.a, e.x, e.y, e.sp, e.p, e.state, e.port, e.game as u8, e.exrom as u8, e.bank); },
			DebugCommand::Reu => self.print_reu(context),
			DebugCommand::Cartridge => self.print_cartridge(context),
			DebugCommand::Pla => self.print_pla(context),
			DebugCommand::Save { path, start, end, physical } => match self.save_memory(context, &path, start, end, physical) { Ok(()) => println!("Saved {} bytes to {}.", usize::from(end.wrapping_sub(start)) + 1, path.display()), Err(e) => println!("Save failed: {e}") },
			DebugCommand::Quit => self.quit_requested = true,
		}
	}

	/* Register output includes the CPU micro-state and external interrupt levels so
	 * a stop in the middle of an instruction remains interpretable. */
	fn print_registers(&self, context: &AppContext) {
		let m = &context.machine;
		println!("CYCLE={} PC=${:04X} IR=${:02X} T={} A=${:02X} X=${:02X} Y=${:02X} SP=${:02X} P=${:02X} STATE={:?} IRQ={} NMI={} $01=${:02X}", m.current_cycle(), m.cpu.pc, m.cpu.ir, m.cpu.t_state, m.cpu.a, m.cpu.x, m.cpu.y, m.cpu.sp, m.cpu.p, m.cpu.state, m.cpu.irq_line as u8, (!m.cpu.nmi_line) as u8, m.cpu.port.get_pins());
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
				if current > end { break; }
				let value = if physical { context.machine.memory.read_ram(current) } else { context.machine.memory.debug_peek(current, context.machine.current_cycle(), &context.machine.vic) };
				print!("{value:02X} ");
			}
			println!();
			if end.wrapping_sub(addr) < 16 { break; }
			addr = addr.wrapping_add(16);
		}
	}
	/* Disassembly follows current CPU visibility and never performs ordinary device
	 * reads.  It is therefore a view of what the processor would fetch under the
	 * present PLA state, not a scan of physical RAM. */
	fn disassemble(&self, context: &AppContext, mut pc: u16, count: usize) {
		for _ in 0..count {
			let op = context.machine.memory.debug_peek(pc, context.machine.current_cycle(), &context.machine.vic);
			let len = instruction_length(op);
			let mut bytes = [0u8; 3];
			for i in 0..len { bytes[i as usize] = context.machine.memory.debug_peek(pc.wrapping_add(i), context.machine.current_cycle(), &context.machine.vic); }
			println!("${pc:04X}: {:02X} {:02X} {:02X}  {}", bytes[0], bytes[1], bytes[2], format_instruction(pc, &bytes));
			pc = pc.wrapping_add(len);
		}
	}
	/* REU output uses explicit debug accessors so register display does not clear
	 * status flags or otherwise emulate a CPU register read. */
	fn print_reu(&self, context: &AppContext) {
		let r = &context.machine.memory.reu;
		print!("REU enabled={} dma={} irq={} regs:", r.enabled, r.dma_active(), r.irq_pending);
		for i in 0..=10 { print!(" {:02X}", r.debug_register(i)); }
		println!();
	}
	/* Cartridge state reports the mapper's logical bank and expansion-port lines;
	 * it does not force a ROM read or advance flash command state. */
	fn print_cartridge(&self, context: &AppContext) {
		let c = &context.machine.memory.cartridge;
		let info = c.mapper.get_info();
		println!("Cartridge {:?} '{}' GAME={} EXROM={} NMI={} bank={}/{}", info.mapper_type, c.crt_name, c.game as u8, c.exrom as u8, c.nmi_low as u8, c.mapper.get_debug_bank(), c.mapper.get_max_bank());
	}
	/* The PLA summary samples representative windows rather than claiming a static
	 * whole-address-space map.  It is intended to explain the current CPU view of
	 * the principal ROM, cartridge and I/O regions. */
	fn print_pla(&self, context: &AppContext) {
		let m = &context.machine;
		println!("PLA $01=${:02X} GAME={} EXROM={} bank={}", m.cpu.port.get_pins(), m.memory.cartridge.game as u8, m.memory.cartridge.exrom as u8, m.memory.cartridge.mapper.get_debug_bank());
		for addr in [0x8000u16, 0xA000, 0xD000, 0xE000] { println!("${addr:04X}: {:?}", m.memory.debug_region(addr)); }
	}
	/* Binary dumps contain exactly the inclusive address interval and no load-address
	 * prefix.  The selected visibility mode is identical to mem or ram, allowing a
	 * displayed range and its saved representation to be compared byte for byte. */
	fn save_memory(&self, context: &AppContext, path: &std::path::Path, start: u16, end: u16, physical: bool) -> io::Result<()> {
		let mut file = File::create(path)?;
		let mut addr = start;
		loop {
			let value = if physical { context.machine.memory.read_ram(addr) } else { context.machine.memory.debug_peek(addr, context.machine.current_cycle(), &context.machine.vic) };
			file.write_all(&[value])?;
			if addr == end { break; }
			addr = addr.wrapping_add(1);
		}
		file.flush()
	}
}

/* Prompt flushing is explicit because standard output is not guaranteed to be
 * line-buffered when Breadbin466 is launched from a build script. */
fn print_prompt() { print!("(breadbin) "); let _ = io::stdout().flush(); }

/* Help describes the stable command surface only; implementation-specific
 * aliases remain accepted by the parser but do not obscure the primary syntax. */
fn print_help() {
	println!("Commands:\n  run|c                 Continue execution\n  pause                 Pause execution\n  step [N]              Step N instructions\n  step-cycle [N]        Step N machine cycles\n  regs                   CPU registers\n  mem START [END]        CPU-visible memory\n  ram START [END]        Physical RAM\n  disasm START [COUNT]   Disassemble\n  break START [END]      Execute breakpoint\n  watchr START [END]     Read watchpoint\n  watchw START [END] [=VALUE] Write watchpoint\n  breaks                 List breakpoints\n  delete ID              Delete breakpoint\n  history [N]            Recent instruction history\n  reu|cart|pla           Hardware state\n  save FILE START END [ram] Dump binary memory\n  quit                   Exit emulator\nNumbers accept decimal, $hex or 0xhex.");
}