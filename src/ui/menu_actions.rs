// =======================================================
// src/ui/menu_actions.rs — Native Menu Actions Controller
// =======================================================

#[cfg(not(target_os = "linux"))]
use winit::window::Fullscreen;
use crate::emulator::context::AppContext;
use crate::emulator::timing::TimeKeeper;
#[cfg(target_os = "linux")]
use crate::ui::shell::Shell;
use crate::motherboard::bus::DriveMode;
use crate::fdd1541::{d64, g64, nib};
use std::path::PathBuf;

/* MenuHandler is the command decoder for desktop menu identifiers. It mutates AppContext and TimeKeeper through the same public operations used by other front ends, keeping platform menu implementations free of emulator policy. */
pub struct MenuHandler;

impl MenuHandler {
	/* Drive mode changes update machine policy, radio-menu state and persisted preference as one transaction. */
	fn set_drive_mode(context: &mut AppContext, mode: DriveMode, selected_id: &str, ids: &[&str]) {
		context.machine.set_drive_mode(mode);
		context.menu.set_radio_selection(selected_id, ids);
		context.history.drive_mode = mode;
		context.history.save_forced();
	}

	fn set_scale(context: &mut AppContext, current_scale: &mut f64, scale: f64, selected_id: &str, ids: &[&str]) {
		*current_scale = scale;
		context.renderer.resize_window_to_fit(scale);
		context.menu.set_radio_selection(selected_id, ids);
	}

	/* A custom ROM becomes durable only after successful loading; reset then starts the machine from the newly selected firmware. */
	fn install_custom_rom(
		context: &mut AppContext,
		title: &str,
		load: impl FnOnce(&mut crate::motherboard::Motherboard, &std::path::Path) -> crate::emulator::Result<()>,
		remember: impl FnOnce(&mut crate::ui::History, PathBuf),
	) {
		let Some(path) = rfd::FileDialog::new().add_filter(title, &["bin", "rom"]).pick_file() else {
			return;
		};

		match load(&mut context.machine, &path) {
			Ok(()) => {
				remember(&mut context.history, path);
				context.history.save_forced();
				context.hard_reset();
			}
			Err(error) => println!("{} error: {}", title, error),
		}
	}

	/* Newly created media is written before the current disk is detached, so a cancelled or failed save cannot disturb the mounted drive. */
	fn create_disk(context: &mut AppContext, extension: &str, data: Vec<u8>) {
		if let Some(path) = rfd::FileDialog::new()
			.set_title("Create New Disk Image")
			.add_filter("Disk Image", &[extension])
			.save_file()
		{
			if std::fs::write(&path, data).is_ok() {
				context.machine.unmount_drive();
				context.mount_disk(path);
			}
		}
	}

