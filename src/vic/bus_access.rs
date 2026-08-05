// =======================================================
// src/vic/bus_access.rs — VIC-II bus arbitration and fetch slots
// =======================================================

use crate::memory::Memory;
use crate::vic::constants::{IDLE_ACCESS_ADDRESS, VIDEO_COUNTER_MASK};
use super::state::VicII;
/* The mask table expresses which active sprite DMA channels must lower BA in each raster cycle before their pointer or data slots. */
#[inline(always)]
fn sprite_dma_ba_mask(cycle: u16) -> u8 {
	/* Each bit corresponds to one sprite. The table covers the three-cycle BA warning window preceding that sprite's pointer/data slots, including the schedule split across adjacent raster lines. */
	match cycle {
		1 => 0x18, 2 => 0x38, 3 => 0x30, 4 => 0x70, 5 => 0x60, 6 => 0xE0,
		7 | 8 => 0xC0, 9 | 10 => 0x80,
		55 | 56 => 0x01, 57 | 58 => 0x03, 59 => 0x07, 60 => 0x06,
		61 => 0x0E, 62 => 0x0C, 63 => 0x1C,
		_ => 0,
	}
}

#[inline(always)]
/* Idle sprite slots still perform a VIC-side bus access when AEC is low. If the CPU retains the bus, no VIC read occurs and the undriven value remains high. */
pub(super) fn floating_bus_read(memory: &mut Memory, bank: u8, cycle: u64, aec_low: bool) -> u8 {
	if aec_low {
		vic_read(memory, IDLE_ACCESS_ADDRESS, bank, cycle)
	} else {
		0xFF
	}
}

impl VicII {
	#[inline(always)]
	/*
	BA combines badline and sprite requests. AEC follows only after three low-BA cycles, modelling the warning interval that lets the 6510 finish write cycles before the VIC takes the bus. (BAUER-VIC-II-1996, memory access timing)
	*/
	pub(super) fn update_bus_arbitration(&mut self, cycle: u16) {
		let cycle = cycle as usize;
		let sprite_request = (self.sprites.sprite_dma & sprite_dma_ba_mask(cycle as u16)) != 0;
		let next_ba_low = self.timing.ba_out.is_active() || sprite_request;
		if next_ba_low {
			if !self.ba_low {
				self.clock_ba_low = self.current_clock;
			}
			if self.aec_counter >= 0 {
				self.aec_counter -= 1;
			}
		} else {
			if self.ba_low {
				self.clock_ba_high = self.current_clock;
			}
			self.aec_counter = 3;
		}
		self.ba_low = next_ba_low;
		self.aec_low = self.aec_counter < 0;
	}

	#[inline(always)]
	/* A c-access fills one entry of the 40-column matrix line with screen code and colour RAM. If AEC is not yet low, the VIC observes the CPU-side bus value instead of owning RAM. */
	pub(super) fn character_access(&mut self, memory: &mut Memory, bank: u8, master_cycle: u64, vm_base: u16) {
		if !self.character_access_state.is_enabled() {
			return;
		}
		let index = self.vmli as usize % 40;
		/* When the VIC owns the bus, screen RAM supplies the low byte and the separate four-bit colour RAM supplies the upper nibble. Before AEC falls, the displaced c-access instead captures the CPU's next opcode pattern, reproducing the bus contamination visible to the matrix pipeline. */
		let value = if self.aec_counter < 0 {
			let offset = self.vc & VIDEO_COUNTER_MASK;
			let matrix_byte = vic_read(memory, vm_base.wrapping_add(offset), bank, master_cycle);
			let colour = read_color_ram(memory, 0xD800u16.wrapping_add(offset)) & 0x0F;
			((colour as u16) << 8) | matrix_byte as u16
		} else {
			(((self.cpu_next_op_code & 0x0F) as u16) << 8) | 0x00FF
		};
		self.matrix_line[index] = value;
	}

