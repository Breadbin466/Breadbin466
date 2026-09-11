// =======================================================
// src/vic/light_pen.rs — Light-pen input and frame-scoped coordinate capture
// =======================================================

use super::constants::{IRQ_LIGHT_PEN, PAL_LINES, TOTAL_WIDTH};
use super::state::VicII;

/* The LP input captures one falling edge per frame. Coordinates retain the upper
eight horizontal bits and the lower eight vertical bits; the interrupt latch is
independent of its enable bit. Software drives the same input through CIA 1 PB4.
(BAUER-VIC-II-1996, section 3.11) */
impl VicII {
	/* The motherboard supplies the pin level after the preceding CPU bus phase.
	The raster timing still names that completed cycle, so the capture coordinate
	is its final dot, before the sequencer advances to the next cycle. */
	pub fn set_light_pen_pin(&mut self, high: bool) {
		let falling = self.light_pen_pin_high && !high;
		self.light_pen_pin_high = high;
		if falling && !self.light_pen_triggered && self.timing.raster_line != PAL_LINES - 1 {
			/* The PAL end-of-cycle coordinate is $03C in cycle 20 and wraps
			through the 504-dot line. (BAUER-VIC-II-1996, section 3.11) */
			let x = (self.timing.cycle * 8 + 404) % TOTAL_WIDTH as u16;
			/* The light-pen Y aperture observes the next line during cycle 63,
			before the CPU-visible raster register advances.
			(VIC-LIGHT-PEN-MEASUREMENTS) */
			let y = self.timing.raster_line + u16::from(self.timing.cycle == 63);
			self.capture_light_pen((x >> 1) as u8, y as u8);
		}
	}

	/* Frame rearming also recognises an input held low across the boundary. The
	6569 capture aperture opens before the ordinary cycle-2 bus-write position.
	(VIC-LIGHT-PEN-MEASUREMENTS) */
	pub(super) fn rearm_light_pen(&mut self) {
		self.light_pen_triggered = false;
		if !self.light_pen_pin_high {
			self.capture_light_pen(0xD1, 0);
		}
	}

	fn capture_light_pen(&mut self, x: u8, y: u8) {
		self.light_pen_x = x;
		self.light_pen_y = y;
		self.light_pen_triggered = true;
		self.irq.trigger(IRQ_LIGHT_PEN);
	}
}