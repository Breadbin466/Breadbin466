// =======================================================
// src/fdd1541/via_control.rs — MOS 6522 control-line transitions
// =======================================================

use super::{
	constants::{VIA_IFR_CA1, VIA_IFR_T2},
	via::ViaChip,
};

impl ViaChip {
	/* CA1 edge polarity comes from PCR. A qualifying edge captures port A when latching is enabled, releases the CA2 handshake output when configured, and sets the CA1 interrupt flag. */
	pub fn set_ca1(&mut self, level: bool) {
		let positive = (self.pcr & 0x01) != 0;
		let edge = if positive {
			!self.ca1_prev && level
		} else {
			self.ca1_prev && !level
		};
		self.ca1_prev = level;
		if edge {
			self.latch_a = (self.ora & self.ddra) | (self.ira & !self.ddra);
			if (self.pcr & 0x0E) == 0x08 {
				self.ca2_handshake = true;
			}
			self.set_flag(VIA_IFR_CA1);
		}
	}

	/* In Timer 2 pulse-count mode only falling transitions on the effective PB6 input decrement the counter. */
	pub fn set_pb6(&mut self, level: bool) {
		let falling = self.pb6_prev && !level;
		self.pb6_prev = level;
		if !falling
			|| !self.t2_running
			|| (self.acr & 0x20) == 0
			|| !self.t2_pulse_count_mode_previous
		{
			return;
		}
		self.t2_counter = self.t2_counter.wrapping_sub(1);
		self.t2_zero_pending = self.t2_counter == 0;
		if self.t2_zero_pending && !self.t2_interrupt_issued {
			self.set_flag(VIA_IFR_T2);
			self.t2_interrupt_issued = true;
			self.t2_zero_pending = false;
		}
	}

	/* Accessing port A completes a CA2 handshake or starts a two-cycle pulse according to the selected output mode. */
	#[inline(always)]
	pub(crate) fn apply_ca2_access(&mut self) {
		match self.pcr & 0x0E {
			0x08 => {
				self.ca2_handshake = false;
				self.ca2_pulse_cycles = 0;
			}
			0x0A => {
				self.ca2_handshake = false;
				self.ca2_pulse_cycles = 2;
			}
			_ => {}
		}
	}

	/* Port B applies the same handshake and pulse rules to CB2 using the upper PCR field. */
	#[inline(always)]
	pub(crate) fn apply_cb2_access(&mut self) {
		match self.pcr & 0xE0 {
			0x80 => {
				self.cb2_handshake = false;
				self.cb2_pulse_cycles = 0;
			}
			0xA0 => {
				self.cb2_handshake = false;
				self.cb2_pulse_cycles = 2;
			}
			_ => {}
		}
	}

	/* PCR writes immediately establish the idle level for manual, handshake and pulse output modes. */
	pub(crate) fn update_ca2_output(&mut self) {
		match self.pcr & 0x0E {
			0x08 | 0x0A => {
				self.ca2_handshake = true;
				self.ca2_pulse_cycles = 0;
			}
			0x0C => {
				self.ca2_handshake = false;
				self.ca2_pulse_cycles = 0;
			}
			0x0E => {
				self.ca2_handshake = true;
				self.ca2_pulse_cycles = 0;
			}
			_ => {}
		}
	}

	pub(crate) fn update_cb2_output(&mut self) {
		match self.pcr & 0xE0 {
			0x80 | 0xA0 => {
				self.cb2_handshake = true;
				self.cb2_pulse_cycles = 0;
			}
			0xC0 => {
				self.cb2_handshake = false;
				self.cb2_pulse_cycles = 0;
			}
			0xE0 => {
				self.cb2_handshake = true;
				self.cb2_pulse_cycles = 0;
			}
			_ => {}
		}
	}

	/* External pin values are stored separately from the output register and DDR; reads combine all three at the point of access. */
	pub fn set_port_a_input(&mut self, val: u8) {
		self.ira = val;
	}

	/* Updating port B also re-evaluates PB6 because that pin may be the external clock source for Timer 2. */
	pub fn set_port_b_input(&mut self, val: u8) {
		self.irb = val;
		self.update_pb6_input();
	}

	#[inline(always)]
	pub(crate) fn update_pb6_input(&mut self) {
		self.set_pb6((self.irb & !self.ddrb & 0x40) != 0);
	}
}