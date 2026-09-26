// =======================================================
// src/emulator/timing.rs — Emulator Synchronisation & Warp Timers
// =======================================================

pub use crate::emulator::constants::{HOST_WAKE_MARGIN, PAL_FRAME_DURATION, WARP_PRESENT_INTERVAL};
use std::time::Instant;

/* TimeKeeper maps wall-clock time onto PAL frame boundaries. It limits catch-up after host stalls, makes warp presentation rate independent from emulation rate, and provides a single resynchronisation point after pauses or disruptive UI operations. */
pub struct TimeKeeper {
	next_frame_time: Instant,
	last_present: Instant,
	pub warp_mode: bool,
	pub warp_1541: bool,
	frames_ran: usize,
	anchored: bool,
}

impl TimeKeeper {
	/* Pacing is anchored on the first execution request, after window and media setup. Construction time must not become an initial catch-up burst. */
	pub fn new() -> Self {
		Self {
			next_frame_time: Instant::now(),
			last_present: Instant::now(),
			warp_mode: false,
			warp_1541: false,
			frames_ran: 0,
			anchored: false,
		}
	}

	/* Resynchronisation abandons accumulated wall-clock debt after pauses, modal operations or resets. */
	pub fn resynchronise(&mut self) {
		let now = Instant::now();
		self.next_frame_time = now;
		self.last_present = now;
		self.frames_ran = 0;
		self.anchored = false;
	}

	/* Full-machine warp and drive-only warp are mutually exclusive because they suspend pacing under different conditions. */
	pub fn toggle_warp_mode(&mut self) {
		self.warp_mode = !self.warp_mode;
		if self.warp_mode {
			self.warp_1541 = false;
		}
	}

	/* Drive warp accelerates only while the 1541 is active and disables unconditional warp when selected. */
	pub fn toggle_drive_warp(&mut self) {
		self.warp_1541 = !self.warp_1541;
		if self.warp_1541 {
			self.warp_mode = false;
		}
	}

	#[inline(always)]
	pub fn is_warping(&self, drive_busy: bool) -> bool {
		self.warp_mode || (self.warp_1541 && drive_busy)
	}

	/* The event loop owns all waiting, including the final polling margin,
	 * so input remains dispatchable until the PAL deadline. This method only
	 * accounts for due frames; warp abandons wall-clock debt. */
	pub fn calculate_frames_to_run(&mut self, drive_busy: bool) -> usize {
		if !self.anchored {
			self.next_frame_time = Instant::now();
			self.anchored = true;
		}
		if self.is_warping(drive_busy) {
			self.next_frame_time = Instant::now();
			self.frames_ran = 1;
			1
		} else {
			let now = Instant::now();

			let mut steps = 0;
			while now >= self.next_frame_time {
				steps += 1;
				self.next_frame_time += PAL_FRAME_DURATION;
			}

			if steps > 3 {
				steps = 3;
				self.next_frame_time = now + PAL_FRAME_DURATION;
			}

			self.frames_ran = steps;
			steps
		}
	}

	/* Presentation follows every completed normal-time batch, but is throttled during warp so rendering cost does not become the speed limit of emulation. */
	pub fn should_present(&mut self, drive_busy: bool) -> bool {
		if self.is_warping(drive_busy) {
			if self.last_present.elapsed() >= WARP_PRESENT_INTERVAL {
				self.last_present = Instant::now();
				true
			} else {
				false
			}
		} else {
			if self.frames_ran > 0 {
				self.frames_ran = 0;
				self.last_present = Instant::now();
				true
			} else {
				false
			}
		}
	}

	pub fn get_next_frame_time(&self) -> Instant {
		self.next_frame_time
	}
}