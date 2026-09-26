// =======================================================
// src/vic/sprite_dma.rs — Sprite DMA memory access, data buffer recomposition and mc_base cycle update
// =======================================================

use super::constants::{IDLE_ACCESS_ADDRESS, SPRITE_POINTER_TABLE_OFFSET};

use crate::memory::Memory;
use crate::vic::bus_access::vic_read;
use super::sprite::Sprite;

impl Sprite {
	/* Each sprite pointer is fetched from the last eight bytes of the active video matrix, independently of whether sprite DMA is currently active. (BAUER-VIC-II-1996, sprite pointer accesses) */
	#[inline(always)]
	pub fn slot_access_pointer(&mut self, mem: &mut Memory, vm_base: u16, bank: u8, cycle: u64) {
		let addr     = vm_base.wrapping_add(SPRITE_POINTER_TABLE_OFFSET + self.index as u16);
		self.pointer = vic_read(mem, addr, bank, cycle) as u16;
	}

	/*
	A sprite data slot either reads the next byte selected by pointer and MC, performs the VIC-II idle access when AEC owns the bus without active DMA, or receives all ones when the CPU retains the bus. Forced-AEC anomalies still advance MC because the internal counter progresses even when the expected memory byte is not obtained.
	*/
	#[inline(always)]
	pub fn slot_access_data(
		&mut self,
		column: usize,
		mem: &mut Memory,
		bank: u8,
		cycle: u64,
		aec_low: bool,
		force_aec_even_if_dma: bool,
	) {
		if self.dma_active {
			if force_aec_even_if_dma && !aec_low {
				self.fetch_buf[column] = 0xFF;
				self.mc = (self.mc + 1) & 63;
			} else {
				let addr = (self.pointer << 6).wrapping_add(self.mc);
				self.fetch_buf[column] = if column == 1 { vic_read(mem, addr, bank, cycle) } else { mem.vic_read_phi2(addr, bank, cycle) };
				self.mc = (self.mc + 1) & 63;
			}
		} else if aec_low {
			self.fetch_buf[column] = if column == 1 { vic_read(mem, IDLE_ACCESS_ADDRESS, bank, cycle) } else { mem.vic_read_phi2(IDLE_ACCESS_ADDRESS, bank, cycle) };
		} else {
			self.fetch_buf[column] = 0xFF;
		}
		if column == 0 {
			self.recompose_data_buffer();
		}
	}

	/* The three fetch slots arrive in VIC-II bus order and are recomposed into the 24-bit shifter with the first displayed bits in the most significant byte. */
	#[inline(always)]
	pub fn recompose_data_buffer(&mut self) {
		self.data_buffer = ((self.fetch_buf[2] as u32) << 16)
			| ((self.fetch_buf[1] as u32) << 8)
			| (self.fetch_buf[0] as u32);
	}

	/*
	MCBASE advances only on the appropriate vertical-expansion phase. Clearing expansion at the critical time combines old and current counter bits rather than performing a simple assignment, modelling the well-known sprite crunch path. DMA ends after the 63rd byte of the sprite image. (BAUER-VIC-II-1996, sprite expansion and DMA termination)
	*/
	#[inline(always)]
	pub fn advance_mcbase(&mut self, vertical_expand_flip_bit: bool, clearing_vertical_expand_bit: bool) {
		if vertical_expand_flip_bit {
			if clearing_vertical_expand_bit {
				self.mc_base = (0x2A & self.mc_base & self.mc) | (0x15 & (self.mc_base | self.mc));
			} else {
				self.mc_base = self.mc;
			}
		}
		if self.mc_base == 63 {
			self.dma_active = false;
		}
	}

	/* At the display-start phase, MC is reloaded from MCBASE so the following three fetches address one 24-bit sprite row. */
	#[inline(always)]
	pub fn load_mc_from_base(&mut self) {
		self.mc = self.mc_base;
	}
}