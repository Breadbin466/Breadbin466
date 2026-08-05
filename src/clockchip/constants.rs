// =======================================================
// src/clockchip/constants.rs — MOS 8701 PAL Timing Constants
// =======================================================

/* The PAL reference machine uses a 17.734475 MHz crystal. The MOS 8701 derives the common system clock by dividing that master frequency by eighteen, yielding the approximately 0.985 MHz cadence shared by the 6510 and VIC-II (C64-SERVICE-MANUAL-1985). */
pub const MASTER_FREQ_HZ: f64 = 17_734_475.0;
pub const SYSTEM_DIVISOR: u64 = 18;
pub const CPU_FREQ_HZ: f64 = MASTER_FREQ_HZ / (SYSTEM_DIVISOR as f64);

/* The PAL 6569R5 raster contains 312 lines, each lasting 63 system cycles. These dimensions define the machine-visible frame timing rather than a host display refresh approximation (C64-SERVICE-MANUAL-1985). */
pub const LINES_PER_FRAME: u64 = 312;
pub const CYCLES_PER_LINE: u64 = 63;

/* A frame is the exact product of the raster dimensions, so every frame-level scheduler remains locked to the same chip clock used by the CPU and VIC-II. */
pub const CYCLES_PER_FRAME: u64 = LINES_PER_FRAME * CYCLES_PER_LINE;