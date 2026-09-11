// =======================================================
// src/iec/bus.rs — IEC Serial Bus wired-OR logic (+ SRQ line)
// =======================================================

use crate::iec::constants::{
	DEVICE_ATN, DEVICE_ATN_ACK, DEVICE_ATNA, DEVICE_CLK, DEVICE_CONNECTED, DEVICE_DATA, DEVICE_SRQ,
	DEVICE_STATE, HOST_ATN, HOST_CLK, HOST_DATA, HOST_PULLS, HOST_SRQ, INITIAL_STATE,
};
use std::cell::Cell;

/* The bus stores each participant's pull-down requests separately and resolves the visible lines afterwards. A released line is high only when neither host nor connected device pulls it low; the 1541 ATN acknowledge circuit can additionally pull DATA low from the relationship between ATN and ATNA. */
#[derive(Clone, Copy, PartialEq, Eq)]
enum Activity {
	Host,
	Device,
}

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
	bit(state, DEVICE_CONNECTED)
}

#[inline(always)]
fn line_atn(state: u32) -> bool {
	!(bit(state, HOST_ATN) || (connected(state) && bit(state, DEVICE_ATN)))
}

#[inline(always)]
fn line_clk(state: u32) -> bool {
	!(bit(state, HOST_CLK) || (connected(state) && bit(state, DEVICE_CLK)))
}

#[inline(always)]
fn ack_pull(state: u32) -> bool {
	connected(state) && bit(state, DEVICE_ATN_ACK) && (bit(state, DEVICE_ATNA) ^ !line_atn(state))
}

#[inline(always)]
fn line_data(state: u32) -> bool {
	!(bit(state, HOST_DATA) || (connected(state) && bit(state, DEVICE_DATA)) || ack_pull(state))
}

#[inline(always)]
fn line_srq(state: u32) -> bool {
	!(bit(state, HOST_SRQ) || (connected(state) && bit(state, DEVICE_SRQ)))
}

/* IecBus is the shared electrical state of the Commodore serial cable. Cell provides interior mutability because the C64-side devices observe and drive the same wire object during one motherboard cycle, while transition timestamps preserve the cycle at which the resolved line actually changed. */
pub struct IecBus {
	state: Cell<u32>,
	last_atn_transition: Cell<u64>,
	last_clk_transition: Cell<u64>,
	last_data_transition: Cell<u64>,
	last_srq_transition: Cell<u64>,
	host_activity: Cell<u64>,
	device_activity: Cell<u64>,
}

impl IecBus {
	/* Construction starts with every participant released and the cable at its pulled-up idle levels. Transition times and activity counters begin at the same electrical origin. */
	pub fn new() -> Self {
		Self {
			state: Cell::new(INITIAL_STATE),
			last_atn_transition: Cell::new(0),
			last_clk_transition: Cell::new(0),
			last_data_transition: Cell::new(0),
			last_srq_transition: Cell::new(0),
			host_activity: Cell::new(0),
			device_activity: Cell::new(0),
		}
	}

	/* Reset releases every line to the initial disconnected state and clears transition timestamps and activity counters, so the next effective edge is measured from the reset boundary. */
	pub fn reset(&self) {
		self.state.set(INITIAL_STATE);
		self.last_atn_transition.set(0);
		self.last_clk_transition.set(0);
		self.last_data_transition.set(0);
		self.last_srq_transition.set(0);
		self.host_activity.set(0);
		self.device_activity.set(0);
	}

	#[inline(always)]
	fn load(&self) -> u32 {
		self.state.get()
	}

