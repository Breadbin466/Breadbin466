// =======================================================
// src/vic/fetch.rs — VIC-II graphics access and character pipeline
// =======================================================

use super::constants::{IDLE_ACCESS_ADDRESS, SCREEN_COLUMNS, VIDEO_COUNTER_MASK};

use super::fsm::DisplayTransition;
use crate::memory::Memory;
use crate::vic::bus_access::vic_read;
use super::state::VicII;

/* Graphics accesses consume the matrix value fetched by the preceding c-access and select character ROM, bitmap RAM or idle data according to the current mode and RC/VC counters. */
impl VicII {
	#[inline(always)]
	/* A g-access either performs the current display fetch or the mode-dependent idle fetch. VC and VMLI advance only after a real display fetch, so border and late-badline transitions cannot consume matrix entries that were never displayed. */
	pub(super) fn graphics_access(&mut self, memory: &mut Memory, bank: u8, cycle: u64) -> u8 {
		if self.display_state.is_idle() || self.display_transition == DisplayTransition::EnterDisplayAfterCharacterAccess {
			return self.read_idle_graphics(memory, bank, cycle);
		}
		self.latch_character_data();
		let graphics_data = self.read_graphics_data(memory, bank, cycle);
		self.vc = (self.vc + 1) & VIDEO_COUNTER_MASK;
		self.vmli = self.vmli.wrapping_add(1);
		graphics_data
	}

	#[inline(always)]
	/* Idle display cycles use the normal idle address or $39FF in extended modes.
	 * 6569R5 DMA-delay measurements identify $38FF for the pending idle fetch.
	 * Keeping the initial display-fetch slots separate is a model inferred from
	 * the colour-fetch diagnostics, rather than a published circuit description. */
	fn read_idle_graphics(&mut self, memory: &mut Memory, bank: u8, cycle: u64) -> u8 {
		if !self.border.char_data_output_disabled {
			self.char_data_fetched = 0;
		}
		let mut address = match self.previous_graphics_mode.code() {
			0..=3 => IDLE_ACCESS_ADDRESS,
			_ => 0x39FF,
		};

		if self.display_transition == DisplayTransition::EnterDisplayAfterCharacterAccess {
			if self.timing.cycle >= 18 && self.current_clock == self.forced_badline_c_access_clock {
				address = 0x38FF;
			}
			self.display_transition = DisplayTransition::Stable;
		}
		vic_read(memory, address, bank, cycle)
	}

	#[inline(always)]
	/* Select the matrix entry feeding the next graphics address. During ordinary display rows the existing line buffer is used; a forced mid-line badline requires the merge path below to model partially refreshed matrix data. */
	fn latch_character_data(&mut self) {
		if self.border.char_data_output_disabled {
			return;
		}
		let index = self.vmli as usize;
		if self.timing.is_badline || self.rc != 0 {
			self.char_data_fetched = self.matrix_line[index % 40];
			return;
		}
		self.merge_forced_badline_character(index);
	}

	#[inline]
	/* A badline forced after the normal c-access window does not replace the matrix line cleanly. Adjacent entries and retained carry bits combine according to the access phase, producing the characteristic corrupted character data. */
	fn merge_forced_badline_character(&mut self, index: usize) {

		let elapsed = self.current_clock - self.forced_badline_c_access_clock - 2;
		if elapsed >= SCREEN_COLUMNS {
			self.char_data_fetched = self.matrix_line[index % 40];
			return;
		}

		let forced_index = (elapsed.max(0) & 0xFF) as u8;
		let distance = forced_index.wrapping_sub(self.vmli);
		let span = if distance == 0 { 1 } else { distance } as usize;
		let previous = (forced_index.wrapping_sub(1) as usize % span) % 40;
		let current = index % 40;
		let next = forced_index as usize % 40;
		let previous_data = self.matrix_line[previous];
		let current_data = self.matrix_line[current];
		let next_data = self.matrix_line[next];
		let carry = if ((self.timing.cycle + 1) & 3) == 0 {
			0x08A6 & previous_data
		} else {
			0x08A6 & (self.char_data_carry | previous_data)
		};
		let merged = ((carry | (next_data & 0x013F)) & (current_data | next_data))
			| (!carry & (current_data & next_data));

		self.char_data_fetched = merged;
		self.matrix_line[current] = merged;
		self.matrix_line[next] = merged;
		self.char_data_carry = carry;
	}

	#[inline(always)]
	/* Build the g-access address from the previously selected graphics mode. Text modes index character data, bitmap modes index VC directly, and ECM masks the character code to six bits while preserving the upper bits for background selection. */
	fn read_graphics_data(&mut self, memory: &mut Memory, bank: u8, cycle: u64) -> u8 {
		let character_data = self.char_data_fetched;
		let character_base = self.regs.cb_base();
		let row = self.rc as u16;
		/* The next PHI1 address uses the live mode bits. The delayed mode latch
		 * belongs to idle selection, not the active graphics address multiplexer. */
		let address = match self.graphics_mode.code() {
			0 | 1 => character_base
				| ((character_data & 0x00FF) << 3)
				| row,
			2 | 3 => (character_base & 0x2000)
				| (self.vc << 3)
				| row,
			4 | 5 => character_base
				| ((character_data & 0x003F) << 3)
				| row,
			_ => (character_base & 0x2000)
				| ((self.vc & 0x033F) << 3)
				| row,
		};
		/* The first fetch after BMM falls still carries the bitmap counter
		 * through the address multiplexer while the character route opens. */
		let address = if self.previous_graphics_mode.bitmap_bit() && !self.graphics_mode.bitmap_bit() {
			if matches!(crate::pla::map_vic_addr(address, bank as usize, memory.cartridge.configuration.phi1_mode), crate::memory::constants::MapRegion::Char) {
				(address & 0x3F00) | (((self.vc << 3) | row) & 0x00FF)
			} else {
				(character_base & 0x2000) | (self.vc << 3) | row
			}
		} else { address };
		/* The ECM address mask survives into the first bitmap or character-ROM
		 * fetch. This transition model is inferred from the mode-split diagnostics. */
		let address = if self.previous_graphics_mode.extended_colour_bit() && !self.graphics_mode.extended_colour_bit()
			&& (self.graphics_mode.bitmap_bit() && (!self.previous_graphics_mode.bitmap_bit() || self.graphics_mode.multicolour_bit()) || matches!(crate::pla::map_vic_addr(address, bank as usize, memory.cartridge.configuration.phi1_mode), crate::memory::constants::MapRegion::Char)) {
			(address & !0x0600) | if self.graphics_mode.bitmap_bit() { 0 } else { character_base & 0x0600 }
		} else { address };
		vic_read(memory, address, bank, cycle)
	}
}