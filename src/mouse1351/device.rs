// =======================================================
// src/mouse1351/device.rs — Commodore 1351 proportional mouse
// =======================================================

use super::constants::{
	LEFT_BUTTON_MASK, PORT_1_SELECT, PORT_SELECT_MASK, POT_SETTLE_CYCLES, REPORT_INTERVAL_CYCLES,
	RIGHT_BUTTON_MASK,
};

/*
 * Mouse1351 models the proportional-mode device connected to control port 1.
 * Host input contributes relative motion and button state only. The device owns
 * the modulo-64 counters seen by the C64, while CIA1 selects the analogue path
 * and SID exposes the resulting POT values. This keeps desktop pointer semantics
 * outside the emulated hardware boundary.
 *
 * Commodore's developer documentation defines proportional position as six bits
 * carried in POT bits 6..1, updated every 512 microseconds (COMMODORE-1351-MANUAL-1986). Bit 7 is unspecified
 * and bit 0 is a noise indicator. Breadbin466 drives bit 7 high and bit 0 low;
 * software using Commodore's published delta algorithm therefore observes the
 * exact six-bit position while remaining insensitive to the unspecified bit.
 */
pub struct Mouse1351 {
	connected: bool,
	position_x: u8,
	position_y: u8,
	latched_x: u8,
	latched_y: u8,
	left_pressed: bool,
	right_pressed: bool,
	last_report_cycle: u64,
	port_selected_since: Option<u64>,
}

impl Mouse1351 {
	pub fn new() -> Self {
		Self {
			connected: false,
			position_x: 0,
			position_y: 0,
			latched_x: Self::encode_position(0),
			latched_y: Self::encode_position(0),
			left_pressed: false,
			right_pressed: false,
			last_report_cycle: 0,
			port_selected_since: None,
		}
	}

	pub fn is_connected(&self) -> bool {
		self.connected
	}

	/* Connecting powers the emulated peripheral in its default proportional mode.
	 * Disconnecting releases both digital lines and removes the POT source. */
	pub fn set_connected(&mut self, connected: bool, cycle: u64, port_a_pins: u8) {
		if self.connected == connected {
			return;
		}
		self.connected = connected;
		self.left_pressed = false;
		self.right_pressed = false;
		self.last_report_cycle = cycle;
		self.port_selected_since = None;
		if connected {
			self.observe_port_selection(port_a_pins, cycle);
		}
	}

	/* A C64 reset does not power-cycle the external mouse. Position and connection
	 * therefore survive, while the analogue selection timing restarts with CIA1. */
	pub fn reset_bus_selection(&mut self, cycle: u64, port_a_pins: u8) {
		self.port_selected_since = None;
		self.last_report_cycle = cycle;
		if self.connected {
			self.observe_port_selection(port_a_pins, cycle);
		}
	}

	/* Host Y coordinates grow downwards. Commodore's published C64 driver subtracts
	 * positive POTY movement from the screen Y coordinate, so downward host motion
	 * decrements the mouse's proportional Y counter. */
	pub fn move_relative(&mut self, dx: i32, dy: i32) {
		if !self.connected {
			return;
		}
		self.position_x = Self::wrap_position(i32::from(self.position_x) + dx);
		self.position_y = Self::wrap_position(i32::from(self.position_y) - dy);
	}

	pub fn set_left_pressed(&mut self, pressed: bool) {
		if self.connected {
			self.left_pressed = pressed;
		}
	}

	pub fn set_right_pressed(&mut self, pressed: bool) {
		if self.connected {
			self.right_pressed = pressed;
		}
	}

	pub fn release_buttons(&mut self) {
		self.left_pressed = false;
		self.right_pressed = false;
	}

	/* The proportional-mode buttons are ordinary active-low joystick signals:
	 * left is FIRE and right is UP. */
	pub fn digital_port_mask(&self) -> u8 {
		if !self.connected {
			return 0xFF;
		}
		let mut mask = 0xFF;
		if self.left_pressed {
			mask &= !LEFT_BUTTON_MASK;
		}
		if self.right_pressed {
			mask &= !RIGHT_BUTTON_MASK;
		}
		mask
	}

	/* CIA1 keyboard scanning can disconnect and reconnect the shared POT path. Every
	 * transition restarts the >1.6 ms validity interval required by Commodore. */
	pub fn observe_port_selection(&mut self, port_a_pins: u8, cycle: u64) {
		if !self.connected {
			self.port_selected_since = None;
			return;
		}
		let selected = port_a_pins & PORT_SELECT_MASK == PORT_1_SELECT;
		match (selected, self.port_selected_since) {
			(true, None) => self.port_selected_since = Some(cycle),
			(false, Some(_)) => self.port_selected_since = None,
			_ => {}
		}
	}

	/* POT values are produced lazily at the CPU read boundary. This preserves the
	 * 512 us mouse report cadence without adding work to every motherboard cycle. */
	pub fn pot_values(&mut self, cycle: u64) -> Option<(u8, u8)> {
		if !self.connected {
			return None;
		}
		let selected_since = self.port_selected_since?;
		if cycle.wrapping_sub(selected_since) < POT_SETTLE_CYCLES {
			return None;
		}
		if cycle.wrapping_sub(self.last_report_cycle) >= REPORT_INTERVAL_CYCLES {
			self.latched_x = Self::encode_position(self.position_x);
			self.latched_y = Self::encode_position(self.position_y);
			self.last_report_cycle = cycle;
		}
		Some((self.latched_x, self.latched_y))
	}

	#[inline]
	fn encode_position(position: u8) -> u8 {
		0x80 | ((position & 0x3F) << 1)
	}

	#[inline]
	fn wrap_position(value: i32) -> u8 {
		value.rem_euclid(64) as u8
	}
}

impl Default for Mouse1351 {
	fn default() -> Self {
		Self::new()
	}
}