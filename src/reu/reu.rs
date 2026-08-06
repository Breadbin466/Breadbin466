/*
 * MOS 8726 RAM Expansion Controller.
 *
 * Reu owns the CPU-visible register latches, the active DMA command and the
 * fixed 512 KiB memory fitted to the Commodore 1764.  The motherboard grants
 * one C64 bus phase at a time; the controller performs only the internal REU
 * memory operation associated with that granted phase.
 */

use crate::memory::memory::Memory;
use crate::vic::VicII;

use super::constants::{
	ADDRESS_CONTROL, ADDRESS_CONTROL_FIX_C64, ADDRESS_CONTROL_FIX_REU,
	ADDRESS_CONTROL_UNUSED_READ_HIGH, COMMAND, COMMAND_AUTOLOAD,
	COMMAND_COMPLETION_MASK, COMMAND_EXECUTE, COMMAND_FF00_DISABLE,
	COMMAND_TRANSFER_TYPE_MASK, C64_ADDRESS_HIGH, C64_ADDRESS_LOW,
	FULL_64K_TRANSFER_LENGTH, INTERRUPT_END_OF_BLOCK_ENABLE,
	INTERRUPT_GLOBAL_ENABLE, INTERRUPT_MASK, INTERRUPT_UNUSED_READ_HIGH,
	INTERRUPT_VERIFY_ENABLE, REGISTER_COUNT, REU_ADDRESS_BANK, REU_ADDRESS_HIGH,
	REU_ADDRESS_LOW, STATUS, STATUS_END_OF_BLOCK, STATUS_IRQ_PENDING,
	STATUS_VERIFY_ERROR, STATUS_VERSION, TRANSFER_LENGTH_HIGH, TRANSFER_LENGTH_LOW, UNUSED_REGISTER_VALUE,
};
use super::dma::{DmaState, SwapPhase, TransferType};
use super::memory::ReuMemory;
use super::timing::{action_for_cycle, ReuBusAction};

/*
 * The motherboard queries the C64-side operation before granting a DMA cycle.
 * Swap exposes a read phase followed by a write phase, while REU memory access
 * remains internal to tick_dma.
 */
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ReuC64Access {
	None,
	Read(u16),
	Write(u16),
}

pub struct Reu {
	pub enabled: bool,
	pub irq_pending: bool,
	storage: Option<ReuMemory>,
	pub(crate) regs: [u8; REGISTER_COUNT],
	pub(crate) shadow_c64_addr: u16,
	pub(crate) shadow_reu_addr: usize,
	pub(crate) shadow_len: usize,
	pub(crate) dma: Option<DmaState>,
	pub(crate) waiting_ff00: bool,
}

impl Reu {
	pub fn new() -> Self {
		let mut reu = Self {
			enabled: false,
			irq_pending: false,
			storage: None,
			regs: [0; REGISTER_COUNT],
			shadow_c64_addr: 0,
			shadow_reu_addr: 0,
			shadow_len: 0,
			dma: None,
			waiting_ff00: false,
		};
		reu.reset_registers();
		reu
	}

	fn reset_registers(&mut self) {
		self.regs = [0; REGISTER_COUNT];
		self.regs[STATUS] = STATUS_VERSION;
		self.regs[COMMAND] = COMMAND_FF00_DISABLE;
		/*
		 * Only the low three bank bits are connected by the 1764.  The five
		 * unconnected inputs are pulled high at RESET, so software observes $F8
		 * until it explicitly programs the complete bank latch.
		 */
		self.regs[REU_ADDRESS_BANK] = 0xF8;
		self.regs[TRANSFER_LENGTH_LOW] = 0xFF;
		self.regs[TRANSFER_LENGTH_HIGH] = 0xFF;
		self.regs[INTERRUPT_MASK] = INTERRUPT_UNUSED_READ_HIGH;
		self.regs[ADDRESS_CONTROL] = ADDRESS_CONTROL_UNUSED_READ_HIGH;
		self.shadow_c64_addr = 0;
		self.shadow_reu_addr = 0;
		self.shadow_len = 0xFFFF;
	}

	/*
	 * RESET reaches the 8726 independently of C64 RAM.  It aborts command state
	 * and restores the controller latches while preserving expansion DRAM.
	 */
	pub fn reset(&mut self, _hard_reset: bool) {
		self.dma = None;
		self.waiting_ff00 = false;
		self.irq_pending = false;
		self.reset_registers();
	}

	/*
	 * Enabling attaches the fixed memory image on first use.  Later transitions
	 * retain its contents; disabling only releases DMA ownership and IRQ state.
	 */
	pub fn set_enabled(&mut self, enabled: bool) {
		if enabled && self.storage.is_none() {
			self.storage = Some(ReuMemory::new());
		}
		self.enabled = enabled;
		if !enabled {
			self.dma = None;
			self.waiting_ff00 = false;
			self.irq_pending = false;
		}
	}

