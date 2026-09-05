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
		if self.divider < ticks_per_tenth {
			return false;
		}
		self.divider = 0;
		self.tenths += 1;
		if self.tenths >= 10 {
			self.tenths = 0;
			self.seconds = Self::bcd_increment(self.seconds, 0x59);
			if self.seconds == 0x00 {
				self.minutes = Self::bcd_increment(self.minutes, 0x59);
				if self.minutes == 0x00 {
					self.increment_hours();
				}
			}
		}
		self.pack_current();
		self.current_val == self.alarm
	}

	#[inline(always)]
	/* Decimal carry is performed nibble by nibble because TOD registers expose packed BCD rather than binary counters. */
	fn bcd_increment(val: u8, wrap_at: u8) -> u8 {
		let mut lo = val & 0x0F;
		let mut hi = val >> 4;
		lo += 1;
		if lo > 9 {
			lo = 0;
			hi += 1;
		}
		let new_val = (hi << 4) | lo;
		if new_val > wrap_at { 0x00 } else { new_val }
	}

	#[inline(always)]
	/* TOD uses a 12-hour representation whose high hour bit records AM or PM (MOS-6526-1981, Time of Day Clock). */
	/* The hour rolls from 11 to 12 by toggling AM/PM, then from 12 to 1 without toggling it again. */
	fn increment_hours(&mut self) {
		let pm_bit = self.hours & 0x80;
		let h_bcd = self.hours & 0x1F;
		let mut lo = h_bcd & 0x0F;
		let mut hi = h_bcd >> 4;
		lo += 1;
		if lo > 9 {
			lo = 0;
			hi += 1;
		}
		let h_bcd = (hi << 4) | lo;
		self.hours = if h_bcd == 0x12 {
			(pm_bit ^ 0x80) | 0x12
		} else if h_bcd == 0x13 {
			pm_bit | 0x01
		} else {
			pm_bit | h_bcd
		};
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
		self.latched = true;
		self.latch_tenths = self.tenths;
		self.latch_seconds = self.seconds;
		self.latch_minutes = self.minutes;
		self.latch_hours = self.hours;
	}
}