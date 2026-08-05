// =======================================================
// src/cpu/decoder.rs — MOS 6510 Opcode Decoding Table
// =======================================================

/* Addressing modes select a bus-cycle state machine; they are not merely formulas for calculating an effective address. */
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressingMode {
	Implied,
	Accumulator,
	Immediate,
	ZeroPage,
	ZeroPageX,
	ZeroPageY,
	Absolute,
	AbsoluteX,
	AbsoluteY,
	Indirect,
	IndexedIndirect,
	IndirectIndexed,
	Relative,
}

/* Official and stable NMOS undocumented operations share the same sequencer so combined read-modify-write opcodes retain their real bus activity. */
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
	ADC, SBC, AND, ORA, EOR,
	ASL, LSR, ROL, ROR,
	INC, DEC, INX, INY, DEX, DEY,
	CMP, CPX, CPY, BIT,
	LDA, LDX, LDY, STA, STX, STY,
	TAX, TAY, TXA, TYA, TSX, TXS,
	PHA, PHP, PLA, PLP,
	BCC, BCS, BEQ, BMI, BNE, BPL, BVC, BVS,
	JMP, JSR, RTS, RTI, BRK,
	CLC, CLD, CLI, CLV, SEC, SED, SEI,
	NOP,
	AHX, ALR, ANC, ANE, ARR, AXS, DCP, ISC, LAS, LAX, LXA,
	RLA, RRA, SAX, SHX, SHY, SLO, SRE, TAS,
	KIL,
}

#[derive(Debug, Clone, Copy)]
/* Decoding separates the operation from its addressing sequence; the opcode table then selects one of each for all 256 byte values. */
pub struct OpcodeInfo {
	pub op: Operation,
	pub mode: AddressingMode,
}