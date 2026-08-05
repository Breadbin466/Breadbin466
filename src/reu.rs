// =======================================================
// src/reu.rs — MOS 8726 RAM Expansion Unit façade
// =======================================================

/* The REU façade exposes the DMA device and its predicted C64 bus access while keeping register and expansion-memory storage internal. */

mod dma;
mod memory;
mod registers;
mod reu;

pub use reu::{Reu, ReuC64Access};