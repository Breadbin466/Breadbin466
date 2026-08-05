// =======================================================
// src/motherboard/constants.rs — Motherboard timing and bus constants
// =======================================================

use crate::clockchip::constants::CPU_FREQ_HZ as PAL_CPU_FREQ_HZ;

/* The motherboard uses the integer PAL CPU rate for cycle-domain peripherals whose counters do not require fractional host time. */
pub(crate) const CPU_FREQ_HZ: u32 = PAL_CPU_FREQ_HZ as u32;
/* Audio conversion keeps positive and negative 16-bit ranges distinct, while THRESHOLD rejects small DC-centred excursions before they reach the host stream. */
pub(crate) const THRESHOLD: i64 = 28_000;
pub(crate) const NEGATIVE_RANGE: f64 = 32_768.0;
pub(crate) const POSITIVE_RANGE: f64 = 32_767.0;
/* CIA time-of-day input is derived from the 50 Hz PAL mains reference rather than from the raster frame boundary. */
pub(crate) const PAL_TOD_INPUT_PERIOD_CYCLES: u32 = 19_705;
/* KERNAL autorun waits for the PETSCII READY. prompt anywhere in the 40 x 25 screen matrix before injecting keyboard-buffer input. */
pub(crate) const READY_PATTERN: [u8; 6] = [18, 5, 1, 4, 25, 46];
pub(crate) const SCREEN_CELLS: usize = 1000;
pub(crate) const READY_POSITIONS: usize = SCREEN_CELLS - READY_PATTERN.len() + 1;