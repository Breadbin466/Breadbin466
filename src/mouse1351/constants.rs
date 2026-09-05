// =======================================================
// src/mouse1351/constants.rs — Commodore 1351 timing and bus constants
// =======================================================

/* The 1351 publishes its modulo-64 position every 512 microseconds (COMMODORE-1351-MANUAL-1986). On the PAL
 * 985248 Hz machine clock this is approximately 504.45 cycles, so the nearest
 * whole motherboard-cycle interval is used for the externally visible latch. */
pub(crate) const REPORT_INTERVAL_CYCLES: u64 = 504;

/* Commodore specifies that the selected POT path must remain stable for more
 * than 1.6 milliseconds before a reliable SID reading is available (COMMODORE-1351-MANUAL-1986). At the PAL
 * clock this corresponds to just over 1576 cycles. */
pub(crate) const POT_SETTLE_CYCLES: u64 = 1_577;

/* CIA1 port-A bits 7..6 control the 4066 analogue switch. %01 selects control
 * port 1, which is the fixed 1351 connection used by Breadbin466. */
pub(crate) const PORT_SELECT_MASK: u8 = 0xC0;
pub(crate) const PORT_1_SELECT: u8 = 0x40;

pub(crate) const LEFT_BUTTON_MASK: u8 = 0x10;
pub(crate) const RIGHT_BUTTON_MASK: u8 = 0x01;