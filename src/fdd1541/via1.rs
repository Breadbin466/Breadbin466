// =======================================================
// src/fdd1541/via1.rs — Discrete VIA #1 (serial IEC bus interface, $1800)
// =======================================================

use super::iec::DriveIecBus;
use super::via::ViaChip;

/* VIA1 is the serial-bus adapter. Its port-B wiring combines IEC inputs, device-number jumpers and ATN acknowledge logic around the generic 6522 core. */
pub struct Via1 {
	pub inner: ViaChip,
	iec: DriveIecBus,
	device_address: u8,
	last_iec_revision: u64,
}

impl Via1 {
	/* Construction applies the 1541 board defaults around a reset 6522 core: all IEC pins begin as inputs and CA1 starts from the inactive ATN level. */
	pub fn new() -> Self {
		let mut via = Self {
			inner: ViaChip::new(),
			iec: DriveIecBus::new(),
			device_address: 8,
			last_iec_revision: u64::MAX,
		};
		via.inner.ddrb = 0x00;
		via.inner.orb = 0x00;
		via.inner.ca1_prev = false;
		via
	}

	/* Reset clears the VIA while preserving the external cable object, then republishes released outputs and resamples the currently visible IEC levels. */
	pub fn reset(&mut self, cycle: u64) {
		self.inner.reset();
		self.inner.ddrb = 0x00;
		self.inner.orb = 0x00;
		self.inner.ca1_prev = false;
		self.last_iec_revision = u64::MAX;
		self.update_bus(cycle);
		self.sync_inputs();
	}

	pub fn iec(&self) -> &DriveIecBus {
		&self.iec
	}

	/* The two device-number jumpers encode addresses 8 through 11 as the offset from eight on port-B bits 5 and 6. */
	fn jumper_bits(&self) -> u8 {
		let n = self.device_address.wrapping_sub(8) & 0x03;
		n << 5
	}

	#[inline(always)]
	fn bus_levels(&self) -> (bool, bool, bool) {
		let (atn, clk, data, _) = self.iec.lines();
		(!atn, !clk, !data)
	}

	pub fn iec_lines(&self) -> (bool, bool, bool) {
		let (atn, clk, data, _) = self.iec.lines();
		(atn, clk, data)
	}

	pub fn host_activity(&self) -> u64 {
		self.iec.host_activity()
	}

	pub fn device_activity(&self) -> u64 {
		self.iec.device_activity()
	}

	pub fn host_released(&self) -> bool {
		self.iec.host_released()
	}

	#[inline(always)]
	/* Port B is assembled from resolved IEC levels and board jumpers before DDR masking, matching the fact that these signals exist outside the VIA. */
	fn compose_input_b(&self) -> u8 {
		let (atn_low, clk_low, data_low) = self.bus_levels();
		let mut value = self.jumper_bits();
		if data_low {
			value |= 0x01;
		}
		if clk_low {
			value |= 0x04;
		}
		if atn_low {
			value |= 0x80;
		}
		value
	}

	#[inline(always)]
	/* A cable revision is consumed once. The resolved lines update port B and feed ATN/SRQ into the VIA edge detectors without repeatedly presenting the same electrical instant. */
	fn sync_inputs(&mut self) {
		let revision = self.iec.revision();
		if revision == self.last_iec_revision {
			return;
		}

		let (atn, _, _, srq) = self.iec.lines();
		self.inner.set_port_b_input(self.compose_input_b());
		self.inner.set_ca1(!atn);
		self.inner.on_cb1_edge(srq);
		self.last_iec_revision = revision;
	}

	#[inline(always)]
	pub fn read(&mut self, addr: u16) -> u8 {
		let reg = (addr & 0x0F) as u8;
		if reg == 0x00 {
			self.sync_inputs();
		}
		self.inner.read(reg)
	}

	#[inline(always)]
	/* Only ORB and DDRB can change the IEC pull network directly; writes to other registers remain internal to the 6522. */
	pub fn write(&mut self, addr: u16, val: u8, cycle: u64) {
		let reg = (addr & 0x0F) as u8;
		self.inner.write(reg, val);
		if matches!(reg, 0x00 | 0x02) {
			self.update_bus(cycle);
			self.sync_inputs();
		}
	}

	#[inline(always)]
	pub fn set_host_state(&mut self, host_state: u32, cycle: u64) {
		self.iec.set_host_state(host_state, cycle);
		self.sync_inputs();
	}

	#[inline(always)]
	pub fn set_connected(&mut self, connected: bool, cycle: u64) {
		self.iec.set_device_connected(connected, cycle);
		self.sync_inputs();
	}

	#[inline(always)]
	pub fn tick(&mut self) -> bool {
		self.inner.tick()
	}

	#[inline(always)]
	/* Output latch and DDR together decide whether CLK, DATA and ATNA are actively pulled or electrically released. */
	fn update_bus(&mut self, cycle: u64) {
		let pins = self.inner.orb | !self.inner.ddrb;
		let data_out = (pins & 0x02) != 0;
		let clk_out = (pins & 0x08) != 0;
		let atna = (pins & 0x10) != 0;
		let atna_output = (self.inner.ddrb & 0x10) != 0;
		self.iec.set_device_lines(clk_out, data_out, atna, atna_output, cycle);
	}

}

impl Default for Via1 {
	fn default() -> Self {
		Self::new()
	}
}