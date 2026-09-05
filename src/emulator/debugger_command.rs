// =======================================================
// src/emulator/debugger_command.rs — Debugger command representation and parser
// =======================================================

use super::debugger_breakpoint::AccessKind;
use std::path::PathBuf;

#[derive(Debug)]
pub enum DebugCommand {
	Help,
	Run,
	Pause,
	StepCycle(u64),
	StepInstruction(u64),
	Registers,
	SetRegister {
		name: String,
		value: u64,
	},
	Memory {
		start: u16,
		end: u16,
		physical: bool,
	},
	PokeRam {
		start: u16,
		values: Vec<u8>,
	},
	Disassemble {
		start: u16,
		count: usize,
	},
	AddBreakpoint {
		kind: AccessKind,
		start: u16,
		end: u16,
		value: Option<u8>,
	},
	AddCycleBreakpoint(u64),
	AddCycleAfter(u64),
	AddRasterBreakpoint {
		line: u16,
		cycle: Option<u16>,
	},
	DeleteBreakpoint(u32),
	ListBreakpoints,
	EnableBreakpoint {
		id: u32,
		enabled: bool,
	},
	ClearBreakpoints,
	History(usize),
	HistoryCapacity(usize),
	HistoryClear,
	HistoryDump {
		path: PathBuf,
		count: usize,
	},
	HistoryAuto {
		path: PathBuf,
		count: usize,
	},
	HistoryAutoOff,
	HistoryStatus,
	Reu,
	Cartridge,
	Pla,
	Vic,
	Cia(u8),
	Sid,
	TraceFile(PathBuf),
	AddTrace {
		kind: AccessKind,
		start: u16,
		end: u16,
		value: Option<u8>,
	},
	DeleteTrace(u32),
	EnableTrace {
		id: u32,
		enabled: bool,
	},
	ListTraces,
	TraceStart,
	TraceStop,
	TraceFlush,
	TraceClear,
	TraceStatus,
	Save {
		path: PathBuf,
		start: u16,
		end: u16,
		physical: bool,
	},
	Quit,
}

