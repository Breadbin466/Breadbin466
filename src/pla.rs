// =======================================================
// src/pla.rs — PLA subsystem façade
// =======================================================

/* The PLA façade separates signal formation, programmed product terms and the CPU/VIC mapping views derived from the same logic matrix. */

mod and_plane;
pub(crate) mod constants;
pub mod cpu_map;
mod or_plane;
mod pla;
pub mod signals;
pub mod vic_map;

pub use cpu_map::map_cpu_read_addr;
pub use vic_map::map_vic_addr;