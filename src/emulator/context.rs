// =======================================================
// src/emulator/context.rs — Application Resource Container
// =======================================================

use std::path::{Path, PathBuf};
use std::sync::Arc;
use winit::window::Window;

use crate::datassette::Datassette;
use crate::motherboard::Motherboard;
use crate::motherboard::injection::ActionManager;
use crate::ui::{AudioHost, History, InputState, JoystickHost, MenuManager, Renderer, WavRecorder};

/* AppContext is the ownership boundary for resources that must move together on the desktop thread: the mutable motherboard, window and GPU state, host input devices, media history and user-interface services. */
pub struct AppContext {
	pub machine: Motherboard,
	pub renderer: Renderer,
	pub window: Arc<Window>,
	pub input: InputState,
	pub actions: ActionManager,
	pub menu: MenuManager,
	pub history: History,
	pub audio: Option<AudioHost>,
	pub wav: Option<WavRecorder>,
	pub joystick: Option<JoystickHost>,
	pub datassette: Datassette,
}

impl AppContext {
	/* A hard reset resets machine state and clears host-side input and deferred injection state so no stale key or scheduled action survives into the restarted machine. */
	pub fn hard_reset(&mut self) {
		println!("Performing Hard Reset.");
		self.machine.reset();
		self.input.clear_all();
		self.actions.reset_state();
	}

	pub fn load_cartridge(&mut self, path: &Path) -> bool {
		println!("Loading Cartridge: {:?}", path);
		if let Err(error) = self.machine.load_cartridge(path) {
			println!("Error: {}", error);
			return false;
		}
		self.hard_reset();
		true
	}

	/* PRG loading is scheduled through ActionManager rather than written into RAM immediately. This preserves the normal READY-prompt and keyboard-buffer sequencing used by runtime injection. */
	pub fn open_prg(&mut self, path: PathBuf, autorun: bool) {
		let Ok(data) = std::fs::read(&path) else {
			eprintln!("[PRG] Failed to read PRG image");
			return;
		};
		self.actions.schedule_injection(data, autorun);
		self.history.add(path);
		self.menu.rebuild_recent(&self.history);
	}

	/* Cartridge history changes only after the new image has been accepted, so a failed replacement cannot advertise media that never became active. */
	pub fn mount_cartridge(&mut self, path: PathBuf) {
		if !self.load_cartridge(&path) {
			return;
		}
		self.history.add(path.clone());
		self.history.active_crt = Some(path);
		self.history.save_forced();
		self.menu.rebuild_recent(&self.history);
	}

	/* History is updated only after the drive accepts the image, so the persisted active-media state cannot advertise a mount that failed in the emulation core. */
	pub fn mount_disk(&mut self, path: PathBuf) -> bool {
		if !self.machine.mount_drive(&path) {
			return false;
		}
		self.history.add(path.clone());
		self.history.active_d64_g64 = Some(path);
		self.history.save_forced();
		self.menu.rebuild_recent(&self.history);
		true
	}

	/* Disk history is cleared only after the drive confirms that pending media writes were persisted and the medium was actually ejected. */
	pub fn unmount_disk(&mut self) -> bool {
		if !self.machine.unmount_drive() {
			eprintln!("[1541] Failed to persist the mounted disk before ejection");
			return false;
		}
		self.history.active_d64_g64 = None;
		self.history.save_forced();
		true
	}

	/* Mounting persists the current cassette before validating a replacement. History changes only after the complete TAP image has been accepted by the deck. */
	pub fn mount_tape(&mut self, path: PathBuf) -> bool {
		if let Err(error) = self.datassette.save_tape_to_host() {
			eprintln!("[TAPE] Failed to persist the current TAP image before replacement: {error}");
			return false;
		}
		let Ok(data) = std::fs::read(&path) else {
			eprintln!("[TAPE] Failed to read TAP image");
			return false;
		};
		if !self.datassette.load_tap(data, path.clone()) {
			eprintln!("[TAPE] Failed to load TAP image");
			return false;
		}
		self.history.add(path.clone());
		self.history.active_tap = Some(path);
		self.history.save_forced();
		self.menu.rebuild_recent(&self.history);
		true
	}