pub fn parse(line: &str) -> Result<DebugCommand, String> {
	let parts: Vec<&str> = line.split_whitespace().collect();
	let Some(command) = parts.first().copied() else {
		return Err(String::new());
	};
	match command.to_ascii_lowercase().as_str() {
		"h" | "help" | "?" => Ok(DebugCommand::Help),
		"r" | "run" | "continue" | "c" => Ok(DebugCommand::Run),
		"pause" | "stop" => Ok(DebugCommand::Pause),
		"sc" | "step-cycle" => Ok(DebugCommand::StepCycle(
			number(parts.get(1).copied()).unwrap_or(1),
		)),
		"s" | "step" | "step-instruction" => Ok(DebugCommand::StepInstruction(
			number(parts.get(1).copied()).unwrap_or(1),
		)),
		"reg" | "regs" => Ok(DebugCommand::Registers),
		"set" => Ok(DebugCommand::SetRegister {
			name: required(&parts, 1)?.to_ascii_lowercase(),
			value: number(Some(required(&parts, 2)?))?,
		}),
		"m" | "mem" => range_command(&parts, false),
		"ram" => range_command(&parts, true),
		"poke" | "pokeram" => {
			let start = address(required(&parts, 1)?)?;
			if parts.len() < 3 {
				return Err("poke requires at least one byte.".into());
			}
			let mut values = Vec::new();
			for value in &parts[2..] {
				values.push(byte(value)?);
			}
			Ok(DebugCommand::PokeRam { start, values })
		}
		"d" | "dis" | "disasm" => {
			let start = address(required(&parts, 1)?)?;
			let count = parts
				.get(2)
				.map(|v| number(Some(v)))
				.transpose()?
				.unwrap_or(16) as usize;
			Ok(DebugCommand::Disassemble { start, count })
		}
		"break" | "b" => breakpoint_command(&parts, AccessKind::Execute),
		"watchr" => breakpoint_command(&parts, AccessKind::Read),
		"watchw" => breakpoint_command(&parts, AccessKind::Write),
		"break-cycle" => Ok(DebugCommand::AddCycleBreakpoint(number(Some(required(
			&parts, 1,
		)?))?)),
		"break-after" => Ok(DebugCommand::AddCycleAfter(number(Some(required(
			&parts, 1,
		)?))?)),
		"break-raster" => {
			let line = number(Some(required(&parts, 1)?))?;
			if line > 311 {
				return Err("PAL raster line must be 0..311.".into());
			}
			let cycle = parts
				.get(2)
				.map(|v| number(Some(v)))
				.transpose()?
				.map(|v| v as u16);
			if cycle.is_some_and(|v| v > 62) {
				return Err("PAL raster cycle must be 0..62.".into());
			}
			Ok(DebugCommand::AddRasterBreakpoint {
				line: line as u16,
				cycle,
			})
		}
		"delete" | "del" => Ok(DebugCommand::DeleteBreakpoint(
			number(Some(required(&parts, 1)?))? as u32,
		)),
		"enable" => Ok(DebugCommand::EnableBreakpoint {
			id: number(Some(required(&parts, 1)?))? as u32,
			enabled: true,
		}),
		"disable" => Ok(DebugCommand::EnableBreakpoint {
			id: number(Some(required(&parts, 1)?))? as u32,
			enabled: false,
		}),
		"breaks" | "bl" => Ok(DebugCommand::ListBreakpoints),
		"clear-breaks" => Ok(DebugCommand::ClearBreakpoints),
		"history" | "hist" => history_command(&parts),
		"reu" => Ok(DebugCommand::Reu),
		"cart" | "cartridge" => Ok(DebugCommand::Cartridge),
		"pla" | "map" => Ok(DebugCommand::Pla),
		"vic" => Ok(DebugCommand::Vic),
		"cia1" => Ok(DebugCommand::Cia(1)),
		"cia2" => Ok(DebugCommand::Cia(2)),
		"sid" => Ok(DebugCommand::Sid),
		"tracer" => trace_range_command(&parts, AccessKind::Read),
		"tracew" => trace_range_command(&parts, AccessKind::Write),
		"tracee" => trace_range_command(&parts, AccessKind::Execute),
		"trace" => trace_command(&parts),
		"save" | "savebin" => {
			let path = PathBuf::from(required(&parts, 1)?);
			let start = address(required(&parts, 2)?)?;
			let end = address(required(&parts, 3)?)?;
			let physical = parts.get(4).is_some_and(|v| v.eq_ignore_ascii_case("ram"));
			Ok(DebugCommand::Save {
				path,
				start,
				end,
				physical,
			})
		}
		"q" | "quit" | "exit" => Ok(DebugCommand::Quit),
		_ => Err(format!("Unknown debugger command: {command}")),
	}
}

fn history_command(parts: &[&str]) -> Result<DebugCommand, String> {
	let Some(sub) = parts.get(1) else {
		return Ok(DebugCommand::History(32));
	};
	match sub.to_ascii_lowercase().as_str() {
		"capacity" | "size" => Ok(DebugCommand::HistoryCapacity(
			number(Some(required(parts, 2)?))? as usize,
		)),
		"clear" => Ok(DebugCommand::HistoryClear),
		"dump" => Ok(DebugCommand::HistoryDump {
			path: PathBuf::from(required(parts, 2)?),
			count: parts
				.get(3)
				.map(|v| number(Some(v)))
				.transpose()?
				.unwrap_or(0) as usize,
		}),
		"auto" => Ok(DebugCommand::HistoryAuto {
			path: PathBuf::from(required(parts, 2)?),
			count: parts
				.get(3)
				.map(|v| number(Some(v)))
				.transpose()?
				.unwrap_or(0) as usize,
		}),
		"auto-off" | "noauto" => Ok(DebugCommand::HistoryAutoOff),
		"status" => Ok(DebugCommand::HistoryStatus),
		_ => Ok(DebugCommand::History(number(Some(sub))? as usize)),
	}
}

