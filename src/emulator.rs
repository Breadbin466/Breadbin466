// =======================================================
// src/emulator.rs — Emulator Top-Level Module Declarations
// =======================================================

/* The emulator layer owns host lifecycle, construction, commands and frame orchestration; hardware state itself remains in AppContext and Motherboard. */

pub mod application;
pub mod builder;
pub mod command_line;
mod commands;
pub(crate) mod constants;
pub mod context;
pub mod debugger;
pub mod debugger_breakpoint;
pub mod debugger_command;
pub mod debugger_disassembly;
pub mod debugger_history;
pub mod debugger_trace;
pub mod orchestrator;
mod session;
mod snapshot;
pub mod timing;

pub use application::Breadbin;
pub use orchestrator::Orchestrator;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;