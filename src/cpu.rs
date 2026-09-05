// =======================================================
// src/cpu.rs — MOS 6510 CPU subsystem façade
// =======================================================

/* The façade exposes the cycle engine and the architectural constants while keeping each timing domain in a focused implementation file. */
pub mod alu;
pub mod bus;
pub mod constants;
pub mod decoder;
pub mod fsm_addr_complex;
pub mod fsm_addr_simple;
pub mod fsm_special_ops;
pub mod fsm_stack_ops;
pub mod op_rmw;
pub mod port;
pub mod processor;
pub mod sequencer;

pub use constants::{
	B_FLAG, C_FLAG, D_FLAG, I_FLAG, INTERRUPT_DELAY, IRQ_VECTOR, N_FLAG, NMI_VECTOR, RESET_VECTOR,
	U_FLAG, V_FLAG, Z_FLAG,
};
pub use processor::{Cpu, CpuModel, CpuState};