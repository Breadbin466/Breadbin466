// =======================================================
// src/emulator/snapshot.rs — Detached Inspector state capture
// =======================================================

use super::orchestrator::Orchestrator;
use crate::{
	motherboard::bus::DriveMode,
	ui::inspector::{InspectorSection, InspectorSnapshot},
};
use std::path::Path;

/* Inspection uses owned values and side-effect-free chip fields. In
 * particular, no CPU-visible SID, CIA or REU reads can acknowledge an IRQ,
 * disturb a data bus, or alter the programme being inspected. */
pub(super) fn redraw_inspector(o: &mut Orchestrator) {
	if o.inspector.is_none() {
		return;
	}
	if o.inspector
		.as_ref()
		.is_some_and(|inspector| inspector.needs_snapshot())
	{
		let snapshot = capture(o);
		if let Some(inspector) = o.inspector.as_mut() {
			inspector.set_snapshot(snapshot);
		}
	}
}

fn section(title: &'static str, rows: Vec<(&'static str, String)>) -> InspectorSection {
	InspectorSection { title, rows }
}
fn state(value: bool) -> String {
	if value { "Enabled" } else { "Disabled" }.into()
}
fn path_name(path: Option<&Path>, fallback: &str) -> String {
	path.map(|p| p.display().to_string())
		.unwrap_or_else(|| fallback.into())
}

