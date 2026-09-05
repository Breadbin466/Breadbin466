// =======================================================
// src/motherboard.rs — Motherboard subsystem façade
// =======================================================

/* The motherboard façade exposes the cycle orchestrator and keeps scheduling and KERNAL-assisted injection as supporting responsibilities. */

pub mod bus;
pub(crate) mod constants;
pub mod injection;
pub mod scheduler;

pub use bus::{DebugBusAccess, DebugBusAccessKind, Motherboard};