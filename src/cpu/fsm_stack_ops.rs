// =======================================================
// src/cpu/fsm_stack_ops.rs — Stack Operations (Generic Bus)
// =======================================================

use super::Cpu;
use crate::cpu::bus::SystemBus;

/* Stack instructions retain their discarded opcode-stream and stack-page reads so peripheral-visible timing matches the NMOS bus sequence. */
impl Cpu {

	pub fn execute_pha<B: SystemBus>(&mut self, bus: &mut B) {
		match self.t_state {
			1 => {
				let _ = self.read_byte(bus, self.pc, true);
				self.t_state += 1;
			}
			2 => {
				self.push_byte(bus, self.a);
				self.t_state = 0;
			}
			_ => self.t_state = 0
		}
	}

	/* PHP sets both the break marker and the normally-high unused bit in the value written to the stack. */
	pub fn execute_php<B: SystemBus>(&mut self, bus: &mut B) {
		match self.t_state {
			1 => {
				let _ = self.read_byte(bus, self.pc, true);
				self.t_state += 1;
			}
			2 => {
				let val = self.p | super::B_FLAG | super::U_FLAG;
				self.push_byte(bus, val);
				self.t_state = 0;
			}
			_ => self.t_state = 0
		}
	}

	/* PLA performs the opcode-stream and stack-page dummy reads before incrementing SP, loading A and updating N and Z. */
	pub fn execute_pla<B: SystemBus>(&mut self, bus: &mut B) {
		match self.t_state {
			1 => {
				let _ = self.read_byte(bus, self.pc, true);
				self.t_state += 1;
			}
			2 => {
				let _ = self.read_byte(bus, 0x0100 | self.sp as u16, true);
				self.sp = self.sp.wrapping_add(1);
				self.t_state += 1;
			}
			3 => {
				self.a = self.read_byte(bus, 0x0100 | self.sp as u16, false);
				self.update_nz(self.a);
				self.t_state = 0;
			}
			_ => self.t_state = 0
		}
	}

	/* PLP restores the writable flags while keeping the internal unused bit high; B exists only in pushed status copies. */
	pub fn execute_plp<B: SystemBus>(&mut self, bus: &mut B) {
		match self.t_state {
			1 => {
				let _ = self.read_byte(bus, self.pc, true);
				self.t_state += 1;
			}
			2 => {
				let _ = self.read_byte(bus, 0x0100 | self.sp as u16, true);
				self.sp = self.sp.wrapping_add(1);
				self.t_state += 1;
			}
			3 => {
				let flags = self.read_byte(bus, 0x0100 | self.sp as u16, false);
				if (flags & super::I_FLAG) == 0 && (self.p & super::I_FLAG) != 0 {
					self.irq_enables = true;
				} else if (flags & super::I_FLAG) != 0 && (self.p & super::I_FLAG) == 0 {
					self.irq_disables = true;
				}
				self.p = (flags & !super::B_FLAG) | super::U_FLAG;
				self.t_state = 0;
			}
			_ => self.t_state = 0
		}
	}
}