	pub fn storage_present(&self) -> bool {
		self.storage.is_some()
	}

	pub fn capacity_bytes(&self) -> usize {
		self.storage.as_ref().map_or(0, ReuMemory::capacity)
	}

	/*
	 * A command armed for $FF00 starts only on the designated CPU access.  The
	 * trigger is ignored after EXECUTE has been cleared or the command cancelled.
	 */
	pub fn trigger_ff00(&mut self) {
		if self.waiting_ff00 && self.regs[COMMAND] & COMMAND_EXECUTE != 0 {
			self.waiting_ff00 = false;
			self.start_dma();
		}
	}

	/*
	 * Command start snapshots the visible counters and address-control bits.  In
	 * Autoload mode the last values explicitly programmed by the CPU are used,
	 * rather than live counters left behind by an earlier transfer.
	 */
	pub(crate) fn start_dma(&mut self) {
		self.waiting_ff00 = false;
		if self.storage.is_none() {
			self.regs[COMMAND] &= !COMMAND_EXECUTE;
			return;
		}

		let command = self.regs[COMMAND];
		let address_control = self.regs[ADDRESS_CONTROL];
		let autoload = command & COMMAND_AUTOLOAD != 0;
		let visible_c64_addr = u16::from(self.regs[C64_ADDRESS_LOW])
			| (u16::from(self.regs[C64_ADDRESS_HIGH]) << 8);
		let visible_reu_addr = usize::from(self.regs[REU_ADDRESS_LOW])
			| (usize::from(self.regs[REU_ADDRESS_HIGH]) << 8)
			| (usize::from(self.regs[REU_ADDRESS_BANK]) << 16);
		let visible_length = usize::from(self.regs[TRANSFER_LENGTH_LOW])
			| (usize::from(self.regs[TRANSFER_LENGTH_HIGH]) << 8);

		let c64_addr = if autoload { self.shadow_c64_addr } else { visible_c64_addr };
		let reu_addr = if autoload { self.shadow_reu_addr } else { visible_reu_addr };
		let length = if autoload { self.shadow_len } else { visible_length };

		self.regs[STATUS] &= STATUS_VERSION;
		self.irq_pending = false;
		self.dma = Some(DmaState {
			transfer_type: TransferType::from_command(command & COMMAND_TRANSFER_TYPE_MASK),
			c64_addr,
			reu_addr,
			remaining: if length == 0 { FULL_64K_TRANSFER_LENGTH } else { length },
			fix_c64: address_control & ADDRESS_CONTROL_FIX_C64 != 0,
			fix_reu: address_control & ADDRESS_CONTROL_FIX_REU != 0,
			autoload,
			swap_phase: SwapPhase::ReadC64,
			latch: 0,
		});
	}

	pub fn dma_active(&self) -> bool {
		self.dma.is_some()
	}

	/*
	 * The motherboard asks once per master cycle whether the processor may run,
	 * whether an active command is merely paused by BA, or whether one transfer
	 * phase may advance.  Keeping this decision inside the REU prevents CPU hold
	 * semantics from drifting away from the controller's byte timing.
	 */
	pub(crate) fn bus_action(&self, ba_high: bool) -> ReuBusAction {
		action_for_cycle(self.dma_active(), ba_high)
	}

	pub fn debug_register(&self, index: usize) -> u8 {
		self.regs.get(index).copied().unwrap_or(UNUSED_REGISTER_VALUE)
	}

	/*
	 * This prediction is the bus-arbitration contract with the motherboard.  A
	 * granted cycle advances exactly the operation reported here.
	 */
	pub fn c64_access(&self) -> ReuC64Access {
		let Some(dma) = self.dma else {
			return ReuC64Access::None;
		};
		match (dma.transfer_type, dma.swap_phase) {
			(TransferType::Store | TransferType::Verify, _) => ReuC64Access::Read(dma.c64_addr),
			(TransferType::Recall, _) => ReuC64Access::Write(dma.c64_addr),
			(TransferType::Swap, SwapPhase::ReadC64) => ReuC64Access::Read(dma.c64_addr),
			(TransferType::Swap, SwapPhase::WriteC64) => ReuC64Access::Write(dma.c64_addr),
		}
	}

