// =======================================================
// src/datassette.rs — Datassette subsystem façade
// =======================================================

/* The façade keeps transport mechanics, TAP encoding constants and the mechanical counter separate while presenting one Datassette type to the rest of the emulator. */
pub mod constants;
pub mod deck;
pub mod odometre;

pub use deck::{Datassette, TapeState};