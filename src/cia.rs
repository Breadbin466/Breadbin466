// =======================================================
// src/cia.rs — MOS 6526A CIA subsystem façade
// =======================================================

/* This façade exposes the two machine-specific CIA wrappers while keeping timers, TOD and serial logic available as one coherent subsystem. */

pub mod constants;
pub mod timer;
pub mod tod;
pub mod serial;
pub mod cia;
pub mod cia1;
pub mod cia2;

pub use cia1::Cia1;
pub use cia2::Cia2;