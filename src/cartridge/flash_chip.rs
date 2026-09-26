// =======================================================
// src/cartridge/flash_chip.rs — AM29F040 flash command and polling state
// =======================================================

use super::constants::{FLASH_ERASE_CYCLES, FLASH_PROGRAM_CYCLES};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum FlashState {
	Idle,
	UnlockSecond,
	Command,
	Program,
	EraseFirst,
	EraseSecond,
	EraseCommand,
	AutoSelect,
}

pub(crate) enum FlashWrite {
	Program(u8),
	EraseSector,
	EraseChip,
}

/* Each flash chip has an independent command latch and embedded-operation
 * timer. Unlock addresses decode only A0-A10; higher address lines select
 * array cells for programming and sectors for erase (AM29F040B-DATASHEET). */
pub(crate) struct FlashChip {
	pub state: FlashState,
	completion_cycle: Option<u64>,
	toggle_bit: bool,
	polling_byte: u8,
}

impl FlashChip {
	pub fn new() -> Self {
		Self {
			state: FlashState::Idle,
			completion_cycle: None,
			toggle_bit: false,
			polling_byte: 0xFF,
		}
	}

	pub fn reset(&mut self) {
		*self = Self::new();
	}

	fn update_status(&mut self, cycle: u64) {
		if self.completion_cycle.is_some_and(|end| cycle >= end) {
			self.completion_cycle = None;
			self.state = FlashState::Idle;
		}
	}

	/* DQ7 reports the inverse of the programmed bit while busy; DQ6 toggles
	 * between reads. DQ5 remains clear during a successful operation. */
	pub fn read_status(&mut self, cycle: u64, real_byte: u8) -> u8 {
		self.update_status(cycle);
		if self.completion_cycle.is_none() {
			return real_byte;
		}
		self.toggle_bit = !self.toggle_bit;
		(!self.polling_byte & 0x80) | if self.toggle_bit { 0x40 } else { 0 }
	}

	fn start_operation(&mut self, cycle: u64, duration: u64, value: u8) {
		self.state = FlashState::Idle;
		self.completion_cycle = Some(cycle.saturating_add(duration));
		self.polling_byte = value;
		self.toggle_bit = false;
	}

	pub fn process_write(&mut self, addr: u16, value: u8, cycle: u64) -> Option<FlashWrite> {
		self.update_status(cycle);
		if self.completion_cycle.is_some() {
			return None;
		}
		/* The byte after A0 is data, including F0 and FF. It must not be
		 * interpreted as a reset or an erase command. */
		if self.state == FlashState::Program {
			self.start_operation(cycle, FLASH_PROGRAM_CYCLES, value);
			return Some(FlashWrite::Program(value));
		}
		if value == 0xF0 {
			self.state = FlashState::Idle;
			return None;
		}
		let offset = addr & 0x07FF;
		let first = offset == 0x0555 && value == 0xAA;
		let second = offset == 0x02AA && value == 0x55;
		self.state = match self.state {
			FlashState::Idle if first => FlashState::UnlockSecond,
			FlashState::UnlockSecond if second => FlashState::Command,
			FlashState::Command if offset == 0x0555 => match value {
				0xA0 => FlashState::Program,
				0x80 => FlashState::EraseFirst,
				0x90 => FlashState::AutoSelect,
				_ => FlashState::Idle,
			},
			FlashState::EraseFirst if first => FlashState::EraseSecond,
			FlashState::EraseSecond if second => FlashState::EraseCommand,
			FlashState::EraseCommand => {
				self.state = FlashState::Idle;
				let operation = if value == 0x30 {
					Some(FlashWrite::EraseSector)
				} else if offset == 0x0555 && value == 0x10 {
					Some(FlashWrite::EraseChip)
				} else {
					None
				};
				if operation.is_some() {
					self.start_operation(cycle, FLASH_ERASE_CYCLES, 0xFF);
				}
				return operation;
			},
			FlashState::AutoSelect => FlashState::AutoSelect,
			_ => FlashState::Idle,
		};
		None
	}
}