fn capture(o: &Orchestrator) -> InspectorSnapshot {
	let m = &o.context.machine;
	let history = &o.context.history;
	let paused = o.paused || o.debugger.as_ref().is_some_and(|d| d.paused);
	let warp = o.timing.is_warping(m.drive_busy_led());
	let status = if paused {
		"Paused"
	} else if warp {
		"Running · Warp"
	} else {
		"Running · PAL"
	};
	let seconds = (o.emulated_cycles as f64 / crate::clockchip::constants::CPU_FREQ_HZ) as u64;
	let audio = if o.silent {
		"Host output disabled"
	} else if !o.context.audio.as_ref().is_some_and(|audio| audio.is_connected()) {
		"Reconnecting output"
	} else if history.mute_enabled {
		"Muted"
	} else if warp && o.mute_sid_warp {
		"Muted during warp"
	} else if paused {
		"Paused"
	} else {
		"Playing"
	};
	let telemetry = &m.vic.telemetry;
	let measured_rate = if paused {
		"Paused".into()
	} else if telemetry.last_fps > 0.0 {
		format!(
			"{:.3} frames/s · {:.3} MHz",
			telemetry.last_fps, telemetry.current_mhz
		)
	} else {
		"Measuring…".into()
	};
	let mut snapshot = InspectorSnapshot {
		status: status.into(),
		overview: vec![
			section(
				"Machine",
				vec![
					("Reference", "Commodore 64 · Assy 250466 · PAL".into()),
					(
						"CPU / video",
						if m.memory.c128_2mhz_debug_enabled {
							"8502 debug mode · MOS 6569R5".into()
						} else {
							"MOS 6510 · MOS 6569R5".into()
						},
					),
					("Sound / I/O", "MOS 6581R4AR · 2 × MOS 6526A".into()),
					(
						"REU",
						if m.memory.reu.enabled {
							"Commodore 1764 · 512 KiB".into()
						} else {
							"Disabled".into()
						},
					),
				],
			),
			section(
				"Execution",
				vec![
					("Measured speed", measured_rate),
					(
						"Emulated time",
						format!(
							"{:02}:{:02}:{:02}",
							seconds / 3600,
							(seconds / 60) % 60,
							seconds % 60
						),
					),
					("Warp", state(o.timing.warp_mode)),
					("Warp on drive", state(o.timing.warp_1541)),
					("Audio output", audio.into()),
					("Mute in warp", state(o.mute_sid_warp)),
				],
			),
			section(
				"Input & display",
				vec![
					(
						"Joystick",
						o.context
							.joystick
							.as_ref()
							.map(|j| j.get_status_string())
							.unwrap_or_else(|| "None".into()),
					),
					("1351 mouse", state(m.mouse_1351_connected())),
					("OSD", state(history.osd_enabled)),
					(
						"Display",
						o.context
							.window
							.current_monitor()
							.and_then(|d| d.name())
							.unwrap_or_else(|| "Unknown".into()),
					),
					(
						"Refresh rate",
						o.context
							.window
							.current_monitor()
							.and_then(|d| d.refresh_rate_millihertz())
							.map(|v| format!("{:.3} Hz", f64::from(v) / 1000.0))
							.unwrap_or_else(|| "Not reported by the display".into()),
					),
				],
			),
		],
		chips: vec![
			section(
				"CPU",
				vec![
					("State", format!("{:?}", m.cpu.state)),
					(
						"Registers",
						format!(
							"PC ${:04X}   A ${:02X}   X ${:02X}   Y ${:02X}",
							m.cpu.pc, m.cpu.a, m.cpu.x, m.cpu.y
						),
					),
					(
						"Stack / flags",
						format!("SP ${:02X}   P ${:02X}  (NV–BDIZC)", m.cpu.sp, m.cpu.p),
					),
					(
						"Processor port",
						format!(
							"DDR ${:02X}   pins ${:02X}",
							m.cpu.port.ddr, m.cpu.port.output
						),
					),
				],
			),
			section(
				"VIC-II · MOS 6569R5",
				vec![
					(
						"Raster position",
						format!(
							"Line {} · cycle {}",
							m.vic.timing.raster_line, m.vic.timing.cycle
						),
					),
					(
						"Bad line",
						if m.vic.timing.is_badline { "Yes" } else { "No" }.into(),
					),
					("Completed frames", telemetry.frame_count.to_string()),
				],
			),
		],
		media: vec![],
	};
	let mut voices = vec![];
	for (i, (oscillator, envelope)) in m
		.memory
		.sid
		.oscillators
		.iter()
		.zip(&m.memory.sid.envelopes)
		.enumerate()
	{
		voices.push((
			["Voice 1", "Voice 2", "Voice 3"][i],
			format!(
				"Frequency ${:04X} · pulse ${:03X}\nWaveform ${:X} · envelope ${:02X} · TEST {}",
				oscillator.frequency,
				oscillator.pulse_width,
				oscillator.waveform,
				envelope.volume,
				if oscillator.test_enabled { "on" } else { "off" }
			),
		));
	}
	snapshot.chips.push(section("SID · MOS 6581R4AR", voices));
	for (title, cia) in [
		("CIA 1 · MOS 6526A", &m.memory.cia1.inner),
		("CIA 2 · MOS 6526A", &m.memory.cia2.inner),
	] {
		snapshot.chips.push(section(
			title,
			vec![
				(
					"Timer A",
					format!(
						"${:04X} · latch ${:04X} · control ${:02X}",
						cia.ta.counter, cia.ta.latch, cia.ta.cr
					),
				),
				(
					"Timer B",
					format!(
						"${:04X} · latch ${:04X} · control ${:02X}",
						cia.tb.counter, cia.tb.latch, cia.tb.cr
					),
				),
				(
					"Interrupts",
					format!(
						"Pending ${:02X} · mask ${:02X} · IRQ {}",
						cia.icr,
						cia.icr_mask,
						if cia.irq_line { "asserted" } else { "released" }
					),
				),
			],
		));
	}
	if m.memory.reu.enabled {
		snapshot.chips.push(section(
			"REU · CSG 8726",
			vec![
				("Capacity", "512 KiB".into()),
				(
					"Status / command",
					format!(
						"${:02X} / ${:02X}",
						m.memory.reu.debug_register(0),
						m.memory.reu.debug_register(1)
					),
				),
				(
					"Interrupt",
					if m.memory.reu.irq_pending {
						"Pending"
					} else {
						"None"
					}
					.into(),
				),
			],
		));
	}
	let cart = m.memory.cartridge.mapper.get_info();
	snapshot.media.push(section(
		"Cartridge",
		if cart.rom_size == 0 {
			vec![("Image", "None".into())]
		} else {
			vec![
				("Image", path_name(history.active_crt.as_deref(), "Mounted")),
				(
					"Hardware",
					format!("{} · {:?}", cart.name, cart.mapper_type),
				),
				(
					"ROM",
					format!("{} KiB · {} banks", cart.rom_size / 1024, cart.bank_count),
				),
				(
					"Cartridge RAM",
					if cart.has_ram { "Present" } else { "None" }.into(),
				),
			]
		},
	));
	let drive_enabled = m.get_drive_mode() != DriveMode::Off;
	let mut drive = vec![
		("Power", state(drive_enabled)),
		(
			"Image",
			path_name(history.active_d64_g64.as_deref(), "None"),
		),
	];
	if drive_enabled {
		drive.extend([
			(
				"Activity",
				if m.drive_busy_led() { "Busy" } else { "Idle" }.into(),
			),
			(
				"Track",
				m.drive_current_track()
					.map(|t| t.to_string())
					.unwrap_or_else(|| "No media".into()),
			),
			(
				"Drive CPU",
				m.drive_pc()
					.map(|pc| format!("PC ${pc:04X}"))
					.unwrap_or_else(|| "Unavailable".into()),
			),
			("DOS status", m.drive_error_string().to_owned()),
		]);
	}
	snapshot
		.media
		.push(section("Disk drive · Commodore 1541", drive));
	let tape = &o.context.datassette;
	let motor = m.cpu.port.output & 0x20 == 0;
	let transport = if !tape.has_tape() {
		"No cassette"
	} else if !tape.play_pressed {
		"Stopped"
	} else if tape.record_pressed {
		if motor {
			"Recording"
		} else {
			"Record armed · motor stopped"
		}
	} else if motor {
		"Playing"
	} else {
		"Play pressed · motor stopped"
	};
	snapshot.media.push(section(
		"Cassette · Commodore 1530",
		vec![
			("Image", path_name(tape.get_path(), "None")),
			("Transport", transport.into()),
			("Counter", format!("{:06.2}", tape.get_odometre_value())),
		],
	));
	snapshot.media.push(section(
		"ROM images",
		vec![
			(
				"BASIC",
				path_name(
					history.custom_basic_rom.as_deref(),
					"Commodore 901226-01 (built-in)",
				),
			),
			(
				"KERNAL",
				path_name(
					history.custom_kernal_rom.as_deref(),
					"Commodore 901227-03 (built-in)",
				),
			),
			(
				"Characters",
				path_name(
					history.custom_char_rom.as_deref(),
					"Commodore 901225-01 (built-in)",
				),
			),
			(
				"1541 DOS",
				path_name(
					history.custom_drive_rom.as_deref(),
					"Built-in Commodore DOS ROM",
				),
			),
		],
	));
	snapshot
}