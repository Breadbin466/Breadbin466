// =======================================================
// src/fdd1541/iec.rs — Single-owner IEC bus used inside the 1541
// =======================================================

use super::constants::{
	IEC_DEVICE_ATN, IEC_DEVICE_ATN_ACK, IEC_DEVICE_ATNA, IEC_DEVICE_CLK, IEC_DEVICE_CONNECTED,
	IEC_DEVICE_DATA, IEC_DEVICE_SRQ, IEC_DEVICE_STATE, IEC_HOST_ATN, IEC_HOST_CLK, IEC_HOST_DATA,
	IEC_HOST_PULLS, IEC_HOST_SRQ, IEC_INITIAL_STATE,
};
use std::cell::Cell;

#[inline(always)]
fn bit(state: u32, mask: u32) -> bool {
	(state & mask) != 0
}

#[inline(always)]
fn with_bit(state: u32, mask: u32, value: bool) -> u32 {
	if value { state | mask } else { state & !mask }
}

#[inline(always)]
fn connected(state: u32) -> bool {
	bit(state, IEC_DEVICE_CONNECTED)
}

#[inline(always)]
fn line_atn(state: u32) -> bool {
	!(bit(state, IEC_HOST_ATN) || (connected(state) && bit(state, IEC_DEVICE_ATN)))
}

#[inline(always)]
fn line_clk(state: u32) -> bool {
	!(bit(state, IEC_HOST_CLK) || (connected(state) && bit(state, IEC_DEVICE_CLK)))
}

#[inline(always)]
fn ack_pull(state: u32) -> bool {
	connected(state)
		&& bit(state, IEC_DEVICE_ATN_ACK)
		&& (bit(state, IEC_DEVICE_ATNA) ^ !line_atn(state))
}

#[inline(always)]
fn line_data(state: u32) -> bool {
	!(bit(state, IEC_HOST_DATA)
		|| (connected(state) && bit(state, IEC_DEVICE_DATA))
		|| ack_pull(state))
}

#[inline(always)]
fn line_srq(state: u32) -> bool {
	!(bit(state, IEC_HOST_SRQ) || (connected(state) && bit(state, IEC_DEVICE_SRQ)))
}

/* The IEC bus stores resolved open-collector line levels and the separate pull intentions of host and drive. A released line rises only when no connected participant pulls it low. */
pub struct DriveIecBus {
	state: Cell<u32>,
	last_atn_transition: Cell<u64>,
	last_clk_transition: Cell<u64>,
	last_data_transition: Cell<u64>,
	last_srq_transition: Cell<u64>,
	host_activity: Cell<u64>,
	device_activity: Cell<u64>,
	revision: Cell<u64>,
}

impl DriveIecBus {
	/* Construction starts with both participants electrically released and all observation counters at the reset boundary. */
	pub fn new() -> Self {
		Self {
			state: Cell::new(IEC_INITIAL_STATE),
			last_atn_transition: Cell::new(0),
			last_clk_transition: Cell::new(0),
			last_data_transition: Cell::new(0),
			last_srq_transition: Cell::new(0),
			host_activity: Cell::new(0),
			device_activity: Cell::new(0),
			revision: Cell::new(0),
		}
	}

	#[inline(always)]
	/* Transition timestamps follow resolved cable levels, not individual pull requests. Two participants changing their intentions without changing the wired result therefore produce no observable edge. */
	fn track(&self, old: u32, new: u32, cycle: u64) {
		if line_atn(old) != line_atn(new) {
			self.last_atn_transition.set(cycle);
		}
		if line_clk(old) != line_clk(new) {
			self.last_clk_transition.set(cycle);
		}
		if line_data(old) != line_data(new) {
			self.last_data_transition.set(cycle);
		}
		if line_srq(old) != line_srq(new) {
			self.last_srq_transition.set(cycle);
		}
	}

