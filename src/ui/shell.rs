// =======================================================
// src/ui/shell.rs — Platform shell façade
// =======================================================

/* This façade selects exactly one platform shell so the emulator can create and manage its host window through a uniform contract. */

#[cfg(target_os = "macos")]
pub use super::shell_macos::Shell;

#[cfg(target_os = "windows")]
pub use super::shell_windows::Shell;

#[cfg(target_os = "linux")]
pub use super::shell_linux::Shell;