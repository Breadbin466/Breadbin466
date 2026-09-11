// =======================================================
// src/vic/constants.rs — VIC-II Global Constants
// =======================================================

/*
The constants in this module describe the PAL 6569R5 raster geometry, memory-access windows and pixel coordinate system used by every VIC-II subunit. Cycle numbers are 1-based and follow Bauer's timing convention; pixel coordinates cover the complete raster rather than only the visible display rectangle. (BAUER-VIC-II-1996; C64-SERVICE-MANUAL-1985)
*/

/* A PAL raster line contains 63 VIC cycles and a frame contains 312 lines. Eight pixels are generated per cycle, giving the complete 504 by 312 raster stored by the renderer. */
pub const PAL_CYCLES_PER_LINE: u16 = 63;
pub const PAL_LINES: u16           = 312;
pub const TOTAL_WIDTH: usize       = 504;
pub const TOTAL_HEIGHT: usize      = 312;
pub const VIDEOWIDTH: i32          = 504;
pub const DISPLAY_START: usize     = 24;

/* Palette-buffer tags above the sixteen visible colours distinguish background and border sources before final RGB conversion. */
pub const BACKCOLORINDEX0: u8  = 0x80;
pub const BACKCOLORINDEX1: u8  = 0x81;
pub const BACKCOLORINDEX2: u8  = 0x82;
pub const BACKCOLORINDEX3: u8  = 0x83;
pub const BORDERCOLORINDEX: u8 = 0x8E;
pub const VIC_BLACK: u8        = 0x00;

/* The low four IRQ bits are the VIC source latches; bit 7 reports whether any enabled source currently asserts IRQ. */
/* Source bits preserve the VIC-II register order: raster compare, sprite/background collision, sprite/sprite collision, and light pen. */
pub const IRQ_RASTER: u8   = 0x01;
pub const IRQ_MBC: u8      = 0x02;
pub const IRQ_MMC: u8      = 0x04;
pub const IRQ_LIGHT_PEN: u8 = 0x08;
pub const IRQ_STATUS: u8   = 0x80;
/* Breadbin466 uses one fixed RGB palette for the reference PAL machine. These presentation values do not participate in VIC timing or colour-source selection. */
pub const VIC_PALETTE: [[u8; 3]; 16] = [
	[0x00, 0x00, 0x00], [0xFF, 0xFF, 0xFF], [0xBF, 0x56, 0x56], [0x82, 0xD2, 0xD4],
	[0x98, 0x5C, 0xA6], [0x82, 0xD6, 0x82], [0x2D, 0x2B, 0xD5], [0xD8, 0xCB, 0x74],
	[0xA7, 0x74, 0x3B], [0x6B, 0x4B, 0x00], [0xCF, 0x8E, 0x90], [0x61, 0x61, 0x61],
	[0x92, 0x92, 0x8D], [0xC1, 0xF8, 0xBE], [0x78, 0x84, 0xFF], [0xD2, 0xD2, 0xD2],
];
/* Pre-expand every monochrome graphics byte into an eight-byte foreground mask so the hot renderer can apply one native-word operation per character cell. */
const fn build_std_pixel_masks() -> [u64; 256] {
	let mut masks = [0u64; 256];
	let mut value = 0usize;
	while value < 256 {
		let mut bytes = [0u8; 8];
		let mut pixel = 0usize;
		while pixel < 8 {
			bytes[pixel] = if value & (0x80 >> pixel) != 0 { 0xFF } else { 0 };
			pixel += 1;
		}
		masks[value] = u64::from_ne_bytes(bytes);
		value += 1;
	}
	masks
}

pub(crate) const STD_PIXEL_MASKS: [u64; 256] = build_std_pixel_masks();
/* Renderer buffers include guard pixels/cycles because horizontal scrolling, sprites and border transitions can write slightly outside the nominal 504-pixel line. */
pub(crate) const CLOCK_SYNC_BAND: i64 = 5 * 60 * 1_000_000;
pub(crate) const SPRITE_PIXEL_OFFSET: i32 = 4;
/* The output crop starts after DISPLAY_START pixels and must retain a complete raster width. */
pub(crate) const LINE_BUF_LEN: usize = DISPLAY_START + TOTAL_WIDTH;
pub(crate) const MASK_BUF_LEN: usize = 504 / 8 + 16;
pub(crate) const BORDER_BITS_LEN: usize = LINE_BUF_LEN.div_ceil(8);
pub(crate) const FRAMEBUFFER_SIZE: usize = TOTAL_WIDTH * TOTAL_HEIGHT * 3;
pub(crate) const SCREEN_COLUMNS: i64 = 40;
pub(crate) const DATA_LOAD_X: [u16; 8] = [0x162, 0x172, 0x182, 0x192, 0x1A2, 0x1B2, 0x1C2, 0x1D2];
pub(crate) const VERTICAL_WINDOW_LUT: [bool; 512] = {
	let mut lut = [false; 512];
	let mut y = 0;
	while y < 512 {
		lut[y] = (y >= 0x30) && (y <= 0xF7);
		y += 1;
	}
	lut
};

/* Bus-address and schedule constants below follow the 14-bit VIC address space and the PAL fetch timetable described by Bauer. */
pub const VIC_ADDRESS_MASK: u16 = 0x3FFF;
pub const VIDEO_COUNTER_MASK: u16 = 0x03FF;
pub const SPRITE_POINTER_TABLE_OFFSET: u16 = 0x03F8;
pub const DRAM_REFRESH_BASE: u16 = 0x3F00;
pub const IDLE_ACCESS_ADDRESS: u16 = 0x3FFF;
pub const DEN_LATCH_RASTER_LINE: u16 = 0x0030;
pub const MCBASE_UPDATE_CYCLE: u16 = 16;
pub const SPRITE_DMA_FIRST_PHASE_CYCLE: u16 = 55;
pub const SPRITE_DMA_SECOND_PHASE_CYCLE: u16 = 56;
pub const SPRITE_DISPLAY_CYCLE: u16 = 58;
pub const POST_SPRITE_CYCLE: u16 = SPRITE_DISPLAY_CYCLE + 1;
pub const PAL_LAST_CYCLE: u16 = 63;
pub const HPIXEL_START: u16 = 0x01A0;