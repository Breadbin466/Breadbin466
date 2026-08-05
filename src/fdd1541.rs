// =======================================================
// src/fdd1541.rs — LLE 1541 façade: module declarations only
// =======================================================

/* The 1541 façade presents one complete independent drive computer while keeping media codecs and internal VIA helpers scoped to the subsystem. */

pub mod cable;
mod ownership;
pub mod status;
pub mod thread;
pub mod computer;
mod constants;
pub mod disk_drive;
pub(crate) mod d64;
pub mod drive;
pub(crate) mod g64;
pub mod gcr;
pub mod iec;
pub(crate) mod nib;
mod nbz;
pub mod via;
mod via_control;
mod via_timers;
pub mod via1;
pub mod via2;

pub use status::DriveStatus;
pub use thread::DriveWorker;
pub use drive::Fdd1541;