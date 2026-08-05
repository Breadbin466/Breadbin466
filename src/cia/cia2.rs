// =======================================================
// src/cia/cia2.rs — CIA 2 (IEC Bus, VIC Bank, NMI, SRQ shift register)
// =======================================================

use super::cia::Cia;
use std::rc::Rc;
use crate::iec::IecBus;
use super::constants::{CR_PBON, IEC_OUTPUT_MASK};

/* CIA 2 couples the shared 6526 core to VIC banking, IEC open-collector outputs and the NMI-facing interrupt path. */
pub struct Cia2 {
	pub inner: Cia,
	iec_clk_in: bool,
	iec_data_in: bool,
	iec: Option<Rc<IecBus>>,
	published_pins: u8,
	pins_published: bool,
	vic_bank_cache: u8,
}

impl Cia2 {
	/* CIA 2 starts with released IEC inputs, no attached bus and a VIC bank cache derived from the reset port pins. */
	pub fn new() -> Self {
		let mut chip = Self {
			inner: Cia::new(),
			iec_clk_in: true,
			iec_data_in: true,
			iec: None,
			published_pins: 0,
			pins_published: false,
			vic_bank_cache: 0,
		};
		chip.refresh_vic_bank();
		chip
	}

	/* Reset releases sampled IEC inputs, invalidates the publication cache and republishes the resulting host pull state. */
	pub fn reset(&mut self) {
		self.inner.reset();
		self.iec_clk_in = true;
		self.iec_data_in = true;
		self.pins_published = false;
		self.refresh_vic_bank();

		self.update_iec_outputs(0);
	}

	/* Attaching a bus invalidates the publication cache so the current pin state is emitted on the next refresh. */
	pub fn attach_iec_bus(&mut self, bus: Rc<IecBus>) {
		self.iec = Some(bus);
		self.pins_published = false;
	}

	#[inline(always)]
	/* IEC input levels are sampled separately from output publication because the bus resolves all connected pulls. */
	pub fn update_iec_inputs(&mut self, clk: bool, data: bool) {
		self.iec_clk_in = clk;
		self.iec_data_in = data;
	}

	#[inline(always)]
	/* CNT is presented to the reusable core before the cycle advances so edge detection and serial/timer sampling share one pin history. */
	pub fn tick(&mut self, tod_pulse: bool, cnt_in: bool) {
		self.inner.set_cnt_pin(cnt_in);
		self.inner.tick(tod_pulse);
	}

	/* CIA 2 port A combines the VIC bank outputs and serial-bus controls, with serial clock and data inputs visible on bits 6 and 7 (C64-PRG-1982, CIA 2 port assignments). */
	pub fn peek(&self, addr: u16) -> u8 {
		let reg = (addr & 0x000F) as u8;
		if reg == 0x01 { return self.read_port_b(); }
		if reg == 0x00 {
			let external = 0x3F | if self.iec_clk_in { 0x40 } else { 0 } | if self.iec_data_in { 0x80 } else { 0 };
			return (self.inner.pra & self.inner.ddra) | (external & !self.inner.ddra);
		}
		self.inner.peek(reg)
	}

	#[inline(always)]
	/* CPU-visible reads merge external IEC levels with driven port-A bits and preserve the core's destructive-read semantics for other registers. */
	pub fn read(&mut self, addr: u16) -> u8 {
		let reg = (addr & 0x000F) as u8;
		if reg == 0x01 {
			return self.read_port_b();
		}
		if reg == 0x00 {
			let external = 0x3F
				| if self.iec_clk_in { 0x40 } else { 0 }
				| if self.iec_data_in { 0x80 } else { 0 };
			return (self.inner.pra & self.inner.ddra) | (external & !self.inner.ddra);
		}
		self.inner.read(reg)
	}

	/* PB6 and PB7 normally read from the data register, but timer output mode replaces those two pin levels without altering the stored port value. */
	fn read_port_b(&self) -> u8 {
		let mut val = self.inner.prb | !self.inner.ddrb;
		let (ta_out, tb_out) = self.inner.timer_outputs();
		if (self.inner.ta.cr & CR_PBON) != 0 {
			if ta_out { val |= 0x40; } else { val &= !0x40; }
		}
		if (self.inner.tb.cr & CR_PBON) != 0 {
			if tb_out { val |= 0x80; } else { val &= !0x80; }
		}
		val
	}

	#[inline(always)]
	/* Only writes that can change port A pins refresh VIC banking and IEC pulls; unrelated CIA register writes leave the external wiring untouched. */
	pub fn write(&mut self, addr: u16, value: u8, cycle: u64) {
		let reg = (addr & 0x000F) as u8;
		self.inner.write(reg, value);
		if reg == 0x00 || reg == 0x02 {
			self.refresh_vic_bank();
			self.update_iec_outputs(cycle);
		}
	}

	#[inline(always)]
	/* Port-A pin levels combine output-latch bits selected by DDRA with pulled-up inputs on every undriven line. */
	pub fn port_a_pins(&self) -> u8 {
		(self.inner.pra & self.inner.ddra) | !self.inner.ddra
	}

	#[inline(always)]
	/* The cached value is the active-low two-bit VIC bank selected by the current port-A pins. */
	pub fn vic_bank(&self) -> u8 {
		self.vic_bank_cache
	}

	#[inline(always)]
	/* CIA 2 port-A bits 0 and 1 select one of four VIC-II 16 KiB banks through active-low board wiring (C64-PRG-1982, VIC-II memory banking through CIA 2). */
	fn refresh_vic_bank(&mut self) {
		self.vic_bank_cache = (!self.port_a_pins()) & 0x03;
	}

	#[inline(always)]
	/* Only a changed set of CIA output pins is published to the IEC bus. This avoids repeating an electrical-state event while preserving the cycle at which a real pin transition becomes visible to the bus model. */
	pub fn update_iec_outputs(&mut self, cycle: u64) {
		let pins = self.port_a_pins() & IEC_OUTPUT_MASK;
		if self.pins_published && pins == self.published_pins {
			return;
		}
		self.published_pins = pins;
		self.pins_published = true;
		if let Some(bus) = &self.iec {
			bus.set_host_pulls(
				(pins & 0x08) != 0,
				(pins & 0x10) != 0,
				(pins & 0x20) != 0,
				cycle,
			);
		}
	}

	pub fn set_flag_pin(&mut self, state: bool) { self.inner.set_flag_pin(state); }
}