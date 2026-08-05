// =======================================================
// src/cpu/fsm_special_ops.rs — Special Operations (Generic Bus)
// =======================================================

use super::Cpu;
use crate::cpu::bus::SystemBus;

/* Control-flow instructions have dedicated cycle machines because their stack accesses and discarded reads do not fit ordinary addressing modes. */
impl Cpu {

	/* JSR fetches the target low byte, performs a stack-page read, pushes the return address high then low, and only then fetches the target high byte. */
	pub fn execute_jsr<B: SystemBus>(&mut self, bus: &mut B) {
		match self.t_state {
			1 => {
				self.addr_lo = self.read_byte(bus, self.pc, false);
				self.pc = self.pc.wrapping_add(1);
				self.t_state += 1;
			}
			2 => {
				let _ = self.read_byte(bus, 0x0100 | self.sp as u16, true);
				self.t_state += 1;
			}
			3 => {
				self.push_byte(bus, (self.pc >> 8) as u8);
				self.t_state += 1;
			}
			4 => {
				self.push_byte(bus, (self.pc & 0xFF) as u8);
				self.t_state += 1;
			}
			5 => {
				self.addr_hi = self.read_byte(bus, self.pc, false);
				self.pc = u16::from_le_bytes([self.addr_lo, self.addr_hi]);
				self.t_state = 0;
			}
			_ => self.t_state = 0
		}
	}

	/* RTS pulls the saved address low then high, performs a read from that address, and increments PC before the next opcode fetch. */
	pub fn execute_rts<B: SystemBus>(&mut self, bus: &mut B) {
		match self.t_state {
			1 => {
				let _ = self.read_byte(bus, self.pc, true);
				self.t_state += 1;
			}
			2 => {
				let _ = self.read_byte(bus, 0x0100 | self.sp as u16, true);
				self.t_state += 1;
			}
			3 => {
				self.sp = self.sp.wrapping_add(1);
				self.addr_lo = self.read_byte(bus, 0x0100 | self.sp as u16, false);
				self.t_state += 1;
			}
			4 => {
				self.sp = self.sp.wrapping_add(1);
				self.addr_hi = self.read_byte(bus, 0x0100 | self.sp as u16, false);
				self.pc = u16::from_le_bytes([self.addr_lo, self.addr_hi]);
				self.t_state += 1;
			}
			5 => {
				let _ = self.read_byte(bus, self.pc, true);
				self.pc = self.pc.wrapping_add(1);
				self.t_state = 0;
			}
			_ => self.t_state = 0
		}
	}

	/* Absolute JMP fetches the destination low byte then high byte and transfers control without any stack or dummy-write phase. */
	pub fn execute_jmp_abs<B: SystemBus>(&mut self, bus: &mut B) {
		match self.t_state {
			1 => {
				self.addr_lo = self.read_byte(bus, self.pc, false);
				self.pc = self.pc.wrapping_add(1);
				self.t_state += 1;
			}
			2 => {
				self.addr_hi = self.read_byte(bus, self.pc, false);
				self.pc = self.pc.wrapping_add(1);
				let target = u16::from_le_bytes([self.addr_lo, self.addr_hi]);
				self.pc = target;
				self.t_state = 0;
			}
			_ => self.t_state = 0
		}
	}

	/* RTI restores status before PC; the stored break marker is not a persistent processor-status bit. */
	pub fn execute_rti<B: SystemBus>(&mut self, bus: &mut B) {
		match self.t_state {
			1 => {
				let _ = self.read_byte(bus, self.pc, true);
				self.t_state += 1;
			}
			2 => {
				let _ = self.read_byte(bus, 0x0100 | self.sp as u16, true);
				self.t_state += 1;
			}
			3 => {
				self.sp = self.sp.wrapping_add(1);
				let flags = self.read_byte(bus, 0x0100 | self.sp as u16, false);
				self.p = (flags & !super::B_FLAG) | super::U_FLAG;
				self.t_state += 1;
			}
			4 => {
				self.sp = self.sp.wrapping_add(1);
				self.addr_lo = self.read_byte(bus, 0x0100 | self.sp as u16, false);
				self.t_state += 1;
			}
			5 => {
				self.sp = self.sp.wrapping_add(1);
				self.addr_hi = self.read_byte(bus, 0x0100 | self.sp as u16, false);
				self.pc = u16::from_le_bytes([self.addr_lo, self.addr_hi]);
				self.t_state = 0;
			}
			_ => self.t_state = 0
		}
	}

	/* BRK consumes its padding byte, pushes the following PC and a status copy with B set, then enters through the IRQ/BRK vector. */
	pub fn execute_brk<B: SystemBus>(&mut self, bus: &mut B) {
		match self.t_state {
			1 => {
				let _ = self.read_byte(bus, self.pc, true);
				self.pc = self.pc.wrapping_add(1);
				self.t_state += 1;
			}
			2 => {
				self.push_byte(bus, (self.pc >> 8) as u8);
				self.t_state += 1;
			}
			3 => {
				self.push_byte(bus, (self.pc & 0xFF) as u8);
				self.t_state += 1;
			}
			4 => {
				self.push_byte(bus, self.p | super::B_FLAG | super::U_FLAG);
				self.p |= super::I_FLAG;
				self.t_state += 1;
			}
			5 => {
				let nmi_ready = self.nmi_pending
					&& self.master_cycles >= self.nmi_clk.wrapping_add(super::INTERRUPT_DELAY);
				let vector = if nmi_ready {
					self.nmi_pending = false;
					self.intr_nmi_latch = false;
					super::NMI_VECTOR
				} else {
					super::IRQ_VECTOR
				};
				self.interrupt_vector = vector;
				self.addr_lo = self.read_byte(bus, vector, false);
				self.t_state += 1;
			}
			6 => {
				self.addr_hi = self.read_byte(bus, self.interrupt_vector.wrapping_add(1), false);
				self.pc = u16::from_le_bytes([self.addr_lo, self.addr_hi]);
				self.brk_shadow = true;
				self.t_state = 0;
			}
			_ => self.t_state = 0
		}
	}
}