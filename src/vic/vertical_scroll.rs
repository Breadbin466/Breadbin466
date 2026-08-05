// =======================================================
// src/vic/vertical_scroll.rs — $D011 write: vertical scroll, badline, VSP, vertical border and raster IRQ update
// =======================================================

use super::fsm::{GraphicsMode, CharacterAccessState, DisplayState, DisplayTransition};
use crate::memory::Memory;
use super::state::VicII;
use super::constants::IRQ_RASTER;

/*
A $D011 write combines several independently timed functions: YSCROLL and badline recognition, RSEL border geometry, DEN latching, ECM/BMM mode selection and the high raster-IRQ compare bit. The implementation updates each consumer at its own cycle boundary rather than treating CTRL1 as an atomic visual change.
*/
/* Vertical-scroll writes can create or cancel a badline after the normal decision point. This module preserves the elapsed fetch phase so only future c-accesses and display-state transitions are affected. */
impl VicII {
	/*
	The write is intentionally decomposed in raster order. Border comparators first observe the old and new RSEL/DEN combination, the raster compare high bit is then updated, and only afterwards is badline state recomputed from the new YSCROLL value. This prevents an atomic host-language assignment from erasing cycle-local VIC-II effects.
	*/
	pub(super) fn apply_vertical_scroll_write(&mut self, data: u8, memory: &mut Memory, cycle: u16, old_rsel: bool, was_badline: bool) {
		let mode_old              = self.graphics_mode;
		self.mode_changing        = true;
		let old_horizontal_scroll = self.regs.x_scroll();
		let old_den               = self.regs.den();

		let rsel_changing         = ((data & 8) >> 3 != 0) != old_rsel;

		if self.timing.raster_line == self.border.top_compare {
			if rsel_changing && old_den {
				self.border.vertical_border           = false;
				self.border.char_data_output_disabled = false;
			}
		}
		if self.timing.raster_line == self.border.bottom_compare {
			if rsel_changing {
				self.border.vertical_border = true;
			}
		}

		let y_scroll = data & 7;
		let rsel     = (data & 8) != 0;
		let den      = (data & 0x10) != 0;
		let ecm      = (data & 0x40) != 0;
		let bmm      = (data & 0x20) != 0;

		self.regs.ctrl1 = data;
		let graphics_mode = GraphicsMode::from_control_bits(ecm, bmm, mode_old.multicolour_bit());
		self.graphics_mode = graphics_mode;
		let mode_new     = graphics_mode;

		let old_raster_compare = self.regs.raster_irq;
		self.regs.raster_irq   = (self.regs.raster_irq & 0x00FF) | (((data as u16) & 0x80) << 1);

		self.border.recompute_compare(rsel);

		if self.timing.raster_line == self.border.top_compare && cycle != 63 {
			if rsel_changing && den {
				self.border.vertical_border           = false;
				self.border.char_data_output_disabled = false;
			}
		}
		if self.timing.raster_line == self.border.bottom_compare && cycle != 63 {
			if rsel_changing {
				self.border.vertical_border = true;
			}
		}

		if self.regs.raster_irq != old_raster_compare && self.timing.raster_line == self.regs.raster_irq {
			self.irq.trigger(IRQ_RASTER);
		}

		/*
		DEN is sampled cumulatively on raster line $30. Once seen high during that line it remains eligible for the badline window, even if a later write clears the visible register bit before the next character-access decision.
		*/
		if self.timing.raster_line == 0x30 {
			self.latch_den |= den;
		}

		self.apply_badline_state(y_scroll, cycle, memory, was_badline);

		if mode_old != mode_new && cycle >= 9 && cycle <= 60 {
			self.apply_graphics_mode_transition(mode_old, mode_new, old_horizontal_scroll, cycle);
		}
	}

	/*
	Re-evaluating YSCROLL during the badline window can start or cancel character accesses on the current line. A newly forced badline enters display state immediately, records the open-bus opcode value and schedules the first late c-access without replaying earlier cycles.
	*/
	fn apply_badline_state(&mut self, y_scroll: u8, cycle: u16, memory: &mut Memory, was_badline: bool) {
		let line = self.timing.raster_line;

		if cycle != 63 {
			self.timing.update_badline_live(line, y_scroll, self.latch_den);
		}

		if self.timing.is_badline {
			/* A late transition into badline state schedules only the first c-access that has not already elapsed. Earlier fetch slots are represented by the captured bus value rather than replayed. */
			if !was_badline && self.display_state.is_idle() {
				self.display_transition = DisplayTransition::EnterDisplayAfterCharacterAccess;
				if cycle >= 15 && cycle < 54 {
					self.forced_badline_c_access_clock = self.current_clock + 2;
				}
			}
			if !was_badline {
				self.char_data_carry = 0;

				self.cpu_next_op_code = memory.bus_state.latched_value();

				self.display_state = DisplayState::Display;
			}
			if cycle >= 15 && cycle <= 54 {
				self.character_access_state = CharacterAccessState::Enabled;
			}
		} else if cycle <= 54 {
			self.character_access_state = CharacterAccessState::Disabled;
		}

	}
}