// =======================================================
// src/vic/raster_line.rs — raster-line entry and raster IRQ transitions
// =======================================================

use crate::memory::Memory;
use super::state::VicII;
use super::fsm::DisplayState;
use super::constants::{IRQ_RASTER, DEN_LATCH_RASTER_LINE};

/* Line-boundary work is kept outside the main cycle match: it advances the raster counter, finalises the previous line, prepares sprite/display state and performs the frame-wrap decisions that become visible at cycles 1 and 2. */
/* Raster-line transitions commit counters and latches whose effects span more than one cycle. Keeping them outside the per-slot fetch code prevents frame wrap, badline entry and light-pen rearming from depending on incidental call ordering. */
impl VicII {
	#[inline(always)]
	/*
	Cycle 1 closes the previous scanline before opening the next one. It flushes rendered pixels, advances the raster counter, evaluates the raster IRQ edge, latches DEN on line $30, recomputes badline state, publishes BA and performs the sprite-3 pointer/data slot that straddles the line boundary.
	*/
	pub(super) fn begin_frame_line(&mut self, memory: &mut Memory, bank: u8, master_cycle: u64, cycle_prev: u8) {
		self.screen.flush_line(self.timing.raster_line as usize);
		self.draw_active_sprites(cycle_prev);

		let hit_last_line = self.timing.advance_line();

		/* The wrap from line 311 to line 0 defers the raster comparison to cycle 2. This preserves the PAL VIC-II edge ordering at the frame boundary instead of treating line zero like an ordinary increment. */
		if hit_last_line {
			self.check_irq_in_cycle2 = true;
			self.raster_match = false;
		} else {
			let line_now = self.timing.raster_line;
			if line_now == self.regs.raster_irq {
				if !self.raster_match {
					self.raster_match = true;
					self.irq.trigger(IRQ_RASTER);
				}
			} else {
				self.raster_match = false;
			}
		}

		let line = self.timing.raster_line;
		let den = self.regs.den();

		self.screen.begin_line();
		self.border.begin_raster_line(line, den);

		if line == DEN_LATCH_RASTER_LINE {
			self.latch_den = den;
		}

		let y_scroll = self.regs.y_scroll();
		let latch_den = self.latch_den;
		if self.timing.update_badline_live(line, y_scroll, latch_den) {
			self.display_state = DisplayState::Display;
		}

		self.update_bus_arbitration(1);

		/* Guard bytes at both edges of the foreground mask prevent stale occupancy from the previous line leaking into sprite collision tests during horizontal wrap. */
		self.screen.mask_buf[16] = 0;
		self.screen.mask_buf[56] = 0;

		self.sprite_pointer_and_third_data_access(3, memory, bank, master_cycle);
	}
}