// =======================================================
// src/ui/menu_actions.rs — Native Menu Actions Controller
// =======================================================

use crate::emulator::context::AppContext;
use crate::emulator::timing::TimeKeeper;
use crate::fdd1541::{convert, d7z, d64, disk_image, g64, nib, reclaim};
use crate::motherboard::bus::DriveMode;
#[cfg(target_os = "linux")]
use crate::ui::shell::Shell;
use rfd::FileDialog;
#[cfg(not(target_os = "macos"))]
use rfd::{MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
use std::path::PathBuf;
#[cfg(not(target_os = "linux"))]
use winit::window::Fullscreen;

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

	fn set_scale(
		context: &mut AppContext,
		current_scale: &mut f64,
		scale: f64,
		selected_id: &str,
		ids: &[&str],
	) {
		*current_scale = scale;
		context.renderer.resize_window_to_fit(scale);
		context.menu.set_radio_selection(selected_id, ids);
	}

	/* A custom ROM becomes durable only after successful loading; reset then starts the machine from the newly selected firmware. */
	fn install_custom_rom(
		context: &mut AppContext,
		title: &str,
		load: impl FnOnce(
			&mut crate::motherboard::Motherboard,
			&std::path::Path,
		) -> crate::emulator::Result<()>,
		remember: impl FnOnce(&mut crate::ui::History, PathBuf),
	) {
		let Some(path) = FileDialog::new()
			.add_filter(title, &["bin", "rom"])
			.pick_file()
		else {
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

	/* Disk utility UI remains deliberately compact. Image formats are selected
	 * explicitly before a destination path is chosen; destructive choices are
	 * explicit confirmation steps rather than hidden conversion policy. */
	fn ask(title: &str, description: &str) -> bool {
		#[cfg(target_os = "macos")]
		{
			return crate::ui::disk_dialog_macos::ask(title, description);
		}
		#[cfg(not(target_os = "macos"))]
		{
			MessageDialog::new()
				.set_level(MessageLevel::Warning)
				.set_title(title)
				.set_description(description)
				.set_buttons(MessageButtons::YesNo)
				.show()
				== MessageDialogResult::Yes
		}
	}

	fn info(title: &str, description: &str) {
		#[cfg(target_os = "macos")]
		{
			crate::ui::disk_dialog_macos::info(title, description);
		}
		#[cfg(not(target_os = "macos"))]
		{
			let _ = MessageDialog::new()
				.set_level(MessageLevel::Info)
				.set_title(title)
				.set_description(description)
				.set_buttons(MessageButtons::Ok)
				.show();
		}
	}

	fn create_disk(context: &mut AppContext) {
		#[cfg(target_os = "macos")]
		let (format, path) = {
			let Some(request) = crate::ui::disk_dialog_macos::create_image() else {
				return;
			};
			(request.format, request.destination)
		};

		#[cfg(target_os = "linux")]
		let (format, path) = {
			let Some(request) = crate::ui::disk_dialog_linux::create_image() else {
				return;
			};
			(request.format, request.destination)
		};

		#[cfg(target_os = "windows")]
		let (format, path) = {
			let Some(request) = crate::ui::disk_dialog_windows::create_image() else {
				return;
			};
			(request.format, request.destination)
		};

		let data = match format {
			disk_image::ImageFormat::D64 => Some(d64::create_formatted()),
			disk_image::ImageFormat::D7z => d7z::encode(&d64::create_formatted()),
			disk_image::ImageFormat::G64 => Some(g64::create_formatted()),
			disk_image::ImageFormat::Nib => Some(nib::create_formatted()),
			disk_image::ImageFormat::Nbz => Some(nib::create_formatted_nbz()),
		};
		let Some(data) = data else {
			Self::info(
				"Create Image",
				"The selected image format could not be created.",
			);
			return;
		};

		/* Creating an image is transactional with respect to the currently mounted medium.
		 * The drive is ejected before any backing file can be replaced, and both the previous
		 * destination contents and the previous mount are restored if a later step fails. */
		let previous = context.history.active_d64_g64.clone();
		if previous.is_some() && !context.unmount_disk() {
			Self::info(
				"Create Image",
				"The currently mounted disk could not be ejected safely, so image creation was cancelled.",
			);
			return;
		}

		let previous_destination = if path.exists() {
			match crate::host_files::read(&path, disk_image::MAX_IMAGE_SIZE) {
				Ok(bytes) => Some(bytes),
				Err(_) => {
					if let Some(previous) = previous {
						let _ = context.mount_disk(previous);
					}
					Self::info(
						"Create Image",
						"The existing destination could not be read safely, so it was not replaced.",
					);
					return;
				}
			}
		} else {
			None
		};

		if !disk_image::replace_atomically(&path, &data) {
			if let Some(previous) = previous {
				let _ = context.mount_disk(previous);
			}
			Self::info("Create Image", "Could not write the new disk image.");
			return;
		}

		if !context.mount_disk(path.clone()) {
			if let Some(old) = previous_destination {
				let _ = disk_image::replace_atomically(&path, &old);
			} else {
				let _ = std::fs::remove_file(&path);
			}
			if let Some(previous) = previous {
				let _ = context.mount_disk(previous);
			}
			Self::info(
				"Create Image",
				"The new disk image could not be mounted. The previous state was restored.",
			);
		}
	}

	fn convert_disk(context: &mut AppContext) {
		#[cfg(target_os = "macos")]
		let (source, destination_format, destination, reclaim, delete_source) = {
			let Some(request) = crate::ui::disk_dialog_macos::convert_image() else {
				return;
			};
			(
				request.source,
				request.destination_format,
				request.destination,
				request.reclaim_space,
				request.delete_source,
			)
		};

		#[cfg(target_os = "linux")]
		let (source, destination_format, destination, reclaim, delete_source) = {
			let Some(request) = crate::ui::disk_dialog_linux::convert_image() else {
				return;
			};
			(
				request.source,
				request.destination_format,
				request.destination,
				request.reclaim_space,
				request.delete_source,
			)
		};

		#[cfg(target_os = "windows")]
		let (source, destination_format, destination, reclaim, delete_source) = {
			let Some(request) = crate::ui::disk_dialog_windows::convert_image() else {
				return;
			};
			(
				request.source,
				request.destination_format,
				request.destination,
				request.reclaim_space,
				request.delete_source,
			)
		};

		let Some(source_format) = disk_image::format_from_path(&source) else {
			return;
		};
		if destination.exists()
			&& !Self::ask(
				"Convert Image",
				&format!("{} already exists. Replace it?", destination.display()),
			)
		{
			return;
		}
		let previous = context.history.active_d64_g64.clone();
		let source_was_mounted = previous.as_ref() == Some(&source);
		if source_was_mounted && !context.unmount_disk() {
			Self::info(
				"Convert Image",
				"The mounted source image could not be ejected safely, so conversion was cancelled.",
			);
			return;
		}
		let Ok(source_bytes) = crate::host_files::read(&source, disk_image::MAX_IMAGE_SIZE) else {
			if source_was_mounted {
				let _ = context.mount_disk(source);
			}
			return;
		};

		if let Some(warning) = convert::warning(source_format, destination_format, &source_bytes) {
			if !Self::ask(
				"Convert Image",
				&format!("{warning}\n\nContinue with this lossy conversion?"),
			) {
				if source_was_mounted {
					let _ = context.mount_disk(source);
				}
				return;
			}
		}

		let prepared_source = if reclaim {
			let Some(prepared) = reclaim::transform_for_conversion(source_format, &source_bytes)
			else {
				if source_was_mounted {
					let _ = context.mount_disk(source);
				}
				Self::info(
					"Reclaim Space",
					"The Commodore DOS BAM or directory structure cannot be interpreted safely, so no sectors were cleared and the conversion was cancelled.",
				);
				return;
			};
			prepared
		} else {
			source_bytes.clone()
		};

		let Some(output) =
			convert::bytes(source_format, destination_format, &prepared_source, false)
		else {
			if source_was_mounted {
				let _ = context.mount_disk(source);
			}
			Self::info(
				"Convert Image",
				"The selected image cannot be converted safely to that format.",
			);
			return;
		};

		/* Keep enough state to roll back the complete user operation. In particular, an
		 * existing destination must not be lost merely because the newly converted image
		 * later fails validation during mount. */
		let previous_destination = if destination.exists() {
			match crate::host_files::read(&destination, disk_image::MAX_IMAGE_SIZE) {
				Ok(bytes) => Some(bytes),
				Err(_) => {
					if source_was_mounted {
						let _ = context.mount_disk(source);
					}
					Self::info(
						"Convert Image",
						"The existing destination could not be read safely, so it was not replaced.",
					);
					return;
				}
			}
		} else {
			None
		};
		if !disk_image::replace_atomically(&destination, &output) {
			if source_was_mounted {
				let _ = context.mount_disk(source);
			}
			Self::info("Convert Image", "Could not write the destination image.");
			return;
		}

		if !source_was_mounted && !context.unmount_disk() {
			if let Some(old) = previous_destination {
				let _ = disk_image::replace_atomically(&destination, &old);
			} else {
				let _ = std::fs::remove_file(&destination);
			}
			Self::info(
				"Convert Image",
				"The currently mounted disk could not be ejected safely. The destination was rolled back.",
			);
			return;
		}
		if !context.mount_disk(destination.clone()) {
			if let Some(old) = previous_destination {
				let _ = disk_image::replace_atomically(&destination, &old);
			} else {
				let _ = std::fs::remove_file(&destination);
			}
			if let Some(previous) = previous {
				let _ = context.mount_disk(previous);
			}
			Self::info(
				"Convert Image",
				"The converted image could not be mounted. The destination was rolled back.",
			);
			return;
		}

		if delete_source && source != destination {
			if std::fs::remove_file(&source).is_err() {
				Self::info(
					"Convert Image",
					"The conversion succeeded, but the source image could not be deleted.",
				);
				return;
			}
		}

		Self::info("Convert Image", "Conversion successful!");
	}

	/* dispatch is intentionally exhaustive at the policy boundary: platform-specific menus emit stable string identifiers, and this function translates them into machine, media, timing and window operations. */
	pub fn dispatch(
		id: &str,
		context: &mut AppContext,
		timing: &mut TimeKeeper,
		current_scale: &mut f64,
	) {
		let ids = context.menu.ids.clone();

		if id == ids.open_prg {
			if let Some(path) = FileDialog::new()
				.add_filter("PRG File", &["prg"])
				.pick_file()
			{
				context.open_prg(path, false);
			}
		} else if id == ids.open_prg_run {
			if let Some(path) = FileDialog::new()
				.add_filter("PRG File", &["prg"])
				.pick_file()
			{
				context.open_prg(path, true);
			}
		} else if id == ids.open_cart {
			if let Some(path) = FileDialog::new().pick_file() {
				context.mount_cartridge(path);
			}
		} else if id == ids.open_d64_g64 {
			if let Some(path) = FileDialog::new()
				.add_filter("Disk Image", &["d64", "d7z", "g64", "nib", "nbz"])
				.pick_file()
			{
				context.mount_disk(path);
			}
		} else if id == ids.unmount_d64_g64 {
			context.unmount_disk();
		} else if id == ids.disk_create {
			Self::create_disk(context);
		} else if id == ids.disk_convert {
			Self::convert_disk(context);
		} else if id == ids.tape_mount {
			if let Some(path) = FileDialog::new().pick_file() {
				context.mount_tape(path);
			}
		} else if id == ids.tape_create {
			if let Some(path) = FileDialog::new()
				.set_title("Create New Tape Image")
				.add_filter("Tape Image", &["tap"])
				.save_file()
			{
				let header = b"C64-TAPE-RAW\x01\x00\x00\x00\x00\x00\x00\x00";
				let previous = context.history.active_tap.clone();
				if previous.is_some() && !context.eject_tape() {
					Self::info(
						"Create Tape Image",
						"The currently mounted tape could not be ejected safely, so tape creation was cancelled.",
					);
					return;
				}

				let previous_destination = if path.exists() {
					match crate::host_files::read(&path, crate::datassette::constants::MAX_TAPE_SIZE) {
						Ok(bytes) => Some(bytes),
						Err(_) => {
							if let Some(previous) = previous {
								let _ = context.mount_tape(previous);
							}
							Self::info(
								"Create Tape Image",
								"The existing destination could not be read safely, so it was not replaced.",
							);
							return;
						}
					}
				} else {
					None
				};

				if !disk_image::replace_atomically(&path, header) {
					if let Some(previous) = previous {
						let _ = context.mount_tape(previous);
					}
					Self::info("Create Tape Image", "Could not write the new TAP image.");
					return;
				}

				if !context.mount_tape(path.clone()) {
					if let Some(old) = previous_destination {
						let _ = disk_image::replace_atomically(&path, &old);
					} else {
						let _ = std::fs::remove_file(&path);
					}
					if let Some(previous) = previous {
						let _ = context.mount_tape(previous);
					}
					Self::info(
						"Create Tape Image",
						"The new TAP image could not be mounted. The previous state was restored.",
					);
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
			if context.detach_cartridge() {
				context.history.active_crt = None;
				context.history.save_forced();
			}
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
			Self::set_drive_mode(
				context,
				DriveMode::Off,
				&ids.drive_off,
				&[&ids.drive_off, &ids.drive_lle],
			);
		} else if id == ids.drive_lle {
			Self::set_drive_mode(
				context,
				DriveMode::Lle,
				&ids.drive_lle,
				&[&ids.drive_off, &ids.drive_lle],
			);
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
				if current {
					context.window.set_fullscreen(None);
				} else {
					context
						.window
						.set_fullscreen(Some(Fullscreen::Borderless(None)));
				}
			}
		} else if id == ids.view_osd {
			context.history.osd_enabled = !context.history.osd_enabled;
			context.history.save_forced();
			context
				.menu
				.set_checked(&ids.view_osd, context.history.osd_enabled);
			context
				.renderer
				.set_osd_enabled(context.history.osd_enabled);
			context.renderer.resize_window_to_fit(*current_scale);
		} else if id == ids.scale_1x {
			Self::set_scale(
				context,
				current_scale,
				1.0,
				&ids.scale_1x,
				&[&ids.scale_1x, &ids.scale_2x, &ids.scale_3x],
			);
		} else if id == ids.scale_2x {
			Self::set_scale(
				context,
				current_scale,
				2.0,
				&ids.scale_2x,
				&[&ids.scale_1x, &ids.scale_2x, &ids.scale_3x],
			);
		} else if id == ids.scale_3x {
			Self::set_scale(
				context,
				current_scale,
				3.0,
				&ids.scale_3x,
				&[&ids.scale_1x, &ids.scale_2x, &ids.scale_3x],
			);
		} else if id == ids.debug_mute_warp {
		} else if id == ids.debug_mute_global {
			context.history.mute_enabled = !context.history.mute_enabled;
			if let Some(audio) = context.audio.as_mut() { audio.discard_pending(); }
			context.history.save_forced();
			context
				.menu
				.set_checked(&ids.debug_mute_global, context.history.mute_enabled);
		} else if id == ids.debug_display_uptime {
			context.history.display_uptime = !context.history.display_uptime;
			context.history.save_forced();
			context.menu.set_checked(
				&ids.debug_display_uptime,
				context.history.display_uptime,
			);
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
			context.history.custom_char_rom = None;
			context.history.custom_basic_rom = None;
			context.history.custom_kernal_rom = None;
			context.history.custom_drive_rom = None;
			context.history.save_forced();
			context.hard_reset();
		} else if id == ids.quit {
			context.input.close_requested = true;
		} else if id == ids.about {
			crate::ui::about::show(&context.window);
		} else if id.starts_with(&ids.recent_crt_base) {
			if let Some(idx) = id
				.strip_prefix(&ids.recent_crt_base)
				.and_then(|s| s.parse::<usize>().ok())
			{
				if let Some(path) = context.history.get_crt(idx) {
					context.mount_cartridge(path);
				}
			}
		} else if id.starts_with(&ids.recent_prg_base) {
			if let Some(idx) = id
				.strip_prefix(&ids.recent_prg_base)
				.and_then(|s| s.parse::<usize>().ok())
			{
				if let Some(path) = context.history.get_prg(idx) {
					context.open_prg(path, true);
				}
			}
		} else if id.starts_with(&ids.recent_d64_g64_base) {
			if let Some(idx) = id
				.strip_prefix(&ids.recent_d64_g64_base)
				.and_then(|s| s.parse::<usize>().ok())
			{
				if let Some(path) = context.history.get_d64_g64(idx) {
					context.mount_disk(path);
				}
			}
		} else if id.starts_with(&ids.recent_tap_base) {
			if let Some(idx) = id
				.strip_prefix(&ids.recent_tap_base)
				.and_then(|s| s.parse::<usize>().ok())
			{
				if let Some(path) = context.history.get_tap(idx) {
					context.mount_tape(path);
				}
			}
		}
	}
}