	/* Ejection lets the deck commit pending recording before the UI forgets the active media path. */
	pub fn eject_tape(&mut self) -> bool {
		if let Err(error) = self.datassette.eject() {
			eprintln!("[TAPE] Failed to persist TAP image before ejection: {error}");
			return false;
		}
		self.history.active_tap = None;
		self.history.save_forced();
		true
	}

	/* The general PLAY toggle preserves RECORD while starting, but stopping PLAY also drops RECORD and commits the image. */
	pub fn toggle_tape_play(&mut self) {
		let new_state = !self.datassette.play_pressed;
		self.datassette.play_pressed = new_state;
		if !new_state {
			self.datassette.record_pressed = false;
			if let Err(error) = self.datassette.save_tape_to_host() {
				eprintln!("[TAPE] Failed to persist TAP image: {error}");
			}
		}
		self.menu
			.set_tape_transport(new_state, self.datassette.record_pressed);
	}

	/* Playback mode always clears RECORD, even when PLAY was already active. */
	pub fn toggle_tape_playback(&mut self) {
		let new_state = !self.datassette.play_pressed;
		self.datassette.play_pressed = new_state;
		self.datassette.record_pressed = false;
		if let Err(error) = self.datassette.save_tape_to_host() {
			eprintln!("[TAPE] Failed to persist TAP image: {error}");
		}
		self.menu.set_tape_transport(new_state, false);
	}

	/* RECORD and PLAY are latched together because the physical deck cannot record with the transport stopped. */
	pub fn toggle_tape_record(&mut self) {
		let new_state = !self.datassette.record_pressed;
		self.datassette.record_pressed = new_state;
		self.datassette.play_pressed = new_state;
		if let Err(error) = self.datassette.save_tape_to_host() {
			eprintln!("[TAPE] Failed to persist TAP image: {error}");
		}
		self.menu.set_tape_transport(new_state, new_state);
	}

	/* Stopping transport commits any pending recording before the UI marks both transport buttons inactive. */
	pub fn stop_tape(&mut self) {
		self.datassette.play_pressed = false;
		self.datassette.record_pressed = false;
		if let Err(error) = self.datassette.save_tape_to_host() {
			eprintln!("[TAPE] Failed to persist TAP image: {error}");
		}
		self.menu.set_tape_transport(false, false);
	}

	pub fn detach_cartridge(&mut self) -> bool {
		println!("Detaching Cartridge.");
		if let Err(error) = self.machine.memory.detach_cartridge() {
			eprintln!("[CARTRIDGE] Failed to persist cartridge NVRAM before detach: {error}");
			return false;
		}
		self.hard_reset();
		true
	}

	/* A normal application close is committed only after every writable host-backed medium has been persisted successfully. Keeping this check outside Drop lets the event loop refuse the close request while all in-memory media state is still available for a later retry. */
	pub fn prepare_shutdown(&mut self) -> bool {
		let mut persisted = true;

		if !self.machine.flush_drive_media() {
			eprintln!("[1541] Failed to persist the mounted disk during application shutdown");
			persisted = false;
		}
		if let Err(error) = self.datassette.save_tape_to_host() {
			eprintln!("[TAPE] Failed to persist TAP image during application shutdown: {error}");
			persisted = false;
		}
		if let Err(error) = self.machine.memory.cartridge.save_associated_nvram() {
			eprintln!(
				"[CARTRIDGE] Failed to persist cartridge NVRAM during application shutdown: {error}"
			);
			persisted = false;
		}

		persisted
	}
}

/* Drop remains a final persistence safety net for abnormal teardown paths that could not pass through the event-loop close transaction. */
impl Drop for AppContext {
	fn drop(&mut self) {
		let _ = self.prepare_shutdown();
	}
}