fn trace_command(parts: &[&str]) -> Result<DebugCommand, String> {
	let subcommand = required(parts, 1)?.to_ascii_lowercase();
	match subcommand.as_str() {
		"file" => Ok(DebugCommand::TraceFile(PathBuf::from(required(parts, 2)?))),
		"add" => {
			let kind = access_kind(required(parts, 2)?)?;
			trace_range_command(&parts[2..], kind)
		}
		"exec" | "execute" => trace_range_command(&parts[1..], AccessKind::Execute),
		"read" => trace_range_command(&parts[1..], AccessKind::Read),
		"write" => trace_range_command(&parts[1..], AccessKind::Write),
		"delete" | "del" => Ok(DebugCommand::DeleteTrace(
			number(Some(required(parts, 2)?))? as u32,
		)),
		"enable" => Ok(DebugCommand::EnableTrace {
			id: number(Some(required(parts, 2)?))? as u32,
			enabled: true,
		}),
		"disable" => Ok(DebugCommand::EnableTrace {
			id: number(Some(required(parts, 2)?))? as u32,
			enabled: false,
		}),
		"list" | "ls" => Ok(DebugCommand::ListTraces),
		"on" | "start" => Ok(DebugCommand::TraceStart),
		"off" | "stop" => Ok(DebugCommand::TraceStop),
		"flush" => Ok(DebugCommand::TraceFlush),
		"clear" => Ok(DebugCommand::TraceClear),
		"status" => Ok(DebugCommand::TraceStatus),
		_ => Err(format!("Unknown trace command: {subcommand}")),
	}
}

fn access_kind(value: &str) -> Result<AccessKind, String> {
	match value.to_ascii_lowercase().as_str() {
		"e" | "exec" | "execute" => Ok(AccessKind::Execute),
		"r" | "read" => Ok(AccessKind::Read),
		"w" | "write" => Ok(AccessKind::Write),
		other => Err(format!("Unknown access kind: {other}")),
	}
}

fn trace_range_command(parts: &[&str], kind: AccessKind) -> Result<DebugCommand, String> {
	let start = address(required(parts, 1)?)?;
	let end = parts
		.get(2)
		.filter(|v| !v.starts_with('='))
		.map(|v| address(v))
		.transpose()?
		.unwrap_or(start);
	let value = parts
		.iter()
		.find_map(|v| v.strip_prefix('='))
		.map(byte)
		.transpose()?;
	Ok(DebugCommand::AddTrace {
		kind,
		start,
		end,
		value,
	})
}

fn range_command(parts: &[&str], physical: bool) -> Result<DebugCommand, String> {
	let start = address(required(parts, 1)?)?;
	let end = parts
		.get(2)
		.map(|v| address(v))
		.transpose()?
		.unwrap_or(start.wrapping_add(0x7F));
	Ok(DebugCommand::Memory {
		start,
		end,
		physical,
	})
}

fn breakpoint_command(parts: &[&str], kind: AccessKind) -> Result<DebugCommand, String> {
	let start = address(required(parts, 1)?)?;
	let end = parts
		.get(2)
		.filter(|v| !v.starts_with('='))
		.map(|v| address(v))
		.transpose()?
		.unwrap_or(start);
	let value = parts
		.iter()
		.find_map(|v| v.strip_prefix('='))
		.map(byte)
		.transpose()?;
	Ok(DebugCommand::AddBreakpoint {
		kind,
		start,
		end,
		value,
	})
}

fn required<'a>(parts: &'a [&str], index: usize) -> Result<&'a str, String> {
	parts
		.get(index)
		.copied()
		.ok_or_else(|| "Missing argument.".into())
}
fn address(value: &str) -> Result<u16, String> {
	number(Some(value))
		.and_then(|v| u16::try_from(v).map_err(|_| format!("Address out of range: {value}")))
}
fn byte(value: &str) -> Result<u8, String> {
	number(Some(value))
		.and_then(|v| u8::try_from(v).map_err(|_| format!("Byte out of range: {value}")))
}
fn number(value: Option<&str>) -> Result<u64, String> {
	let value = value.ok_or_else(|| "Missing number.".to_string())?;
	let (radix, digits) = if let Some(v) = value.strip_prefix('$') {
		(16, v)
	} else if let Some(v) = value.strip_prefix("0x") {
		(16, v)
	} else {
		(10, value)
	};
	u64::from_str_radix(digits, radix).map_err(|_| format!("Invalid number: {value}"))
}