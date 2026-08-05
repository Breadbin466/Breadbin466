// =======================================================
// src/emulator/context.rs — Application Resource Container
// =======================================================

use std::path::{Path, PathBuf};
use std::sync::Arc;
use winit::window::Window;

use crate::motherboard::Motherboard;
use crate::motherboard::injection::ActionManager;
use crate::datassette::Datassette;
use crate::ui::{Renderer, InputState, MenuManager, History, AudioHost, JoystickHost};

/* AppContext is the ownership boundary for resources that must move together on the desktop thread: the mutable motherboard, window and GPU state, host input devices, media history and user-interface services. */
pub struct AppContext {
	pub machine:    Motherboard,
	pub renderer:   Renderer,
	pub window:     Arc<Window>,
	pub input:      InputState,
	pub actions:    ActionManager,
	pub menu:       MenuManager,
	pub history:    History,
	pub audio:      Option<AudioHost>,
	pub joystick:  Option<JoystickHost>,
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

	pub fn load_cartridge(&mut self, path: &Path) {
		println!("Loading Cartridge: {:?}", path);
		if let Err(e) = self.machine.load_cartridge(path) {
			println!("Error: {}", e);
		} else {
			self.hard_reset();
		}
	}

	/* PRG loading is scheduled through ActionManager rather than written into RAM immediately. This preserves the normal READY-prompt and keyboard-buffer sequencing used by runtime injection. */
	pub fn open_prg(&mut self, path: PathBuf, autorun: bool) {
		self.history.add(path.clone());
		self.menu.rebuild_recent(&self.history);
		if let Ok(data) = std::fs::read(path) {
			self.actions.schedule_injection(data, autorun);
		}
	}

	pub fn mount_cartridge(&mut self, path: PathBuf) {
		self.history.add(path.clone());
		self.history.active_crt = Some(path.clone());
		self.history.save_forced();
		self.menu.rebuild_recent(&self.history);
		self.load_cartridge(&path);
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

	pub fn unmount_disk(&mut self) {
		self.history.active_d64_g64 = None;
		self.history.save_forced();
		self.machine.unmount_drive();
	}

	/* Mounting updates recent-media state and then validates the TAP image. The deck itself changes only when the complete file passes structural validation. */
	pub fn mount_tape(&mut self, path: PathBuf) {
		self.history.add(path.clone());
		self.history.active_tap = Some(path.clone());
		self.history.save_forced();
		self.menu.rebuild_recent(&self.history);
		if let Ok(data) = std::fs::read(&path) {
			if !self.datassette.load_tap(data, path) {
				eprintln!("[TAPE] Failed to load TAP image");
			}
		}
	}

	/* Ejection lets the deck commit pending recording before the UI forgets the active media path. */
	pub fn eject_tape(&mut self) {
		self.datassette.eject();
		self.history.active_tap = None;
		self.history.save_forced();
	}

	/* The general PLAY toggle preserves RECORD while starting, but stopping PLAY also drops RECORD and commits the image. */
	pub fn toggle_tape_play(&mut self) {
		let new_state = !self.datassette.play_pressed;
		self.datassette.play_pressed = new_state;
		if !new_state {
			self.datassette.record_pressed = false;
			self.datassette.save_tape_to_host();
		}
		self.menu.set_tape_transport(new_state, self.datassette.record_pressed);
	}

	/* Playback mode always clears RECORD, even when PLAY was already active. */
	pub fn toggle_tape_playback(&mut self) {
		let new_state = !self.datassette.play_pressed;
		self.datassette.play_pressed = new_state;
		self.datassette.record_pressed = false;
		self.datassette.save_tape_to_host();
		self.menu.set_tape_transport(new_state, false);
	}

	/* RECORD and PLAY are latched together because the physical deck cannot record with the transport stopped. */
	pub fn toggle_tape_record(&mut self) {
		let new_state = !self.datassette.record_pressed;
		self.datassette.record_pressed = new_state;
		self.datassette.play_pressed = new_state;
		self.datassette.save_tape_to_host();
		self.menu.set_tape_transport(new_state, new_state);
	}

	/* Stopping transport commits any pending recording before the UI marks both transport buttons inactive. */
	pub fn stop_tape(&mut self) {
		self.datassette.play_pressed = false;
		self.datassette.record_pressed = false;
		self.datassette.save_tape_to_host();
		self.menu.set_tape_transport(false, false);
	}

	pub fn detach_cartridge(&mut self) {
		println!("Detaching Cartridge.");
		self.machine.memory.detach_cartridge();
		self.hard_reset();
	}
}