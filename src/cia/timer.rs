// =======================================================
// src/cia/timer.rs — MOS 6526A timer state and control
// =======================================================

use super::constants::{CR_LOAD, CR_OUTMODE, CR_RUNMODE, CR_START};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
/* This state machine separates a newly enabled clock source from the cycle that actually decrements the counter. Combined states preserve a decrement already due while scheduling the following clock event. */
enum CounterClockState {
	#[default]
	/* No decrement or delayed PHI2 clock is pending. */
	Halted,
	/* A selected PHI2 clock will become a decrement on the next step. */
	StartDelay,
	/* The counter decrements during the current step. */
	Decrement,
	/* The current step decrements while another PHI2 decrement is scheduled. */
	DecrementAndStartDelay,
}

impl CounterClockState {
	#[inline(always)]
	fn decrements_this_cycle(self) -> bool {
		matches!(self, Self::Decrement | Self::DecrementAndStartDelay)
	}

	#[inline(always)]
	fn start_delay_elapsed(self) -> bool {
		matches!(self, Self::StartDelay | Self::DecrementAndStartDelay)
	}

	#[inline(always)]
	fn after_counter_load(self) -> Self {
		match self {
			Self::Decrement => Self::Halted,
			Self::DecrementAndStartDelay => Self::StartDelay,
			state => state,
		}
	}

	#[inline(always)]
	fn after_stop_command(self) -> Self {
		match self {
			Self::StartDelay => Self::Halted,
			Self::DecrementAndStartDelay => Self::Decrement,
			state => state,
		}
	}
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
/* Latch transfers are pipelined independently of counter clocks. The combined states retain a second load request made while an earlier request is delayed or being transferred, preventing either request from being lost. */
enum CounterLoadState {
	#[default]
	/* No latch transfer or software load request is pending. */
	Quiescent,
	/* Software requested a load during the current step. */
	Requested,
	/* A requested load has entered its one-step delay. */
	Delayed,
	/* The latch transfers into the counter during this step. */
	Transfer,
	/* A second request arrived while the first request was delayed. */
	RequestedDuringDelay,
	/* A new request arrived while a transfer was already taking place. */
	RequestedDuringTransfer,
	/* A delayed request remains queued while another transfer takes place. */
	DelayedDuringTransfer,
	/* One request is delayed, one transfers, and another has just arrived. */
	RequestedDelayedDuringTransfer,
}

impl CounterLoadState {
	#[inline(always)]
	fn request(self) -> Self {
		match self {
			Self::Quiescent => Self::Requested,
			Self::Requested => Self::Requested,
			Self::Delayed => Self::RequestedDuringDelay,
			Self::Transfer => Self::RequestedDuringTransfer,
			Self::RequestedDuringDelay => Self::RequestedDuringDelay,
			Self::RequestedDuringTransfer => Self::RequestedDuringTransfer,
			Self::DelayedDuringTransfer => Self::RequestedDelayedDuringTransfer,
			Self::RequestedDelayedDuringTransfer => Self::RequestedDelayedDuringTransfer,
		}
	}

	#[inline(always)]
	fn advance(self) -> Self {
		match self {
			Self::Quiescent => Self::Quiescent,
			Self::Requested => Self::Delayed,
			Self::Delayed => Self::Transfer,
			Self::Transfer => Self::Quiescent,
			Self::RequestedDuringDelay => Self::DelayedDuringTransfer,
			Self::RequestedDuringTransfer => Self::Delayed,
			Self::DelayedDuringTransfer => Self::Transfer,
			Self::RequestedDelayedDuringTransfer => Self::DelayedDuringTransfer,
		}
	}

	#[inline(always)]
	fn begin_transfer(self) -> Self {
		match self {
			Self::Quiescent => Self::Transfer,
			Self::Requested => Self::RequestedDuringTransfer,
			Self::Delayed => Self::DelayedDuringTransfer,
			Self::Transfer => Self::Transfer,
			Self::RequestedDuringDelay => Self::RequestedDelayedDuringTransfer,
			Self::RequestedDuringTransfer => Self::RequestedDuringTransfer,
			Self::DelayedDuringTransfer => Self::DelayedDuringTransfer,
			Self::RequestedDelayedDuringTransfer => Self::RequestedDelayedDuringTransfer,
		}
	}

	#[inline(always)]
	fn transfers_this_cycle(self) -> bool {
		matches!(
			self,
			Self::Transfer
				| Self::RequestedDuringTransfer
				| Self::DelayedDuringTransfer
				| Self::RequestedDelayedDuringTransfer
		)
	}
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
/* One-shot state is remembered across the underflow cycle so a simultaneous control write cannot lose the pending stop. */
enum RunModeState {
	#[default]
	/* Underflow reloads the counter and counting may continue. */
	Continuous,
	/* The next underflow must also clear START after completing its reload. */
	OneShotArmed,
}

#[derive(Clone, Copy, Default)]
pub struct Timer {
	pub latch: u16,
	pub counter: u16,
	pub cr: u8,
	pub toggle: bool,
	phi2_selected: bool,
	counter_clock: CounterClockState,
	counter_load: CounterLoadState,
	run_mode: RunModeState,
	cnt_edge_pending: bool,
	pb_pulse: bool,
}

impl Timer {
	/* Reset stops each timer and initialises both its latch and counter to all ones (MOS-6526-1981, Reset Characteristics and Interval Timers). */
	pub fn new() -> Self {
		Self {
			latch: 0xFFFF,
			counter: 0xFFFF,
			cr: 0,
			toggle: false,
			phi2_selected: true,
			counter_clock: CounterClockState::Halted,
			counter_load: CounterLoadState::Quiescent,
			run_mode: RunModeState::Continuous,
			cnt_edge_pending: false,
			pb_pulse: false,
		}
	}

