// =======================================================
// src/ui/inspector.rs — Native Inspector lifecycle and refresh
// =======================================================

#[cfg(target_os = "macos")]
use super::inspector_macos as platform;
#[cfg(target_os = "windows")]
use super::inspector_windows as platform;
#[cfg(target_os = "linux")]
use super::inspector_linux as platform;
pub(crate) use super::inspector_content::{InspectorSection, InspectorSnapshot};

use crate::emulator::Result;
use std::time::{Duration, Instant};
use winit::window::Window;

/* Native toolkits own Inspector layout, text, scrolling and repainting.
 * Only detached state crosses this boundary, at most four times a second.
 * Opening the Inspector creates no GPU surface or presentation worker. */
pub struct InspectorWindow {
	native: platform::NativeInspector,
	last_refresh: Option<Instant>,
}

impl InspectorWindow {
	pub fn new(parent: &Window) -> Result<Self> {
		Ok(Self {
			native: platform::NativeInspector::new(parent)?,
			last_refresh: None,
		})
	}
	pub fn show(&self) {
		self.native.show();
	}
	pub fn is_open(&self) -> bool {
		self.native.is_open()
	}
	pub(crate) fn needs_snapshot(&self) -> bool {
		self.native.is_visible()
			&& self
				.last_refresh
				.is_none_or(|t| t.elapsed() >= Duration::from_millis(250))
	}
	pub fn next_refresh_time(&self) -> Option<Instant> {
		if !self.is_open() {
			return None;
		}
		/* GTK shares the host loop and needs regular event pumping while the
		 * machine is paused. State capture still observes the 250 ms limit. */
		#[cfg(target_os = "linux")]
		return Some(Instant::now() + Duration::from_millis(16));
		#[cfg(not(target_os = "linux"))]
		Some(Instant::now() + Duration::from_millis(250))
	}
	pub(crate) fn set_snapshot(&mut self, snapshot: InspectorSnapshot) {
		self.native.update(&snapshot);
		self.last_refresh = Some(Instant::now());
	}
}