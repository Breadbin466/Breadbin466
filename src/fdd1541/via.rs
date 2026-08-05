// =======================================================
// src/fdd1541/via.rs — Abstract MOS 6522 VIA (timers, ports, IRQ logic, shift register)
// =======================================================

use super::constants::{
	VIA_IFR_CA1, VIA_IFR_CA2, VIA_IFR_CB1, VIA_IFR_CB2, VIA_IFR_IRQ, VIA_IFR_SR,
	VIA_IFR_T1, VIA_IFR_T2,
};

#[inline(always)]
pub(crate) fn shift_register_mode(acr: u8) -> u8 {
	(acr >> 2) & 0x07
}

#[derive(Clone)]
/* ViaChip models one complete MOS 6522 clock domain. Each tick first advances pulse outputs and timer state, then lets Timer 2 provide shift-register clocks before the aggregated IFR/IER gate determines the IRQ output. Port reads and writes are not passive storage operations: they acknowledge edge flags, start handshakes, schedule pulses and expose either live pins or latched inputs according to ACR. CA1/CB1 sample external edges, CA2/CB2 can act as inputs or programmed outputs, and PB7 may be driven by Timer 1 instead of the port latch. Via1 and Via2 add the board-specific IEC and mechanism wiring around this shared core. */
pub struct ViaChip {
	pub ora: u8,
	pub orb: u8,
	pub ddra: u8,
	pub ddrb: u8,
	pub ira: u8,
	pub irb: u8,

	pub t1_counter: u16,
	pub t1_latch: u16,
	pub t1_start_delay: bool,
	pub t1_running: bool,
	pub t1_one_shot_complete: bool,
	pub(crate) t1_reload_pending: bool,
	pub(crate) t1_interrupts_enabled: bool,

	pub t2_counter: u16,
	pub t2_latch_lo: u8,
	pub t2_start_delay: bool,
	pub t2_running: bool,
	pub t2_interrupt_issued: bool,
	pub(crate) t2_zero_pending: bool,
	pub(crate) t2_low_byte_wraps: u8,
	pub(crate) t2_shift_clock_high: bool,
	pub(crate) t2_shift_edge: bool,
	pub(crate) t2_pulse_count_mode_previous: bool,

	pub sr: u8,
	pub sr_count: u8,
	pub(crate) shift_output_bit: bool,

	pub(crate) cb1_prev: bool,
	pub(crate) cb2_prev: bool,
	pub(crate) pb6_prev: bool,

	pub acr: u8,
	pub pcr: u8,
	pub ifr: u8,
	pub ier: u8,

	pub ca1_prev: bool,
	pub(crate) ca2_prev: bool,
	pub cb2_in: bool,
	pub ca2_handshake: bool,
	pub(crate) ca2_pulse_cycles: u8,
	pub cb2_handshake: bool,
	pub(crate) cb2_pulse_cycles: u8,

	pub(crate) latch_a: u8,
	pub(crate) latch_b: u8,

	pub pb7: bool,
}

impl ViaChip {
	pub fn new() -> Self {
		Self {
			ora: 0,
			orb: 0,
			ddra: 0,
			ddrb: 0,
			ira: 0xFF,
			irb: 0xFF,
			t1_counter: 0xFFFF,
			t1_latch: 0xFFFF,
			t1_start_delay: false,
			t1_running: false,
			t1_one_shot_complete: false,
			t1_reload_pending: false,
			t1_interrupts_enabled: true,
			t2_counter: 0xFBC9,
			t2_latch_lo: 0x00,
			t2_start_delay: false,
			t2_running: false,
			t2_interrupt_issued: false,
			t2_zero_pending: false,
			t2_low_byte_wraps: 0,
			t2_shift_clock_high: true,
			t2_shift_edge: false,
			t2_pulse_count_mode_previous: false,
			sr: 0,
			sr_count: 0,
			shift_output_bit: true,
			cb1_prev: false,
			cb2_prev: true,
			pb6_prev: false,
			acr: 0,
			pcr: 0,
			ifr: 0,
			ier: 0,
			ca1_prev: false,
			ca2_prev: true,
			cb2_in: true,
			ca2_handshake: true,
			ca2_pulse_cycles: 0,
			cb2_handshake: true,
			cb2_pulse_cycles: 0,
			latch_a: 0xFF,
			latch_b: 0xFF,
			pb7: true,
		}
	}

	pub fn reset(&mut self) {
		*self = Self::new();
	}

	#[inline(always)]
	pub(crate) fn update_irq(&mut self) {
		if (self.ifr & self.ier & 0x7F) != 0 {
			self.ifr |= VIA_IFR_IRQ;
		} else {
			self.ifr &= !VIA_IFR_IRQ;
		}
	}

