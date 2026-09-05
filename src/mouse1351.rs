// =======================================================
// src/mouse1351.rs — Commodore 1351 subsystem façade
// =======================================================

/* The 1351 façade exposes the complete proportional mouse while timing and bus
 * constants remain internal to the peripheral implementation. */
mod constants;
mod device;

pub use device::Mouse1351;