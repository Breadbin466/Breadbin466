// =======================================================
// src/clockchip/signals.rs — System Control Bus State
// =======================================================

/* Control lines are represented by their electrical level rather than by a logical assertion flag. This keeps active-low signals readable at subsystem boundaries. */
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineLevel {
	Low,
	High,
}

impl LineLevel {
	pub fn is_active(self) -> bool {
		self == LineLevel::Low
	}
}