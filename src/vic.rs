// =======================================================
// src/vic.rs — VIC-II graphics subsystem façade
// =======================================================

/*
The VIC-II subsystem is split by hardware responsibility rather than by rendering convenience. Timing and bus modules decide which memory access occurs in each raster cycle; foreground, border and sprite modules transform the resulting latches into pixels; register and state modules expose the chip-visible control surface and preserve the internal pipelines that connect one cycle to the next. The separation keeps raster timing auditable without making the renderer the owner of hardware state.
*/
pub mod constants;
pub mod registers;
pub mod timing;
pub mod border;
pub mod screen;
pub mod telemetry;
pub mod state;
pub mod fsm;

pub mod bus_access;
pub mod fetch;
pub mod memory_cycle;
pub mod raster_line;
pub mod sequencer;

pub mod graphics_pixels;
pub mod graphics_mask;
pub mod graphics_mode_transition;
pub mod foreground;
pub mod foreground_renderer;
pub mod foreground_transition;
pub mod border_renderer;

pub mod sprite;
pub mod sprite_pixels;
pub mod sprite_dma;
pub mod sprite_unit;
pub mod sprite_timing;
pub mod sprite_display;

pub mod io;
pub mod vertical_scroll;
pub mod horizontal_scroll;
pub mod horizontal_transition;
pub mod horizontal_standard;
pub mod horizontal_multicolour;

pub use state::VicII;