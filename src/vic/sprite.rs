// =======================================================
// src/vic/sprite.rs — Per-sprite state, geometry and shifter initialisation
// =======================================================

pub use crate::vic::constants::VIDEOWIDTH;
use crate::vic::constants::DATA_LOAD_X;
/*
The shifter state is independent from sprite DMA. DMA may already be fetching the next row while the current row remains armed or shifting on screen. A horizontal-position write during the trigger column receives its own state because the VIC-II can begin output with a shortened first group rather than restarting at the next cycle. (BAUER-VIC-II-1996, sprite display)
*/
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpriteShifterState {
	/* No display sequence is pending for this sprite. */
	Idle,
	/* Sprite display is enabled and waits for the programmed X comparator. */
	Armed,
	/* The 24-bit data register is currently feeding pixels. */
	Shifting,
	/* An X write crossed the comparator inside the current column, so only the remaining sub-column pixels are emitted before normal shifting resumes. */
	ShiftingHorizontalPositionChanged,
}

/*
The VIC-II horizontal counter wraps through a 504-dot domain whose cycle zero lies inside horizontal blanking. Converting a cycle to that domain lets register writes and pixel triggers be compared without introducing a special case at the raster-line boundary. (BAUER-VIC-II-1996, horizontal timing)
*/
#[inline(always)]
pub fn horizontal_pos_from_cycle(cycle: u16, offset: i16) -> u16 {
	let mut x: i32 = if cycle < 14 {
		0x18C + (cycle as i32) * 8
	} else {
		((cycle as i32) - 14) * 8 + 4
	};
	x += offset as i32;
	if x >= 0x1F8 {
		x -= 0x1F8;
	} else if x < 0 {
		x += 0x1F8;
	}
	(x & 0x1FF) as u16
}

/*
Sprite pixels are stored in a line buffer whose visible origin differs from the VIC-II horizontal counter origin. This conversion preserves the wrap-around region where sprites programmed near the end of the 9-bit X range appear at the left edge of the next visible span.
*/
#[inline(always)]
pub fn sprite_trigger_index_from_horizontal_pos(horizontal_pos: u16) -> u16 {
	if horizontal_pos >= 0x194 {
		horizontal_pos - 0x194
	} else {
		horizontal_pos.wrapping_add(100)
	}
}

/*
Sprite contains the state of one hardware sprite engine. Pointer and memory counters describe the DMA side, while the shifter, expansion flip-flops and horizontal cursors describe the independent display side. Keeping both sets of state together is necessary because register writes can affect an in-flight row after its bytes have already been fetched. (BAUER-VIC-II-1996, sprite operation)
*/
#[derive(Debug, Clone, Copy)]
pub struct Sprite {
	pub index:                         usize,
	pub spr_bit:                       u8,
	pub pointer:                       u16,
	pub fetch_buf:                     [u8; 3],
	pub mc:                            u16,
	pub mc_base:                       u16,
	pub dma_active:                    bool,

	pub shifter_state:                 SpriteShifterState,
	pub position_missed_this_column:   bool,
	pub bits_remaining:                i16,
	pub horizontal_expand_flip:        bool,
	pub multicolour_flip:              bool,
	pub data_reload_stall:             bool,
	pub pending_pixel_colour:          u8,
	pub data_buffer:                   u32,
	pub initial_pixel_offset:          i16,

	pub horizontal_pos:                u16,
	pub trigger_column:                u8,
	pub trigger_column_horizontal_pos: i32,
	pub horizontal_pixel_start:        i32,
	pub horizontal_pixel_cursor:       i32,

	pub reload_horizontal_pos:          u16,
	pub reload_complete_horizontal_pos: u16,
	pub reload_trigger_index:           u16,
}

impl Sprite {

