// =======================================================
// src/ui/joystick.rs — Host Gamepad and Joystick Input Subsystem
// =======================================================

use crate::emulator::Result;
use crate::emulator::command_line::JoystickSelection;
use crate::ui::constants::THRESHOLD;
use gilrs::{Axis, Button, Event, EventType, Gamepad, Gilrs};
use winit::keyboard::KeyCode;

/* VirtualMode selects whether keyboard-held directions are electrically combined with joystick port 1, port 2, or neither. */
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VirtualMode {
	None,
	Port2,
	Port1,
}

/* JoystickHost merges two host input paths: gilrs devices are assigned to physical ports, while a keyboard-backed virtual joystick can drive one selected port. Both produce active-low CIA bytes and therefore combine by bitwise AND. */
pub struct JoystickHost {
	gilrs: Gilrs,
	physical_enabled: bool,
	port1_reserved: bool,
	pub connected_count: usize,
	pub swapped: bool,
	pub virtual_mode: VirtualMode,
}

impl JoystickHost {
	/* Construction snapshots currently connected gamepads and drains startup events so later polling reports only changes that occur after initialisation. */
	pub fn new() -> Result<Self> {
		let mut gilrs = Gilrs::new().map_err(|e| format!("Gilrs init error: {}", e))?;

		let mut count = 0;
		for (_id, gamepad) in gilrs.gamepads() {
			if gamepad.is_connected() {
				count += 1;
			}
		}

		while let Some(_) = gilrs.next_event() {}

		Ok(Self {
			gilrs,
			physical_enabled: true,
			port1_reserved: false,
			connected_count: count,
			swapped: false,
			virtual_mode: VirtualMode::None,
		})
	}

	/* Configuration resolves command-line policy into one physical or virtual routing mode without changing the low-level port encoding. */
	pub fn configure(&mut self, selection: JoystickSelection, port: u8) -> Result<()> {
		let virtual_mode = if port == 1 {
			VirtualMode::Port1
		} else {
			VirtualMode::Port2
		};
		match selection {
			JoystickSelection::Auto => {
				self.physical_enabled = true;
				self.virtual_mode = VirtualMode::None;
			}
			JoystickSelection::Keyboard => {
				self.physical_enabled = false;
				self.virtual_mode = virtual_mode;
			}
			JoystickSelection::Gilrs => {
				if self.connected_count == 0 {
					return Err("No GILRS joystick is connected.".into());
				}
				self.physical_enabled = true;
				self.virtual_mode = VirtualMode::None;
				self.swapped = port == 1;
			}
			JoystickSelection::None => {
				self.physical_enabled = false;
				self.virtual_mode = VirtualMode::None;
			}
		}
		Ok(())
	}

	/* Cycling first refreshes device events, including while emulation is paused.
	 * It honours control-port ownership. When a 1351 reserves port 1,
	 * joystick routing can still select port 2 or no joystick, but it can never
	 * attach a host joystick to the occupied port. */
	pub fn cycle(&mut self) {
		self.process_gilrs_events();
		if self.connected_count > 0 {
			if self.port1_reserved {
				self.swapped = false;
				self.physical_enabled = !self.physical_enabled;
			} else {
				self.physical_enabled = true;
				self.swapped = !self.swapped;
			}
		} else if self.port1_reserved {
			self.virtual_mode = match self.virtual_mode {
				VirtualMode::None => VirtualMode::Port2,
				VirtualMode::Port2 | VirtualMode::Port1 => VirtualMode::None,
			};
		} else {
			self.virtual_mode = match self.virtual_mode {
				VirtualMode::None => VirtualMode::Port2,
				VirtualMode::Port2 => VirtualMode::Port1,
				VirtualMode::Port1 => VirtualMode::None,
			};
		}
	}

	/* Port 1 is physically exclusive. Reserving it for another peripheral
	 * disconnects any joystick currently routed there and prevents subsequent
	 * cycling from selecting it. Releasing the reservation does not silently
	 * restore a previously disconnected joystick. */
	pub fn set_port1_reserved(&mut self, reserved: bool) {
		if self.port1_reserved == reserved {
			return;
		}

		self.port1_reserved = reserved;
		if !reserved {
			return;
		}

		if self.connected_count > 0 {
			if self.connected_count == 1 && self.physical_enabled && self.swapped {
				self.physical_enabled = false;
			}
			self.swapped = false;
		}

		if self.virtual_mode == VirtualMode::Port1 {
			self.virtual_mode = VirtualMode::None;
		}
	}

