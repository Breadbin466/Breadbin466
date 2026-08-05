// =======================================================
// src/vic/memory_cycle.rs — graphics pipeline transfer and DRAM refresh access
// =======================================================

use crate::memory::Memory;
use super::state::VicII;
use super::bus_access::vic_read;
use super::constants::DRAM_REFRESH_BASE;

/* These helpers advance the shared graphics fetch pipeline and issue DRAM refresh accesses. Both are real VIC bus cycles even though neither directly produces a visible pixel at the moment it executes. */
impl VicII {
	#[inline(always)]
	/* Move the newly fetched matrix and graphics bytes through two delay stages. Pixel generation consumes the older pair, preserving the one-cell separation between bus fetch and visible output. */
	pub(super) fn shift_graphics_pipelines(&mut self) {
		/* Character metadata and graphics data advance as a pair. Moving one without the other would associate a glyph byte with the wrong colour or screen-code context. */
		self.char_data_pipeline_2 = self.char_data_pipeline_1;
		self.graphics_data_pipeline_2 = self.graphics_data_pipeline_1;
		self.char_data_pipeline_1 = self.char_data_fetched;
		self.graphics_data_pipeline_1 = self.graphics_data_fetched;
	}

	#[inline(always)]
	/* Refresh cycles read descending addresses in the $3Fxx page. The data is discarded; the address sequence exists to reproduce the VIC-II's DRAM row-refresh traffic and observable bus value. */
	pub(super) fn dram_refresh(&mut self, memory: &mut Memory, bank: u8, master_cycle: u64) {
		let counter = self.dram_refresh_counter;
		/* Although the fetched byte is unused internally, vic_read still refreshes the shared memory data-bus latch. CPU-visible open-bus behaviour can therefore depend on the most recent refresh address. */
		vic_read(memory, DRAM_REFRESH_BASE | (counter as u16), bank, master_cycle);
		self.dram_refresh_counter = counter.wrapping_sub(1);
	}
}