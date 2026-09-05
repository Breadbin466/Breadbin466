// =======================================================
// src/cpu/sequencer.rs — MOS 6510 CPU Micro-Op Sequencer
// =======================================================

use crate::cpu::bus::SystemBus;
use crate::cpu::decoder::{AddressingMode, Operation};
use crate::cpu::{Cpu, CpuState};

/* The decoder chooses an addressing-cycle machine first; exceptional stack and control-flow opcodes replace only the implied or absolute sequence that would otherwise run. */
impl Cpu {
	#[inline(always)]
	pub fn execute_opcode_step<B: SystemBus>(&mut self, bus: &mut B) {
		let info = self.opcode_info;

		match info.mode {
			AddressingMode::Implied | AddressingMode::Accumulator => match info.op {
				Operation::JSR => self.execute_jsr(bus),
				Operation::RTS => self.execute_rts(bus),
				Operation::RTI => self.execute_rti(bus),
				Operation::BRK => self.execute_brk(bus),
				Operation::PHA => self.execute_pha(bus),
				Operation::PHP => self.execute_php(bus),
				Operation::PLA => self.execute_pla(bus),
				Operation::PLP => self.execute_plp(bus),
				Operation::KIL => self.state = CpuState::Jammed,
				_ => self.execute_implied(bus, info),
			},
			AddressingMode::Immediate => {
				self.execute_immediate(bus, info);
			}
			AddressingMode::ZeroPage => {
				self.execute_zeropage(bus, info);
			}
			AddressingMode::ZeroPageX | AddressingMode::ZeroPageY => {
				self.execute_zp_indexed(bus, info);
			}
			AddressingMode::Absolute => match info.op {
				Operation::JSR => self.execute_jsr(bus),
				Operation::JMP => self.execute_jmp_abs(bus),
				_ => self.execute_absolute(bus, info),
			},
			AddressingMode::AbsoluteX | AddressingMode::AbsoluteY => {
				self.execute_abs_indexed(bus, info);
			}
			AddressingMode::Indirect => {
				self.execute_indirect(bus, info);
			}
			AddressingMode::IndexedIndirect => {
				self.execute_indexed_indirect(bus, info);
			}
			AddressingMode::IndirectIndexed => {
				self.execute_indirect_indexed(bus, info);
			}
			AddressingMode::Relative => {
				self.execute_relative(bus, info);
			}
		}
	}
}