	/*
	 * One motherboard grant advances one external C64 bus phase.  Store, Recall
	 * and Verify complete a byte in one grant; Swap requires two grants and keeps
	 * the displaced REU byte in its internal latch between them.
	 */
	pub fn tick_dma(&mut self, memory: &mut Memory, cycle: u64, vic: &mut VicII) -> bool {
		let Some(mut dma) = self.dma else {
			return false;
		};
		let Some(storage) = self.storage.as_mut() else {
			self.dma = None;
			return false;
		};

		let mut byte_complete = false;
		let mut verify_error = false;

		match dma.transfer_type {
			TransferType::Store => {
				let value = memory.cpu_read(dma.c64_addr, cycle, vic);
				storage.write(dma.reu_addr, value);
				byte_complete = true;
			}
			TransferType::Recall => {
				let value = storage.read(dma.reu_addr);
				memory.cpu_write(dma.c64_addr, value, cycle, vic);
				byte_complete = true;
			}
			TransferType::Swap => match dma.swap_phase {
				SwapPhase::ReadC64 => {
					let c64_value = memory.cpu_read(dma.c64_addr, cycle, vic);
					let reu_value = storage.read(dma.reu_addr);
					storage.write(dma.reu_addr, c64_value);
					dma.latch = reu_value;
					dma.swap_phase = SwapPhase::WriteC64;
				}
				SwapPhase::WriteC64 => {
					memory.cpu_write(dma.c64_addr, dma.latch, cycle, vic);
					byte_complete = true;
				}
			},
			TransferType::Verify => {
				let c64_value = memory.cpu_read(dma.c64_addr, cycle, vic);
				if c64_value != storage.read(dma.reu_addr) {
					self.regs[STATUS] |= STATUS_VERIFY_ERROR;
					verify_error = true;
				}
				byte_complete = true;
			}
		}

		if !byte_complete {
			self.dma = Some(dma);
			return true;
		}

		/*
		 * The terminal byte is recognised while remaining equals one.  The DMA
		 * state owns the counter transition so wrap and fixed-address semantics
		 * cannot diverge between transfer paths.
		 */
		let transfer_complete = dma.complete_byte();

		self.publish_live_counters(&dma);

		if transfer_complete || verify_error {
			self.finish_dma(dma, !verify_error);
			false
		} else {
			self.dma = Some(dma);
			true
		}
	}

	/* Publish the internal counters through the CPU-visible address and length registers. */
	fn publish_live_counters(&mut self, dma: &DmaState) {
		self.regs[C64_ADDRESS_LOW] = dma.c64_addr as u8;
		self.regs[C64_ADDRESS_HIGH] = (dma.c64_addr >> 8) as u8;
		self.regs[REU_ADDRESS_LOW] = dma.reu_addr as u8;
		self.regs[REU_ADDRESS_HIGH] = (dma.reu_addr >> 8) as u8;
		self.regs[REU_ADDRESS_BANK] = (dma.reu_addr >> 16) as u8;
		let visible_remaining = if dma.remaining == FULL_64K_TRANSFER_LENGTH {
			0
		} else {
			dma.remaining as u16
		};
		self.regs[TRANSFER_LENGTH_LOW] = visible_remaining as u8;
		self.regs[TRANSFER_LENGTH_HIGH] = (visible_remaining >> 8) as u8;
	}

	/*
	 * Completion publishes status before evaluating the interrupt mask.  Autoload
	 * restores the programmed latches; otherwise the live terminal counters stay
	 * visible.  EXECUTE is cleared and immediate-command mode is restored.
	 */
	fn finish_dma(&mut self, dma: DmaState, completed: bool) {
		self.dma = None;
		if completed {
			self.regs[STATUS] |= STATUS_END_OF_BLOCK;
		}
		self.regs[COMMAND] = (self.regs[COMMAND] & COMMAND_COMPLETION_MASK) | COMMAND_FF00_DISABLE;

		if dma.autoload {
			self.regs[C64_ADDRESS_LOW] = self.shadow_c64_addr as u8;
			self.regs[C64_ADDRESS_HIGH] = (self.shadow_c64_addr >> 8) as u8;
			self.regs[REU_ADDRESS_LOW] = self.shadow_reu_addr as u8;
			self.regs[REU_ADDRESS_HIGH] = (self.shadow_reu_addr >> 8) as u8;
			self.regs[REU_ADDRESS_BANK] = (self.shadow_reu_addr >> 16) as u8;
			self.regs[TRANSFER_LENGTH_LOW] = self.shadow_len as u8;
			self.regs[TRANSFER_LENGTH_HIGH] = (self.shadow_len >> 8) as u8;
		}

		let status = self.regs[STATUS];
		let mask = self.regs[INTERRUPT_MASK];
		let event_enabled = (status & STATUS_END_OF_BLOCK != 0
			&& mask & INTERRUPT_END_OF_BLOCK_ENABLE != 0)
			|| (status & STATUS_VERIFY_ERROR != 0 && mask & INTERRUPT_VERIFY_ENABLE != 0);
		if mask & INTERRUPT_GLOBAL_ENABLE != 0 && event_enabled {
			self.regs[STATUS] |= STATUS_IRQ_PENDING;
			self.irq_pending = true;
		}
	}
}

impl Default for Reu {
	fn default() -> Self {
		Self::new()
	}
}