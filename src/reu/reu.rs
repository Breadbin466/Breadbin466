// =======================================================
// src/reu/reu.rs — MOS 8726 RAM Expansion Unit emulation
// =======================================================

use crate::memory::ram::RAMController;

use super::dma::DmaState;
use super::memory::{read_c64, write_c64};

/* The motherboard asks which C64 bus operation the active DMA phase requires before granting the cycle. Swap separates its C64 read and write into distinct phases. */
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ReuC64Access {
	None,
	Read(u16),
	Write(u16),
}

/* Reu models the MOS 8726 register file and DMA sequencer separately from the expansion RAM array. Register writes prepare a command, the motherboard grants individual transfer cycles, and completion updates status, autoload registers and IRQ state as one transaction. */
pub struct Reu {
	pub enabled: bool,
	pub irq_pending: bool,
	pub storage: Option<Box<[u8; 512 * 1024]>>,
	pub(crate) regs: [u8; 11],
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
			regs: [0; 11],
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
		self.regs = [0; 11];
		self.regs[0] = 0x10;
		self.regs[1] = 0x10;
		self.regs[6] = 0xF8;
		self.regs[7] = 0xFF;
		self.regs[8] = 0xFF;
		self.regs[9] = 0x1F;
		self.regs[10] = 0x3F;
		self.shadow_c64_addr = 0;
		self.shadow_reu_addr = 0;
		self.shadow_len = 0xFFFF;
	}

	/* RESET reaches the 8726 independently of whether C64 RAM is preserved. Expansion RAM is retained across reset, while the controller registers return to their hardware defaults. */
	pub fn reset(&mut self, _hard_reset: bool) {
		self.dma = None;
		self.waiting_ff00 = false;
		self.irq_pending = false;
		self.reset_registers();
	}

	/* A command armed for $FF00 begins only when the CPU performs the designated trigger access; merely programming COMMAND leaves the DMA sequencer idle. */
	pub fn trigger_ff00(&mut self) {
		if self.waiting_ff00 && self.regs[1] & 0x80 != 0 {
			self.waiting_ff00 = false;
			self.start_dma();
		}
	}

	/* Starting a command snapshots the visible address, length and address-control registers. A programmed length of zero represents 65,536 bytes, and fixed-address bits suppress the corresponding post-byte increment. */
	pub(crate) fn start_dma(&mut self) {
		self.waiting_ff00 = false;
		if self.storage.is_none() { self.regs[1] &= 0x7F; return; }
		let length = usize::from(self.regs[7]) | (usize::from(self.regs[8]) << 8);
		let command = self.regs[1];
		let address_control = self.regs[10];
		self.regs[0] &= 0x1F;
		self.irq_pending = false;
		self.dma = Some(DmaState {
			transfer_type: self.regs[1] & 0x03,
			c64_addr: u16::from(self.regs[2]) | (u16::from(self.regs[3]) << 8),
			reu_addr: usize::from(self.regs[4]) | (usize::from(self.regs[5]) << 8) | (usize::from(self.regs[6] & 0x07) << 16),
			remaining: if length == 0 { 65_536 } else { length },
			fix_c64: address_control & 0x80 != 0,
			fix_reu: address_control & 0x40 != 0,
			autoload: command & 0x20 != 0,
			phase: 0,
			latch: 0,
		});
	}

	pub fn dma_active(&self) -> bool { self.dma.is_some() }

	/* This prediction is the bus-arbitration contract with the motherboard. It exposes only the C64-side half of the current DMA phase; REU memory access remains internal to tick_dma. */
	pub fn c64_access(&self) -> ReuC64Access {
		let Some(dma) = self.dma else { return ReuC64Access::None; };
		match (dma.transfer_type, dma.phase) {
			(0, 0) | (2, 0) | (3, 0) => ReuC64Access::Read(dma.c64_addr),
			(1, 0) | (2, 1) => ReuC64Access::Write(dma.c64_addr),
			_ => ReuC64Access::None,
		}
	}

	/* One granted motherboard cycle advances at most one external C64 bus phase. Transfer, store and verify complete in one phase; swap first exchanges the C64 byte into REU RAM, then writes the displaced REU byte back on the following grant. */
	pub fn tick_dma(&mut self, ram: &mut RAMController) -> bool {
		let Some(mut dma) = self.dma else { return false; };
		let Some(storage) = self.storage.as_mut() else {
			self.dma = None;
			return false;
		};
		let offset = dma.reu_addr & 0x0007_FFFF;
		let mut byte_complete = false;
		let mut verify_error = false;

		match dma.transfer_type {
			/* Store transfers one byte from C64 memory into REU RAM. */
			0 => {
				let value = read_c64(ram, dma.c64_addr);
				storage[offset] = value;
				byte_complete = true;
			}
			/* Recall transfers one byte from REU RAM into C64 memory. */
			1 => {
				let value = storage[offset];
				write_c64(ram, dma.c64_addr, value);
				byte_complete = true;
			}
			/* Swap requires two granted C64 bus phases. The first stores the C64 byte in REU RAM while preserving the displaced REU byte; the second writes that displaced byte back to the C64. */
			2 => {
				if dma.phase == 0 {
					let value = read_c64(ram, dma.c64_addr);
					let reu_value = storage[offset];
					storage[offset] = value;
					dma.latch = reu_value;
					dma.phase = 1;
				} else {
					write_c64(ram, dma.c64_addr, dma.latch);
					byte_complete = true;
				}
			}
			/* Verify compares C64 and REU bytes without modifying either side. The first mismatch latches the verify-error status and terminates the command. */
			3 => {
				let value = read_c64(ram, dma.c64_addr);
				if value != storage[offset] {
					self.regs[0] |= 0x20;
					verify_error = true;
				}
				byte_complete = true;
			}
			_ => byte_complete = true,
		}

		if !byte_complete {
			self.dma = Some(dma);
			return true;
		}

		dma.phase = 0;
		if !dma.fix_c64 {
			dma.c64_addr = dma.c64_addr.wrapping_add(1);
		}
		if !dma.fix_reu {
			dma.reu_addr = dma.reu_addr.wrapping_add(1);
		}
		dma.remaining -= 1;
		if dma.remaining == 0 || verify_error {
			self.finish_dma(dma, !verify_error);
			false
		} else {
			self.dma = Some(dma);
			true
		}
	}

	/* Completion publishes status before evaluating the interrupt mask. Autoload restores the original programmed addresses and length; otherwise the registers expose the post-transfer positions and a residual length of one, matching the controller's terminal register convention. */
	fn finish_dma(&mut self, dma: DmaState, completed: bool) {
		self.dma = None;
		if completed {
			/* Status bit 6 records end-of-block completion. */
			self.regs[0] |= 0x40;
		}
		self.regs[1] &= 0x7F;
		if dma.autoload {
			self.regs[2] = self.shadow_c64_addr as u8;
			self.regs[3] = (self.shadow_c64_addr >> 8) as u8;
			self.regs[4] = self.shadow_reu_addr as u8;
			self.regs[5] = (self.shadow_reu_addr >> 8) as u8;
			self.regs[6] = (self.shadow_reu_addr >> 16) as u8;
			self.regs[7] = self.shadow_len as u8;
			self.regs[8] = (self.shadow_len >> 8) as u8;
		} else {
			self.regs[2] = dma.c64_addr as u8;
			self.regs[3] = (dma.c64_addr >> 8) as u8;
			self.regs[4] = dma.reu_addr as u8;
			self.regs[5] = (dma.reu_addr >> 8) as u8;
			self.regs[6] = (dma.reu_addr >> 16) as u8;
			self.regs[7] = 1;
			self.regs[8] = 0;
		}
		let status = self.regs[0];
		let mask = self.regs[9];
		/* Status bit 6 is end-of-block and bit 5 is verify error. Their matching mask bits select which completed event may request an interrupt. */
		let event = (status & 0x40 != 0 && mask & 0x40 != 0) || (status & 0x20 != 0 && mask & 0x20 != 0);
		/* Mask bit 7 globally enables REU interrupts; status bit 7 records that the controller has actually asserted its IRQ request. */
		if mask & 0x80 != 0 && event {
			self.regs[0] |= 0x80;
			self.irq_pending = true;
		}
	}
}

impl Default for Reu { fn default() -> Self { Self::new() } }