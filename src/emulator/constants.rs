// =======================================================
// src/emulator/constants.rs — Emulator timing and coordination constants
// =======================================================

use std::time::Duration;

/* Host pacing targets one PAL frame period; emulated cycle counts remain defined by clockchip rather than this wall-clock duration. */
pub const PAL_FRAME_DURATION: Duration = Duration::from_nanos(19_950_727);
/* Warp mode decouples emulation speed from display refresh but still presents periodically so the host interface remains responsive. */
pub const WARP_PRESENT_INTERVAL: Duration = Duration::from_millis(50);
/* Deferred startup text waits for the reset path and BASIC prompt to settle before synthetic keyboard input begins. */
pub const STARTUP_COMMAND_DELAY_FRAMES: u64 = 45;