	/* Activity counters record effective participant state changes, while transition timestamps advance only when the wired result visible on the cable changes. A device may therefore toggle an internal pull without producing an external edge if another participant already holds the line low. */
	#[inline(always)]
	fn track(&self, old: u32, new: u32, cycle: u64, activity: Activity) {
		match activity {
			Activity::Host => {
				self.host_activity
					.set(self.host_activity.get().wrapping_add(1));
			}
			Activity::Device => {
				self.device_activity
					.set(self.device_activity.get().wrapping_add(1));
			}
		}
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

	/* All drive changes pass through one commit point so state publication, activity accounting and edge timing remain inseparable. */
	#[inline(always)]
	fn commit<F>(&self, cycle: u64, activity: Activity, transform: F) -> bool
	where
		F: Fn(u32) -> u32,
	{
		let old = self.load();
		let new = transform(old);
		if new == old {
			return false;
		}
		self.state.set(new);
		self.track(old, new, cycle, activity);
		true
	}

	/* Host arguments describe active pull-downs, not line levels: true means that the C64 side actively holds the corresponding open-collector line low. */
	#[inline(always)]
	pub fn set_host_pulls(&self, atn: bool, clk: bool, data: bool, cycle: u64) {
		self.commit(cycle, Activity::Host, |state| {
			let mut next = with_bit(state, HOST_ATN, atn);
			next = with_bit(next, HOST_CLK, clk);
			with_bit(next, HOST_DATA, data)
		});
	}

	/* The drive supplies its CLK and DATA pull-downs together with the ATNA state and the enable for the discrete ATN acknowledge path. Disconnected devices cannot influence the cable. */
	#[inline(always)]
	pub fn set_device_lines(
		&self,
		clk: bool,
		data: bool,
		atna: bool,
		atna_output: bool,
		cycle: u64,
	) {
		if !connected(self.load()) {
			return;
		}
		self.commit(cycle, Activity::Device, |state| {
			if !connected(state) {
				return state;
			}
			let mut next = with_bit(state, DEVICE_CLK, clk);
			next = with_bit(next, DEVICE_DATA, data);
			next = with_bit(next, DEVICE_ATNA, atna);
			with_bit(next, DEVICE_ATN_ACK, atna_output)
		});
	}

	/* Disconnecting a device releases every device-owned pull in the same transition, preventing stale VIA outputs from remaining electrically visible. */
	pub fn set_device_connected(&self, connect: bool, cycle: u64) {
		self.commit(cycle, Activity::Device, |state| {
			if connected(state) == connect {
				return state;
			}
			let next = with_bit(state, DEVICE_CONNECTED, connect);
			if connect { next } else { next & !DEVICE_STATE }
		});
	}

	pub fn atn(&self) -> bool {
		line_atn(self.load())
	}

	pub fn clk(&self) -> bool {
		line_clk(self.load())
	}

	pub fn data(&self) -> bool {
		line_data(self.load())
	}

	pub fn srq(&self) -> bool {
		line_srq(self.load())
	}

	#[inline(always)]
	pub fn lines(&self) -> (bool, bool, bool, bool) {
		let state = self.load();
		(
			line_atn(state),
			line_clk(state),
			line_data(state),
			line_srq(state),
		)
	}

	/* Resolve a completed device boundary against the current host outputs
	without publishing artificial transitions on the live cable. */
	pub(crate) fn input_lines_for_device(&self, device: u32) -> (bool, bool) {
		let current = self.load();
		let state = if connected(current) { (current & !DEVICE_STATE) | (device & DEVICE_STATE) } else { current };
		(line_clk(state), line_data(state))
	}

	pub fn last_atn_transition(&self) -> u64 {
		self.last_atn_transition.get()
	}

	pub fn last_clk_transition(&self) -> u64 {
		self.last_clk_transition.get()
	}

	pub fn last_data_transition(&self) -> u64 {
		self.last_data_transition.get()
	}

	pub fn last_srq_transition(&self) -> u64 {
		self.last_srq_transition.get()
	}

	pub fn host_activity(&self) -> u64 {
		self.host_activity.get()
	}

	pub fn device_activity(&self) -> u64 {
		self.device_activity.get()
	}

	pub fn host_released(&self) -> bool {
		(self.load() & (HOST_PULLS | HOST_SRQ)) == 0
	}

	pub fn raw_state(&self) -> u32 {
		self.load()
	}

	/* Raw restoration is reserved for complete state loading. It deliberately does not synthesise transition timestamps because the restored snapshot already defines the current electrical instant. */
	pub fn set_raw_state(&self, state: u32) {
		self.state.set(state);
	}

	pub fn host_state(&self) -> u32 {
		self.load() & (HOST_PULLS | HOST_SRQ)
	}

	pub fn device_state(&self) -> u32 {
		self.load() & DEVICE_STATE
	}

	pub fn set_host_state(&self, host: u32, cycle: u64) {
		let bits = host & (HOST_PULLS | HOST_SRQ);
		self.commit(cycle, Activity::Host, |state| {
			(state & !(HOST_PULLS | HOST_SRQ)) | bits
		});
	}

	pub fn set_device_state(&self, device: u32, cycle: u64) {
		let bits = device & DEVICE_STATE;
		self.commit(cycle, Activity::Device, |state| {
			if !connected(state) {
				return state;
			}
			(state & !DEVICE_STATE) | bits
		});
	}
}

impl Default for IecBus {
	fn default() -> Self {
		Self::new()
	}
}