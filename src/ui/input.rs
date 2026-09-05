// =======================================================
// src/ui/input.rs — Host Input State and Keyboard Matrix Dispatcher
// =======================================================

use crate::cia::Cia1;
use crate::ui::joystick::VirtualMode;
use crate::ui::keyboard::key_to_matrix;
use std::collections::HashSet;
use winit::event::ElementState;
use winit::keyboard::KeyCode;

/* InputState separates level state from one-frame edges. Held keys drive the C64 matrix continuously, while pressed and released sets remain available to host-side shortcuts until clear_frame closes the current UI frame. */
pub struct InputState {
	keys_held: HashSet<KeyCode>,
	keys_pressed: HashSet<KeyCode>,
	keys_released: HashSet<KeyCode>,
	pub close_requested: bool,
}

impl InputState {
	/* Construction begins with no held levels or pending edges; the snapshot becomes authoritative for the first UI frame. */
	pub fn new() -> Self {
		Self {
			keys_held: HashSet::new(),
			keys_pressed: HashSet::new(),
			keys_released: HashSet::new(),
			close_requested: false,
		}
	}

	/* Clearing all levels is used after focus changes so stale host state cannot leave C64 keys held indefinitely. */
	pub fn clear_all(&mut self) {
		self.keys_held.clear();
		self.keys_pressed.clear();
		self.keys_released.clear();
	}

	/* Auto-repeat cannot create repeated press edges because a key enters keys_pressed only on the transition into keys_held. */
	pub fn update_key(&mut self, key: KeyCode, state: ElementState) {
		match state {
			ElementState::Pressed => {
				if !self.keys_held.contains(&key) {
					self.keys_pressed.insert(key);
				}
				self.keys_held.insert(key);
			}
			ElementState::Released => {
				self.keys_held.remove(&key);
				self.keys_released.insert(key);
			}
		}
	}

	/* Frame clearing consumes press and release edges while preserving held levels for continuous matrix scanning. */
	pub fn clear_frame(&mut self) {
		self.keys_pressed.clear();
		self.keys_released.clear();
	}

	pub fn key_held(&self, key: KeyCode) -> bool {
		self.keys_held.contains(&key)
	}

	/* Host keys are translated into the CIA keyboard matrix after joystick routing. Keys consumed by a virtual joystick are withheld from the matrix to avoid generating both joystick and keyboard actions. */
	pub fn map_keyboard_to_cia(&self, cia1: &mut Cia1, virt_mode: VirtualMode) {
		use winit::keyboard::KeyCode::*;

		let keys = [
			KeyA,
			KeyB,
			KeyC,
			KeyD,
			KeyE,
			KeyF,
			KeyG,
			KeyH,
			KeyI,
			KeyJ,
			KeyK,
			KeyL,
			KeyM,
			KeyN,
			KeyO,
			KeyP,
			KeyQ,
			KeyR,
			KeyS,
			KeyT,
			KeyU,
			KeyV,
			KeyW,
			KeyX,
			KeyY,
			KeyZ,
			Digit0,
			Digit1,
			Digit2,
			Digit3,
			Digit4,
			Digit5,
			Digit6,
			Digit7,
			Digit8,
			Digit9,
			Space,
			Enter,
			Backspace,
			Escape,
			ShiftLeft,
			ShiftRight,
			ControlLeft,
			AltRight,
			F1,
			F2,
			F3,
			F4,
			F5,
			F6,
			F7,
			F8,
			Comma,
			Period,
			Slash,
			Semicolon,
			Equal,
			Minus,
			BracketLeft,
			BracketRight,
			Backslash,
			Quote,
			Backquote,
			ArrowUp,
			ArrowDown,
			ArrowLeft,
			ArrowRight,
			F9,
			F10,
			F12,
		];

		let virt_active = virt_mode != VirtualMode::None;

		for k in keys {
			if self.keys_held.contains(&k) {
				if virt_active
					&& matches!(
						k,
						ArrowUp | ArrowDown | ArrowLeft | ArrowRight | ShiftLeft | ShiftRight
					)
				{
					continue;
				}
				if let Some((r, c)) = key_to_matrix(k) {
					cia1.set_key_pressed(r, c);
				}
				if matches!(k, F2 | F4 | F6 | F8) {
					cia1.set_key_pressed(7, 1);
				}
			}
		}
	}
}