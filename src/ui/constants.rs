// =======================================================
// src/ui/constants.rs — User interface constants
// =======================================================

/* Persistence limits and debounce intervals bound recent-file growth and coalesce rapid preference changes. */
pub(crate) const MAX_RECENT: usize = 10;
pub(crate) const SAVE_DEBOUNCE_SECS: u64 = 2;
/* Win32 resource identifiers, dialog geometry and native control styles belong only to the Windows About box backend. */
#[cfg(target_os = "windows")]
pub(crate) const ID_OK: i32 = 1;
#[cfg(target_os = "windows")]
pub(crate) const ID_NAME: i32 = 100;
#[cfg(target_os = "windows")]
pub(crate) const ID_VERSION: i32 = 101;
#[cfg(target_os = "windows")]
pub(crate) const ID_DESCRIPTION: i32 = 102;
#[cfg(target_os = "windows")]
pub(crate) const ID_COPYRIGHT: i32 = 103;
#[cfg(target_os = "windows")]
pub(crate) const ID_ICON: i32 = 104;
#[cfg(target_os = "windows")]
pub(crate) const CLIENT_WIDTH: i32 = 560;
#[cfg(target_os = "windows")]
pub(crate) const CLIENT_HEIGHT: i32 = 452;
#[cfg(target_os = "windows")]
pub(crate) const MARGIN: i32 = 24;
#[cfg(target_os = "windows")]
pub(crate) const SS_LEFT_STYLE: u32 = 0x0000_0000;
#[cfg(target_os = "windows")]
pub(crate) const SS_ICON_STYLE: u32 = 0x0000_0003;
#[cfg(target_os = "windows")]
pub(crate) const SUBTLE_COLOUR: u32 = 0x0060_6060;
/* OSD text and drive indicators reuse the bundled C64 character ROM so diagnostic text matches the machine visual language. */
pub(crate) const CHAR_ROM: &[u8] = include_bytes!("../../roms/characters_901225-01.bin");
pub(crate) const FONT_OFFSET_UPPER: usize = 0;
pub(crate) const FONT_OFFSET_LOWER: usize = 2048;
pub(crate) const PETSCII_CIRCLE: usize = 0x51;
/* OSD colours encode persistent text, active-low style indicators and inactive transport controls in the CPU-rendered status surface. */
pub(crate) const COLOR_LED_GREEN_ON: u32 = 0xFF4BA646;
pub(crate) const COLOR_LED_GREEN_OFF: u32 = 0xFF1A3A18;
pub(crate) const COLOR_LED_RED_ON: u32 = 0xFF2222B2;
pub(crate) const COLOR_LED_RED_OFF: u32 = 0xFF0A0A3A;
pub(crate) const COLOR_TEXT: u32 = 0xFF000000;
pub(crate) const COLOR_REVERSE_BG: u32 = 0xFF000000;
pub(crate) const COLOR_REVERSE_FG: u32 = 0xFFFFFFFF;
/* RGBA texture bytes are packed as 0xAABBGGRR on the supported little-endian hosts. */
pub(crate) const COLOR_TRANSPORT_PLAY: u32 = 0xFF36852B;
pub(crate) const COLOR_TRANSPORT_READY: u32 = 0xFF303030;
pub(crate) const COLOR_TRANSPORT_RECORD_ON: u32 = 0xFF2020D5;
pub(crate) const COLOR_TRANSPORT_OFF: u32 = 0xFFAAAAAA;
/* Host input thresholds and audio queue geometry define presentation services rather than emulated hardware timing. */
pub(crate) const WINDOW_TITLE: &str = concat!("Breadbin466 ", env!("CARGO_PKG_VERSION"), " – PAL Assy 250466 Commodore 64 emulator");
pub(crate) const THRESHOLD: f32 = 0.4;
pub(crate) const AUDIO_SAMPLE_RATE: u32 = 44_100;
pub(crate) const BUFFER_CAPACITY: u16 = 1 << 12;
/* OSD geometry is expressed in native framebuffer pixels and fixed eight-pixel glyph cells. */
pub(crate) const LINE_HEIGHT: usize = 10;
pub(crate) const OSD_MEDIA_LABEL_OVERHEAD: usize =
	"Disk: ".len() + "  -  Tape: ".len() + "  -  Cartridge: ".len();
