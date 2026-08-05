// =======================================================
// src/cpu/fsm_addr_complex.rs — Complex Addressing Modes (Generic Bus)
// =======================================================

use super::Cpu;
use crate::cpu::bus::SystemBus;
use crate::cpu::decoder::{OpcodeInfo, AddressingMode};

/* Indexed addressing preserves the NMOS processor's provisional addresses and correction cycles instead of collapsing them into a single effective-address calculation. */
impl Cpu {
	/* Zero-page indexing performs a discarded read from the unindexed address, then wraps the addition within page zero. */
	pub fn execute_zp_indexed<B: SystemBus>(&mut self, bus: &mut B, info: &OpcodeInfo) {
		let index = if info.mode == AddressingMode::ZeroPageX { self.x } else { self.y };
		match self.t_state {
			1 => {
				self.addr_lo = self.read_byte(bus, self.pc, false);
				self.pc = self.pc.wrapping_add(1);
				self.t_state += 1;
			}
			2 => {
				let _ = self.read_byte(bus, self.addr_lo as u16, true);
				self.addr_abs = self.addr_lo.wrapping_add(index) as u16;
				self.t_state += 1;
			}
			3 => {
				if crate::cpu::op_rmw::is_rmw_opcode(self.ir) {
					self.op_val = self.read_byte(bus, self.addr_abs, false);
					self.t_state += 1;
				} else if self.is_write_op(info.op) {
					self.exec_write(bus, info);
					self.t_state = 0;
				} else {
					self.exec_read(bus, info);
					self.t_state = 0;
				}
			}
			4 => {
				self.write_byte(bus, self.addr_abs, self.op_val);
				self.t_state += 1;
			}
			5 => {
				crate::cpu::op_rmw::rmw_phase_modify_write(self, bus, self.ir);
				self.t_state = 0;
			}
			_ => self.t_state = 0
		}
	}

	/* Reads can finish without a correction cycle when the index stays on the same page. Stores and read-modify-write operations always expose the provisional address cycle. */
	pub fn execute_abs_indexed<B: SystemBus>(&mut self, bus: &mut B, info: &OpcodeInfo) {
		let index = if info.mode == AddressingMode::AbsoluteX { self.x } else { self.y };
		match self.t_state {
			1 => {
				self.addr_lo = self.read_byte(bus, self.pc, false);
				self.pc = self.pc.wrapping_add(1);
				self.t_state += 1;
			}
			2 => {
				self.addr_hi = self.read_byte(bus, self.pc, false);
				self.pc = self.pc.wrapping_add(1);
				let base = u16::from_le_bytes([self.addr_lo, self.addr_hi]);
				let effective = base.wrapping_add(index as u16);
				self.addr_abs = effective;
				self.page_crossed = (base & 0xFF00) != (effective & 0xFF00);
				self.t_state += 1;
			}
			3 => {
				let is_rmw = crate::cpu::op_rmw::is_rmw_opcode(self.ir);
				let is_write = self.is_write_op(info.op);

				if is_rmw || is_write || self.page_crossed {
					let bad_addr = ((self.addr_hi as u16) << 8) | (self.addr_abs & 0x00FF);
					let _ = self.read_byte(bus, bad_addr, true);
					self.t_state += 1;
				} else {
					self.exec_read(bus, info);
					self.t_state = 0;
				}
			}
			4 => {
				if crate::cpu::op_rmw::is_rmw_opcode(self.ir) {
					self.op_val = self.read_byte(bus, self.addr_abs, false);
					self.t_state += 1;
				} else if self.is_write_op(info.op) {
					self.exec_write(bus, info);
					self.t_state = 0;
				} else {
					self.exec_read(bus, info);
					self.t_state = 0;
				}
			}
			5 => {
				self.write_byte(bus, self.addr_abs, self.op_val);
				self.t_state += 1;
			}
			6 => {
				crate::cpu::op_rmw::rmw_phase_modify_write(self, bus, self.ir);
				self.t_state = 0;
			}
			_ => self.t_state = 0
		}
	}

	/* JMP indirect reproduces the NMOS page-boundary wrap: a pointer ending in $FF reads its high byte from the start of the same page. */
	pub fn execute_indirect<B: SystemBus>(&mut self, bus: &mut B, _info: &OpcodeInfo) {
		match self.t_state {
			1 => {
				self.addr_lo = self.read_byte(bus, self.pc, false);
				self.pc = self.pc.wrapping_add(1);
				self.t_state += 1;
			}
			2 => {
				self.addr_hi = self.read_byte(bus, self.pc, false);
				self.pc = self.pc.wrapping_add(1);
				self.pointer = u16::from_le_bytes([self.addr_lo, self.addr_hi]);
				self.t_state += 1;
			}
			3 => {
				self.addr_lo = self.read_byte(bus, self.pointer, false);
				self.t_state += 1;
			}
			4 => {
				let ptr_hi = if (self.pointer & 0x00FF) == 0x00FF {
					self.pointer & 0xFF00
				} else {
					self.pointer.wrapping_add(1)
				};
				self.addr_hi = self.read_byte(bus, ptr_hi, false);
				self.pc = u16::from_le_bytes([self.addr_lo, self.addr_hi]);
				self.t_state = 0;
			}
			_ => self.t_state = 0
		}
	}