	#[inline(always)]
	fn started(&self) -> bool { (self.cr & CR_START) != 0 }

	#[inline(always)]
	fn one_shot(&self) -> bool { (self.cr & CR_RUNMODE) != 0 }

	#[inline(always)]
	fn uses_phi2(&self) -> bool { self.phi2_selected }

	#[inline(always)]
	/* A queued external edge and a completed PHI2 start delay become the same abstract decrement request. */
	fn advance_counter_clock(&mut self) {
		let decrement_now = self.counter_clock.start_delay_elapsed() || (self.cnt_edge_pending && self.started());
		let start_delay = self.started() && self.uses_phi2();
		self.cnt_edge_pending = false;
		self.counter_clock = match (decrement_now, start_delay) {
			(false, false) => CounterClockState::Halted,
			(false, true) => CounterClockState::StartDelay,
			(true, false) => CounterClockState::Decrement,
			(true, true) => CounterClockState::DecrementAndStartDelay,
		};
	}

	#[inline(always)]
	/* A timer underflow reloads the counter from its latch, drives the selected PB6 or PB7 timer output, and clears START when one-shot mode is selected (MOS-6526-1981, Interval Timers and Control Registers). */
	/* The decrement, clock scheduling and latch-transfer phases are evaluated in a fixed order so an underflow can request a reload without erasing an overlapping software load already moving through the pipeline. */
	/* One timer step first advances pending loads and clock requests, then performs the decrement selected for this cycle. Underflow handling may reload, stop one-shot operation and update the PB output in the same step. */
	pub fn step(&mut self) -> bool {
		if self.counter_clock.decrements_this_cycle() && self.counter != 0 {
			self.counter -= 1;
		}

		self.advance_counter_clock();
		self.counter_load = self.counter_load.advance();
		let mut load_counter_now = self.counter_load.transfers_this_cycle();
		let one_shot_was_armed = self.run_mode == RunModeState::OneShotArmed;
		let one_shot_selected = self.one_shot();
		self.run_mode = if one_shot_selected { RunModeState::OneShotArmed } else { RunModeState::Continuous };
		self.pb_pulse = false;

		let underflow = self.counter_clock.decrements_this_cycle() && self.counter == 0;
		if underflow {
			load_counter_now = true;
			self.counter_load = self.counter_load.begin_transfer();
			self.pb_pulse = true;
			self.toggle = !self.toggle;
		}

		if load_counter_now {
			self.counter = self.latch;
			self.counter_clock = self.counter_clock.after_counter_load();
		}

		if self.pb_pulse && (one_shot_was_armed || one_shot_selected) {
			self.cr &= !CR_START;
			self.counter_clock = self.counter_clock.after_stop_command();
		}

		underflow
	}

	#[inline(always)]
	/* Timer A may count CNT events and Timer B may use CNT or Timer A underflows as its count source (MOS-6526-1981, Interval Timers). */
	/* External count events are queued into the clock pipeline. They do not decrement the counter directly, which keeps CNT-driven and PHI2-driven operation on the same timing path. */
	pub fn observe_cnt_edge(&mut self) {
		if self.started() { self.cnt_edge_pending = true; }
	}

	#[inline(always)]
	/* LOAD is a write strobe rather than stored state, while START and the remaining control selections are readable state (MOS-6526-1981, Control Registers). */
	/* START controls future counting, while LOAD is handled as a separate one-shot request. This lets a control write stop, start or reload the timer without collapsing those operations into one state change. */
	pub fn write_cr(&mut self, value: u8, phi2_selected: bool) {
		let was_started = self.started();
		self.cr = value & !CR_LOAD;
		self.phi2_selected = phi2_selected;
		if (value & CR_START) != 0 && !was_started { self.toggle = true; }
		if (value & CR_LOAD) != 0 { self.counter_load = self.counter_load.request(); }
	}

	#[inline(always)]
	/* Writing the low byte changes only the latch. The running counter is left untouched until a load event transfers the complete 16-bit value. */
	pub fn write_latch_lo(&mut self, value: u8) {
		self.latch = (self.latch & 0xFF00) | u16::from(value);
		if self.counter_load.transfers_this_cycle() { self.counter = (self.counter & 0xFF00) | u16::from(value); }
	}

	#[inline(always)]
	/* Writing a timer high byte completes the latch value and loads a stopped timer from that latch (MOS-6526-1981, Interval Timers). */
	/* A high-byte write completes the programmed latch value and loads it immediately only while the timer is stopped; otherwise the current countdown continues. */
	pub fn write_latch_hi(&mut self, value: u8) {
		self.latch = (self.latch & 0x00FF) | (u16::from(value) << 8);
		if self.counter_load.transfers_this_cycle() {
			self.counter = self.latch;
		} else if !self.started() {
			self.counter_load = self.counter_load.request();
		}
	}

	#[inline(always)]
	/* Idle means no hidden pipeline state can change the counter or output without a new external action. */
	pub fn is_idle(&self) -> bool {
		self.counter_clock == CounterClockState::Halted
			&& self.counter_load == CounterLoadState::Quiescent
			&& self.run_mode == RunModeState::Continuous
			&& !self.cnt_edge_pending
			&& !self.pb_pulse
			&& !(self.started() && self.uses_phi2())
	}

	#[inline(always)]
	/* A timer output may produce either a one-cycle underflow pulse or a level that toggles on every underflow (MOS-6526-1981, Timer Output to Port B). */
	pub fn timer_output(&self) -> bool {
		if (self.cr & CR_OUTMODE) != 0 { self.toggle } else { self.pb_pulse }
	}
}