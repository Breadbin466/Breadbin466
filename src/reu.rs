// =======================================================
// src/reu.rs — MOS 8726 RAM Expansion Unit façade
// =======================================================

/*
 * MOS 8726 RAM Expansion Unit façade.
 *
 * The façade exposes the controller and its predicted C64 bus operation while
 * keeping register semantics, DMA state and 1764 DRAM storage private.
 */

mod constants;
mod dma;
mod memory;
mod registers;
mod reu;
mod timing;

pub use memory::REU_1764_CAPACITY_BYTES;
pub use reu::{Reu, ReuC64Access};
pub(crate) use timing::ReuBusAction;