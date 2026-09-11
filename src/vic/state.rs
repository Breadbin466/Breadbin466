// =======================================================
// src/vic/state.rs — VicII chip state: fields, construction and reset
// =======================================================

use super::registers::{Registers, IrqState};
use super::timing::VicTiming;
use super::sprite_unit::SpriteUnit;
use super::screen::Screen;
use super::telemetry::VicTelemetry;
use super::border::BorderUnit;
use super::fsm::{CharacterAccessState, DisplayState, DisplayTransition, GraphicsMode};
use super::constants::CLOCK_SYNC_BAND;

/*
VicII is the complete cycle-visible state of the PAL 6569R5. A host tick advances one VIC cycle through the sequencer, but the visible result depends on several pipelines that overlap in time: raster and badline decisions reserve the bus, character and graphics fetches fill delayed latches, border logic gates foreground output, and eight sprite units fetch and display independently. Keeping these latches explicit preserves the ordering described by the VIC-II timing model rather than collapsing a raster line into a scanline renderer. (BAUER-VIC-II-1996, sections 3.5 to 3.9)
*/
pub struct VicII {
	pub regs:                Registers,
	pub irq:                 IrqState,
	pub timing:              VicTiming,

	pub sprites:             SpriteUnit,
	pub screen:              Screen,
	pub border:              BorderUnit,

	/* VC, VCBASE, RC and VMLI form the matrix/graphics address pipeline. VC advances through the 40-column row, VCBASE is retained between raster lines, RC selects the character or bitmap row, and VMLI indexes the line buffer filled by character accesses. */
	pub vc: u16, pub vc_base: u16, pub rc: u8, pub vmli: u8,
	pub matrix_line:                    [u16; 40],
	pub horizontal_pixel_counter:       u16,

	pub telemetry: VicTelemetry,

	/* BA warns the CPU before ownership is removed; AEC marks the cycles where the VIC actually owns the address bus. irq_changed is the motherboard-facing snapshot of the current interrupt output. */
	pub ba_low: bool, pub aec_low: bool, pub irq_changed: bool,

	/* Display and character-access states are related but distinct: a badline can enable matrix fetches while the display pipeline separately enters or leaves foreground output. */
	pub display_state: DisplayState,
	pub display_transition: DisplayTransition,
	pub character_access_state: CharacterAccessState,
	pub graphics_mode: GraphicsMode,
	pub previous_graphics_mode: GraphicsMode,
	pub mode_changing:    bool,

	/* Character and graphics values cross two delayed latches before becoming pixels. The explicit stages allow register changes and mode transitions to affect the same cycles as on the chip. */
	pub char_data_fetched:    u16,
	pub char_data_pipeline_1: u16,
	pub char_data_pipeline_2: u16,
	pub char_data_carry:      u16,
	pub graphics_data_fetched:     u8,
	pub graphics_data_pipeline_1:  u8,
	pub graphics_data_pipeline_2:  u8,

	pub latch_den:                    bool,
	pub pixels_to_skip:               u8,
	pub dram_refresh_counter:         u8,
	pub horizontal_scroll_at_cycle57: u8,
	pub check_irq_in_cycle2:          bool,
	pub raster_match:                 bool,

	/* The bus-arbitration clocks record when BA changed and model the three-cycle warning before AEC is asserted. forced_badline_c_access_clock preserves exceptional character-access timing caused by register writes. */
	pub aec_counter:                       i8,
	pub clock_ba_low:                      i64,
	pub clock_ba_high:                     i64,
	pub forced_badline_c_access_clock: i64,
	pub current_clock:                     i64,

	pub(super) pending_character_access: Option<usize>,

	/* Light-pen coordinates are frame-scoped latches. Once triggered, subsequent edges in the same frame are ignored until the frame boundary rearms the capture path. */
	pub light_pen_x:           u8,
	pub light_pen_y:           u8,
	pub light_pen_triggered:   bool,
	pub(super) light_pen_pin_high: bool,
}