	/* Polling first consumes connection events, then combines physical and virtual sources into the two bytes presented to the CIA keyboard/joystick matrix. */
	pub fn poll_manual(&mut self, input: &crate::ui::InputState) -> (u8, u8) {
		self.process_gilrs_events();

		let (mut j1, mut j2) = if self.physical_enabled {
			self.read_physical_state()
		} else {
			(0xFF, 0xFF)
		};
		if self.port1_reserved {
			j1 = 0xFF;
		}

		if self.virtual_mode != VirtualMode::None {
			let v_byte = self.read_virtual_state_manual(input);
			match self.virtual_mode {
				VirtualMode::Port2 => j2 &= v_byte,
				VirtualMode::Port1 => j1 &= v_byte,
				_ => {}
			}
		}

		(j1, j2)
	}

	fn process_gilrs_events(&mut self) {
		while let Some(Event { event, .. }) = self.gilrs.next_event() {
			match event {
				EventType::Connected => {
					self.connected_count += 1;
					if self.virtual_mode != VirtualMode::None {
						self.virtual_mode = VirtualMode::None;
					}
				}
				EventType::Disconnected => {
					if self.connected_count > 0 {
						self.connected_count -= 1;
					}
				}
				_ => (),
			}
		}
	}

	/* The first two connected gamepads map to ports 2 and 1 by default, matching the common single-player convention; swapping reverses only that assignment. */
	fn read_physical_state(&self) -> (u8, u8) {
		let mut port1 = 0xFF;
		let mut port2 = 0xFF;
		let mut device_index = 0;

		for (_id, device) in self.gilrs.gamepads() {
			if !device.is_connected() {
				continue;
			}

			let state = self.read_stick_state(&device);
			let target_is_port2 = if !self.swapped {
				device_index == 0
			} else {
				device_index == 1
			};
			let target_is_port1 = if !self.swapped {
				device_index == 1
			} else {
				device_index == 0
			};

			if target_is_port2 {
				port2 &= state;
			} else if target_is_port1 {
				port1 &= state;
			}
			device_index += 1;
		}
		(port1, port2)
	}

	fn read_stick_state(&self, device: &Gamepad) -> u8 {
		let mut cia_byte = 0xFF;

		let val_x = device.value(Axis::LeftStickX);
		let val_y = device.value(Axis::LeftStickY);

		let stick_up = device.is_pressed(Button::DPadUp) || (val_y > THRESHOLD);
		let stick_down = device.is_pressed(Button::DPadDown) || (val_y < -THRESHOLD);
		let stick_left = device.is_pressed(Button::DPadLeft) || (val_x < -THRESHOLD);
		let stick_right = device.is_pressed(Button::DPadRight) || (val_x > THRESHOLD);

		let fire = device.is_pressed(Button::South)
			|| device.is_pressed(Button::West)
			|| device.is_pressed(Button::East);

		if stick_up {
			cia_byte &= !0x01;
		}
		if stick_down {
			cia_byte &= !0x02;
		}
		if stick_left {
			cia_byte &= !0x04;
		}
		if stick_right {
			cia_byte &= !0x08;
		}
		if fire {
			cia_byte &= !0x10;
		}

		cia_byte
	}

	fn read_virtual_state_manual(&self, input: &crate::ui::InputState) -> u8 {
		let mut v_byte = 0xFF;
		if input.key_held(KeyCode::ArrowUp) {
			v_byte &= !0x01;
		}
		if input.key_held(KeyCode::ArrowDown) {
			v_byte &= !0x02;
		}
		if input.key_held(KeyCode::ArrowLeft) {
			v_byte &= !0x04;
		}
		if input.key_held(KeyCode::ArrowRight) {
			v_byte &= !0x08;
		}
		if input.key_held(KeyCode::ShiftLeft) || input.key_held(KeyCode::ShiftRight) {
			v_byte &= !0x10;
		}
		v_byte
	}

	pub fn get_status_string(&self) -> String {
		if self.connected_count > 0 && self.physical_enabled {
			if self.port1_reserved {
				"USB (Port 2)".to_string()
			} else if self.connected_count == 1 {
				if !self.swapped {
					"USB (Port 2)".to_string()
				} else {
					"USB (Port 1)".to_string()
				}
			} else {
				"USB (2 devices)".to_string()
			}
		} else {
			match self.virtual_mode {
				VirtualMode::None => "None".to_string(),
				VirtualMode::Port2 => "Keyboard (Port 2)".to_string(),
				VirtualMode::Port1 => "Keyboard (Port 1)".to_string(),
			}
		}
	}
}