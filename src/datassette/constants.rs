// =======================================================
// src/datassette/constants.rs — Datassette subsystem constants
// =======================================================

/* TAP images use a 20-byte header. Version 0 represents a zero pulse as the legacy long-pulse marker, while version 1 follows the marker with an exact 24-bit little-endian cycle count (TAP-FORMAT-SCHEPERS). */
pub(crate) const TAP_SIGNATURE: &[u8; 12] = b"C64-TAPE-RAW";
pub(crate) const TAP_HEADER_SIZE: usize = 20;
pub(crate) const TAP_VERSION_OFFSET: usize = 12;
pub(crate) const TAP_DATA_SIZE_OFFSET: usize = 16;
pub(crate) const TAP_VERSION_0: u8 = 0;
pub(crate) const TAP_VERSION_1: u8 = 1;
pub(crate) const TAP_EXTENDED_PULSE_SIZE: usize = 3;
pub(crate) const TAP_SHORT_PULSE_SCALE: u32 = 8;
pub(crate) const TAP_MAX_SHORT_PULSE: u32 = u8::MAX as u32;
pub(crate) const TAP_MAX_EXTENDED_PULSE: u32 = 0x00FF_FFFF;
/* The size limit bounds malformed or continuously recorded images before they can exhaust host memory. */
pub(crate) const MAX_TAPE_SIZE: usize = 4 * 1024 * 1024;
/* A detected flux transition is held low for two machine cycles so CIA1 FLAG observes a stable edge rather than a zero-duration host event. */
pub(crate) const PULSE_HOLD_CYCLES: u8 = 2;
/* Counter motion is derived from elapsed PAL machine cycles, tape speed, reel geometry and tape thickness rather than from host frame time. */
pub(crate) const C64_PAL_CYCLES_PER_SECOND: f64 = 985_248.0;
pub(crate) const ODOMETRE_INITIAL_REEL_RADIUS_METRES: f64 = 0.011;
pub(crate) const ODOMETRE_TAPE_SPEED_METRES_PER_SECOND: f64 = 0.04762;
pub(crate) const ODOMETRE_TAPE_THICKNESS_METRES: f64 = 12.0e-6;
pub(crate) const ODOMETRE_TURNS_PER_COUNTER_UNIT: f64 = 3.0;
pub(crate) const ODOMETRE_MAX_VALUE: f64 = 999.99;