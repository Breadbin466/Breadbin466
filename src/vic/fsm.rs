// =======================================================
// src/vic/fsm.rs — VIC-II finite-state machines using Bauer terminology
// =======================================================

/* DisplayState is Bauer's idle/display flip-flop. It controls whether fetched graphics are interpreted as active foreground data and is updated by badlines and RC rollover. */
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayState {
	/* Foreground generation is inactive and idle accesses feed the graphics path. */
	Idle,
	/* Character or bitmap data is being produced for the current text row. */
	Display,
}

impl DisplayState {
	#[inline(always)]
	/* This predicate keeps display-state tests descriptive at call sites where idle fetches and active graphics fetches diverge. */
	pub const fn is_idle(self) -> bool { matches!(self, Self::Idle) }
}

/* DisplayTransition records a delayed state change that becomes visible only after the character-access phase has completed. */
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayTransition {
	/* No delayed display-state edge is pending. */
	Stable,
	/* A live badline change occurred after the c-access decision; display begins once that character-access phase has completed. */
	EnterDisplayAfterCharacterAccess,
}

/* CharacterAccessState gates the forty c-access slots of a badline independently of whether the display flip-flop is already active. */
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharacterAccessState {
	/* Matrix fetch slots leave the existing line buffer untouched. */
	Disabled,
	/* Each c-access refreshes one screen-code/colour entry during the badline window. */
	Enabled,
}

impl CharacterAccessState {
	#[inline(always)]
	/* Character access is a separate gate from display state because a late badline can enter display after the current c-access decision. */
	pub const fn is_enabled(self) -> bool { matches!(self, Self::Enabled) }
}

/* The three ECM/BMM/MCM control bits select eight electrical decode combinations. Five are documented display modes; the remaining combinations are retained because real software can select them and the VIC still produces deterministic output. (C64-PRG-1982, VIC-II registers and display modes) */
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum GraphicsMode {
	/* One bit per pixel; screen colour supplies foreground and background register 0 supplies the background. */
	StandardText = 0,
	/* Character cells may use two-bit pixels when the per-cell colour bit enables multicolour decoding. */
	MulticolourText = 1,
	/* Bitmap data supplies one-bit pixels while the screen byte supplies the two colours. */
	StandardBitmap = 2,
	/* Bitmap data is decoded as four two-bit symbols using screen, colour RAM and background register 0. */
	MulticolourBitmap = 3,
	/* The upper two character-code bits select one of four background registers while the remaining six bits select the glyph. */
	ExtendedColourText = 4,
	/* ECM and MCM are both set without bitmap mode; the resulting decode is undocumented but deterministic. */
	InvalidTextMulticolour = 5,
	/* ECM and bitmap mode are both set; foreground generation follows the chip's invalid bitmap path. */
	InvalidBitmap = 6,
	/* All three mode bits are set, selecting the final undocumented decode combination. */
	InvalidBitmapMulticolour = 7,
}

impl GraphicsMode {
	#[inline(always)]
	/* Preserve the hardware bit ordering ECM:BMM:MCM when converting the three control latches into one decode state. */
	pub const fn from_control_bits(ecm: bool, bmm: bool, mcm: bool) -> Self {
		Self::from_code(((ecm as u8) << 2) | ((bmm as u8) << 1) | (mcm as u8))
	}

	#[inline(always)]
	/* Masking to three bits makes restored or diagnostic values obey the same eight-way decode as the physical control lines. */
	pub const fn from_code(code: u8) -> Self {
		match code & 7 {
			0 => Self::StandardText,
			1 => Self::MulticolourText,
			2 => Self::StandardBitmap,
			3 => Self::MulticolourBitmap,
			4 => Self::ExtendedColourText,
			5 => Self::InvalidTextMulticolour,
			6 => Self::InvalidBitmap,
			_ => Self::InvalidBitmapMulticolour,
		}
	}

	#[inline(always)]
	/* Return the original three-bit control-line encoding used by transition logic. */
	pub const fn code(self) -> u8 { self as u8 }

	#[inline(always)]
	/* Expose the MCM line without re-decoding the enum at each pixel helper. */
	pub const fn multicolour_bit(self) -> bool { (self.code() & 1) != 0 }

	#[inline(always)]
	/* Expose the BMM line used to choose character or bitmap address generation. */
	pub const fn bitmap_bit(self) -> bool { (self.code() & 2) != 0 }

	#[inline(always)]
	/* Expose the ECM line used by extended-colour character addressing and invalid modes. */
	pub const fn extended_colour_bit(self) -> bool { (self.code() & 4) != 0 }
}

/* MemoryAccess names the bus slot currently being performed for telemetry and diagnostics; it does not schedule the access itself. */
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryAccess {
	/* Idle or floating-bus slot. */
	Idle,
	/* Screen matrix and colour RAM c-access. */
	Character,
	/* Character or bitmap g-access. */
	Graphics,
	/* DRAM row-refresh access. */
	Refresh,
	/* Pointer-table access for the named sprite. */
	SpritePointer(usize),
	/* One of the three data-byte accesses for the named sprite. */
	SpriteData(usize),
}