// =======================================================
// src/ui/renderer.rs — Main-window presentation interface
// =======================================================

#[cfg(target_os = "linux")]
use super::shell::Shell;
use super::{graphics::GpuRenderer, presentation::Presentation};
use crate::emulator::Result;
use crate::ui::constants::BUFFER_SCALE;
pub use crate::ui::constants::{CRT_HEIGHT, CRT_WIDTH, GUI_HEIGHT, INITIAL_WINDOW_SCALE};
use std::sync::Arc;
#[cfg(not(target_os = "linux"))]
use winit::dpi::LogicalSize;
use winit::window::Window;

/* The event thread publishes completed images through a bounded mailbox.
 * GPU back-pressure cannot postpone the next emulated PAL frame. */
pub struct Renderer {
	presentation: Presentation,
	pub osd_enabled: bool,
	pub src_width: usize,
	pub src_height: usize,
	#[cfg(not(target_os = "linux"))]
	window_ref: Arc<Window>,
}

impl Renderer {
	pub fn new(window: &Arc<Window>) -> Result<Self> {
		let gpu = GpuRenderer::new(window)?;
		let src_width = gpu.src_width;
		let src_height = gpu.src_height;
		Ok(Self {
			presentation: Presentation::new(gpu)?,
			osd_enabled: true,
			src_width,
			src_height,
			#[cfg(not(target_os = "linux"))]
			window_ref: window.clone(),
		})
	}

	pub fn set_osd_enabled(&mut self, enabled: bool) {
		self.osd_enabled = enabled;
		self.src_height = (CRT_HEIGHT + if enabled { GUI_HEIGHT } else { 0 }) * BUFFER_SCALE;
	}

	pub fn draw(&mut self, vic_buffer: &[u8], osd: super::osd::OsdData) -> Result<()> {
		self.presentation.publish(vic_buffer, osd, self.osd_enabled)
	}

	pub fn resize_window_to_fit(&self, scale: f64) {
		let w = CRT_WIDTH as f64 * scale;
		let mut h = CRT_HEIGHT as f64 * scale;
		if self.osd_enabled {
			h += GUI_HEIGHT as f64 * scale;
		}
		#[cfg(target_os = "linux")]
		Shell::resize_content(w.round() as u32, h.round() as u32);
		#[cfg(not(target_os = "linux"))]
		let _ = self.window_ref.request_inner_size(LogicalSize::new(w, h));
	}

	/* The host window must retain the aspect ratio of the complete presentation surface. The OSD is part of that surface when enabled, so toggling it deliberately changes the locked ratio rather than stretching either the emulated picture or the status bar. Two candidate sizes are considered and the one requiring the smaller correction is chosen; this preserves the edge the user is most clearly dragging and avoids a width-biased resize policy. */
	pub fn constrain_resize(&self, width: u32, height: u32) -> Option<(u32, u32)> {
		if width == 0 || height == 0 {
			return None;
		}

		let target_width = CRT_WIDTH as f64;
		let mut target_height = CRT_HEIGHT as f64;
		if self.osd_enabled {
			target_height += GUI_HEIGHT as f64;
		}
		let aspect_ratio = target_width / target_height;

		let height_from_width = (width as f64 / aspect_ratio).round().max(1.0) as u32;
		let width_from_height = (height as f64 * aspect_ratio).round().max(1.0) as u32;
		let height_correction = height.abs_diff(height_from_width);
		let width_correction = width.abs_diff(width_from_height);

		let constrained = if height_correction <= width_correction {
			(width, height_from_width)
		} else {
			(width_from_height, height)
		};

		/* Integer pixel sizes cannot always represent the ratio exactly. A one-pixel discrepancy is therefore accepted to prevent resize feedback loops at dimensions whose exact counterpart lies between two pixels. */
		if width.abs_diff(constrained.0) <= 1 && height.abs_diff(constrained.1) <= 1 {
			None
		} else {
			Some(constrained)
		}
	}

	/* The View/Scale entries describe three exact native presentation sizes rather than a continuously rounded zoom value. A manually resized window therefore matches a menu entry only when both physical dimensions are exactly those produced by that integer scale on the current host DPI. */
	pub fn integer_scale_for_window_size(&self, width: u32, height: u32) -> Option<u8> {
		let logical_height = if self.osd_enabled {
			(CRT_HEIGHT + GUI_HEIGHT) as f64
		} else {
			CRT_HEIGHT as f64
		};

		for integer_scale in [1u8, 2, 3] {
			let scale = f64::from(integer_scale);
			#[cfg(target_os = "linux")]
			let expected = (
				(CRT_WIDTH as f64 * scale).round() as u32,
				(logical_height * scale).round() as u32,
			);
			#[cfg(not(target_os = "linux"))]
			let expected = {
				let dpi_scale = self.window_ref.scale_factor();
				(
					(CRT_WIDTH as f64 * scale * dpi_scale).round() as u32,
					(logical_height * scale * dpi_scale).round() as u32,
				)
			};

			if (width, height) == expected {
				return Some(integer_scale);
			}
		}

		None
	}

	/* Resizing updates only presentation geometry. The returned scale factor lets later window operations preserve the user's effective zoom even when it no longer corresponds to one of the three exact View/Scale presets. */
	pub fn handle_resize(&mut self, width: u32, height: u32) -> f64 {
		if width == 0 || height == 0 {
			return 0.0;
		}

		let ratio_w = CRT_WIDTH as f64;
		let mut ratio_h = CRT_HEIGHT as f64;
		if self.osd_enabled {
			ratio_h += GUI_HEIGHT as f64;
		}

		#[cfg(target_os = "linux")]
		let (logical_width, logical_height) = (width as f64, height as f64);
		#[cfg(not(target_os = "linux"))]
		let (logical_width, logical_height) = {
			let dpi_scale = self.window_ref.scale_factor();
			(width as f64 / dpi_scale, height as f64 / dpi_scale)
		};
		let scale = (logical_width / ratio_w).min(logical_height / ratio_h);

		self.presentation.resize(width, height);

		scale
	}

}