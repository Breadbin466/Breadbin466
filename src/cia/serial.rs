// =======================================================
// src/cia/serial.rs — MOS 6526A serial shift register
// =======================================================

/* The measured output-completion path takes four PHI2 cycles after the
 * eighth outgoing bit reaches SP (CIA-SERIAL-MEASUREMENTS). */
const OUTPUT_IRQ_DELAY: u8 = 4;

#[derive(Clone, Copy, Default)]
/* The visible SDR byte is kept separate from the active shift byte so software can queue the following transfer. */
pub struct SerialShiftRegister {
	pub data: u8,
	pub shift_data: u8,
	pub shift_count: u8,
	pub shifting: bool,
	pub input_mode: bool,
	pub output_bit: bool,
	pub cnt_output: bool,
	pub write_pending: bool,
	/* Remaining PHI2 stages between the final output bit and ICR. */
	pub irq_delay: u8,
	/* Timer A underflow is a level; the divider accepts its leading edge. */
	pub clock_previous: bool,
	/* An SDR write reaches an empty shifter after two PHI2 stages. */
	pub load_delay: u8,
	/* Low CNT samples, youngest first, retain the measured direction-change
	 * propagation phases independently of the byte counter. */
	pub cnt_history: u8,
	/* The byte-counter state reaches the direction-reset path after
	 * the same four PHI2 stages as serial completion. */
	pub progress_history: u8,
	/* A direction change can retain a serial request until output resumes. */
	pub direction_irq: bool,
}

impl SerialShiftRegister {
	#[inline(always)]
	/* The serial port transfers eight bits synchronously through SP and requests an interrupt after a complete byte (MOS-6526-1981, Serial Port). */
	/* Falling CNT presents and removes the next bit. The eighth falling edge
	 * frees the shifter and loads a queued byte; ICR follows through its own
	 * PHI2 delay, even if Timer A stops. Rising CNT leaves SP unchanged. */
	pub fn tick(&mut self, clock_pulse: bool, sp_in: bool) -> (bool, bool) {
		self.progress_history = ((self.progress_history << 1) | u8::from(self.shifting && self.shift_count != 0)) & 0x0f;
		self.cnt_history = ((self.cnt_history << 1) | u8::from(!self.cnt_output)) & 0x0F;
		self.load_delay = self.load_delay.saturating_sub(1);
		let ready = self.load_delay == 0;
		let clock_edge = clock_pulse && ready && (!self.clock_previous || self.input_mode);
		/* An empty shifter rearms the edge detector. A sustained underflow
		 * can start a newly loaded byte, but cannot clock the whole byte. */
		self.clock_previous = clock_pulse && self.shifting && ready;
		let interrupt = self.irq_delay == 1;
		self.irq_delay = self.irq_delay.saturating_sub(1);
		if !self.shifting || !clock_edge {
			if clock_edge && !self.input_mode {
				self.cnt_output = true;
			}
			return (self.output_bit, interrupt);
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
			return (true, interrupt);
		}

		self.cnt_output = !self.cnt_output;
		if self.cnt_output {
			return (self.output_bit, interrupt);
		}
		self.output_bit = (self.shift_data & 0x80) != 0;
		self.shift_data <<= 1;
		self.shift_count += 1;
		if self.shift_count == 8 {
			self.irq_delay = OUTPUT_IRQ_DELAY;
			self.shift_count = 0;
			if self.write_pending {
				self.shift_data = self.data;
				self.write_pending = false;
			} else {
				self.shifting = false;
			}
		}
		(self.output_bit, interrupt)
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
	/* Loading an empty shifter first synchronises the write, then arms the
	 * next falling CNT edge (CIA-SERIAL-MEASUREMENTS). SP retains its
	 * preceding bit until that edge presents the new most significant bit. */
	fn start_output(&mut self, value: u8) {
		self.load_delay = 2;
		self.shift_data = value;
		self.shift_count = 0;
		self.shifting = true;
		self.input_mode = false;
		self.cnt_output = true;
	}

	#[inline(always)]
	/* Direction switching aborts the byte, but does not erase the sampled
	 * CNT phase. Hardware measurements show a transient request after a
	 * falling edge, a one-cycle gap, then a settled request window. The
	 * eighth edge has no transient window once it releases the shifter
	 * (CIA-SERIAL-MEASUREMENTS, normal-CIA second result plane). */
	pub fn stop(&mut self) {
		self.load_delay = 0;
		if !self.input_mode {
			/* Resetting a propagated non-empty bit counter produces a serial
			 * request even when the sampled CNT phase is high. This is distinct
			 * from the phase-dependent request when output resumes
			 * (CIA-SERIAL-MEASUREMENTS, test2 first result plane). */
			if self.progress_history & 0b1000 != 0 {
				self.irq_delay = OUTPUT_IRQ_DELAY;
			}
			let h = self.cnt_history;
			let settled_low = (h & 0b1100) != 0;
			let recent_fall = (h & 0b0011) == 0b0001 && self.shifting;
			self.direction_irq = settled_low || recent_fall;
		} else if self.direction_irq {
			self.irq_delay = OUTPUT_IRQ_DELAY;
			self.direction_irq = false;
		}
		self.shift_count = 0;
		self.shifting = false;
		self.output_bit = true;
		self.cnt_output = true;
		self.write_pending = false;
	}
}