	/* Indexed-indirect adds X to the zero-page pointer before fetching both address bytes, with both pointer accesses wrapping in page zero. */
	pub fn execute_indexed_indirect<B: SystemBus>(&mut self, bus: &mut B, info: &OpcodeInfo) {
		match self.t_state {
			1 => {
				self.addr_lo = self.read_byte(bus, self.pc, false);
				self.pc = self.pc.wrapping_add(1);
				self.t_state += 1;
			}
			2 => {
				let _ = self.read_byte(bus, self.addr_lo as u16, true);
				self.pointer = self.addr_lo.wrapping_add(self.x) as u16;
				self.t_state += 1;
			}
			3 => {
				self.addr_lo = self.read_byte(bus, self.pointer as u16, false);
				self.t_state += 1;
			}
			4 => {
				self.addr_hi = self.read_byte(bus, (self.pointer as u8).wrapping_add(1) as u16, false);
				self.addr_abs = u16::from_le_bytes([self.addr_lo, self.addr_hi]);
				self.t_state += 1;
			}
			5 => {
				/* This is the first access to the corrected effective address: a final read, a store, or the operand read that begins an RMW sequence. */
				if crate::cpu::op_rmw::is_rmw_opcode(self.ir) {
					self.op_val = self.read_byte(bus, self.addr_abs, false);
					self.t_state += 1;
				} else if self.is_write_op(info.op) {
					self.exec_write(bus, info);
					self.t_state = 0;
				} else {
					self.exec_read(bus, info);
					self.t_state = 0;
				}
			}
			6 => {
				self.write_byte(bus, self.addr_abs, self.op_val);
				self.t_state += 1;
			}
			7 => {
				crate::cpu::op_rmw::rmw_phase_modify_write(self, bus, self.ir);
				self.t_state = 0;
			}
			_ => self.t_state = 0
		}
	}

	/* Indirect-indexed fetches the base pointer first, then adds Y. Page crossing, stores and read-modify-write operations force the provisional-address cycle. */
	pub fn execute_indirect_indexed<B: SystemBus>(&mut self, bus: &mut B, info: &OpcodeInfo) {
		match self.t_state {
			1 => {
				self.addr_lo = self.read_byte(bus, self.pc, false);
				self.pc = self.pc.wrapping_add(1);
				self.t_state += 1;
			}
			2 => {
				let ptr_lo = self.read_byte(bus, self.addr_lo as u16, false);
				self.pointer = ptr_lo as u16;
				self.t_state += 1;
			}
			3 => {
				/* Fetch the pointer high byte, form the base address and decide whether the provisional-address cycle is externally visible. */
				let ptr_addr_hi = self.addr_lo.wrapping_add(1) as u16;
				let ptr_hi = self.read_byte(bus, ptr_addr_hi, false);
				self.addr_hi = ptr_hi;
				let base = self.pointer | ((ptr_hi as u16) << 8);
				let effective = base.wrapping_add(self.y as u16);
				self.addr_abs = effective;
				self.page_crossed = (base & 0xFF00) != (effective & 0xFF00);

				let is_rmw = crate::cpu::op_rmw::is_rmw_opcode(self.ir);
				let is_write = self.is_write_op(info.op);

				if is_rmw || is_write || self.page_crossed {
					self.t_state = 4;
				} else {
					self.t_state = 5;
				}
			}
			4 => {
				/* Page crossings, stores and read-modify-write instructions expose a read from the uncorrected high byte before using the final address. */
				let dummy_addr = ((self.addr_hi as u16) << 8) | (self.addr_abs & 0x00FF);
				let _ = self.read_byte(bus, dummy_addr, true);
				self.t_state += 1;
			}
			5 => {
				if crate::cpu::op_rmw::is_rmw_opcode(self.ir) {
					self.op_val = self.read_byte(bus, self.addr_abs, false);
					self.t_state += 1;
				} else if self.is_write_op(info.op) {
					self.exec_write(bus, info);
					self.t_state = 0;
				} else {
					self.exec_read(bus, info);
					self.t_state = 0;
				}
			}
			6 => {
				self.write_byte(bus, self.addr_abs, self.op_val);
				self.t_state += 1;
			}
			7 => {
				crate::cpu::op_rmw::rmw_phase_modify_write(self, bus, self.ir);
				self.t_state = 0;
			}
			_ => self.t_state = 0
		}
	}
}