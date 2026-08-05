// =======================================================
// src/cpu/port.rs — MOS 6510/8502 I/O Port State
// =======================================================

use super::CpuModel;
use super::constants::{FADE_CYCLES, NEVER_PULLED_DOWN};

#[derive(Debug, Clone, Copy)]
/*
The integrated port exposes its direction register at $0000 and data register at $0001. Output bits come from the committed latch; input bits reflect external pins and pull-up behaviour. Writes are separated into a visible data latch and a committed output state so the motherboard observes the change at the intended CPU-cycle boundary (MOS-6510-DATASHEET-1982, on-chip I/O port).
*/
pub struct CpuPort {
	pub ddr: u8,
	/* The CPU-visible latch accepts the current write before the pin-driving state is committed. */
	pub latch_write: u8,
	/* The committed latch is the value currently participating in pin and pull-down behaviour. */
	pub latch_committed: u8,
	pub output: u8,
	/* Floating input pins retain a recent low level for a finite decay interval after an output stops pulling them down. */
	pub last_pull_down: [u64; 8],
	pub cassette_write: bool,
	pub cassette_sense: bool,
}

impl CpuPort {
	/* The integrated port starts with the direction, latch and pull-state values used by the 6510 reference machine. */
	pub fn new() -> Self {
		let mut port = Self {
			ddr: 0x2F,
			latch_write: 0x37,
			latch_committed: 0x37,
			output: 0x37,
			last_pull_down: [NEVER_PULLED_DOWN; 8],
			cassette_write: true,
			cassette_sense: true,
		};
		port.update_output_state(0, CpuModel::Mos6510);
		port
	}

	/* The convenience reset selects the MOS 6510 electrical model used by the reference C64. */
	pub fn reset(&mut self) {
		self.reset_for_model(CpuModel::Mos6510);
	}

	/* Model-aware reset restores both CPU-visible registers and the internal pin-decay bookkeeping before recomputing the driven outputs. */
	pub fn reset_for_model(&mut self, model: CpuModel) {
		self.ddr = 0x2F;
		self.latch_write = 0x37;
		self.latch_committed = 0x37;
		self.output = 0x37;
		self.last_pull_down = [NEVER_PULLED_DOWN; 8];
		self.cassette_write = true;
		self.cassette_sense = true;
		self.update_output_state(0, model);
	}

	#[inline(always)]
	/* A driven low refreshes the decay timestamp; a driven high removes any residual low charge immediately. */
	fn update_timestamps(&mut self, current_cycle: u64) {
		let low_pins = !self.latch_committed & self.ddr;
		let high_pins = self.latch_committed & self.ddr;
		for i in 0..8 {
			if (low_pins & (1 << i)) != 0 {
				self.last_pull_down[i] = current_cycle;
			} else if (high_pins & (1 << i)) != 0 {

				self.last_pull_down[i] = NEVER_PULLED_DOWN;
			}
		}
	}

	/* Direction bits select between the output latch and the externally pulled input state; cassette sense can pull port bit 4 low when configured as input. */
	fn update_output_state(&mut self, current_cycle: u64, model: CpuModel) {
		self.update_timestamps(current_cycle);

		if model == CpuModel::Mos8502 {
			self.output = (self.latch_committed & self.ddr) | (!self.ddr & 0x7F);
		} else {
			self.output = (self.latch_committed & self.ddr) | (!self.ddr);
		}

		if (self.ddr & 0x10) == 0 && !self.cassette_sense {
			self.output &= !0x10;
		}

		self.cassette_write = (self.ddr & 0x08 == 0) || (self.latch_committed & 0x08 != 0);
	}

	/* Reading $0001 combines output-latch bits with the live input pins rather than returning the data latch wholesale (MOS-6510-DATASHEET-1982, port read operation). */
	pub fn cpu_read(&mut self, addr: u16, current_cycle: u64, model: CpuModel) -> u8 {
		match addr {
			0x0000 => self.ddr,
			0x0001 => {
				let mut pins = 0xFF;

				if !self.cassette_sense {
					pins &= !0x10;
				}

				if model != CpuModel::Mos8502 {
					pins &= !0x20;
				}

				let input_mask = !self.ddr;
				for i in 0..8 {
					if (input_mask & (1 << i)) != 0 {
						if matches!(i, 3 | 6 | 7) {
							if current_cycle.wrapping_sub(self.last_pull_down[i]) < FADE_CYCLES {
								pins &= !(1 << i);
							}
						}
					}
				}

				if model == CpuModel::Mos8502 {
					((self.latch_write & self.ddr) | (pins & input_mask)) & 0x7F
				} else {
					(self.latch_write & self.ddr) | (pins & input_mask)
				}
			},
			_ => 0xFF,
		}
	}

	/* A register write changes the direction register or pending data latch; external pins do not change until commit_write orders the visible transition. */
	pub fn cpu_write(&mut self, addr: u16, value: u8, current_cycle: u64, model: CpuModel) {
		self.update_timestamps(current_cycle);

		match addr {
			0x0000 => self.ddr = value,
			0x0001 => {
				if model == CpuModel::Mos8502 {
					self.latch_write = value & 0x7F;
				} else {
					self.latch_write = value;
				}
			}
			_ => {}
		}
	}

	/* Committing after the register write keeps latch visibility and external pin changes in a single ordered CPU access. */
	pub fn commit_write(&mut self, current_cycle: u64, model: CpuModel) {
		self.latch_committed = self.latch_write;
		self.update_output_state(current_cycle, model);
	}

	#[inline(always)]
	/* The motherboard observes the resolved pin state rather than the raw data latch. */
	pub fn get_pins(&self) -> u8 {
		self.output
	}

	/* Cassette sense is active low at the port pin, so the external pressed state is inverted before the output state is resolved. */
	pub fn set_cassette_sense(&mut self, pressed: bool, current_cycle: u64, model: CpuModel) {
		self.update_timestamps(current_cycle);
		self.cassette_sense = !pressed;
		self.update_output_state(current_cycle, model);
	}
}

impl Default for CpuPort {
	fn default() -> Self {
		Self::new()
	}
}