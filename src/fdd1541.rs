// =======================================================
// src/fdd1541.rs — LLE 1541 façade: module declarations only
// =======================================================

/* The 1541 façade presents one complete independent drive computer while keeping media codecs and internal VIA helpers scoped to the subsystem. */

pub mod cable;
pub mod computer;
mod constants;
pub(crate) mod convert;
pub(crate) mod d64;
pub(crate) mod d7z;
pub mod disk_drive;
pub(crate) mod disk_image;
mod disk_rotation;
pub mod drive;
pub(crate) mod g64;
pub mod gcr;
pub mod iec;
mod load;
mod mechanics;
mod media;
mod nbz;
pub(crate) mod nib;
mod ownership;
mod read_channel;
pub(crate) mod reclaim;
mod save;
pub mod status;
pub mod thread;
pub mod via;
pub mod via1;
pub mod via2;
mod via_control;
mod via_timers;

pub use drive::Fdd1541;
pub use status::DriveStatus;
pub use thread::DriveWorker;