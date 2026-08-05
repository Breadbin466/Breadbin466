// =======================================================
// src/vic/timing.rs — VIC-II raster/cycle counters, badline latch, BA window
// =======================================================

use crate::vic::constants::VERTICAL_WINDOW_LUT;
use crate::vic::constants::{PAL_LINES, PAL_CYCLES_PER_LINE};
use crate::clockchip::signals::LineLevel;

/*
VicTiming owns the PAL raster counters and the live badline predicate. Register values are sampled into this structure before the cycle advances, allowing DEN, RSEL and YSCROLL writes to affect exactly the cycles that consume them. BA is derived here for badline character fetches; sprite DMA contributes its own request in bus_access.rs. (BAUER-VIC-II-1996, sections 3.5 and 3.6)
*/
pub struct VicTiming {
	/* Current 1-based PAL cycle and raster line. Frame wrap is deliberately split across cycles 1 and 2. */
	pub cycle:               u16,
	pub raster_line:         u16,
	/* Control-register values sampled for the current timing decision. */
	pub den:                 bool,
	pub rsel:                bool,
	pub y_scroll:            u8,
	/* Live badline state and the edge used to enter display at the exact enabling cycle. */
	pub is_badline:          bool,
	pub badline_rising_edge: bool,
	/* Badline contribution to the shared active-low BA output; sprite requests are combined elsewhere. */
	pub ba_out:              LineLevel,
}

impl VicTiming {
	/*
	Construction starts at PAL line 311, cycle 63 so the first tick enters cycle 1 and immediately executes the same frame-boundary path used during normal operation. This avoids a special start-up raster phase.
	*/
	pub fn new() -> Self {
		let t = Self {
			cycle:               63,
			raster_line:         311,
			den:                 false,
			rsel:                false,
			y_scroll:            0,
			is_badline:          false,
			badline_rising_edge: false,
			ba_out:              LineLevel::High,
		};
		t
	}

	/* Reset restores the pre-cycle-1 position and releases BA; the sequencer then rebuilds all line-dependent state through the ordinary raster path. */
	pub fn reset(&mut self) {
		*self = Self::new();
	}

	#[inline(always)]
	/* Advance the 1-based raster cycle and recompute badline-driven BA for the new slot. Line advancement is handled separately when cycle 1 begins. */
	pub fn tick(&mut self) {
		self.cycle += 1;
		if self.cycle > PAL_CYCLES_PER_LINE {
			self.cycle = 1;
		}
		self.badline_rising_edge = false;
		self.update_ba();
	}

	#[inline(always)]
	/* Line 311 reports frame wrap without changing the counter immediately. Cycle 2 performs the visible transition to line 0, matching the split frame-boundary sequence used by the main sequencer. */
	pub fn advance_line(&mut self) -> bool {
		if self.raster_line == PAL_LINES - 1 {
			true
		} else {
			self.raster_line += 1;
			false
		}
	}

	#[inline(always)]
	/* Complete the deferred PAL frame wrap prepared at cycle 1. */
	pub fn reset_line_to_zero(&mut self) {
		self.raster_line = 0;
	}

	#[inline(always)]
	/* A PAL badline requires the visible vertical window, a raster/YSCROLL match and the DEN latch set during line $30. */
	pub fn compute_is_badline(raster_line: u16, y_scroll: u8, latch_den: bool) -> bool {
		let in_vertical_window = VERTICAL_WINDOW_LUT[raster_line as usize];
		let scroll_match       = (raster_line as u8 & 7) == y_scroll;
		in_vertical_window && scroll_match && latch_den
	}

	#[inline(always)]
	/* Re-evaluate the badline condition after live register changes. The returned rising edge lets the display flip-flop enter display state at the exact cycle where the condition first becomes true. */
	pub fn update_badline_live(&mut self, raster_line: u16, y_scroll: u8, latch_den: bool) -> bool {
		let new_badline          = Self::compute_is_badline(raster_line, y_scroll, latch_den);
		let rising_edge          = new_badline && !self.is_badline;
		self.is_badline          = new_badline;
		self.badline_rising_edge = rising_edge;
		self.update_ba();
		rising_edge
	}

	#[inline(always)]
	/* Badline BA remains low from cycle 12 through 54, giving the CPU advance notice before AEC removes bus ownership for character accesses. */
	fn update_ba(&mut self) {
		self.ba_out = if self.is_badline && self.cycle >= 12 && self.cycle <= 54 {
			LineLevel::Low
		} else {
			LineLevel::High
		};
	}

	/* Snapshot the control bits used by timing decisions so one VIC cycle observes a coherent register state even when the CPU writes the live registers between cycles. */
	/* Capture the control values consumed by the next timing decision. RSEL is retained here even though badline detection itself depends only on DEN and YSCROLL. */
	pub fn latch_registers(&mut self, den: bool, rsel: bool, y_scroll: u8) {
		self.den      = den;
		self.rsel     = rsel;
		self.y_scroll = y_scroll;
	}

}

/* Default construction is identical to the documented hardware reset baseline exposed by new(). */
impl Default for VicTiming {
	fn default() -> Self { Self::new() }
}