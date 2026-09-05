// =======================================================
// src/memory.rs — Memory subsystem façade
// =======================================================

/* The memory façade joins physical storage, bus latching and CPU/VIC access paths without hiding the PLA-selected source of each access. */

pub mod bus;
pub mod color_ram;
pub mod constants;
pub mod memory;
pub mod ram;
pub mod rom;
pub mod vic_access;

pub use constants::MapRegion;
pub use memory::Memory;