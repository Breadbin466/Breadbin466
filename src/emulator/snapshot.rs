// =======================================================
// src/emulator/snapshot.rs — Inspector state snapshot rendering
// =======================================================

use super::orchestrator::Orchestrator;
use crate::motherboard::bus::DriveMode;

/* The Inspector consumes a detached textual snapshot rather than borrowing live UI state during rendering. Each value is sampled from the orchestrator on the application thread, converted to a stable display form, and then handed to the satellite window as one coherent update. */

pub(super) fn redraw_inspector(orchestrator: &mut Orchestrator) {
	let Some(inspector) = orchestrator.inspector.as_mut() else {
		return;
	};
	let rom_or_original = |path: &Option<std::path::PathBuf>| -> String {
		path.as_ref()
			.and_then(|value| value.file_name())
			.and_then(|name| name.to_str())
			.map(str::to_string)
			.unwrap_or_else(|| "Original".to_string())
	};
	let file_or_none = |path: &Option<std::path::PathBuf>| -> String {
		path.as_ref()
			.and_then(|value| value.file_name())
			.and_then(|name| name.to_str())
			.map(str::to_string)
			.unwrap_or_else(|| "None".to_string())
	};
	let drive_mode = match orchestrator.context.machine.get_drive_mode() {
		DriveMode::Off => "Disabled",
		DriveMode::Lle => "Enabled (LLE)",
	};
	let drive_error = if orchestrator.context.machine.get_drive_mode() != DriveMode::Off {
		orchestrator
			.context
			.machine
			.drive_error_string()
			.to_string()
	} else {
		"N/A".to_string()
	};
	let cartridge = {
		let info = orchestrator
			.context
			.machine
			.memory
			.cartridge
			.mapper
			.get_info();
		if info.rom_size == 0 {
			"None".to_string()
		} else {
			info.name.clone()
		}
	};
	let tape = orchestrator
		.context
		.datassette
		.get_path()
		.and_then(|path| path.file_name())
		.and_then(|name| name.to_str())
		.map(str::to_string)
		.unwrap_or_else(|| "None".to_string());
	let lines = vec![
		"=== INSPECTOR ===".to_string(),
		"".to_string(),
		format!("Cartridge : {}", cartridge),
		format!(
			"Disk      : {}",
			file_or_none(&orchestrator.context.history.active_d64_g64)
		),
		format!("Tape      : {}", tape),
		"".to_string(),
		"=== ROMS ===".to_string(),
		format!(
			"Character ROM    : {}",
			rom_or_original(&orchestrator.context.history.custom_char_rom)
		),
		format!(
			"BASIC ROM        : {}",
			rom_or_original(&orchestrator.context.history.custom_basic_rom)
		),
		format!(
			"KERNAL ROM       : {}",
			rom_or_original(&orchestrator.context.history.custom_kernal_rom)
		),
		format!(
			"1541 ROM         : {}",
			rom_or_original(&orchestrator.context.history.custom_drive_rom)
		),
		"".to_string(),
		"=== DRIVE ===".to_string(),
		format!("1541 Mode        : {}", drive_mode),
		format!("1541 Status      : {}", drive_error),
		"".to_string(),
		"=== OPTIONS ===".to_string(),
		format!(
			"Warp Mode        : {}",
			if orchestrator.timing.warp_mode {
				"ON"
			} else {
				"OFF"
			}
		),
		format!(
			"Warp on 1541     : {}",
			if orchestrator.timing.warp_1541 {
				"ON"
			} else {
				"OFF"
			}
		),
		format!(
			"Mute SID (Warp)  : {}",
			if orchestrator.mute_sid_warp {
				"ON"
			} else {
				"OFF"
			}
		),
		format!(
			"Mute Audio       : {}",
			if orchestrator.context.history.mute_enabled {
				"ON"
			} else {
				"OFF"
			}
		),
		format!(
			"OSD              : {}",
			if orchestrator.context.history.osd_enabled {
				"ON"
			} else {
				"OFF"
			}
		),
		format!(
			"1764 REU 512 KB  : {}",
			if orchestrator.context.machine.memory.reu.enabled {
				"ON"
			} else {
				"OFF"
			}
		),
		format!(
			"8502 Mode        : {}",
			if orchestrator.context.machine.memory.c128_2mhz_debug_enabled {
				"ON"
			} else {
				"OFF"
			}
		),
		"".to_string(),
		format!(
			"Joystick         : {}",
			orchestrator
				.context
				.joystick
				.as_ref()
				.map(|joy| joy.get_status_string())
				.unwrap_or_else(|| "None".to_string())
		),
		format!(
			"Tape PLAY        : {}",
			if orchestrator.context.datassette.play_pressed {
				"ON"
			} else {
				"OFF"
			}
		),
		format!(
			"Tape RECORD      : {}",
			if orchestrator.context.datassette.record_pressed {
				"ON"
			} else {
				"OFF"
			}
		),
	];
	let _ = inspector.draw(&lines, orchestrator.is_dark_mode);
}