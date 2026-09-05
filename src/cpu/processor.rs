// =======================================================
// src/cpu/processor.rs — MOS 6502/6510/8502 CPU core execution state
// =======================================================

use super::bus::SystemBus;
use super::constants::*;
use super::decoder::{AddressingMode, OpcodeInfo, Operation};
use super::port::CpuPort;

/* The outer state distinguishes normal opcode execution from the fixed reset and interrupt bus sequences. A jammed NMOS opcode stops instruction progress but leaves the clocked component alive. */
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuState {
	/* Normal opcode fetch and execution, including all addressing-mode cycle machines. */
	Running,
	/* The fixed reset bus sequence is in progress and will fetch the reset vector before returning to opcode execution. */
	ResetSequence,
	/* A maskable interrupt has been recognised and the IRQ stack/vector sequence is in progress. */
	IrqSequence,
	/* A latched NMI edge has won priority and the NMI stack/vector sequence is in progress. */
	NmiSequence,
	/* A jam opcode has stopped instruction progress while the external clock and bus-facing component remain alive. */
	Jammed,
}

/* The shared core models the common NMOS execution engine while the integrated port and a few unstable opcodes vary by silicon family. */
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuModel {
	/* Stand-alone NMOS 6502 without an integrated memory-control port. */
	Mos6502,
	/* C64-family NMOS core with the six-bit port mapped at $0000 and $0001. */
	Mos6510,
	/* C128-family derivative retaining the integrated port but differing in selected unstable-opcode details. */
	Mos8502,
}

/*
The CPU advances one externally visible bus cycle per successful tick. t_state identifies the cycle within the current opcode or fixed interrupt sequence; the addressing machines retain intermediate bytes so every dummy read, stack access and page correction occurs on its documented cycle.

Interrupt recognition is deliberately separate from line sampling. IRQ is level-sensitive, NMI is latched on its falling edge, and both are sampled near instruction execution before the next opcode boundary decides whether to enter an interrupt sequence. CLI, SEI and taken branches carry explicit delay state because changing P or PC near that boundary affects which line value the NMOS processor recognises.

A host tick first advances the external clock, then decides whether RDY may stretch the pending bus cycle. A completed cycle dispatches to the active opcode, reset or interrupt machine, after which the interrupt inputs are latched for the following instruction boundary. This ordering keeps bus direction, state-machine progress and interrupt recognition tied to the same externally visible cycle.
*/
pub struct Cpu {
	pub model: CpuModel,
	pub a: u8,
	pub x: u8,
	pub y: u8,
	pub sp: u8,
	pub pc: u16,
	pub p: u8,

	/* The instruction register and decoded descriptor remain stable throughout all cycles of the current opcode. */
	pub ir: u8,
	pub(super) opcode_info: &'static OpcodeInfo,
	/* Zero denotes opcode fetch; positive values select the following addressing or special-operation cycles. */
	pub t_state: u8,
	pub state: CpuState,

	/* Addressing state is shared by the small cycle machines so page crossings and dummy addresses remain observable on the bus. */
	pub addr_abs: u16,
	pub addr_lo: u8,
	pub addr_hi: u8,
	pub op_val: u8,
	pub pointer: u16,
	pub page_crossed: bool,

	/* IRQ is sampled as a level, whereas NMI records a falling edge until the sequence consumes it. */
	pub irq_line: bool,
	pub nmi_line: bool,
	pub nmi_pending: bool,
	/* These latches retain the interrupt inputs sampled during the active instruction cycle until the next opcode boundary. */
	pub intr_irq_latch: bool,
	pub intr_nmi_latch: bool,
	/* Instruction-specific delay flags preserve the NMOS recognition timing around CLI, SEI and related boundary cases. */
	pub irq_delay: bool,
	pub irq_disables: bool,
	pub irq_enables: bool,

	pub so_line: bool,

	pub port: CpuPort,

	/* ANE and LXA depend on analogue internal bus behaviour; configurable magic values isolate that model-specific uncertainty from the sequencer. */
	pub magic_ane: u8,
	pub magic_lxa: u8,

	pub cycles: u64,