	#[inline(always)]
	/* Sprite pointer and data accesses occupy fixed slots split across the end and beginning of adjacent raster lines. */
	pub(super) fn sprite_ptr_data_slots(&mut self, memory: &mut Memory, bank: u8, master_cycle: u64, raster_cycle: u16) {
		match raster_cycle {
			3 => self.sprite_pointer_and_third_data_access(4, memory, bank, master_cycle),
			4 => self.sprite_first_and_second_data_access(4, memory, bank, master_cycle),
			5 => self.sprite_pointer_and_third_data_access(5, memory, bank, master_cycle),
			6 => self.sprite_first_and_second_data_access(5, memory, bank, master_cycle),
			7 => self.sprite_pointer_and_third_data_access(6, memory, bank, master_cycle),
			8 => self.sprite_first_and_second_data_access(6, memory, bank, master_cycle),
			9 => self.sprite_pointer_and_third_data_access(7, memory, bank, master_cycle),
			10 => self.sprite_first_and_second_data_access(7, memory, bank, master_cycle),
			59 => self.sprite_first_and_second_data_access(0, memory, bank, master_cycle),
			60 => self.sprite_pointer_and_third_data_access(1, memory, bank, master_cycle),
			61 => self.sprite_first_and_second_data_access(1, memory, bank, master_cycle),
			62 => self.sprite_pointer_and_third_data_access(2, memory, bank, master_cycle),
			63 => self.sprite_first_and_second_data_access(2, memory, bank, master_cycle),
			_ => {}
		}
	}

	#[inline(always)]
	/* The first slot reads the sprite pointer and the third data byte. Disabled DMA substitutes an idle/floating read so the fetch buffer still follows the physical bus schedule. */
	pub(super) fn sprite_pointer_and_third_data_access(&mut self, index: usize, memory: &mut Memory, bank: u8, cycle: u64) {
		let matrix_base = self.regs.vm_base();
		self.sprites.slot_ptr(index, memory, matrix_base, bank, cycle);
		if (self.sprites.sprite_dma & (1 << index)) != 0 {
			self.sprites.slot_data(index, 2, memory, bank, cycle, self.aec_low, false);
		} else {
			let value = floating_bus_read(memory, bank, cycle, self.aec_low);
			self.sprites.sprites[index].fetch_buf[2] = value;
		}
	}

	#[inline(always)]
	/* The paired slot fetches data bytes one and zero in bus order, then recomposes the 24-bit shift register. Disabled DMA performs the same number of idle reads. */
	pub(super) fn sprite_first_and_second_data_access(&mut self, index: usize, memory: &mut Memory, bank: u8, cycle: u64) {
		if (self.sprites.sprite_dma & (1 << index)) != 0 {
			self.sprites.slot_data(index, 1, memory, bank, cycle, self.aec_low, false);
			self.sprites.slot_data(index, 0, memory, bank, cycle, self.aec_low, false);
			return;
		}
		let byte_1 = floating_bus_read(memory, bank, cycle, self.aec_low);
		let byte_0 = floating_bus_read(memory, bank, cycle, self.aec_low);
		let sprite = &mut self.sprites.sprites[index];
		sprite.fetch_buf[1] = byte_1;
		sprite.fetch_buf[0] = byte_0;
		sprite.recompose_data_buffer();
	}
}

#[inline(always)]
/* CIA2 selects one of four inverted 16 KiB VIC banks; the memory subsystem resolves the electrical inversion before returning the bank number. */
pub fn get_vic_bank(memory: &Memory) -> u8 {
	memory.get_vic_bank()
}

#[inline(always)]
/* VIC addresses are fourteen bits wide inside the selected bank. All VIC memory traffic passes through this helper so bus latching and character-ROM visibility remain centralised in Memory. */
pub fn vic_read(memory: &mut Memory, addr: u16, bank: u8, cycle: u64) -> u8 {
	memory.vic_read(addr & crate::vic::constants::VIC_ADDRESS_MASK, bank, cycle)
}

#[inline(always)]
/* Colour RAM is a separate four-bit store and is therefore read outside the normal VIC bank address space. */
pub fn read_color_ram(memory: &Memory, addr: u16) -> u8 {
	memory.read_color_ram(addr)
}