// =======================================================
// src/vic/border.rs — VIC-II Border State
// =======================================================

#[derive(Debug, Clone, Copy)]
/*
BorderUnit models the horizontal and vertical border flip-flops. Their state is updated at specific raster positions rather than inferred from the final pixel coordinate, which preserves border-opening effects caused by writes to DEN, RSEL and CSEL. (BAUER-VIC-II-1996, border generation)
*/
pub struct BorderUnit {
	pub vertical_border:            bool,
	pub main_border:                bool,
	pub main_border_old:            bool,
	pub char_data_output_disabled:  bool,
	pub border_part_40:             u8,
	pub border_part_38:             u8,
	pub top_compare:                u16,
	pub bottom_compare:             u16,
}

impl BorderUnit {
	/*
	Construction starts inside both horizontal and vertical borders. The compare values select the 24-row geometry until RSEL is written, matching the state seen before the first display window can open.
	*/
	pub fn new() -> Self {
		Self {
			vertical_border:           true,
			main_border:               true,
			main_border_old:           true,
			char_data_output_disabled: true,
			border_part_40:            0,
			border_part_38:            0,
			top_compare:               0x37,
			bottom_compare:            0xF7,
		}
	}

	/* Reset restores the pre-display border state and the 24-row comparison geometry through the normal constructor path. */
	pub fn reset(&mut self) {
		*self = Self::new();
	}

	/*
	RSEL changes the two vertical comparison lines rather than directly opening or closing the border. The live flip-flops are reconsidered only at the cycle-specific comparison points.
	*/
	#[inline(always)]
	pub fn recompute_compare(&mut self, rsel: bool) {
		if rsel {
			self.top_compare    = 0x33;
			self.bottom_compare = 0xFB;
		} else {
			self.top_compare    = 0x37;
			self.bottom_compare = 0xF7;
		}
	}

	#[inline(always)]
	/* At the line boundary, only the programmed vertical compare lines may change the vertical border flip-flop. DEN is sampled when the top compare is reached. */
	pub fn begin_raster_line(&mut self, raster_y: u16, den: bool) {
		if raster_y == self.top_compare && den {
			self.vertical_border           = false;
			self.char_data_output_disabled = false;
		}
		if raster_y == self.bottom_compare {
			self.vertical_border           = true;
			self.char_data_output_disabled = true;
		}
	}

	#[inline(always)]
	/* Cycle 63 repeats the vertical comparison after late register writes so the next line starts from the state the hardware would have latched at the end of this one. */
	pub fn tick_cycle63(&mut self, raster_y: u16, den: bool) {
		if raster_y == self.bottom_compare {
			self.vertical_border           = true;
			self.char_data_output_disabled = true;
		} else if raster_y == self.top_compare && den {
			self.vertical_border           = false;
			self.char_data_output_disabled = false;
		}
	}

	/*
	The 40-column left edge is sampled in two half-cycle-sized pieces. Keeping both observations allows a CSEL or DEN write near the comparator to expose only part of the eight-pixel border cell.
	*/
	#[inline(always)]
	pub fn update_left_border_40_column(&mut self, csel: bool, raster_y: u16, den: bool) {
		self.border_part_40 = self.main_border as u8;
		if csel {
			if raster_y == self.bottom_compare {
				self.vertical_border           = true;
				self.char_data_output_disabled = true;
			} else if raster_y == self.top_compare && den {
				self.vertical_border           = false;
				self.char_data_output_disabled = false;
			}
			if !self.vertical_border {
				self.main_border = false;
			}
		}
		self.border_part_40 |= (self.main_border as u8) << 1;
	}

	/*
	The 38-column left comparator occurs four pixels later than the 40-column comparator. The two stored bits preserve the border state on either side of that transition for split-cell rendering.
	*/
	#[inline(always)]
	pub fn update_left_border_38_column(&mut self, csel: bool, raster_y: u16, den: bool) {
		self.border_part_38 = self.main_border as u8;
		if !csel {
			if raster_y == self.bottom_compare {
				self.vertical_border           = true;
				self.char_data_output_disabled = true;
			} else if raster_y == self.top_compare && den {
				self.vertical_border           = false;
				self.char_data_output_disabled = false;
			}
			if !self.vertical_border {
				self.main_border = false;
			}
		}
		self.border_part_38 |= (self.main_border as u8) << 1;
	}

	#[inline(always)]
	/* The 38-column right comparator closes the main border four pixels earlier than the 40-column comparator. */
	pub fn update_right_border_38_column(&mut self, csel: bool) {
		if !csel {
			self.main_border = true;
		}
	}

	#[inline(always)]
	/* The 40-column right comparator closes the main border at the later horizontal position. */
	pub fn update_right_border_40_column(&mut self, csel: bool) {
		if csel {
			self.main_border = true;
		}
	}

	#[inline(always)]
	/* Preserve the border state seen by the first part of a split cell before a register write changes the live flip-flop. */
	pub fn latch_main_border_old(&mut self) {
		self.main_border_old = self.main_border;
	}
}

/* Default construction is identical to the documented hardware reset baseline exposed by new(). */
impl Default for BorderUnit {
	fn default() -> Self {
		Self::new()
	}
}