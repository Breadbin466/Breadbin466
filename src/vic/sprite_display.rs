// =======================================================
// src/vic/sprite_display.rs — sprite display, shifter transitions and collision handling
// =======================================================

use super::state::VicII;
use crate::vic::registers::{Registers, IrqState};
use crate::vic::screen::Screen;
use crate::vic::constants::{IRQ_MMC, IRQ_MBC};
use super::sprite::SpriteShifterState;
use super::sprite_unit::{SpriteUnit, SpriteDrawFlags};

impl SpriteUnit {
	/* Arming preserves fetched data and waits for the X comparator; it does not imply that DMA is still active. */
	#[inline(always)]
	pub(super) fn arm_sprite(&mut self, index: usize) {
		let bit = 1u8 << index;
		if self.sprites[index].shifter_state == SpriteShifterState::Idle {
			self.sprites[index].shifter_state = SpriteShifterState::Armed;
			self.active_sprite_mask          |= bit as u16;
		}
		self.sprites[index].position_missed_this_column = false;
	}

	/* A sprite returns to idle only after its shifter has stopped and the display-enable latch no longer requests another comparator start. */
	#[inline(always)]
	pub(super) fn idle_sprite(&mut self, index: usize) {
		let bit = 1u8 << index;
		if self.sprites[index].shifter_state == SpriteShifterState::Armed {
			self.sprites[index].shifter_state  = SpriteShifterState::Idle;
			self.active_sprite_mask           &= !(bit as u16);
		}
	}

	/*
	All eight sprite engines are evaluated in priority order for one VIC-II column. Collision results are pipelined across the intra-cycle pixel split, while writes to multicolour, expansion and priority registers can affect only the pixels that occur after the write point. IRQ is raised when a collision latch changes from empty to non-empty, not for every additional colliding pixel. (BAUER-VIC-II-1996, sprite collisions)
	*/
	#[inline]
	pub fn draw_sprites(&mut self, trigger_column: u8, regs: &Registers, screen: &mut Screen, irq: &mut IrqState) {
		self.coll.sprite_sprite_int >>= 1;
		self.coll.sprite_data_int >>= 1;
		self.coll.curr_sprite_sprite_coll = self.coll.next_sprite_sprite_coll;
		self.coll.curr_sprite_data_coll = self.coll.next_sprite_data_coll;
		self.coll.next_sprite_sprite_coll = 0;
		self.coll.next_sprite_data_coll = 0;

		let mc_changed_now = self.mc_changed_at == self.current_cycle_counter;
		let horizontal_expand_changed_now = self.horizontal_expand_changed_at == self.current_cycle_counter;
		let prio_changed_now = self.prio_changed_cycle == self.current_cycle_counter;
		let mut index = 0;

		while index < 8 {
			let bit = 1u8 << index;
			let flags = SpriteDrawFlags {
				mc_changed_now,
				horizontal_expand_changed_now,
				prio_changed_now,
				mc_prev: self.mc_prev & bit != 0,
				horizontal_expand_prev: self.horizontal_expand_prev & bit != 0,
				prio_prev: self.prio_prev & bit != 0,
				sprite_display_bit: self.sprite_display & bit != 0,
			};

			match self.sprites[index].shifter_state {
				SpriteShifterState::Idle => {}
				SpriteShifterState::Armed => {
					if trigger_column == self.sprites[index].trigger_column
						&& !self.sprites[index].position_missed_this_column
					{
						let pixels = self.sprites[index].init_draw();
						self.sprites[index].draw_sprite(pixels as i32, regs, screen, &flags, &mut self.coll);
					}
				}
				SpriteShifterState::ShiftingHorizontalPositionChanged => {
					let pixels = self.sprites[index].initial_pixel_offset;
					self.sprites[index].draw_sprite(pixels as i32, regs, screen, &flags, &mut self.coll);
					self.sprites[index].shifter_state = SpriteShifterState::Shifting;
				}
				SpriteShifterState::Shifting => {
					self.sprites[index].draw_sprite(8, regs, screen, &flags, &mut self.coll);
				}
			}

			if self.sprites[index].shifter_state == SpriteShifterState::Idle {
				self.active_sprite_mask &= !(bit as u16);
			}
			self.sprites[index].position_missed_this_column = false;
			index += 1;
		}

		let sprite_sprite_was_clear = self.sprite_sprite_collision == 0;
		self.sprite_sprite_collision |= self.coll.curr_sprite_sprite_coll;
		if sprite_sprite_was_clear && self.sprite_sprite_collision != 0 {
			irq.trigger(IRQ_MMC);
		}

		let sprite_data_was_clear = self.sprite_data_collision == 0;
		self.sprite_data_collision |= self.coll.curr_sprite_data_coll;
		if sprite_data_was_clear && self.sprite_data_collision != 0 {
			irq.trigger(IRQ_MBC);
		}

		let collision_activity = self.coll.sprite_data_int
			| self.coll.sprite_sprite_int
			| self.coll.next_sprite_sprite_coll
			| self.coll.next_sprite_data_coll;
		self.active_sprite_mask = (self.active_sprite_mask & 0xFF) | ((collision_activity as u16) << 8);
	}

	/* Reading the sprite-sprite collision register returns the accumulated latch immediately, while clearing is delayed so pixels already in the display pipeline can still contribute. */
	#[inline(always)]
	pub fn read_sprite_sprite_collision(&mut self) -> u8 {
		let v = self.sprite_sprite_collision;
		self.coll.sprite_sprite_int        = 1;
		self.clock_read_sprite_sprite_coll = self.current_cycle_counter + 2;
		v
	}

	/* Reading the sprite-background collision register schedules the same delayed clear used by the hardware-visible pixel pipeline. */
	#[inline(always)]
	pub fn read_sprite_data_collision(&mut self) -> u8 {
		let v = self.sprite_data_collision;
		self.coll.sprite_data_int        = 1;
		self.clock_read_sprite_data_coll = self.current_cycle_counter + 2;
		v
	}
}

impl VicII {
	/* The fast path skips sprite rendering only when neither display shifting nor collision tail activity remains armed. */
	#[inline(always)]
	pub(super) fn draw_active_sprites(&mut self, cycle_prev: u8) {
		if self.sprites.any_armed_or_active() {
			self.sprites.draw_sprites(cycle_prev, &self.regs, &mut self.screen, &mut self.irq);
		}
	}
}