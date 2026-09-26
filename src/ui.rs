// =======================================================
// src/ui.rs — User interface subsystem façade
// =======================================================

/* The UI façade assembles platform-neutral rendering, input and application state with native menu, About and shell backends selected at compile time. */

pub mod about;
#[cfg(target_os = "linux")]
pub mod about_linux;
#[cfg(target_os = "macos")]
pub mod about_macos;
#[cfg(target_os = "windows")]
pub mod about_windows;
pub mod audio;
mod audio_resampler;
pub(crate) mod constants;
#[cfg(target_os = "linux")]
pub mod disk_dialog_linux;
#[cfg(target_os = "macos")]
pub mod disk_dialog_macos;
#[cfg(target_os = "windows")]
pub mod disk_dialog_windows;
pub mod history;
mod history_persistence;
pub mod input;
pub mod inspector;
mod inspector_content;
#[cfg(target_os = "linux")]
mod inspector_linux;
#[cfg(target_os = "macos")]
mod inspector_macos;
#[cfg(target_os = "windows")]
mod inspector_windows;
pub mod joystick;
pub mod keyboard;
pub mod menu;
pub mod menu_actions;
#[cfg(target_os = "linux")]
pub mod menu_linux;
#[cfg(target_os = "macos")]
pub mod menu_macos;
#[cfg(target_os = "windows")]
pub mod menu_windows;
pub mod mouse;
pub mod osd;
mod graphics;
mod presentation;
pub mod renderer;
pub mod routing;
pub mod shell;
#[cfg(target_os = "linux")]
pub mod shell_linux;
#[cfg(target_os = "macos")]
pub mod shell_macos;
#[cfg(target_os = "windows")]
pub mod shell_windows;
pub mod wav;
#[cfg(any(target_os = "windows", target_os = "linux"))]
pub mod window_icon;

pub use audio::AudioHost;
pub use history::History;
pub use input::InputState;
pub use inspector::InspectorWindow;
pub use joystick::JoystickHost;
pub use menu::MenuManager;
pub use mouse::MouseHost;
pub use osd::OsdMonitor;
pub use renderer::Renderer;
pub use wav::WavRecorder;

pub use crate::emulator::Result;