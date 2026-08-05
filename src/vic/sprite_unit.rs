// =======================================================
// src/vic/sprite_unit.rs — SpriteUnit: global sprite state, construction, reset and per-cycle DMA dispatch helpers
// =======================================================

use crate::memory::Memory;
use super::sprite::Sprite;

/* SpriteDrawFlags snapshots register values that changed during the current VIC-II cycle, allowing pixels before and after the write point to observe different settings. */
#[derive(Debug, Clone, Copy)]
pub struct SpriteDrawFlags {
	pub mc_changed_now:                bool,
	pub horizontal_expand_changed_now: bool,
	pub prio_changed_now:              bool,
	pub mc_prev:                       bool,
	pub horizontal_expand_prev:        bool,
	pub prio_prev:                     bool,
	pub sprite_display_bit:            bool,
}

/* CollisionOutputs separates current and next pixel groups because register reads and IRQ generation occur at cycle boundaries while sprite pixels are produced in two temporal halves. */
#[derive(Debug, Clone, Copy, Default)]
pub struct SpriteCollisionOutputs {
	pub curr_sprite_sprite_coll: u8,
	pub curr_sprite_data_coll:   u8,
	pub next_sprite_sprite_coll: u8,
	pub next_sprite_data_coll:   u8,
	pub sprite_sprite_int:       u8,
	pub sprite_data_int:         u8,
}

/*
SpriteUnit coordinates the eight independent sprite engines. DMA activation, memory counters, display enable, expansion flip-flops, fetched data and collision latches advance in separate phases because a sprite can be fetching the next line while pixels from the current line are still being displayed. (BAUER-VIC-II-1996, sprite operation)
*/
pub struct SpriteUnit {
	pub sprites: [Sprite; 8],

	pub active_sprite_mask: u16,

	pub sprite_sprite_collision: u8,
	pub sprite_data_collision:   u8,
	pub coll:                    SpriteCollisionOutputs,

	pub mc_prev:                      u8,
	pub horizontal_expand_prev:       u8,
	pub prio_prev:                    u8,
	pub mc_changed_at:                i64,
	pub horizontal_expand_changed_at: i64,
	pub prio_changed_cycle:           i64,
	pub current_cycle_counter:        i64,

	pub clock_read_sprite_sprite_coll: i64,
	pub clock_read_sprite_data_coll:   i64,

	pub vertical_expand_flip:          u8,
	pub vertical_expand_clear_pending: u8,
	pub sprite_dma:                    u8,
	pub sprite_dma_previous:               u8,
	pub sprite_display:                u8,
	pub vertical_match_mask:           u8,
}

impl SpriteUnit {
	/* Construction creates eight fixed sprite engines and initialises expansion flip-flops to the inactive-high state used before the first raster comparison. */
	pub fn new() -> Self {
		Self {
			sprites: [
				Sprite::new(0), Sprite::new(1), Sprite::new(2), Sprite::new(3),
				Sprite::new(4), Sprite::new(5), Sprite::new(6), Sprite::new(7),
			],
			active_sprite_mask:               0,
			sprite_sprite_collision:          0,
			sprite_data_collision:            0,
			coll:                             SpriteCollisionOutputs::default(),
			mc_prev:                          0,
			horizontal_expand_prev:           0,
			prio_prev:                        0,
			mc_changed_at:                    -1,
			horizontal_expand_changed_at:     -1,
			prio_changed_cycle:               -1,
			current_cycle_counter:            0,
			clock_read_sprite_sprite_coll:    -1,
			clock_read_sprite_data_coll:      -1,
			vertical_expand_flip:             0xFF,
			vertical_expand_clear_pending:    0,
			sprite_dma:                       0,
			sprite_dma_previous:                  0,
			sprite_display:                   0,
			vertical_match_mask:              0,
		}
	}

	/* Reset clears DMA, display and collision pipelines together so no stale sprite activity leaks into the first post-reset line. */
	pub fn reset(&mut self) {
		let mut index = 0;
		while index < 8 {
			self.sprites[index].reset();
			index += 1;
		}
		self.active_sprite_mask              = 0;
		self.sprite_sprite_collision         = 0;
		self.sprite_data_collision           = 0;
		self.coll                            = SpriteCollisionOutputs::default();
		self.mc_prev                         = 0;
		self.horizontal_expand_prev          = 0;
		self.prio_prev                       = 0;
		self.mc_changed_at                   = -1;
		self.horizontal_expand_changed_at    = -1;
		self.prio_changed_cycle              = -1;
		self.current_cycle_counter           = 0;
		self.clock_read_sprite_sprite_coll   = -1;
		self.clock_read_sprite_data_coll     = -1;
		self.vertical_expand_flip            = 0xFF;
		self.vertical_expand_clear_pending   = 0;
		self.sprite_dma                      = 0;
		self.sprite_dma_previous                 = 0;
		self.sprite_display                  = 0;
		self.vertical_match_mask             = 0;
	}