	#[inline(always)]
	/* A committed transition advances the monotonic revision and activity counters, allowing the two emulated computers to detect electrical changes without sharing protocol state. */
	fn commit(&self, new: u32, cycle: u64, host: bool, device: bool) -> bool {
		let old = self.state.get();
		if new == old {
			return false;
		}
		self.state.set(new);
		self.revision.set(self.revision.get().wrapping_add(1));
		if host {
			self.host_activity
				.set(self.host_activity.get().wrapping_add(1));
		}
		if device {
			self.device_activity
				.set(self.device_activity.get().wrapping_add(1));
		}
		self.track(old, new, cycle);
		true
	}

	#[inline(always)]
	/* VIA1 outputs are translated into pull-down requests. ATNA participates in the 1541 attention acknowledge circuit rather than acting as an independent IEC wire. */
	pub fn set_device_lines(
		&self,
		clk: bool,
		data: bool,
		atna: bool,
		atna_output: bool,
		cycle: u64,
	) {
		let state = self.state.get();
		if !connected(state) {
			return;
		}
		let mut next = with_bit(state, IEC_DEVICE_CLK, clk);
		next = with_bit(next, IEC_DEVICE_DATA, data);
		next = with_bit(next, IEC_DEVICE_ATNA, atna);
		next = with_bit(next, IEC_DEVICE_ATN_ACK, atna_output);
		self.commit(next, cycle, false, true);
	}

	#[inline(always)]
	/* Disconnecting the drive releases every device-side pull in the same committed transition so stale outputs cannot remain visible on the cable. */
	pub fn set_device_connected(&self, connect: bool, cycle: u64) {
		let state = self.state.get();
		if connected(state) == connect {
			return;
		}
		let next = if connect {
			state | IEC_DEVICE_CONNECTED
		} else {
			(state & !IEC_DEVICE_CONNECTED) & !IEC_DEVICE_STATE
		};
		self.commit(next, cycle, false, true);
	}

	#[inline(always)]
	pub fn atn(&self) -> bool {
		line_atn(self.state.get())
	}

	#[inline(always)]
	pub fn srq(&self) -> bool {
		line_srq(self.state.get())
	}

	#[inline(always)]
	pub fn lines(&self) -> (bool, bool, bool, bool) {
		let state = self.state.get();
		(
			line_atn(state),
			line_clk(state),
			line_data(state),
			line_srq(state),
		)
	}

	#[inline(always)]
	pub fn revision(&self) -> u64 {
		self.revision.get()
	}

	#[inline(always)]
	pub fn host_activity(&self) -> u64 {
		self.host_activity.get()
	}

	#[inline(always)]
	pub fn device_activity(&self) -> u64 {
		self.device_activity.get()
	}

	#[inline(always)]
	pub fn host_released(&self) -> bool {
		(self.state.get() & (IEC_HOST_PULLS | IEC_HOST_SRQ)) == 0
	}

	#[inline(always)]
	pub fn raw_state(&self) -> u32 {
		self.state.get()
	}

	#[inline(always)]
	/* Raw restoration replaces a complete electrical snapshot. It advances the revision for consumers but deliberately does not invent transition times or activity events. */
	pub fn set_raw_state(&self, state: u32) {
		if self.state.replace(state) != state {
			self.revision.set(self.revision.get().wrapping_add(1));
		}
	}

	#[inline(always)]
	/* The host publishes only its current pull intentions and connection state; the drive resolves them with its own outputs locally. */
	pub fn set_host_state(&self, host: u32, cycle: u64) {
		let state = self.state.get();
		let bits = host & (IEC_HOST_PULLS | IEC_HOST_SRQ);
		let next = (state & !(IEC_HOST_PULLS | IEC_HOST_SRQ)) | bits;
		self.commit(next, cycle, true, false);
	}

	#[inline(always)]
	pub fn device_state(&self) -> u32 {
		self.state.get() & IEC_DEVICE_STATE
	}
}

impl Default for DriveIecBus {
	fn default() -> Self {
		Self::new()
	}
}