// =======================================================
// src/cia/tod.rs — MOS 6526A time-of-day clock
// =======================================================

#[derive(Clone, Copy, Default)]
/* Live, latched and alarm values are stored separately so CPU reads cannot disturb the running clock. */
pub struct TimeOfDay {
	pub tenths: u8,
	pub seconds: u8,
	pub minutes: u8,
	pub hours: u8,
	pub running: bool,
	pub latched: bool,
	pub latch_tenths: u8,
	pub latch_seconds: u8,
	pub latch_minutes: u8,
	pub latch_hours: u8,
	pub alarm: u32,
	pub current_val: u32,
	pub divider: u8,
	pub alarm_matched: bool,
}

impl TimeOfDay {
	#[inline(always)]
	/* TOD maintains tenths, seconds, minutes and 12-hour AM/PM time in BCD and compares the running clock against a programmable alarm (MOS-6526-1981, Time of Day Clock). */
	/* Divider pulses are accumulated until one tenth of a second elapses; only then are the BCD clock fields advanced and compared with the packed alarm value. */
	pub fn tick(&mut self, ticks_per_tenth: u8) -> bool {
		if !self.running {
			return false;
		}
		self.divider += 1;
		if self.divider != ticks_per_tenth {
			/* The mains divider wraps at six even when the selected terminal
			 * count is five (CIA-TOD-MEASUREMENTS, hzsync6). */
			if self.divider == 6 {
				self.divider = 0;
			}
			return false;
		}
		self.divider = 0;
		if self.tenths == 9 {
			self.tenths = 0;
			let (seconds, carry) = Self::bcd_increment(self.seconds);
			self.seconds = seconds;
			if carry {
				let (minutes, carry) = Self::bcd_increment(self.minutes);
				self.minutes = minutes;
				if carry {
					self.increment_hours();
				}
			}
		} else {
			self.tenths = (self.tenths + 1) & 0x0F;
		}
		self.pack_current();
		self.compare_alarm()
	}

	#[inline(always)]
	/* Only entry into equality raises the alarm; acknowledging ICR does not
	 * re-arm a comparison that remains equal (CIA-TOD-MEASUREMENTS, 4tod and 5tod). */
	pub fn compare_alarm(&mut self) -> bool {
		let matched = self.current_val == self.alarm;
		let rising = matched && !self.alarm_matched;
		self.alarm_matched = matched;
		rising
	}

	#[inline(always)]
	/* Each digit is a binary counter with an equality detector at its decimal
	 * terminal value. Invalid digits count through the remaining binary states;
	 * binary wrap alone does not propagate a decimal carry. Seconds and minutes
	 * have a four-bit units counter and a three-bit tens counter
	 * (CIA-TOD-MEASUREMENTS, fix-sec and fix-min). */
	fn bcd_increment(val: u8) -> (u8, bool) {
		let lo = val & 0x0F;
		let hi = val >> 4;
		if lo != 9 {
			return ((hi << 4) | ((lo + 1) & 0x0F), false);
		}
		if hi == 5 {
			(0, true)
		} else {
			(((hi + 1) & 7) << 4, false)
		}
	}

	#[inline(always)]
	/* TOD uses a 12-hour representation whose high hour bit records AM or PM (MOS-6526-1981, Time of Day Clock). */
	/* The hour rolls from 11 to 12 by toggling AM/PM, then from 12 to 1 without toggling it again. */
	fn increment_hours(&mut self) {
		let pm_bit = self.hours & 0x80;
		let h_bcd = self.hours & 0x1F;
		/* Only hour 09 carries into the high digit. Invalid low digits wrap
		 * independently, including 19 to 1A (CIA-TOD-MEASUREMENTS, fix-hour). */
		let next = match h_bcd {
			0x09 => 0x10,
			0x12 => 0x01,
			_ => (h_bcd & 0x10) | ((h_bcd + 1) & 0x0F),
		};
		self.hours = (if h_bcd == 0x11 { pm_bit ^ 0x80 } else { pm_bit }) | next;
	}

	#[inline(always)]
	pub fn pack_current(&mut self) {
		self.current_val = (self.tenths as u32)
			| ((self.seconds as u32) << 8)
			| ((self.minutes as u32) << 16)
			| ((self.hours as u32) << 24);
	}

	#[inline(always)]
	/* Reading the hours register freezes a coherent TOD snapshot until the tenths register is read (MOS-6526-1981, Time of Day Clock). */
	/* The four TOD fields are copied together so subsequent reads see one coherent timestamp even if the live clock advances between register accesses. */
	pub fn latch(&mut self) {
		/* Further hour reads retain the first snapshot until tenths release it. */
		if self.latched {
			return;
		}
		self.latched = true;
		self.latch_tenths = self.tenths;
		self.latch_seconds = self.seconds;
		self.latch_minutes = self.minutes;
		self.latch_hours = self.hours;
	}
}