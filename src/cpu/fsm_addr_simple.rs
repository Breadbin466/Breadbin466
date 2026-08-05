// =======================================================
// src/cpu/fsm_addr_simple.rs — Simple Addressing Modes (Generic Bus)
// =======================================================

use super::Cpu;
use crate::cpu::bus::SystemBus;
use crate::cpu::decoder::{OpcodeInfo, Operation};

/* Each method below is a bus-cycle state machine. Reads marked as dummy remain externally visible and can trigger memory-mapped side effects even though their values are discarded (MOS-6500-HARDWARE-1976, single-cycle execution tables). */
impl Cpu {

	/* Implied and accumulator instructions perform a second opcode-stream read while the internal register operation completes. CLI and SEI record the old I state because interrupt recognition does not simply follow the newly written flag. */
	pub fn execute_implied<B: SystemBus>(&mut self, bus: &mut B, info: &OpcodeInfo) {
		match self.t_state {
			1 => {
				let _ = self.read_byte(bus, self.pc, true);

				match info.op {
					Operation::NOP => {},
					Operation::CLC => self.p &= !super::C_FLAG,
					Operation::SEC => self.p |= super::C_FLAG,
					Operation::CLI => {
						if (self.p & super::I_FLAG) != 0 {
							self.irq_enables = true;
						}
						self.p &= !super::I_FLAG;
					},
					Operation::SEI => {
						if (self.p & super::I_FLAG) == 0 {
							self.irq_disables = true;
						}
						self.p |= super::I_FLAG;
					},
					Operation::CLV => self.p &= !super::V_FLAG,
					Operation::CLD => self.p &= !super::D_FLAG,
					Operation::SED => self.p |= super::D_FLAG,
					Operation::TAX => { self.x = self.a; self.update_nz(self.x); },
					Operation::TAY => { self.y = self.a; self.update_nz(self.y); },
					Operation::TXA => { self.a = self.x; self.update_nz(self.a); },
					Operation::TYA => { self.a = self.y; self.update_nz(self.a); },
					Operation::TSX => { self.x = self.sp; self.update_nz(self.x); },
					Operation::TXS => { self.sp = self.x; },
					Operation::DEX => { self.x = self.x.wrapping_sub(1); self.update_nz(self.x); },
					Operation::DEY => { self.y = self.y.wrapping_sub(1); self.update_nz(self.y); },
					Operation::INX => { self.x = self.x.wrapping_add(1); self.update_nz(self.x); },
					Operation::INY => { self.y = self.y.wrapping_add(1); self.update_nz(self.y); },
					Operation::ASL => self.a = self.alu_asl(self.a),
					Operation::LSR => self.a = self.alu_lsr(self.a),
					Operation::ROL => self.a = self.alu_rol(self.a),
					Operation::ROR => self.a = self.alu_ror(self.a),
					_ => {}
				}
				self.t_state = 0;
			}
			_ => self.t_state = 0
		}
	}

	/* Immediate addressing consumes the operand directly from the opcode stream and completes in the same cycle because no effective-address bus phase follows. */
	pub fn execute_immediate<B: SystemBus>(&mut self, bus: &mut B, info: &OpcodeInfo) {
		if self.t_state == 1 {
			self.op_val = self.read_byte(bus, self.pc, false);
			self.pc = self.pc.wrapping_add(1);
			self.exec_op_read(info);
			self.t_state = 0;
		}
	}

	/* NMOS read-modify-write instructions read the operand, write the unmodified value, then write the modified value on consecutive cycles. */
	pub fn execute_zeropage<B: SystemBus>(&mut self, bus: &mut B, info: &OpcodeInfo) {
		match self.t_state {
			1 => {
				self.addr_abs = self.read_byte(bus, self.pc, false) as u16;
				self.pc = self.pc.wrapping_add(1);
				self.t_state += 1;
			}
			2 => {
				if super::op_rmw::is_rmw_opcode(self.ir) {
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
			3 => {
				self.write_byte(bus, self.addr_abs, self.op_val);
				self.t_state += 1;
			}
			4 => {
				super::op_rmw::rmw_phase_modify_write(self, bus, self.ir);
				self.t_state = 0;
			}
			_ => self.t_state = 0
		}
	}

	/* Absolute addressing fetches the low byte before the high byte and then reuses the same read/write tail as zero page. */
	pub fn execute_absolute<B: SystemBus>(&mut self, bus: &mut B, info: &OpcodeInfo) {
		match self.t_state {
			1 => {
				self.addr_lo = self.read_byte(bus, self.pc, false);
				self.pc = self.pc.wrapping_add(1);
				self.t_state += 1;
			}
			2 => {
				self.addr_hi = self.read_byte(bus, self.pc, false);
				self.pc = self.pc.wrapping_add(1);
				self.addr_abs = u16::from_le_bytes([self.addr_lo, self.addr_hi]);
				self.t_state += 1;
			}
			3 => {
				if super::op_rmw::is_rmw_opcode(self.ir) {
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
				super::op_rmw::rmw_phase_modify_write(self, bus, self.ir);
				self.t_state = 0;
			}
			_ => self.t_state = 0
		}
	}

	/* A taken branch always performs a discarded read; crossing a page adds a second read using the old page and new low byte before PC is corrected (MOS-6500-HARDWARE-1976, branch timing). */
	pub fn execute_relative<B: SystemBus>(&mut self, bus: &mut B, info: &OpcodeInfo) {
		match self.t_state {
			1 => {
				let offset = self.read_byte(bus, self.pc, false) as i8;
				self.pc = self.pc.wrapping_add(1);

				let cond = match info.op {
					Operation::BPL => (self.p & super::N_FLAG) == 0,
					Operation::BMI => (self.p & super::N_FLAG) != 0,
					Operation::BVC => (self.p & super::V_FLAG) == 0,
					Operation::BVS => (self.p & super::V_FLAG) != 0,
					Operation::BCC => (self.p & super::C_FLAG) == 0,
					Operation::BCS => (self.p & super::C_FLAG) != 0,
					Operation::BNE => (self.p & super::Z_FLAG) == 0,
					Operation::BEQ => (self.p & super::Z_FLAG) != 0,
					_ => false,
				};

				if cond {
					let target = self.pc.wrapping_add(offset as i16 as u16);
					self.pointer = target;
					self.page_crossed = (self.pc & 0xFF00) != (target & 0xFF00);
					if !self.page_crossed {
						self.skip_intr_latch = true;
						self.branch_delays_int = true;
					}
					self.t_state = 2;
				} else {
					self.t_state = 0;
				}
			}
			2 => {
				let _ = self.read_byte(bus, self.pc, true);
				if self.page_crossed {
					self.t_state = 3;
				} else {
					self.pc = self.pointer;
					self.t_state = 0;
				}
			}
			3 => {
				let dummy_addr = (self.pc & 0xFF00) | (self.pointer & 0x00FF);
				let _ = self.read_byte(bus, dummy_addr, true);
				self.pc = self.pointer;
				self.t_state = 0;
			}
			_ => self.t_state = 0
		}
	}
}