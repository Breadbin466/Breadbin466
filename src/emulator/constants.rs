// =======================================================
// src/emulator/constants.rs — Emulator timing and coordination constants
// =======================================================

use crate::clockchip::constants::{CPU_FREQ_HZ, CYCLES_PER_FRAME};
use std::time::Duration;

/* Host pacing uses the same oscillator and raster length as the chips. */
pub const PAL_FRAME_DURATION: Duration =
	Duration::from_nanos((CYCLES_PER_FRAME as f64 / CPU_FREQ_HZ * 1_000_000_000.0 + 0.5) as u64);
/* The event loop sleeps until this final interval, then polls native events
 * until the PAL deadline to retain timer precision without blocking input. */
pub const HOST_WAKE_MARGIN: Duration = Duration::from_millis(1);
/* Warp mode decouples emulation speed from display refresh but still presents periodically so the host interface remains responsive. */
pub const WARP_PRESENT_INTERVAL: Duration = Duration::from_millis(50);
/* Bound uninterrupted warp work so native input is serviced between short batches. */
pub const WARP_SERVICE_BUDGET: Duration = Duration::from_millis(4);
/* Deferred startup text waits for the reset path and BASIC prompt to settle before synthetic keyboard input begins. */
pub const STARTUP_COMMAND_DELAY_FRAMES: u64 = 45;
/* Long host stalls abandon pacing debt instead of replaying a burst after sleep or a modal operation. */
pub const HOST_STALL_THRESHOLD: Duration = Duration::from_millis(250);