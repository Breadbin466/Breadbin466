// =======================================================
// src/emulator.rs — Emulator Top-Level Module Declarations
// =======================================================

/* The emulator layer owns host lifecycle, construction, commands and frame orchestration; hardware state itself remains in AppContext and Motherboard. */

pub(crate) mod constants;
pub mod command_line;
pub mod context;
pub mod builder;
pub mod timing;
pub mod debugger;
pub mod debugger_breakpoint;
pub mod debugger_command;
pub mod debugger_disassembly;
pub mod debugger_history;
mod commands;
pub mod orchestrator;
mod session;
mod snapshot;
pub mod application;

pub use orchestrator::Orchestrator;
pub use application::Breadbin;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;