	/* RDY can extend read cycles, but write cycles must complete before the processor can stop (MOS-6500-HARDWARE-1976, RDY operation). */
	pub is_stalled: bool,

	pub interrupt_vector: u16,
	/* Branch and special-cycle flags prevent a host-side resampling step from erasing the processor's instruction-boundary interrupt timing. */
	pub skip_intr_latch: bool,
	pub master_cycles: u64,
	pub nmi_clk: u64,
	pub brk_shadow: bool,
	pub branch_delays_int: bool,
}

impl Cpu {
	/* Construction establishes the power-on register defaults and enters the reset sequence so the first visible activity is the documented vector-fetch path rather than an already-initialised PC. */
	pub fn new(model: CpuModel) -> Self {
		Cpu {
			model,
			a: 0,
			x: 0,
			y: 0,
			sp: 0xFD,
			pc: 0,
			p: 0x24,
			ir: 0,
			opcode_info: &OPCODES[0],
			t_state: 0,
			state: CpuState::ResetSequence,
			addr_abs: 0,
			addr_lo: 0,
			addr_hi: 0,
			op_val: 0,
			pointer: 0,
			page_crossed: false,
			irq_line: false,
			nmi_line: true,
			nmi_pending: false,
			intr_irq_latch: false,
			intr_nmi_latch: false,
			irq_delay: false,
			irq_disables: false,
			irq_enables: false,
			so_line: false,
			port: CpuPort::new(),
			magic_ane: 0xEE,
			magic_lxa: 0xEE,
			cycles: 0,
			is_stalled: false,
			interrupt_vector: 0,
			skip_intr_latch: false,
			master_cycles: 0,
			nmi_clk: 0,
			brk_shadow: false,
			branch_delays_int: false,
		}
	}

	#[inline(always)]
	/* IRQ is a level input. The line is sampled during instruction execution and the sampled level is considered at the following opcode boundary. */
	pub fn set_irq_line(&mut self, active: bool) {
		self.irq_line = active;
	}

	#[inline(always)]
	/* NMI is edge-sensitive: only a high-to-low transition creates a pending request (MOS-6500-HARDWARE-1976, interrupt inputs). */
	pub fn set_nmi_line(&mut self, voltage_high: bool) {
		let falling_edge = self.nmi_line && !voltage_high;
		self.nmi_line = voltage_high;
		if falling_edge {
			self.nmi_pending = true;
			self.nmi_clk = self.master_cycles.wrapping_add(1);
		}
	}

	/* A low-to-high SO transition sets overflow asynchronously from instruction execution (MOS-6500-HARDWARE-1976, SO input). */
	pub fn set_so_line(&mut self, active: bool) {
		if active && !self.so_line {
			self.p |= V_FLAG;
		}
		self.so_line = active;
	}

	/* A full reset restores the model's power-on execution state and restarts the externally visible reset bus sequence rather than loading the vector immediately. */
	pub fn reset<B: SystemBus>(&mut self, _bus: &mut B) {
		self.a = 0;
		self.x = 0;
		self.y = 0;
		self.sp = 0xFD;
		self.p = 0x24;
		self.ir = 0;
		self.opcode_info = &OPCODES[0];
		self.t_state = 0;
		self.state = CpuState::ResetSequence;
		self.addr_abs = 0;
		self.addr_lo = 0;
		self.addr_hi = 0;
		self.op_val = 0;
		self.pointer = 0;
		self.page_crossed = false;
		self.irq_line = false;
		self.nmi_line = true;
		self.nmi_pending = false;
		self.intr_irq_latch = false;
		self.intr_nmi_latch = false;
		self.irq_delay = false;
		self.irq_disables = false;
		self.irq_enables = false;
		self.so_line = false;
		self.is_stalled = false;
		self.interrupt_vector = 0;
		self.master_cycles = 0;
		self.nmi_clk = 0;
		self.brk_shadow = false;
		self.branch_delays_int = false;
		self.port.reset_for_model(self.model);
	}