pub(crate) const OSD_TOOLTIP_PADDING_X: usize = 6;
pub(crate) const OSD_TOOLTIP_PADDING_Y: usize = 4;
/* The native window rounds its lower corners into the OSD surface. This inset keeps all status text and controls inside the rectangular safe area on every supported host. */
pub(crate) const OSD_SAFE_PADDING_X: usize = 16;
pub(crate) const OSD_TOOLTIP_BORDER: u32 = 0xFF000000;
pub(crate) const OSD_TOOLTIP_BACKGROUND: u32 = 0xFFFFFFFF;
pub(crate) const OSD_GUI_HEIGHT_CHARS: usize = 12;
pub(crate) const OSD_BUFFER_SCALE: usize = 2;
pub(crate) const OSD_TRANSPORT_COUNT: usize = 6;
pub(crate) const OSD_TRANSPORT_PITCH: usize = 12;
pub(crate) const OSD_TRANSPORT_RIGHT: usize = 86;
/* Application identity strings are shared by native About implementations and packaging metadata. */
pub const APP_NAME: &str = "Breadbin466";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const COPYRIGHT: &str = "Copyright © 2025–2026 The Breadbin466 Team";
pub const DESCRIPTION_PARAGRAPHS: [&str; 3] = [
	"Breadbin466 is a cycle-accurate Commodore 64 emulator written from scratch in Rust.",
	"Its goal is to reproduce a 1986 PAL Commodore 64 as faithfully as possible: a machine based on the Assy 250466 motherboard, featuring a 6510 CPU, 6569R5 VIC-II, 6581R4AR SID, 6526A CIA chips, a Commodore 1541 disk drive and a Commodore 1530 (C2N) Datassette.",
	"Rather than emulating multiple hardware revisions or adding convenience features, Breadbin466 focuses on faithfully reproducing this single reference machine in order to achieve the broadest possible software compatibility while remaining true to the original hardware.",
];
/* Main display geometry separates the emulated CRT area from the optional host OSD extension. */
pub(crate) const BUFFER_SCALE: usize = 2;
pub const GUI_HEIGHT: usize = 12;
pub const CRT_WIDTH: usize = 416;
pub const CRT_HEIGHT: usize = 288;
pub(crate) const CRT_OFFSET_X_NATIVE: usize = 52;
pub(crate) const CRT_OFFSET_Y_NATIVE: usize = 7;
pub const INITIAL_WINDOW_SCALE: f64 = 2.0;
#[cfg(target_os = "linux")]
pub(crate) const ICON_CANDIDATES: [&str; 3] = ["breadbin466", "applications-games", "computer"];
/* The presentation shader performs a direct sampled copy; all emulated video composition has already happened in the CPU framebuffer. */
pub(crate) const SHADER_SRC: &str = r#"
struct VertexOut {
	@builtin(position) pos: vec4<f32>,
	@location(0)       uv:  vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VertexOut {
	var positions = array<vec2<f32>, 6>(
		vec2<f32>(-1.0, -1.0),
		vec2<f32>( 1.0, -1.0),
		vec2<f32>(-1.0,  1.0),
		vec2<f32>(-1.0,  1.0),
		vec2<f32>( 1.0, -1.0),
		vec2<f32>( 1.0,  1.0),
	);
	var uvs = array<vec2<f32>, 6>(
		vec2<f32>(0.0, 1.0),
		vec2<f32>(1.0, 1.0),
		vec2<f32>(0.0, 0.0),
		vec2<f32>(0.0, 0.0),
		vec2<f32>(1.0, 1.0),
		vec2<f32>(1.0, 0.0),
	);
	var out: VertexOut;
	out.pos = vec4<f32>(positions[vi], 0.0, 1.0);
	out.uv  = uvs[vi];
	return out;
}

@group(0) @binding(0) var t_screen: texture_2d<f32>;
@group(0) @binding(1) var s_screen: sampler;

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
	return textureSample(t_screen, s_screen, in.uv);
}
"#;