// =======================================================
// src/ui/mouse.rs — Host pointer routing for the 1351
// =======================================================

use crate::ui::constants::{CRT_HEIGHT, CRT_WIDTH, GUI_HEIGHT};

/* MouseHost converts the absolute desktop pointer into scale-independent C64
 * display deltas. Cursor visibility remains an Orchestrator policy so this host
 * mapping layer stays independent from window management. */
pub struct MouseHost {
	last_display_position: Option<(f64, f64)>,
	fraction_x: f64,
	fraction_y: f64,
	over_display: bool,
}

impl MouseHost {
	pub fn new() -> Self {
		Self {
			last_display_position: None,
			fraction_x: 0.0,
			fraction_y: 0.0,
			over_display: false,
		}
	}

	pub fn over_display(&self) -> bool {
		self.over_display
	}

	pub fn reset_tracking(&mut self) {
		self.reset_motion_baseline();
		self.over_display = false;
	}

	/* Recentring the hidden host pointer must never become synthetic 1351
	 * movement. Clearing only the motion baseline preserves the current
	 * in-display state while making the following warp event establish a
	 * fresh relative origin. */
	pub fn reset_motion_baseline(&mut self) {
		self.last_display_position = None;
		self.fraction_x = 0.0;
		self.fraction_y = 0.0;
	}

	/* The renderer stretches one fixed-aspect presentation surface across the host
	 * window. Mapping through native C64 dimensions makes sensitivity independent
	 * from 1x/2x/3x window size and from HiDPI backing scale. */
	pub fn cursor_moved(
		&mut self,
		x: f64,
		y: f64,
		window_width: u32,
		window_height: u32,
		osd_enabled: bool,
	) -> Option<(i32, i32)> {
		if window_width == 0 || window_height == 0 {
			self.reset_tracking();
			return None;
		}

		let presentation_height = if osd_enabled {
			CRT_HEIGHT + GUI_HEIGHT
		} else {
			CRT_HEIGHT
		};
		let source_x = x * CRT_WIDTH as f64 / f64::from(window_width);
		let source_y = y * presentation_height as f64 / f64::from(window_height);
		let inside = source_x >= 0.0
			&& source_x < CRT_WIDTH as f64
			&& source_y >= 0.0
			&& source_y < CRT_HEIGHT as f64;

		self.over_display = inside;
		if !inside {
			self.last_display_position = None;
			self.fraction_x = 0.0;
			self.fraction_y = 0.0;
			return None;
		}

		let Some((previous_x, previous_y)) =
			self.last_display_position.replace((source_x, source_y))
		else {
			return None;
		};

		self.fraction_x += source_x - previous_x;
		self.fraction_y += source_y - previous_y;
		let dx = self.fraction_x.trunc() as i32;
		let dy = self.fraction_y.trunc() as i32;
		self.fraction_x -= f64::from(dx);
		self.fraction_y -= f64::from(dy);

		(dx != 0 || dy != 0).then_some((dx, dy))
	}
}

impl Default for MouseHost {
	fn default() -> Self {
		Self::new()
	}
}