	/* Asserting RESET abandons the current instruction and prepares the fixed reset sequence while preserving the general-purpose registers and cycle counters that are not reset by this entry path. */
	pub fn assert_reset(&mut self) {
		self.ir = 0;
		self.opcode_info = &OPCODES[0];
		self.t_state = 0;
		self.state = CpuState::ResetSequence;
		self.addr_abs = 0;
		self.addr_lo = 0;
		self.addr_hi = 0;
		self.op_val = 0;
		self.pointer = 0;
		self.page_crossed = false;
		self.nmi_pending = false;
		self.nmi_line = true;
		self.intr_irq_latch = false;
		self.intr_nmi_latch = false;
		self.irq_delay = false;
		self.irq_disables = false;
		self.irq_enables = false;
		self.so_line = false;
		self.is_stalled = false;
		self.interrupt_vector = 0;
		self.skip_intr_latch = false;
		self.nmi_clk = 0;
		self.brk_shadow = false;
		self.branch_delays_int = false;
		self.port.reset_for_model(self.model);
	}

	#[inline(always)]
	/* RDY may stretch only read cycles, so this function predicts the bus direction from the current addressing mode and t_state before the cycle executes. Store instructions become writes at their final effective-address phase, while read-modify-write instructions report both the dummy write of the original operand and the final write of the modified value. Stack pushes and interrupt entry are handled as writes independently of the addressing mode. */
	fn running_cycle_is_write(&self) -> bool {
		let info = self.opcode_info;
		let is_write = self.is_write_op(info.op);
		match info.mode {
			AddressingMode::Implied | AddressingMode::Accumulator => match info.op {
				Operation::JSR => matches!(self.t_state, 3 | 4),
				Operation::BRK => matches!(self.t_state, 2 | 3 | 4),
				Operation::PHA | Operation::PHP => self.t_state == 2,
				_ => false,
			},
			AddressingMode::Immediate => false,
			AddressingMode::ZeroPage => match self.t_state {
				2 => is_write,
				3 | 4 => true,
				_ => false,
			},
			AddressingMode::ZeroPageX | AddressingMode::ZeroPageY => match self.t_state {
				3 => is_write,
				4 | 5 => true,
				_ => false,
			},
			AddressingMode::Absolute => match info.op {
				Operation::JSR => matches!(self.t_state, 3 | 4),
				Operation::JMP => false,
				_ => match self.t_state {
					3 => is_write,
					4 | 5 => true,
					_ => false,
				},
			},
			AddressingMode::AbsoluteX | AddressingMode::AbsoluteY => match self.t_state {
				4 => is_write,
				5 | 6 => true,
				_ => false,
			},
			AddressingMode::Indirect => false,
			AddressingMode::IndexedIndirect => match self.t_state {
				5 => is_write,
				6 | 7 => true,
				_ => false,
			},
			AddressingMode::IndirectIndexed => match self.t_state {
				5 => is_write,
				6 | 7 => true,
				_ => false,
			},
			AddressingMode::Relative => false,
		}
	}

	#[inline(always)]
	/* RDY is evaluated before the cycle runs, so this dispatcher predicts whether the active fixed sequence or opcode machine is about to perform a write. */
	fn next_cycle_is_write(&self) -> bool {
		match self.state {
			CpuState::IrqSequence | CpuState::NmiSequence => matches!(self.t_state, 1 | 2 | 3),
			CpuState::Running if self.t_state != 0 => self.running_cycle_is_write(),
			_ => false,
		}
	}

	#[inline(always)]
	/* A low RDY repeats a read cycle without advancing CPU time. Interrupt inputs are still sampled so a stretched cycle does not make an external transition disappear. */
	pub fn tick<B: SystemBus>(&mut self, bus: &mut B, rdy: bool) -> u8 {
		self.master_cycles = self.master_cycles.wrapping_add(1);
		if !rdy && !self.next_cycle_is_write() {
			self.is_stalled = true;
			if self.state == CpuState::Running && self.t_state != 0 && !self.skip_intr_latch {
				self.intr_irq_latch = self.irq_line;
				self.intr_nmi_latch = self.nmi_pending;
			}
			return 0;
		}
		self.is_stalled = false;
		self.cycles = self.cycles.wrapping_add(1);
		match self.state {
			CpuState::ResetSequence => self.sequence_reset(bus),
			CpuState::IrqSequence => self.sequence_interrupt(bus, IRQ_VECTOR, false),
			CpuState::NmiSequence => self.sequence_interrupt(bus, NMI_VECTOR, true),
			CpuState::Jammed => {}
			CpuState::Running => self.sequence_running(bus),
		}
		if self.state == CpuState::Running && self.t_state != 0 {
			if !self.skip_intr_latch {
				self.intr_irq_latch = self.irq_line;
				self.intr_nmi_latch = self.nmi_pending;
			}
		}
		self.skip_intr_latch = false;
		1
	}

