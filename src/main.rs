// =======================================================
// src/main.rs — Application Entry Point and Top-Level Modules
// =======================================================

#![windows_subsystem = "windows"]

pub mod cpu;
pub mod memory;
pub mod vic;
pub mod cia;
pub mod pla;
pub mod clockchip;
pub mod sid;
pub mod motherboard;
pub mod cartridge;
pub mod iec;
pub mod fdd1541;
pub mod datassette;
pub mod ui;
pub mod emulator;
pub mod reu;

/*
The binary crate exposes each hardware subsystem at the crate root while the
emulator module owns application orchestration.
*/

use crate::emulator::{Breadbin, Result};
use crate::emulator::command_line::{Command, CommandLine, CreateKind};

fn main() -> Result<()> {
	/*
	Command-line actions are resolved before any window, audio device or emulation
	thread is created. Help and media creation therefore remain usable in headless
	environments.
	*/
	let args: Vec<String> = std::env::args().collect();
	let command_line = CommandLine::parse(&args).map_err(|error| {
		eprintln!("{error}

{}", CommandLine::help());
		error
	})?;

	match &command_line.command {
		Command::Help => {
			print!("{}", CommandLine::help());
			return Ok(());
		}
		Command::Create { kind, path } => {
			/*
			Media type is selected from the requested command, while the filename
			extension chooses the exact on-disk representation. Invalid combinations
			fail before a file is written.
			*/
			let data = match kind {
				CreateKind::Disk => match path.extension().and_then(|item| item.to_str()).map(str::to_ascii_lowercase).as_deref() {
					Some("d64") => crate::fdd1541::d64::create_formatted(),
					Some("g64") => crate::fdd1541::g64::create_formatted(),
					Some("nib") => crate::fdd1541::nib::create_formatted(),
					Some("nbz") => crate::fdd1541::nib::create_formatted_nbz(),
					_ => return Err("Disk output must use .d64, .g64, .nib or .nbz.".into()),
				},
				CreateKind::Tape => {
					if path.extension().and_then(|item| item.to_str()).map(str::to_ascii_lowercase).as_deref() != Some("tap") {
						return Err("Tape output must use .tap.".into());
					}
					b"C64-TAPE-RAW\x00\x00\x00\x00".to_vec()
				}
			};
			std::fs::write(path, data)?;
			return Ok(());
		}
		Command::Run => {}
	}

	/*
	Panics represent unrecoverable failures. The custom hook presents the payload
	and source location through a native dialog before the aborting release profile
	terminates the process.
	*/
	std::panic::set_hook(Box::new(|info| {
		let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
			format!("Critical Error: {}", s)
		} else if let Some(s) = info.payload().downcast_ref::<String>() {
			format!("Critical Error: {}", s)
		} else {
			"Unknown Critical Error occurred.".to_string()
		};
		let location = if let Some(loc) = info.location() {
			format!("\n\nSource: {}:{}", loc.file(), loc.line())
		} else {
			String::new()
		};
		rfd::MessageDialog::new()
			.set_title("Breadbin466 - Fatal Error")
			.set_description(&format!("{}{}", msg, location))
			.set_level(rfd::MessageLevel::Error)
			.set_buttons(rfd::MessageButtons::Ok)
			.show();
	}));

	/*
	Ordinary application errors are reported separately from panics so expected
	startup or runtime failures can still be returned to the caller.
	*/
	if let Err(e) = Breadbin::run(command_line) {
		rfd::MessageDialog::new()
			.set_title("Breadbin466 - Error")
			.set_description(&format!("Application Error:\n{}", e))
			.set_level(rfd::MessageLevel::Error)
			.show();
		return Err(e);
	}

	Ok(())
}