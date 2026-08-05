// =======================================================
// src/cia/serial.rs — MOS 6526A serial shift register
// =======================================================

#[derive(Clone, Copy, Default)]
/* The visible SDR byte is kept separate from the active shift byte so software can queue the following transfer. */
pub struct SerialShiftRegister {
	pub data:          u8,
	pub shift_data:    u8,
	pub shift_count:   u8,
	pub shifting:      bool,
	pub input_mode:    bool,
	pub output_bit:    bool,
	pub cnt_output:    bool,
	pub write_pending: bool,
}

impl SerialShiftRegister {
	#[inline(always)]
	/* The serial port transfers eight bits synchronously through SP and requests an interrupt after a complete byte (MOS-6526-1981, Serial Port). */
	/* Output mode alternates CNT phases: one phase exposes the next SP bit and the other advances the shift register. A queued byte replaces the completed byte without dropping the active serial stream. */
	pub fn tick(&mut self, clock_pulse: bool, sp_in: bool) -> (bool, bool) {
		if !self.shifting || !clock_pulse {
			return (self.output_bit, false);
		}

		/* Input mode consumes one SP level per CNT edge; output mode needs alternating phases for bit presentation and shifting. */
		if self.input_mode {
			self.shift_data = (self.shift_data << 1) | u8::from(sp_in);
			self.shift_count += 1;
			if self.shift_count == 8 {
				self.data = self.shift_data;
				self.shift_count = 0;
				self.shifting = false;
				return (true, true);
			}
			return (true, false);
		}

		self.cnt_output = !self.cnt_output;
		if !self.cnt_output {
			self.output_bit = (self.shift_data & 0x80) != 0;
			return (self.output_bit, false);
		}

		self.shift_data <<= 1;
		self.shift_count += 1;
		if self.shift_count == 8 {
			self.shift_count = 0;
			if self.write_pending {
				self.shift_data = self.data;
				self.write_pending = false;
			} else {
				self.shifting = false;
				self.output_bit = true;
				self.cnt_output = true;
			}
			return (self.output_bit, true);
		}
		(self.output_bit, false)
	}

	#[inline(always)]
	/* In output mode, writing SDR supplies the byte shifted under Timer A control; a following write may be buffered while transmission is active (MOS-6526-1981, Serial Port). */
	pub fn write_data(&mut self, value: u8, output_mode: bool) {
		self.data = value;
		if !output_mode {
			return;
		}
		if self.shifting {
			self.write_pending = true;
		} else {
			self.start_output(value);
		}
	}

	#[inline(always)]
	/* Input reception starts with an empty shift register and samples one SP bit per accepted CNT edge until a complete byte is assembled. */
	pub fn start_input(&mut self) {
		self.shift_data = 0;
		self.shift_count = 0;
		self.shifting = true;
		self.input_mode = true;
		self.output_bit = true;
		self.cnt_output = true;
	}

	#[inline(always)]
	/* Output begins idle-high; the first active CNT phase presents the most significant data bit before any shift occurs. */
	fn start_output(&mut self, value: u8) {
		self.shift_data = value;
		self.shift_count = 0;
		self.shifting = true;
		self.input_mode = false;
		self.output_bit = true;
		self.cnt_output = true;
	}

	#[inline(always)]
	/* Stopping the serial engine restores idle pin levels and discards any queued byte, making a later transfer start from a clean state. */
	pub fn stop(&mut self) {
		self.shift_count = 0;
		self.shifting = false;
		self.output_bit = true;
		self.cnt_output = true;
		self.write_pending = false;
	}
}