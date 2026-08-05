// =======================================================
// src/vic/sprite_timing.rs — Per-raster-line sprite DMA enable, Y-expansion flip-flop and display activation (cycles 16, 55, 56, 58)
// =======================================================

use crate::vic::registers::Registers;
use super::sprite_unit::SpriteUnit;

impl SpriteUnit {
	/* Cycle 16 advances each active sprite's row counter and terminates DMA after the final row; pending vertical-expansion clears are consumed at this boundary. */
	#[inline]
	pub fn update_mcbase_and_dma(&mut self) {
		self.sprite_dma_previous = self.sprite_dma;
		let mut index = 0;
		while index < 8 {
			let bit = 1u8 << index;
			let vertical_expand = self.vertical_expand_flip & bit != 0;
			let clearing = self.vertical_expand_clear_pending & bit != 0;
			self.sprites[index].advance_mcbase(vertical_expand, clearing);
			if !self.sprites[index].dma_active {
				self.sprite_dma &= !bit;
			}
			index += 1;
		}
		self.vertical_expand_clear_pending = 0;
	}

	/*
	The first Y-comparison phase can start DMA when an enabled sprite matches the low eight bits of the raster line. Starting an expanded sprite forces its expansion flip-flop low so the first row is repeated on the correct subsequent line. (BAUER-VIC-II-1996, cycles 55 and 56)
	*/
	#[inline]
	pub fn start_sprite_dma_first_phase(&mut self, raster_line: u16, regs: &Registers) {
		self.sprite_dma_previous = self.sprite_dma;
		self.vertical_match_mask = 0;
		self.vertical_expand_flip = (!self.vertical_expand_flip & regs.sprite_y_exp)
			| (!regs.sprite_y_exp & self.vertical_expand_flip);
		let raster_y = raster_line as u8;
		let mut index = 0;
		while index < 8 {
			let bit = 1u8 << index;
			if self.sprite_dma & bit == 0
				&& regs.sprite_en & bit != 0
				&& regs.my[index] == raster_y
			{
				self.sprite_dma |= bit;
				self.sprites[index].dma_active = true;
				self.sprites[index].mc_base = 0;
				if regs.sprite_y_exp & bit != 0 {
					self.vertical_expand_flip &= !bit;
				}
			}
			index += 1;
		}
	}

	/* The second comparison phase catches sprites enabled or moved onto the current raster line between the two hardware sampling points. */
	#[inline]
	pub fn start_sprite_dma_second_phase(&mut self, raster_line: u16, regs: &Registers) {
		self.sprite_dma_previous = self.sprite_dma;
		let raster_y = raster_line as u8;
		let mut index = 0;
		while index < 8 {
			let bit = 1u8 << index;
			if self.sprite_dma & bit == 0
				&& regs.sprite_en & bit != 0
				&& regs.my[index] == raster_y
			{
				self.sprites[index].mc_base = 0;
				self.sprites[index].dma_active = true;
				self.sprite_dma |= bit;
				if regs.sprite_y_exp & bit != 0 {
					self.vertical_expand_flip &= !bit;
				}
			}
			index += 1;
		}
	}

	/*
	Cycle 58 reloads MC and derives display enable separately from DMA enable. A sprite can therefore finish fetching while its current row remains visible, or remain in DMA without yet reaching the X comparator.
	*/
	#[inline]
	pub fn load_mc_and_update_sprite_display(&mut self, raster_line: u16, regs: &Registers) {
		let mut index = 0;
		while index < 8 {
			self.sprites[index].load_mc_from_base();
			index += 1;
		}
		self.sprite_display &= self.sprite_dma;
		let raster_y = raster_line as u8;
		index = 0;
		while index < 8 {
			let bit = 1u8 << index;
			if self.sprite_dma & regs.sprite_en & bit != 0 && regs.my[index] == raster_y {
				self.arm_sprite(index);
				self.sprite_display |= bit;
			}
			if self.sprite_display & bit == 0 {
				self.idle_sprite(index);
			}
			index += 1;
		}
	}

	/*
	Writes to the Y-expansion register interact with two cycle-sensitive latches. Clearing a bit near cycles 15 or 55 can alter the expansion flip-flop before MCBASE is updated, which is the basis of vertical sprite crunch effects.
	*/
	#[inline(always)]
	pub fn write_y_expand(&mut self, data: u8, cycle: u16, regs: &mut Registers) {
		if cycle == 15 {
			self.vertical_expand_clear_pending = !(data | self.vertical_expand_flip);
		}
		if cycle == 55 {
			let old_y_exp             = regs.sprite_y_exp;
			self.vertical_expand_flip = (old_y_exp & self.vertical_expand_flip) | (!old_y_exp & self.vertical_expand_flip & !data);
		}
		self.vertical_expand_flip = (self.vertical_expand_flip & data) | !data;
		regs.sprite_y_exp         = data;
	}
}