	#[inline(always)]
	/* The aggregate mask lets the sequencer skip sprite pixel work only when no engine is armed, shifting or awaiting a mid-line start. */
	pub fn any_armed_or_active(&self) -> bool { self.active_sprite_mask != 0 }

	#[inline(always)]
	/* Forward a position write together with the current raster column so the sprite engine can decide whether the comparator was crossed before or after the write. */
	pub fn set_horizontal_pos(&mut self, index: usize, x: u16, current_column: u8) {
		self.sprites[index].set_horizontal_pos(x, current_column);
	}

	#[inline(always)]
	/* Execute the pointer fetch assigned to this sprite slot. Pointer accesses occur even when the corresponding sprite DMA is inactive. */
	pub fn slot_ptr(&mut self, index: usize, mem: &mut Memory, vm_base: u16, bank: u8, cycle: u64) {
		self.sprites[index].slot_access_pointer(mem, vm_base, bank, cycle);
	}

	#[inline(always)]
	/* Execute one of the three sprite data fetches, passing the resolved AEC state so an interrupted access can be recovered later without advancing the wrong byte lane. */
	pub fn slot_data(
		&mut self,
		index: usize,
		column: usize,
		mem: &mut Memory,
		bank: u8,
		cycle: u64,
		aec_low: bool,
		force_aec_even_if_dma: bool,
	) {
		self.sprites[index].slot_access_data(column, mem, bank, cycle, aec_low, force_aec_even_if_dma);
	}

	/* Delayed clears become effective after the final collision contributions already present in the pixel pipeline have reached their latches. */
	#[inline(always)]
	pub fn tick_collision_clear(&mut self) {
		if self.clock_read_sprite_sprite_coll >= 0
			&& self.current_cycle_counter >= self.clock_read_sprite_sprite_coll
		{
			self.sprite_sprite_collision = 0;
			self.coll.next_sprite_sprite_coll = 0;
			self.clock_read_sprite_sprite_coll = -1;
		}

		if self.clock_read_sprite_data_coll >= 0
			&& self.current_cycle_counter >= self.clock_read_sprite_data_coll
		{
			self.sprite_data_collision = 0;
			self.coll.next_sprite_data_coll = 0;
			self.clock_read_sprite_data_coll = -1;
		}
	}

	/*
	When the CPU regains the bus in the middle of a sprite fetch sequence, the displaced byte is delivered later. The cycle number identifies the exact sprite and fetch column whose buffer must be completed before the 24-bit word is recomposed.
	*/
	#[inline(always)]
	pub fn complete_interrupted_sprite_access(&mut self, cycle: u16, data: u8) {
		match cycle {
			1 => self.sprites[3].fetch_buf[2] = data,
			2 => { self.sprites[3].fetch_buf[0] = data; self.sprites[3].recompose_data_buffer(); }
			3 => self.sprites[4].fetch_buf[2] = data,
			4 => { self.sprites[4].fetch_buf[0] = data; self.sprites[4].recompose_data_buffer(); }
			5 => self.sprites[5].fetch_buf[2] = data,
			6 => { self.sprites[5].fetch_buf[0] = data; self.sprites[5].recompose_data_buffer(); }
			7 => self.sprites[6].fetch_buf[2] = data,
			8 => { self.sprites[6].fetch_buf[0] = data; self.sprites[6].recompose_data_buffer(); }
			9 => self.sprites[7].fetch_buf[2] = data,
			10 => { self.sprites[7].fetch_buf[0] = data; self.sprites[7].recompose_data_buffer(); }
			58 => self.sprites[0].fetch_buf[2] = data,
			59 => { self.sprites[0].fetch_buf[0] = data; self.sprites[0].recompose_data_buffer(); }
			60 => self.sprites[1].fetch_buf[2] = data,
			61 => { self.sprites[1].fetch_buf[0] = data; self.sprites[1].recompose_data_buffer(); }
			62 => self.sprites[2].fetch_buf[2] = data,
			63 => { self.sprites[2].fetch_buf[0] = data; self.sprites[2].recompose_data_buffer(); }
			_ => {}
		}
	}

}

/* Default construction is identical to the documented hardware reset baseline exposed by new(). */
impl Default for SpriteUnit {
	fn default() -> Self { Self::new() }
}