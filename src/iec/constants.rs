// =======================================================
// src/iec/constants.rs — IEC bus constants
// =======================================================

/* The packed state keeps participant pull-down requests distinct from resolved line levels. Host and device fields can therefore be saved independently and recombined through the wired-OR equations in bus.rs. */
pub(crate) const HOST_ATN: u32 = 1 << 0;
pub(crate) const HOST_CLK: u32 = 1 << 1;
pub(crate) const HOST_DATA: u32 = 1 << 2;
pub(crate) const HOST_SRQ: u32 = 1 << 3;
pub(crate) const DEVICE_ATN: u32 = 1 << 4;
pub(crate) const DEVICE_CLK: u32 = 1 << 5;
pub(crate) const DEVICE_DATA: u32 = 1 << 6;
pub(crate) const DEVICE_SRQ: u32 = 1 << 7;
pub(crate) const DEVICE_ATNA: u32 = 1 << 8;
pub(crate) const DEVICE_ATN_ACK: u32 = 1 << 9;
pub(crate) const DEVICE_CONNECTED: u32 = 1 << 10;
pub(crate) const HOST_PULLS: u32 = HOST_ATN | HOST_CLK | HOST_DATA;
pub(crate) const DEVICE_PULLS: u32 = DEVICE_ATN | DEVICE_CLK | DEVICE_DATA;
pub(crate) const DEVICE_STATE: u32 = DEVICE_PULLS | DEVICE_SRQ | DEVICE_ATNA | DEVICE_ATN_ACK;
pub(crate) const INITIAL_STATE: u32 = DEVICE_CONNECTED;