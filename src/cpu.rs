// =======================================================
// src/cpu.rs — MOS 6510 CPU subsystem façade
// =======================================================

/* The façade exposes the cycle engine and the architectural constants while keeping each timing domain in a focused implementation file. */
pub mod constants;
pub mod bus;
pub mod decoder;
pub mod sequencer;
pub mod alu;
pub mod port;
pub mod fsm_addr_simple;
pub mod fsm_addr_complex;
pub mod fsm_special_ops;
pub mod fsm_stack_ops;
pub mod op_rmw;
pub mod processor;

pub use processor::{Cpu, CpuModel, CpuState};
pub use constants::{
	C_FLAG, Z_FLAG, I_FLAG, D_FLAG, B_FLAG, U_FLAG, V_FLAG, N_FLAG,
	INTERRUPT_DELAY, NMI_VECTOR, RESET_VECTOR, IRQ_VECTOR,
};