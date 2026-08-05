// =======================================================
// src/memory.rs — Memory subsystem façade
// =======================================================

/* The memory façade joins physical storage, bus latching and CPU/VIC access paths without hiding the PLA-selected source of each access. */

pub mod constants;
pub mod ram;
pub mod rom;
pub mod color_ram;
pub mod bus;
pub mod vic_access;
pub mod memory;

pub use constants::MapRegion;
pub use memory::Memory;