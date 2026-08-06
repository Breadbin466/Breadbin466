/*
 * Breadbin466 interactive debugger: 6510/8502 disassembly formatting.
 *
 * The debugger reuses the CPU decoder table as the sole opcode authority.  This
 * prevents the disassembler and execution core from silently disagreeing about
 * instruction names or addressing modes.  Formatting is observational only;
 * bytes are supplied by the caller so this module never reads emulated memory
 * and therefore cannot trigger device side effects.
 */

use crate::cpu::constants::OPCODES;
use crate::cpu::decoder::AddressingMode;

/* Instruction length follows the addressing mode selected by the execution
 * decoder, including undocumented opcodes represented in that table. */
pub fn instruction_length(opcode: u8) -> u16 {
	match OPCODES[opcode as usize].mode {
		AddressingMode::Implied | AddressingMode::Accumulator => 1,
		AddressingMode::Immediate | AddressingMode::ZeroPage | AddressingMode::ZeroPageX
		| AddressingMode::ZeroPageY | AddressingMode::IndexedIndirect
		| AddressingMode::IndirectIndexed | AddressingMode::Relative => 2,
		_ => 3,
	}
}

/* Relative branches are resolved against the address following the two-byte
 * instruction, with the signed displacement wrapped in the 16-bit CPU address
 * space.  Other operands are rendered in conventional Commodore hexadecimal
 * notation without attempting symbol resolution. */
pub fn format_instruction(pc: u16, bytes: &[u8]) -> String {
	let opcode = bytes[0];
	let info = OPCODES[opcode as usize];
	let operand = match info.mode {
		AddressingMode::Implied => String::new(),
		AddressingMode::Accumulator => " A".into(),
		AddressingMode::Immediate => format!(" #${:02X}", bytes[1]),
		AddressingMode::ZeroPage => format!(" ${:02X}", bytes[1]),
		AddressingMode::ZeroPageX => format!(" ${:02X},X", bytes[1]),
		AddressingMode::ZeroPageY => format!(" ${:02X},Y", bytes[1]),
		AddressingMode::Absolute => format!(" ${:04X}", u16::from_le_bytes([bytes[1], bytes[2]])),
		AddressingMode::AbsoluteX => format!(" ${:04X},X", u16::from_le_bytes([bytes[1], bytes[2]])),
		AddressingMode::AbsoluteY => format!(" ${:04X},Y", u16::from_le_bytes([bytes[1], bytes[2]])),
		AddressingMode::Indirect => format!(" (${:04X})", u16::from_le_bytes([bytes[1], bytes[2]])),
		AddressingMode::IndexedIndirect => format!(" (${:02X},X)", bytes[1]),
		AddressingMode::IndirectIndexed => format!(" (${:02X}),Y", bytes[1]),
		AddressingMode::Relative => {
			let target = pc.wrapping_add(2).wrapping_add((bytes[1] as i8) as u16);
			format!(" ${target:04X}")
		}
	};
	format!("{:?}{operand}", info.op)
}