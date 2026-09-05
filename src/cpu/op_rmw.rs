// =======================================================
// src/cpu/op_rmw.rs — Read-Modify-Write Operations (Generic Bus)
// =======================================================

use crate::cpu::Cpu;
use crate::cpu::bus::SystemBus;
use crate::cpu::constants::RMW_BITMAP;

#[inline(always)]
/* The bitmap includes official shifts and increments together with undocumented composite instructions that share the NMOS three-access memory sequence. */
pub fn is_rmw_opcode(opcode: u8) -> bool {
	(RMW_BITMAP[(opcode >> 5) as usize] >> (opcode & 31)) & 1 != 0
}

/* This is the final write of the read-modify-write sequence; the preceding cycle has already written the original operand back to the same address. */
pub fn rmw_phase_modify_write<B: SystemBus>(cpu: &mut Cpu, bus: &mut B, opcode: u8) {
	let val = cpu.op_val;

	let result = match opcode {
		0x06 | 0x0E | 0x16 | 0x1E => cpu.alu_asl(val),
		0x46 | 0x4E | 0x56 | 0x5E => cpu.alu_lsr(val),
		0x26 | 0x2E | 0x36 | 0x3E => cpu.alu_rol(val),
		0x66 | 0x6E | 0x76 | 0x7E => cpu.alu_ror(val),
		0xC6 | 0xD6 | 0xCE | 0xDE => {
			let r = val.wrapping_sub(1);
			cpu.update_nz(r);
			r
		}
		0xE6 | 0xF6 | 0xEE | 0xFE => {
			let r = val.wrapping_add(1);
			cpu.update_nz(r);
			r
		}
		/* SLO performs ASL on memory, then ORA with the shifted value. */
		0x07 | 0x17 | 0x0F | 0x1F | 0x03 | 0x13 | 0x1B => {
			let r = cpu.alu_asl(val);
			cpu.a |= r;
			cpu.update_nz(cpu.a);
			r
		}
		/* RLA performs ROL on memory, then AND with the rotated value. */
		0x27 | 0x37 | 0x2F | 0x3F | 0x23 | 0x33 | 0x3B => {
			let r = cpu.alu_rol(val);
			cpu.a &= r;
			cpu.update_nz(cpu.a);
			r
		}
		/* SRE performs LSR on memory, then EOR with the shifted value. */
		0x47 | 0x57 | 0x4F | 0x5F | 0x43 | 0x53 | 0x5B => {
			let r = cpu.alu_lsr(val);
			cpu.a ^= r;
			cpu.update_nz(cpu.a);
			r
		}
		/* RRA performs ROR on memory, then ADC with the rotated value. */
		0x67 | 0x77 | 0x6F | 0x7F | 0x63 | 0x73 | 0x7B => {
			let r = cpu.alu_ror(val);
			cpu.alu_adc(r);
			r
		}
		/* DCP decrements memory, then compares A with the decremented value. */
		0xC7 | 0xD7 | 0xCF | 0xDF | 0xC3 | 0xD3 | 0xDB => {
			let r = val.wrapping_sub(1);
			cpu.alu_cmp(cpu.a, r);
			r
		}
		/* ISC increments memory, then performs SBC with the incremented value. */
		0xE7 | 0xF7 | 0xEF | 0xFF | 0xE3 | 0xF3 | 0xFB => {
			let r = val.wrapping_add(1);
			cpu.alu_sbc(r);
			r
		}
		_ => val,
	};

	cpu.write_byte(bus, cpu.addr_abs, result);
}