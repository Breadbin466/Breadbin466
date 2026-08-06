/*
 * Breadbin466 interactive debugger: command representation and parser.
 *
 * The console thread transports complete text lines only.  Parsing is performed
 * on the emulator thread so command execution never races the machine.  The
 * parser accepts a deliberately small grammar: whitespace separates tokens,
 * numeric values use decimal, $ hexadecimal or 0x hexadecimal notation, and a
 * write-watchpoint value filter is introduced by '='.  Commands are converted
 * into typed values before they can inspect or modify debugger state.
 */

use std::path::PathBuf;
use super::debugger_breakpoint::AccessKind;

/* DebugCommand is the boundary between textual input and debugger behaviour.
 * Keeping paths, ranges and counts typed here prevents individual command
 * handlers from repeating validation or interpreting partially parsed text. */
#[derive(Debug)]
pub enum DebugCommand {
	Help, Run, Pause, StepCycle(u64), StepInstruction(u64), Registers,
	Memory { start: u16, end: u16, physical: bool }, Disassemble { start: u16, count: usize },
	AddBreakpoint { kind: AccessKind, start: u16, end: u16, value: Option<u8> },
	DeleteBreakpoint(u32), ListBreakpoints, History(usize), Reu, Cartridge, Pla,
	Save { path: PathBuf, start: u16, end: u16, physical: bool }, Quit,
}

/* Empty input is reported as an empty error so the interactive loop can ignore a
 * blank line without printing a diagnostic.  Every non-empty invalid command
 * returns a user-facing explanation and leaves emulation state unchanged. */
pub fn parse(line: &str) -> Result<DebugCommand, String> {
	let parts: Vec<&str> = line.split_whitespace().collect();
	let Some(command) = parts.first().copied() else { return Err(String::new()); };
	match command.to_ascii_lowercase().as_str() {
		"h" | "help" | "?" => Ok(DebugCommand::Help),
		"r" | "run" | "continue" | "c" => Ok(DebugCommand::Run),
		"pause" | "stop" => Ok(DebugCommand::Pause),
		"sc" | "step-cycle" => Ok(DebugCommand::StepCycle(number(parts.get(1).copied()).unwrap_or(1) as u64)),
		"s" | "step" | "step-instruction" => Ok(DebugCommand::StepInstruction(number(parts.get(1).copied()).unwrap_or(1) as u64)),
		"reg" | "regs" => Ok(DebugCommand::Registers),
		"m" | "mem" => range_command(&parts, false),
		"ram" => range_command(&parts, true),
		"d" | "dis" | "disasm" => {
			let start = address(required(&parts, 1)?)?;
			let count = parts.get(2).map(|v| number(Some(v))).transpose()?.unwrap_or(16) as usize;
			Ok(DebugCommand::Disassemble { start, count })
		}
		"break" | "b" => breakpoint_command(&parts, AccessKind::Execute),
		"watchr" => breakpoint_command(&parts, AccessKind::Read),
		"watchw" => breakpoint_command(&parts, AccessKind::Write),
		"delete" | "del" => Ok(DebugCommand::DeleteBreakpoint(number(Some(required(&parts, 1)?))? as u32)),
		"breaks" | "bl" => Ok(DebugCommand::ListBreakpoints),
		"history" | "hist" => Ok(DebugCommand::History(parts.get(1).map(|v| number(Some(v))).transpose()?.unwrap_or(32) as usize)),
		"reu" => Ok(DebugCommand::Reu),
		"cart" | "cartridge" => Ok(DebugCommand::Cartridge),
		"pla" => Ok(DebugCommand::Pla),
		"save" | "savebin" => {
			let path = PathBuf::from(required(&parts, 1)?);
			let start = address(required(&parts, 2)?)?;
			let end = address(required(&parts, 3)?)?;
			let physical = parts.get(4).is_some_and(|v| *v == "ram");
			Ok(DebugCommand::Save { path, start, end, physical })
		}
		"q" | "quit" | "exit" => Ok(DebugCommand::Quit),
		_ => Err(format!("Unknown debugger command: {command}")),
	}
}

/* Memory commands default to a 128-byte window.  wrapping_add matches the CPU's
 * 16-bit address space and lets a range intentionally cross $FFFF. */
fn range_command(parts: &[&str], physical: bool) -> Result<DebugCommand, String> {
	let start = address(required(parts, 1)?)?;
	let end = parts.get(2).map(|v| address(v)).transpose()?.unwrap_or(start.wrapping_add(0x7F));
	Ok(DebugCommand::Memory { start, end, physical })
}
/* The second positional argument is an end address unless it begins with '='.
 * A value filter is meaningful for bus accesses and is retained for all kinds
 * so the representation remains uniform. */
fn breakpoint_command(parts: &[&str], kind: AccessKind) -> Result<DebugCommand, String> {
	let start = address(required(parts, 1)?)?;
	let end = parts.get(2).filter(|v| !v.starts_with('=')).map(|v| address(v)).transpose()?.unwrap_or(start);
	let value = parts.iter().find_map(|v| v.strip_prefix('=')).map(address).transpose()?.map(|v| v as u8);
	Ok(DebugCommand::AddBreakpoint { kind, start, end, value })
}
/* Required arguments fail before any command object is constructed. */
fn required<'a>(parts: &'a [&str], index: usize) -> Result<&'a str, String> { parts.get(index).copied().ok_or_else(|| "Missing argument.".into()) }
/* Address conversion rejects values outside the physical 16-bit CPU address
 * space instead of truncating them. */
fn address(value: &str) -> Result<u16, String> { number(Some(value)).and_then(|v| u16::try_from(v).map_err(|_| format!("Address out of range: {value}"))) }
/* Numeric parsing is shared by addresses, counts and breakpoint identifiers.
 * No sign or arithmetic expression is accepted at this layer. */
fn number(value: Option<&str>) -> Result<u64, String> {
	let value = value.ok_or_else(|| "Missing number.".to_string())?;
	let (radix, digits) = if let Some(v) = value.strip_prefix('$') { (16, v) } else if let Some(v) = value.strip_prefix("0x") { (16, v) } else { (10, value) };
	u64::from_str_radix(digits, radix).map_err(|_| format!("Invalid number: {value}"))
}