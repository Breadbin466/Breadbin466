// =======================================================
// src/main.rs — Application Entry Point and Top-Level Modules
// =======================================================

#![windows_subsystem = "windows"]

pub mod cartridge;
pub mod cia;
pub mod clockchip;
pub mod cpu;
pub mod datassette;
pub mod emulator;
pub mod fdd1541;
pub mod iec;
pub mod memory;
pub mod motherboard;
pub mod mouse1351;
pub mod pla;
pub mod reu;
pub mod sid;
pub mod ui;
pub mod vic;

/*
The binary crate exposes each hardware subsystem at the crate root while the
emulator module owns application orchestration.
*/

use crate::emulator::command_line::{Command, CommandLine, CreateKind};
use crate::emulator::{Breadbin, Result};

#[cfg(target_os = "windows")]
fn attach_parent_console() {
	use std::fs::OpenOptions;
	use std::os::windows::io::AsRawHandle;
	use std::os::windows::io::RawHandle;

	const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
	const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
	const STD_ERROR_HANDLE: u32 = -12i32 as u32;

	unsafe extern "system" {
		fn AttachConsole(process_id: u32) -> i32;
		fn SetStdHandle(std_handle: u32, handle: RawHandle) -> i32;
	}

	/* A GUI-subsystem binary does not inherit the invoking CMD or PowerShell console.
	 * Calling the Win32 console functions through their stable ABI avoids coupling this
	 * small bootstrap path to optional windows-rs feature gates. A normal graphical
	 * launch remains console-free because the attachment simply fails when no parent
	 * console exists. */
	if unsafe { AttachConsole(ATTACH_PARENT_PROCESS) } != 0 {
		if let Ok(stdout) = OpenOptions::new().write(true).open("CONOUT$") {
			let _ = unsafe { SetStdHandle(STD_OUTPUT_HANDLE, stdout.as_raw_handle()) };
			std::mem::forget(stdout);
		}
		if let Ok(stderr) = OpenOptions::new().write(true).open("CONOUT$") {
			let _ = unsafe { SetStdHandle(STD_ERROR_HANDLE, stderr.as_raw_handle()) };
			std::mem::forget(stderr);
		}
	}
}

fn main() -> Result<()> {
	#[cfg(target_os = "windows")]
	attach_parent_console();

	/*
	Command-line actions are resolved before any window, audio device or emulation
	thread is created. Help and media creation therefore remain usable in headless
	environments.
	*/
	let args: Vec<String> = std::env::args().collect();
	let command_line = CommandLine::parse(&args).map_err(|error| {
		eprintln!(
			"{error}

{}",
			CommandLine::help()
		);
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
				CreateKind::Disk => match path
					.extension()
					.and_then(|item| item.to_str())
					.map(str::to_ascii_lowercase)
					.as_deref()
				{
					Some("d64") => crate::fdd1541::d64::create_formatted(),
					Some("d7z") => {
						crate::fdd1541::d7z::encode(&crate::fdd1541::d64::create_formatted())
							.ok_or("Failed to encode D7Z image.")?
					}
					Some("g64") => crate::fdd1541::g64::create_formatted(),
					Some("nib") => crate::fdd1541::nib::create_formatted(),
					Some("nbz") => crate::fdd1541::nib::create_formatted_nbz(),
					_ => return Err("Disk output must use .d64, .d7z, .g64, .nib or .nbz.".into()),
				},
				CreateKind::Tape => {
					if path
						.extension()
						.and_then(|item| item.to_str())
						.map(str::to_ascii_lowercase)
						.as_deref()
						!= Some("tap")
					{
						return Err("Tape output must use .tap.".into());
					}
					b"C64-TAPE-RAW\x01\x00\x00\x00\x00\x00\x00\x00".to_vec()
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