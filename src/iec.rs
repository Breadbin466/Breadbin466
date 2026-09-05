// =======================================================
// src/iec.rs — IEC Serial Bus subsystem façade
// =======================================================

/* The IEC façade exports the shared electrical bus model used by the motherboard and the independently clocked 1541. */

pub mod bus;
pub(crate) mod constants;

pub use bus::IecBus;