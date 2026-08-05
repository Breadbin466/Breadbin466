// =======================================================
// src/vic/horizontal_scroll.rs — $D016 horizontal scroll and MCM register transition
// =======================================================

use super::fsm::GraphicsMode;

use super::state::VicII;

/*
A $D016 write updates XSCROLL, CSEL and MCM at the cycle boundary at which the VIC-II samples the register. Border comparators, graphics decoding and the current shifter span are therefore updated as related but distinct effects.
*/
/* Horizontal scroll changes move the character pipeline relative to the raster without rewinding already emitted pixels. The transition path records both old and new offsets so the affected cell can be assembled from two alignments. */
impl VicII {
pub(super) fn apply_horizontal_scroll_write(&mut self, data: u8, cycle: u16) {
		let mode_old              = self.graphics_mode;
		self.mode_changing        = true;
		let old_horizontal_scroll = self.regs.x_scroll();

		let mcm  = (data & 0x10) != 0;
		self.regs.ctrl2 = data;
		let ecm         = self.regs.ecm();
		let bmm         = self.regs.bmm();
		let graphics_mode = GraphicsMode::from_control_bits(ecm, bmm, mcm);
		self.graphics_mode = graphics_mode;
		let mode_new = graphics_mode;

		let mut new_horizontal_scroll = data & 7;
		/* At cycle 56 the final display cell has already entered the drain phase, so the live comparator behaves as though XSCROLL were seven for the transition that remains visible. */
		if cycle == 56 {
			new_horizontal_scroll = 7;
		}
		self.horizontal_scroll_at_cycle57 = new_horizontal_scroll;

		if cycle >= 9 && cycle <= 60 {
			self.apply_scroll_decrement_pixel_transition(mode_old, mode_new, old_horizontal_scroll, new_horizontal_scroll, cycle);
		}
	}
}