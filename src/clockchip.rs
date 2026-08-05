// =======================================================
// src/clockchip.rs — MOS 8701 clock generator subsystem façade
// =======================================================

/* The clockchip façade groups PAL timing constants, shared signal levels and the per-cycle CIA clock distributor. */

pub mod constants;
pub mod signals;
pub mod tick_cia;

pub use constants::*;