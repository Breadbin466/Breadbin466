// =======================================================
// src/cia/cia1.rs — CIA 1 (Keyboard, Joysticks, Timer Outputs, Pins)
// =======================================================

use super::cia::Cia;
use super::constants::CR_PBON;

/* CIA 1 adds the C64 keyboard and joystick wiring around the reusable 6526 core. */
pub struct Cia1 {
	pub inner: Cia,
	pub keyboard_matrix: [u8; 8],
	pub joystick_1: u8,
	pub joystick_2: u8,
}

impl Cia1 {
	/* CIA 1 starts with all keyboard and joystick lines released while the reusable core assumes its reset pin state. */
	pub fn new() -> Self {
		Self {
			inner: Cia::new(),
			keyboard_matrix: [0xFF; 8],
			joystick_1: 0xFF,
			joystick_2: 0xFF,
		}
	}

	/* Reset clears the core and releases every key and joystick line so no host-input state survives. */
	pub fn reset(&mut self) {
		self.inner.reset();
		self.keyboard_matrix = [0xFF; 8];
		self.joystick_1 = 0xFF;
		self.joystick_2 = 0xFF;
	}

	#[inline(always)]
	/* The core advances first, then PB6 and PB7 are refreshed from the timer outputs produced by that same cycle. */
	pub fn tick(&mut self, tod_pulse: bool) {
		self.inner.tick(tod_pulse);
	}

	pub fn peek(&self, addr: u16) -> u8 {
		match addr & 0x000F {
			0x00 => self.read_port_a(),
			0x01 => self.read_port_b(),
			reg => self.inner.peek(reg as u8),
		}
	}

	#[inline(always)]
	/* CPU-visible reads route the two wired ports through the keyboard/joystick matrix and delegate every other register to the shared core. */
	pub fn read(&mut self, addr: u16) -> u8 {
		match addr & 0x000F {
			0x00 => self.read_port_a(),
			0x01 => self.read_port_b(),
			reg => self.inner.read(reg as u8),
		}
	}

	#[inline(always)]
	/* Register writes are decoded on the CIA's repeated sixteen-byte window before reaching the shared core. */
	pub fn write(&mut self, addr: u16, val: u8) {
		self.inner.write((addr & 0x000F) as u8, val);
	}

	/* CIA 1 scans the keyboard as an active-low matrix and shares its two ports with the joystick inputs (C64-PRG-1982, Keyboard and joystick interfaces). */
	/* Port reads combine the output latch on driven bits with the keyboard and joystick levels on input bits. The matrix is therefore evaluated from the actual pin state, not from the raw port register alone. */
	fn read_port_a(&self) -> u8 {
		let out_b = self.inner.prb | !self.inner.ddrb;
		let mut val = 0xFF;

		for row in 0..8 {
			if (out_b & (1 << row)) == 0 {
				val &= self.keyboard_matrix[row];
			}
		}

		val &= self.joystick_2;
		val & (self.inner.pra | !self.inner.ddra)
	}

	/* Driving selected CIA 1 port-A columns low makes pressed keys appear as low rows on port B, which also carries joystick 1 (C64-PRG-1982, Keyboard and joystick interfaces). */
	/* Enabling PBON substitutes Timer A and Timer B outputs for port-B bits 6 and 7 respectively (MOS-6526-1981, Timer Output to Port B). */
	/* The keyboard matrix is scanned in both directions because software may drive either CIA port and read the other. Joystick lines are then combined with the same active-low pin levels. */
	fn read_port_b(&self) -> u8 {
		let out_a = self.inner.pra | !self.inner.ddra;
		let mut val = 0xFF;

		for col in 0..8 {
			if (out_a & (1 << col)) == 0 {
				for row in 0..8 {
					if (self.keyboard_matrix[row] & (1 << col)) == 0 {
						val &= !(1 << row);
					}
				}
			}
		}

		val &= self.joystick_1;
		val &= self.inner.prb | !self.inner.ddrb;

		let (ta_out, tb_out) = self.inner.timer_outputs();

		if (self.inner.ta.cr & CR_PBON) != 0 {
			if ta_out {
				val |= 0x40;
			} else {
				val &= !0x40;
			}
		}

		if (self.inner.tb.cr & CR_PBON) != 0 {
			if tb_out {
				val |= 0x80;
			} else {
				val &= !0x80;
			}
		}

		val
	}

	/* A pressed key clears one matrix connection; clearing the matrix releases every key at once. */
	pub fn set_key_pressed(&mut self, row: u8, col: u8) {
		if row < 8 && col < 8 {
			self.keyboard_matrix[row as usize] &= !(1 << col);
		}
	}

	/* Clearing the matrix releases all keys without disturbing joystick state or CIA registers. */
	pub fn clear_keyboard_matrix(&mut self) {
		self.keyboard_matrix = [0xFF; 8];
	}

	/* The physical port-A pin levels select which control port reaches the shared SID POT inputs. */
	pub fn port_a_pin_levels(&self) -> u8 {
		self.inner.pra | !self.inner.ddra
	}

	/* PB4 shares the active-low keyboard/joystick line with the VIC-II light-pen input. */
	pub fn light_pen_pin_high(&self) -> bool {
		self.read_port_b() & 0x10 != 0
	}

	pub fn set_flag_pin(&mut self, state: bool) {
		self.inner.set_flag_pin(state);
	}
	pub fn sp_output(&self) -> bool {
		self.inner.sp_output()
	}
	pub fn is_irq_active(&self) -> bool {
		self.inner.irq_line
	}
}