impl VicII {
	/* Construction starts immediately before raster line 0, cycle 1, so the first tick crosses the PAL frame boundary exactly as a running chip would. */
	pub fn new() -> Self {
		Self {
			regs: Registers::new(), irq: IrqState::new(), timing: VicTiming::new(),
			sprites: SpriteUnit::new(), screen: Screen::new(),
			border: BorderUnit::new(),
			vc: 0, vc_base: 0, rc: 0, vmli: 0,
			horizontal_pixel_counter: 0x1A0,
			matrix_line: [0; 40],
			telemetry: VicTelemetry::new(),
			ba_low: false, aec_low: false, irq_changed: false,

			display_state: DisplayState::Idle, display_transition: DisplayTransition::Stable, character_access_state: CharacterAccessState::Disabled,
			graphics_mode: GraphicsMode::StandardText, previous_graphics_mode: GraphicsMode::StandardText,
			mode_changing: false,

			char_data_fetched: 0, char_data_pipeline_1: 0, char_data_pipeline_2: 0, char_data_carry: 0,
			graphics_data_fetched: 0, graphics_data_pipeline_1: 0, graphics_data_pipeline_2: 0,

			latch_den: false, pixels_to_skip: 0, dram_refresh_counter: 0xFF,
			horizontal_scroll_at_cycle57: 0, check_irq_in_cycle2: false, raster_match: false,

			aec_counter: 3,
			clock_ba_low: 0, clock_ba_high: 0,
			forced_badline_c_access_clock: -40,
			current_clock: 0,

			pending_character_access: None,

			light_pen_x:           0,
			light_pen_y:           0,
			light_pen_triggered:   false,
			light_pen_pin_high: true,
		}
	}

	/* Reset clears the programmable and pipeline state while retaining allocated framebuffer storage. Raster timing returns to the same pre-frame position used by construction. */
	pub fn reset(&mut self) {
		self.regs.reset(); self.irq.reset(); self.timing.reset();
		self.sprites.reset();
		self.screen.reset(); self.border.reset();
		self.vc = 0; self.vc_base = 0; self.rc = 0; self.vmli = 0;
		self.horizontal_pixel_counter = 0x1A0;
		self.matrix_line.fill(0);
		self.ba_low = false; self.aec_low = false; self.irq_changed = false;

		self.display_state = DisplayState::Idle; self.display_transition = DisplayTransition::Stable; self.character_access_state = CharacterAccessState::Disabled;
		self.graphics_mode = GraphicsMode::StandardText; self.previous_graphics_mode = GraphicsMode::StandardText;
		self.mode_changing = false;

		self.char_data_fetched = 0; self.char_data_pipeline_1 = 0; self.char_data_pipeline_2 = 0; self.char_data_carry = 0;
		self.graphics_data_fetched = 0; self.graphics_data_pipeline_1 = 0; self.graphics_data_pipeline_2 = 0;

		self.latch_den = false; self.pixels_to_skip = 0; self.dram_refresh_counter = 0xFF;
		self.horizontal_scroll_at_cycle57 = 0; self.check_irq_in_cycle2 = false; self.raster_match = false;

		self.aec_counter = 3;
		self.clock_ba_low = 0; self.clock_ba_high = 0;
		self.forced_badline_c_access_clock = -40;
		self.current_clock = 0;

		self.pending_character_access = None;

		self.light_pen_x           = 0;
		self.light_pen_y           = 0;
		self.light_pen_triggered   = false;
		self.light_pen_pin_high = true;
	}

	/* The renderer owns a stable RGB framebuffer whose storage survives reset; callers borrow it without gaining access to VIC pipeline state. */
	pub fn get_framebuffer(&self) -> &[u8] {
		self.screen.framebuffer.as_slice()
	}

	/* Exposes the resolved IRQ pin rather than the raw set of pending source latches. */
	pub fn is_irq_active(&self) -> bool    { self.irq.line_active }

	/* The optional C128 host path may use its fast clock only while the VIC is not warning that it needs the shared bus. */
	pub fn c128_2mhz_allowed(&self) -> bool  { !self.ba_low }
}

impl Default for VicII {
	fn default() -> Self { Self::new() }
}

/*
Long-running host clocks are periodically rebased only for relative timestamps. The raster counters and visible chip state are untouched; old transition times are clamped to the largest interval any timing comparison still needs.
*/
pub fn normalise_clock(vic: &mut VicII) {
	let current = vic.current_clock;

	/* Only timestamps used for bounded age comparisons are rebased. Absolute raster position, counters and pending events remain untouched, so normalisation cannot create or remove a chip-visible transition. */

	let sync = |clock: &mut i64| {
		if current - *clock > CLOCK_SYNC_BAND {
			*clock = current - CLOCK_SYNC_BAND;
		}
	};

	/* Sprite register-transition timestamps are retained only as far back as the longest mid-cycle comparison can inspect. */
	sync(&mut vic.sprites.mc_changed_at);
	sync(&mut vic.sprites.horizontal_expand_changed_at);
	sync(&mut vic.sprites.prio_changed_cycle);
	sync(&mut vic.sprites.current_cycle_counter);

	/* Bus-arbitration and forced-badline timestamps use the same bounded-age convention. */
	sync(&mut vic.forced_badline_c_access_clock);
	sync(&mut vic.clock_ba_low);
	sync(&mut vic.clock_ba_high);
}