	#[inline(always)]
	pub(crate) fn set_flag(&mut self, bit: u8) {
		self.ifr |= bit;
		self.update_irq();
	}

	#[inline(always)]
	pub(crate) fn clear_flag(&mut self, bit: u8) {
		self.ifr &= !bit;
		self.update_irq();
	}

	/* The external IRQ output is asserted only when at least one latched IFR source is enabled by IER; bit 7 of a register read is derived from the same condition. */
	pub fn irq_line(&self) -> bool {
		(self.ifr & VIA_IFR_IRQ) != 0
	}

	#[inline(always)]
	pub(crate) fn tick_shift_register(&mut self, timer_shift_edge: bool) {
		let mode = shift_register_mode(self.acr);
		if mode == 0 {
			return;
		}

		let shift_now = match mode {
			1 | 4 | 5 => timer_shift_edge,
			2 | 6 => true,
			_ => false,
		};

		if !shift_now {
			return;
		}

		if mode == 4 {
			self.shift_output_bit = (self.sr & 0x80) != 0;
			self.sr = self.sr.rotate_left(1);
			return;
		}

		if self.sr_count < 8 {
			self.shift_one_bit(mode);
		}
	}

	/* CB1 edges can acknowledge handshakes, latch an interrupt and clock the shift register, so all three effects are resolved from the same sampled transition. */
	pub fn on_cb1_edge(&mut self, level: bool) {
		let previous = self.cb1_prev;
		let falling = previous && !level;
		let positive = (self.pcr & 0x10) != 0;
		let selected_edge = if positive {
			!previous && level
		} else {
			falling
		};
		self.cb1_prev = level;

		if selected_edge {
			self.latch_b = (self.orb & self.ddrb) | (self.irb & !self.ddrb);
			if (self.pcr & 0xE0) == 0x80 {
				self.cb2_handshake = true;
			}
			self.set_flag(VIA_IFR_CB1);
		}

		let mode = shift_register_mode(self.acr);
		if falling && matches!(mode, 3 | 7) && self.sr_count < 8 {
			self.shift_one_bit(mode);
		}
	}

	pub fn set_ca2(&mut self, level: bool) {
		if (self.pcr & 0x08) != 0 {
			self.ca2_prev = level;
			return;
		}

		let positive = (self.pcr & 0x04) != 0;
		let edge = if positive {
			!self.ca2_prev && level
		} else {
			self.ca2_prev && !level
		};
		self.ca2_prev = level;

		if edge {
			self.set_flag(VIA_IFR_CA2);
		}
	}

	pub fn set_cb2(&mut self, level: bool) {
		self.cb2_in = level;
		if (self.pcr & 0x80) != 0 {
			self.cb2_prev = level;
			return;
		}

		let positive = (self.pcr & 0x40) != 0;
		let edge = if positive {
			!self.cb2_prev && level
		} else {
			self.cb2_prev && !level
		};
		self.cb2_prev = level;

		if edge {
			self.set_flag(VIA_IFR_CB2);
		}
	}

	#[inline(always)]
	pub(crate) fn shift_one_bit(&mut self, mode: u8) {
		if matches!(mode, 5 | 6 | 7) {
			self.shift_output_bit = (self.sr & 0x80) != 0;
			self.sr = self.sr.rotate_left(1);
		} else {
			self.sr = (self.sr << 1) | u8::from(self.cb2_in);
		}

		self.sr_count = self.sr_count.saturating_add(1);
		if self.sr_count == 8 {
			self.set_flag(VIA_IFR_SR);
		}
	}

	#[inline(always)]
	fn port_a_value(&self) -> u8 {
		if (self.acr & 0x01) != 0 && (self.ifr & VIA_IFR_CA1) != 0 {
			self.latch_a
		} else {
			(self.ora & self.ddra) | (self.ira & !self.ddra)
		}
	}

	#[inline(always)]
	fn port_b_value(&self) -> u8 {
		let input = if (self.acr & 0x02) != 0 && (self.ifr & VIA_IFR_CB1) != 0 {
			self.latch_b
		} else {
			self.irb
		};
		let port_value = if (self.acr & 0x02) != 0 && (self.ifr & VIA_IFR_CB1) != 0 {
			(self.orb & self.ddrb) | (input & !self.ddrb)
		} else {
			(self.orb & self.ddrb) | (self.irb & !self.ddrb)
		};
		if (self.acr & 0x80) != 0 {
			(port_value & 0x7F) | if self.pb7 { 0x80 } else { 0x00 }
		} else {
			port_value
		}
	}

