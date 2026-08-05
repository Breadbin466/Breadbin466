// =======================================================
// src/fdd1541/via2.rs — Discrete VIA #2 (disk controller, $1C00)
// =======================================================

use super::via::ViaChip;

/* VIA2 connects the 6522 core to the disk electronics: spindle and LED outputs, stepper phase, density selection, read data, byte-ready, sync and write-protect signals. */
pub struct Via2 {
	pub inner: ViaChip,
	head_byte: u8,
	sync_detected: bool,
	write_protect: bool,
	motor_on: bool,
	led_on: bool,
	stepper_phase: u8,
	density: u8,
	head_write_mode: bool,
	so_enabled: bool,
}

impl Via2 {
	/* Construction binds a reset 6522 core to the initial disk-electronics inputs and derives all actuator outputs from the board-visible latch and DDR state. */
	pub fn new() -> Self {
		Self {
			inner: ViaChip::new(),
			head_byte: 0,
			sync_detected: false,
			write_protect: false,
			motor_on: false,
			led_on: false,
			stepper_phase: 0,
			density: 3,
			head_write_mode: false,
			so_enabled: false,
		}
	}

	/* Reset clears programmable VIA state but immediately restores the externally supplied head, sync and write-protect levels before recomputing motor, LED, stepper and density outputs. */
	pub fn reset(&mut self) {
		self.inner.reset();
		self.head_byte = 0;
		self.sync_detected = false;
		self.write_protect = true;
		self.inner.set_port_a_input(0);
		self.inner.set_port_b_input(self.compose_input_b());
		self.inner.set_ca1(true);
		self.inner.on_cb1_edge(true);
		self.inner.set_ca2(true);
		self.inner.set_cb2(true);
		self.refresh_outputs();
	}

	#[inline(always)]
	fn compose_input_b(&self) -> u8 {
		let mut value = 0xFF;
		if self.sync_detected {
			value &= !0x80;
		}
		if self.write_protect {
			value &= !0x10;
		}
		value
	}

	#[inline(always)]
	fn refresh_input_b(&mut self) {
		self.inner.set_port_b_input(self.compose_input_b());
	}

	#[inline(always)]
	fn output_level(&self, mask: u8, floating_high: bool) -> bool {
		if (self.inner.ddrb & mask) != 0 {
			(self.inner.orb & mask) != 0
		} else {
			floating_high
		}
	}

	#[inline(always)]
	/* Board-facing outputs are recomputed from ORB and DDR so changing a direction bit can start the motor or move the head without another data write. Input-configured bits are electrically released rather than interpreted as asserted controls. */
	fn refresh_outputs(&mut self) {
		self.motor_on = self.output_level(0x04, false);
		self.led_on = self.output_level(0x08, false);
		self.stepper_phase = u8::from(self.output_level(0x01, false))
			| (u8::from(self.output_level(0x02, false)) << 1);
		self.density = u8::from(self.output_level(0x20, true))
			| (u8::from(self.output_level(0x40, true)) << 1);
		self.head_write_mode = (self.inner.pcr & 0x80) != 0 && (self.inner.pcr & 0x20) == 0;
		self.so_enabled = (self.inner.pcr & 0x02) != 0;
	}

	#[inline(always)]
	pub fn read(&mut self, addr: u16) -> u8 {
		let reg = (addr & 0x0F) as u8;
		if reg == 0x00 {
			self.inner.set_port_b_input(self.compose_input_b());
		}
		self.inner.read(reg)
	}

	#[inline(always)]
	pub fn write(&mut self, addr: u16, val: u8) {
		let reg = (addr & 0x0F) as u8;
		self.inner.write(reg, val);
		if matches!(reg, 0x00 | 0x02 | 0x0C) {
			self.refresh_outputs();
		}
	}

	#[inline(always)]
	pub fn tick(&mut self) -> bool {
		self.inner.tick()
	}

	#[inline(always)]
	/* The read channel presents a completed byte and a separate byte-ready pulse; firmware observes them through different VIA pins. */
	pub fn set_head_byte(&mut self, byte: u8, pulse: bool) {
		if pulse {
			self.head_byte = byte;
			self.inner.set_port_a_input(byte);
		}
	}

	#[inline(always)]
	pub fn so_enabled(&self) -> bool {
		self.so_enabled
	}

	#[inline(always)]
	/* Byte-ready is wired as an active-low CA1 pulse. Repeated calls at the same level do not create artificial edges. */
	pub fn set_byte_ready(&mut self, pulse: bool) {
		let level = !pulse;
		if self.inner.ca1_prev != level {
			self.inner.set_ca1(level);
		}
	}

	#[inline(always)]
	/* Sync detection is a level derived from a run of one bits on disk, not a decoded byte value. */
	pub fn set_sync(&mut self, detected: bool) {
		if self.sync_detected != detected {
			self.sync_detected = detected;
			self.refresh_input_b();
		}
	}

	#[inline(always)]
	/* Write protect is an active-low external input on port B and is republished only when the mechanical sensor changes. */
	pub fn set_write_protect(&mut self, protect: bool) {
		if self.write_protect != protect {
			self.write_protect = protect;
			self.refresh_input_b();
		}
	}

	#[inline(always)]
	pub fn motor_on(&self) -> bool {
		self.motor_on
	}

	pub fn led_on(&self) -> bool {
		self.led_on
	}

	#[inline(always)]
	pub fn stepper_phase(&self) -> u8 {
		self.stepper_phase
	}

	#[inline(always)]
	pub fn density(&self) -> u8 {
		self.density
	}

	#[inline(always)]
	pub fn head_write_mode(&self) -> bool {
		self.head_write_mode
	}

	#[inline(always)]
	pub fn head_byte_out(&self) -> u8 {
		self.inner.ora
	}
}

impl Default for Via2 {
	fn default() -> Self {
		Self::new()
	}
}