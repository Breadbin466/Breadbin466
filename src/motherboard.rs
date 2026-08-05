// =======================================================
// src/motherboard.rs — Motherboard subsystem façade
// =======================================================

/* The motherboard façade exposes the cycle orchestrator and keeps scheduling and KERNAL-assisted injection as supporting responsibilities. */

pub(crate) mod constants;
pub mod bus;
pub mod scheduler;
pub mod injection;

pub use bus::Motherboard;