	/* dispatch is intentionally exhaustive at the policy boundary: platform-specific menus emit stable string identifiers, and this function translates them into machine, media, timing and window operations. */
	pub fn dispatch(id: &str, context: &mut AppContext, timing: &mut TimeKeeper, current_scale: &mut f64) {
		let ids = context.menu.ids.clone();

		if id == ids.open_prg {
			if let Some(path) = rfd::FileDialog::new().add_filter("PRG File", &["prg"]).pick_file() {
				context.open_prg(path, false);
			}
		} else if id == ids.open_prg_run {
			if let Some(path) = rfd::FileDialog::new().add_filter("PRG File", &["prg"]).pick_file() {
				context.open_prg(path, true);
			}
		} else if id == ids.open_cart {
			if let Some(path) = rfd::FileDialog::new().pick_file() {
				context.mount_cartridge(path);
			}
		} else if id == ids.open_d64_g64 {
			if let Some(path) = rfd::FileDialog::new()
				.add_filter("Disk Image", &["d64", "g64", "nib", "nbz"])
				.pick_file() {
				context.mount_disk(path);
			}
		} else if id == ids.unmount_d64_g64 {
			context.unmount_disk();
		} else if id == ids.disk_create_d64 {
			Self::create_disk(context, "d64", d64::create_formatted());
		} else if id == ids.disk_create_g64 {
			Self::create_disk(context, "g64", g64::create_formatted());
		} else if id == ids.disk_create_nib {
			Self::create_disk(context, "nib", nib::create_formatted());
		} else if id == ids.disk_create_nbz {
			Self::create_disk(context, "nbz", nib::create_formatted_nbz());
		} else if id == ids.tape_mount {
			if let Some(path) = rfd::FileDialog::new().pick_file() {
				context.mount_tape(path);
			}
		} else if id == ids.tape_create {
			if let Some(path) = rfd::FileDialog::new().set_title("Create New Tape Image").add_filter("Tape Image", &["tap"]).save_file() {
				let header = b"C64-TAPE-RAW\x00\x00\x00\x00";
				if std::fs::write(&path, header).is_ok() {
					context.mount_tape(path);
				}
			}
		} else if id == ids.tape_eject {
			context.eject_tape();
		} else if id == ids.tape_rewind {
			context.datassette.rewind();
		} else if id == ids.tape_play {
			context.toggle_tape_play();
		} else if id == ids.tape_record_play {
			context.toggle_tape_record();
		} else if id == ids.reset {
			context.hard_reset();
		} else if id == ids.soft_reset {
			context.machine.soft_reset();
		} else if id == ids.reset_detach {
			context.history.active_crt = None;
			context.history.save_forced();
			context.detach_cartridge();
		} else if id == ids.warp_mode {
			timing.toggle_warp_mode();
			context.menu.set_checked(&ids.warp_mode, timing.warp_mode);
			context.menu.set_checked(&ids.warp_1541, timing.warp_1541);
		} else if id == ids.warp_1541 {
			timing.toggle_drive_warp();
			context.menu.set_checked(&ids.warp_1541, timing.warp_1541);
			context.menu.set_checked(&ids.warp_mode, timing.warp_mode);
		} else if id == ids.cmd_directory {
			context.actions.schedule_text_entry("LOAD\"$\",8\r");
		} else if id == ids.cmd_load_first {
			context.actions.schedule_text_entry("LOAD\"*\",8,1\r");
		} else if id == ids.drive_off {
			Self::set_drive_mode(context, DriveMode::Off, &ids.drive_off, &[&ids.drive_off, &ids.drive_lle]);
		} else if id == ids.drive_lle {
			Self::set_drive_mode(context, DriveMode::Lle, &ids.drive_lle, &[&ids.drive_off, &ids.drive_lle]);
		} else if id == ids.show_debug_menu {
			let new_state = !context.history.debug_menu_visible;
			context.history.debug_menu_visible = new_state;
			context.history.save_forced();
			context.menu.set_debug_menu_visible(new_state);
			context.menu.set_checked(id, new_state);
		} else if id == ids.fullscreen {
			#[cfg(target_os = "linux")]
			{
				Shell::set_fullscreen(!Shell::fullscreen());
			}
			#[cfg(not(target_os = "linux"))]
			{
				let current = context.window.fullscreen().is_some();
				if current { context.window.set_fullscreen(None); }
				else { context.window.set_fullscreen(Some(Fullscreen::Borderless(None))); }
			}
		} else if id == ids.view_osd {
			context.history.osd_enabled = !context.history.osd_enabled;
			context.history.save_forced();
			context.menu.set_checked(&ids.view_osd, context.history.osd_enabled);
			context.renderer.set_osd_enabled(context.history.osd_enabled);
			context.renderer.resize_window_to_fit(*current_scale);
		} else if id == ids.scale_1x {
			Self::set_scale(context, current_scale, 1.0, &ids.scale_1x, &[&ids.scale_1x, &ids.scale_2x, &ids.scale_3x]);
		} else if id == ids.scale_2x {
			Self::set_scale(context, current_scale, 2.0, &ids.scale_2x, &[&ids.scale_1x, &ids.scale_2x, &ids.scale_3x]);
		} else if id == ids.scale_3x {
			Self::set_scale(context, current_scale, 3.0, &ids.scale_3x, &[&ids.scale_1x, &ids.scale_2x, &ids.scale_3x]);
		} else if id == ids.debug_mute_warp {

		} else if id == ids.debug_mute_global {
			context.history.mute_enabled = !context.history.mute_enabled;
			context.history.save_forced();
			context.menu.set_checked(&ids.debug_mute_global, context.history.mute_enabled);
		} else if id == ids.reu_1764_512k {
			/* The Computer menu toggles an installed expansion device, not a debug aid.
			 * Controller-owned activation preserves DRAM allocation while cancelling any
			 * in-flight DMA and releasing its interrupt line when disabled. */
			let next = !context.machine.memory.reu.enabled;
			context.machine.memory.reu.set_enabled(next);
			context.menu.set_checked(&ids.reu_1764_512k, next);
		} else if id == ids.debug_c128_2mhz {
			let enabled = !context.machine.memory.c128_2mhz_debug_enabled;
			context.machine.set_c128_debug_enabled(enabled);
			context.menu.set_checked(&ids.debug_c128_2mhz, enabled);
			context.hard_reset();
		} else if id == ids.rom_char {
			Self::install_custom_rom(
				context,
				"Character ROM",
				|machine, path| machine.load_custom_char_rom(path),
				|history, path| history.custom_char_rom = Some(path),
			);
		} else if id == ids.rom_basic {
			Self::install_custom_rom(
				context,
				"BASIC ROM",
				|machine, path| machine.load_custom_basic_rom(path),
				|history, path| history.custom_basic_rom = Some(path),
			);
		} else if id == ids.rom_kernal {
			Self::install_custom_rom(
				context,
				"KERNAL ROM",
				|machine, path| machine.load_custom_kernal_rom(path),
				|history, path| history.custom_kernal_rom = Some(path),
			);
		} else if id == ids.rom_drive {
			Self::install_custom_rom(
				context,
				"1541 ROM",
				|machine, path| machine.load_custom_drive_rom(path),
				|history, path| history.custom_drive_rom = Some(path),
			);
		} else if id == ids.rom_use_original {
			context.machine.reset_char_rom();
			context.machine.reset_basic_rom();
			context.machine.reset_kernal_rom();
			context.machine.reset_drive_rom();
			context.history.custom_char_rom   = None;
			context.history.custom_basic_rom  = None;
			context.history.custom_kernal_rom = None;
			context.history.custom_drive_rom  = None;
			context.history.save_forced();
			context.hard_reset();
		} else if id == ids.quit {
			context.input.close_requested = true;
		} else if id == ids.about {
			crate::ui::about::show(&context.window);
		} else if id.starts_with(&ids.recent_crt_base) {
			if let Some(idx) = id.strip_prefix(&ids.recent_crt_base).and_then(|s| s.parse::<usize>().ok()) {
				if let Some(path) = context.history.get_crt(idx) {
					context.mount_cartridge(path);
				}
			}
		} else if id.starts_with(&ids.recent_prg_base) {
			if let Some(idx) = id.strip_prefix(&ids.recent_prg_base).and_then(|s| s.parse::<usize>().ok()) {
				if let Some(path) = context.history.get_prg(idx) {
					context.open_prg(path, true);
				}
			}
		} else if id.starts_with(&ids.recent_d64_g64_base) {
			if let Some(idx) = id.strip_prefix(&ids.recent_d64_g64_base).and_then(|s| s.parse::<usize>().ok()) {
				if let Some(path) = context.history.get_d64_g64(idx) {
					context.mount_disk(path);
				}
			}
		} else if id.starts_with(&ids.recent_tap_base) {
			if let Some(idx) = id.strip_prefix(&ids.recent_tap_base).and_then(|s| s.parse::<usize>().ok()) {
				if let Some(path) = context.history.get_tap(idx) {
					context.mount_tape(path);
				}
			}
		}
	}
}