	#[inline(always)]
	/* Opcode fetch is also the instruction boundary at which previously sampled IRQ and pending NMI requests are accepted or deferred. */
	fn sequence_running<B: SystemBus>(&mut self, bus: &mut B) {
		match self.t_state {
			0 => {
				let op = self.read_byte(bus, self.pc, true);
				self.ir = op;
				self.opcode_info = &OPCODES[op as usize];
				let irq_delay = self.irq_delay;
				let irq_disables = self.irq_disables;
				let irq_enables = self.irq_enables;
				self.irq_delay = false;
				self.irq_disables = false;
				self.irq_enables = false;

				let shadowed = self.brk_shadow;
				self.brk_shadow = false;
				let extra_delay: u64 = if self.branch_delays_int { 1 } else { 0 };
				self.branch_delays_int = false;
				let nmi_eligible = !shadowed
					&& self.nmi_pending
					&& self.master_cycles
						>= self.nmi_clk.wrapping_add(INTERRUPT_DELAY + extra_delay);
				if nmi_eligible {
					self.state = CpuState::NmiSequence;
					self.t_state = 0;
					self.nmi_pending = false;
					self.intr_nmi_latch = false;
				/* Normal recognition requires I to have been clear at the sampling boundary. A just-executed CLI sets irq_enables so the newly cleared I flag does not take effect too early; a just-executed SEI sets irq_disables so an IRQ already recognised before I was set is still honoured. irq_delay covers the remaining one-instruction boundary cases. */
				} else if self.intr_irq_latch
					&& ((self.p & I_FLAG) == 0 && !irq_enables || irq_disables)
					&& !irq_delay
				{
					self.state = CpuState::IrqSequence;
					self.t_state = 0;
				} else {
					self.pc = self.pc.wrapping_add(1);
					self.t_state = 1;
				}
			}
			_ => {
				self.execute_opcode_step(bus);
			}
		}
	}

	#[inline(always)]
	/* Reset performs discarded reads and three stack-page reads before fetching the reset vector; the stack pointer decrements without writing memory (MOS-6500-HARDWARE-1976, reset sequence). */
	fn sequence_reset<B: SystemBus>(&mut self, bus: &mut B) {
		match self.t_state {
			0 => {
				let _ = self.read_byte(bus, self.pc, true);
				self.t_state += 1;
			}
			1 => {
				let _ = self.read_byte(bus, 0x0100 | self.sp as u16, true);
				self.sp = self.sp.wrapping_sub(1);
				self.t_state += 1;
			}
			2 => {
				let _ = self.read_byte(bus, 0x0100 | self.sp as u16, true);
				self.sp = self.sp.wrapping_sub(1);
				self.t_state += 1;
			}
			3 => {
				let _ = self.read_byte(bus, 0x0100 | self.sp as u16, true);
				self.sp = self.sp.wrapping_sub(1);
				self.t_state += 1;
			}
			4 => {
				let _ = self.read_byte(bus, self.pc, true);
				self.t_state += 1;
			}
			5 => {
				self.addr_abs = self.read_byte(bus, RESET_VECTOR, true) as u16;
				self.p |= I_FLAG;
				self.t_state += 1;
			}
			6 => {
				let hi = self.read_byte(bus, RESET_VECTOR.wrapping_add(1), true) as u16;
				self.pc = (hi << 8) | self.addr_abs;
				self.state = CpuState::Running;
				self.t_state = 0;
			}
			_ => self.t_state = 0,
		}
	}