	/* Construction fixes the sprite identity and the cycle-specific data-reload position used by the shared VIC-II fetch schedule. */
	pub fn new(index: usize) -> Self {
		let reload_horizontal_pos = DATA_LOAD_X[index];
		Self {
			index,
			spr_bit:                         1u8 << index,
			pointer:                         0,
			fetch_buf:                       [0; 3],
			mc:                              0,
			mc_base:                         0,
			dma_active:                      false,
			shifter_state:                   SpriteShifterState::Idle,
			position_missed_this_column:     false,
			bits_remaining:                  0,
			horizontal_expand_flip:          false,
			multicolour_flip:                false,
			data_reload_stall:               false,
			pending_pixel_colour:            0,
			data_buffer:                     0,
			initial_pixel_offset:            0,
			horizontal_pos:                  0,
			trigger_column:                  0,
			trigger_column_horizontal_pos:   0,
			horizontal_pixel_start:          0,
			horizontal_pixel_cursor:         0,
			reload_horizontal_pos,
			reload_complete_horizontal_pos:  reload_horizontal_pos + 13,
			reload_trigger_index:            sprite_trigger_index_from_horizontal_pos(reload_horizontal_pos),
		}
	}

	/* Reset clears display-side transient state without inventing a DMA fetch or changing the sprite's fixed identity. */
	pub fn reset(&mut self) {
		self.shifter_state               = SpriteShifterState::Idle;
		self.position_missed_this_column = false;
		self.bits_remaining              = 0;
		self.horizontal_expand_flip      = false;
		self.data_reload_stall           = false;
		self.horizontal_pos              = 0;
		self.data_buffer                 = 0;
		self.trigger_column              = 0;
		self.initial_pixel_offset        = 0;
		self.multicolour_flip            = false;
	}

	/*
	An X-position write updates both the future comparator column and the current display sequence. If software moves an armed sprite behind the comparator during the matching column, the comparator is considered missed; moving it across the comparator can instead start a shortened sequence immediately.
	*/
	pub fn set_horizontal_pos(&mut self, x: u16, current_column: u8) {
		let current_column_horizontal_pos = horizontal_pos_from_cycle(current_column as u16, 4);
		let mut x_diff = current_column_horizontal_pos as i32 - self.horizontal_pos as i32;
		if x_diff > 0xFA { x_diff -= 0x1F8; } else if x_diff < -0xFA { x_diff += 0x1F8; }

		if self.horizontal_pos != x && self.shifter_state == SpriteShifterState::Armed {
			if current_column == self.trigger_column && x_diff > 0 {
				self.init_draw();
				self.shifter_state = SpriteShifterState::ShiftingHorizontalPositionChanged;
			}
		}

		if x >= 0x1F8 {
			self.trigger_column = 0;
		} else if x >= 0x194 {
			self.trigger_column                  = (((x - 0x18C) / 8) & 0xFF) as u8;
			self.trigger_column_horizontal_pos   = (self.trigger_column as i32) * 8 + 0x18C;
			self.horizontal_pixel_start          = x as i32 - 0x194;
		} else {
			self.trigger_column                  = (((x as i32 + 4) / 8 + 13) & 0xFF) as u8;
			self.trigger_column_horizontal_pos   = ((self.trigger_column as i32) - 13) * 8 - 4;
			self.horizontal_pixel_start          = x as i32 + 100;
		}

		if self.trigger_column == current_column && self.shifter_state == SpriteShifterState::Armed && self.horizontal_pos != x {
			let mut xd = current_column_horizontal_pos as i32 - x as i32;
			if xd > 0xFA { xd -= 0x1F8; } else if xd < -0xFA { xd += 0x1F8; }
			if xd > 0 { self.position_missed_this_column = true; }
		}

		self.horizontal_pos = x;
	}

	/*
	Starting display resets the horizontal expansion and multicolour phase flip-flops. A trigger that overlaps the sprite data-reload window suppresses the normal 24-bit start, reproducing the gap caused when the display shifter and DMA reload timing collide.
	*/
	pub fn init_draw(&mut self) -> i16 {
		self.shifter_state           = SpriteShifterState::Shifting;
		self.horizontal_pixel_cursor = self.horizontal_pixel_start;
		self.horizontal_expand_flip  = false;
		self.pending_pixel_colour    = 0xFF;
		self.data_reload_stall       = false;
		self.multicolour_flip        = false;

		if self.horizontal_pos > self.reload_horizontal_pos && self.horizontal_pos < self.reload_complete_horizontal_pos {
			self.bits_remaining       = 0;
			self.initial_pixel_offset = 0;
		} else {
			self.bits_remaining       = 24;
			self.initial_pixel_offset = 8 - (self.horizontal_pos as i16 - self.trigger_column_horizontal_pos as i16);
		}
		self.initial_pixel_offset
	}
}