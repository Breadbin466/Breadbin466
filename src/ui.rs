// =======================================================
// src/ui.rs — User interface subsystem façade
// =======================================================

/* The UI façade assembles platform-neutral rendering, input and application state with native menu, About and shell backends selected at compile time. */

pub(crate) mod constants;
pub mod audio;
pub mod renderer;
pub mod input;
pub mod keyboard;
pub mod joystick;
pub mod menu;
#[cfg(target_os = "macos")]
pub mod menu_macos;
#[cfg(target_os = "windows")]
pub mod menu_windows;
#[cfg(target_os = "linux")]
pub mod menu_linux;
pub mod about;
#[cfg(target_os = "macos")]
pub mod about_macos;
#[cfg(target_os = "windows")]
pub mod about_windows;
#[cfg(target_os = "linux")]
pub mod about_linux;
pub mod shell;
#[cfg(target_os = "macos")]
pub mod shell_macos;
#[cfg(target_os = "windows")]
pub mod shell_windows;
#[cfg(target_os = "linux")]
pub mod shell_linux;
pub mod history;
pub mod osd;
pub mod routing;
pub mod menu_actions;
pub mod inspector;

pub use input::InputState;
pub use audio::AudioHost;
pub use joystick::JoystickHost;
pub use renderer::Renderer;
pub use menu::MenuManager;
pub use history::History;
pub use osd::OsdMonitor;
pub use inspector::InspectorWindow;

pub use crate::emulator::Result;