	#[inline(always)]
	/* Hardware interrupts push PC and status with the break bit clear, set I, then fetch the selected vector (MOS-6500-HARDWARE-1976, interrupt sequence). */
	fn sequence_interrupt<B: SystemBus>(&mut self, bus: &mut B, vector: u16, is_nmi: bool) {
		match self.t_state {
			0 => {
				let _ = self.read_byte(bus, self.pc, true);
				self.t_state += 1;
			}
			1 => {
				self.write_byte(bus, 0x0100 | self.sp as u16, (self.pc >> 8) as u8);
				self.sp = self.sp.wrapping_sub(1);
				self.t_state += 1;
			}
			2 => {
				self.write_byte(bus, 0x0100 | self.sp as u16, (self.pc & 0xFF) as u8);
				self.sp = self.sp.wrapping_sub(1);
				self.t_state += 1;
			}
			3 => {
				let status = (self.p | U_FLAG) & !B_FLAG;
				self.write_byte(bus, 0x0100 | self.sp as u16, status);
				self.sp = self.sp.wrapping_sub(1);
				self.p |= I_FLAG;
				self.t_state += 1;
			}
			4 => {
				self.addr_abs = self.read_byte(bus, vector, true) as u16;
				self.t_state += 1;
			}
			5 => {
				let hi = self.read_byte(bus, vector.wrapping_add(1), true) as u16;
				self.pc = (hi << 8) | self.addr_abs;
				if is_nmi {
					self.nmi_pending = false;
				}
				self.state = CpuState::Running;
				self.t_state = 0;
				self.irq_delay = false;
				self.irq_disables = false;
				self.irq_enables = false;
			}
			_ => {
				self.t_state = 0;
			}
		}
	}

	#[inline(always)]
	/* The final read cycle captures the operand, applies the decoded operation and returns the sequencer to opcode fetch. */
	pub fn exec_read<B: SystemBus>(&mut self, bus: &mut B, info: &OpcodeInfo) {
		let val = self.read_byte(bus, self.addr_abs, false);
		self.op_val = val;
		self.exec_op_read(info);
		self.t_state = 0;
	}

	/* Read operations share one decoded dispatch so addressing machines can deliver an operand without duplicating register and flag semantics. */
	pub fn exec_op_read(&mut self, info: &OpcodeInfo) {
		let val = self.op_val;
		match info.op {
			Operation::LDA => {
				self.a = val;
				self.update_nz(self.a);
			}
			Operation::LDX => {
				self.x = val;
				self.update_nz(self.x);
			}
			Operation::LDY => {
				self.y = val;
				self.update_nz(self.y);
			}
			Operation::EOR => {
				self.a ^= val;
				self.update_nz(self.a);
			}
			Operation::AND => {
				self.a &= val;
				self.update_nz(self.a);
			}
			Operation::ORA => {
				self.a |= val;
				self.update_nz(self.a);
			}
			Operation::ADC => self.alu_adc(val),
			Operation::SBC => self.alu_sbc(val),
			Operation::CMP => self.alu_cmp(self.a, val),
			Operation::CPX => self.alu_cmp(self.x, val),
			Operation::CPY => self.alu_cmp(self.y, val),
			Operation::BIT => self.alu_bit(val),
			Operation::LAX => {
				self.a = val;
				self.x = val;
				self.update_nz(self.a);
			}
			Operation::LXA => {
				let magic = if self.model == CpuModel::Mos8502 {
					0x00
				} else {
					self.magic_lxa
				};
				let old_z = self.p & Z_FLAG;
				self.a = (self.a | magic) & val;
				self.x = self.a;
				self.update_nz(self.a);

				if self.model == CpuModel::Mos8502 {
					self.p = (self.p & !Z_FLAG) | old_z;
				}
			}
			Operation::ANE => {
				let magic = if self.model == CpuModel::Mos8502 {
					0x00
				} else {
					self.magic_ane
				};
				let old_z = self.p & Z_FLAG;
				self.a = (self.a | magic) & self.x & val;
				self.update_nz(self.a);
				if self.model == CpuModel::Mos8502 {
					self.p = (self.p & !Z_FLAG) | old_z;
				}
			}
			Operation::ANC => {
				self.a &= val;
				self.update_nz(self.a);
				if (self.a & 0x80) != 0 {
					self.p |= C_FLAG;
				} else {
					self.p &= !C_FLAG;
				}
			}
			Operation::ALR => {
				self.a &= val;
				if (self.a & 0x01) != 0 {
					self.p |= C_FLAG;
				} else {
					self.p &= !C_FLAG;
				}
				self.a >>= 1;
				self.update_nz(self.a);
			}
			Operation::ARR => self.alu_arr(val),
			Operation::AXS => self.alu_axs(val),
			Operation::LAS => {
				let res = self.sp & val;
				self.a = res;
				self.x = res;
				self.sp = res;
				self.update_nz(res);
			}
			Operation::NOP => {}
			_ => {}
		}
	}