	#[inline(always)]
	/* Register reads include 6522 side effects: port and timer accesses acknowledge selected interrupt flags rather than behaving as passive memory. */
	pub fn read(&mut self, reg: u8) -> u8 {
		match reg & 0x0F {
			0x00 => {
				let value = self.port_b_value();
				self.clear_flag(VIA_IFR_CB1 | VIA_IFR_CB2);
				self.apply_cb2_access();
				value
			}
			0x01 => {
				let value = self.port_a_value();
				self.clear_flag(VIA_IFR_CA1 | VIA_IFR_CA2);
				self.apply_ca2_access();
				value
			}
			0x02 => self.ddrb,
			0x03 => self.ddra,
			0x04 => {
				self.clear_flag(VIA_IFR_T1);
				self.t1_counter as u8
			}
			0x05 => (self.t1_counter >> 8) as u8,
			0x06 => self.t1_latch as u8,
			0x07 => (self.t1_latch >> 8) as u8,
			0x08 => {
				self.clear_flag(VIA_IFR_T2);
				self.t2_counter as u8
			}
			0x09 => (self.t2_counter >> 8) as u8,
			0x0A => {
				let completed = (self.ifr & VIA_IFR_SR) != 0;
				let value = self.sr;
				self.clear_flag(VIA_IFR_SR);
				if completed {
					self.sr_count = 0;
				}
				value
			}
			0x0B => self.acr,
			0x0C => self.pcr,
			0x0D => self.ifr,
			0x0E => self.ier | 0x80,
			0x0F => self.port_a_value(),
			_ => 0xFF,
		}
	}

	#[inline(always)]
	/* Register writes update latches and modes immediately, while timer reload and handshake effects follow the sequencing encoded by the 6522 core. */
	pub fn write(&mut self, reg: u8, val: u8) {
		match reg & 0x0F {
			0x00 => {
				self.orb = val;
				self.clear_flag(VIA_IFR_CB1 | VIA_IFR_CB2);
				self.apply_cb2_access();
			}
			0x01 => {
				self.ora = val;
				self.clear_flag(VIA_IFR_CA1 | VIA_IFR_CA2);
				self.apply_ca2_access();
			}
			0x02 => {
				self.ddrb = val;
				self.update_pb6_input();
			}
			0x03 => self.ddra = val,
			0x04 => self.t1_latch = (self.t1_latch & 0xFF00) | u16::from(val),
			0x05 => {
				self.t1_latch = (self.t1_latch & 0x00FF) | (u16::from(val) << 8);
				self.t1_counter = self.t1_latch;
				self.t1_start_delay = true;
				self.t1_running = true;
				self.t1_one_shot_complete = false;
				self.t1_reload_pending = self.t1_counter == 0;
				self.t1_interrupts_enabled = true;
				self.clear_flag(VIA_IFR_T1);
				if (self.acr & 0x80) != 0 {
					self.pb7 = false;
				}
			}
			0x06 => self.t1_latch = (self.t1_latch & 0xFF00) | u16::from(val),
			0x07 => {
				self.t1_latch = (self.t1_latch & 0x00FF) | (u16::from(val) << 8);
				if (self.ier & VIA_IFR_T1) == 0 {
					self.t1_interrupts_enabled = false;
				}
				self.clear_flag(VIA_IFR_T1);
			}
			0x08 => self.t2_latch_lo = val,
			0x09 => {
				self.t2_counter = (u16::from(val) << 8) | u16::from(self.t2_latch_lo);
				self.t2_start_delay = true;
				self.t2_running = true;
				self.t2_interrupt_issued = false;
				self.t2_zero_pending = false;
				self.t2_low_byte_wraps = 0;
				self.t2_shift_clock_high = true;
				self.t2_shift_edge = false;
				self.clear_flag(VIA_IFR_T2);
			}
			0x0A => {
				let completed = (self.ifr & VIA_IFR_SR) != 0;
				self.sr = val;
				if completed {
					self.sr_count = 0;
				}
				self.shift_output_bit = (val & 0x80) != 0;
				self.t2_shift_clock_high = true;
				self.t2_shift_edge = false;
				self.clear_flag(VIA_IFR_SR);
			}
			0x0B => self.acr = val,
			0x0C => {
				self.pcr = val;
				self.update_ca2_output();
				self.update_cb2_output();
			}
			0x0D => {
				self.ifr &= !(val & 0x7F);
				self.update_irq();
			}
			0x0E => {
				if (val & 0x80) != 0 {
					self.ier |= val & 0x7F;
				} else {
					self.ier &= !(val & 0x7F);
				}
				self.update_irq();
			}
			0x0F => self.ora = val,
			_ => {}
		}
	}

}

impl Default for ViaChip {
	fn default() -> Self {
		Self::new()
	}
}