	#[inline(always)]
	/* The unstable high-byte-masked store opcodes derive both value and, on a crossing, effective bus behaviour from the pre-correction address. */
	pub fn exec_write<B: SystemBus>(&mut self, bus: &mut B, info: &OpcodeInfo) {
		let mut val = match info.op {
			Operation::STA => self.a,
			Operation::STX => self.x,
			Operation::STY => self.y,
			Operation::SAX => self.a & self.x,
			Operation::SHY => {
				if self.model == CpuModel::Mos8502 {
					0x00
				} else {
					self.y & self.addr_hi.wrapping_add(1)
				}
			}
			Operation::SHX => {
				if self.model == CpuModel::Mos8502 {
					0x00
				} else {
					self.x & self.addr_hi.wrapping_add(1)
				}
			}
			Operation::AHX => {
				if self.model == CpuModel::Mos8502 {
					0x00
				} else {
					self.a & self.x & self.addr_hi.wrapping_add(1)
				}
			}
			Operation::TAS => {
				self.sp = self.a & self.x;
				if self.model == CpuModel::Mos8502 {
					0x00
				} else {
					self.sp & self.addr_hi.wrapping_add(1)
				}
			}
			_ => 0,
		};

		let final_addr = if matches!(
			info.op,
			Operation::SHY | Operation::SHX | Operation::AHX | Operation::TAS
		) && self.page_crossed
		{
			val &= (self.addr_abs >> 8) as u8;
			self.addr_abs
		} else {
			self.addr_abs
		};

		self.write_byte(bus, final_addr, val);
		self.t_state = 0;
	}

	#[inline(always)]
	/* The 6510 and 8502 intercept addresses $0000 and $0001 inside the processor package; a plain 6502 forwards them to the system bus. */
	pub fn read_byte<B: SystemBus>(&mut self, bus: &mut B, addr: u16, _dummy: bool) -> u8 {
		if self.model != CpuModel::Mos6502 && addr <= 0x0001 {
			return self.port.cpu_read(addr, self.cycles, self.model);
		}
		bus.read(addr, self.cycles)
	}

	#[inline(always)]
	/* Writes to the integrated 6510/8502 port update and commit its latch during the same CPU access; all other addresses are forwarded unchanged to the system bus. */
	pub fn write_byte<B: SystemBus>(&mut self, bus: &mut B, addr: u16, val: u8) {
		if self.model != CpuModel::Mos6502 && addr <= 0x0001 {
			self.port.cpu_write(addr, val, self.cycles, self.model);
			self.port.commit_write(self.cycles, self.model);
			return;
		}
		bus.write(addr, val, self.cycles);
	}

	/* Stack pushes write at $0100 plus the current stack pointer, then decrement the pointer as the NMOS bus sequence expects. */
	pub fn push_byte<B: SystemBus>(&mut self, bus: &mut B, val: u8) {
		self.write_byte(bus, 0x0100 | self.sp as u16, val);
		self.sp = self.sp.wrapping_sub(1);
	}

	/* This classification covers pure store operations only. Read-modify-write and stack cycles are identified separately from their current t_state. */
	pub fn is_write_op(&self, op: Operation) -> bool {
		matches!(
			op,
			Operation::STA
				| Operation::STX
				| Operation::STY
				| Operation::SAX
				| Operation::AHX
				| Operation::SHX
				| Operation::SHY
				| Operation::TAS
		)
	}
}