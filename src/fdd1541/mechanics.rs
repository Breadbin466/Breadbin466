// =======================================================
// src/fdd1541/mechanics.rs — DiskMechanism head position and stepper motor mechanics
// =======================================================

impl DiskMechanism {
	/* Refreshing the cached track view is the boundary between stepper motion and rotation. A head move selects a new half-track, then this helper snapshots the circular byte length and whether that track carries a per-byte G64 speed map. */
	fn refresh_phase_track_metadata(&mut self) {
		let track_index = self.track_index();
		self.phase_track_length = self.tracks.get(track_index).map(Vec::len).unwrap_or(0);
		self.phase_track_variable_speed = matches!(
			self.track_speed.get(track_index),
			Some(TrackSpeed::PerByte(_))
		);
	}
	/* Internal half-tracks start at physical half-track 2, which represents DOS track 1. Subtracting two converts that hardware numbering into the zero-based vector index used by the mounted media. */
	fn track_index(&self) -> usize {
		self.half_track.saturating_sub(2) as usize
	}
	/* The public track number intentionally hides the half-track phase used internally by the stepper. */
	pub fn current_track(&self) -> Option<u8> {
		if !self.disk_present || self.tracks.is_empty() {
			return None;
		}
		let track = (self.half_track / 2).max(1);
		Some(track)
	}
	/* Adjacent phase patterns move the head by one half-track. Reversing the sequence moves inward, while skipped or repeated phases leave the head in place. */
	fn update_stepper(&mut self, phase: u8) {
		let old_index = self.track_index();
		let diff = phase.wrapping_sub(self.prev_phase) & 0x03;
		if diff == 1 {
			let maximum = G64_MAX_HALF_TRACKS as u8 + 1;
			if self.half_track < maximum {
				self.half_track += 1;
			}
		} else if diff == 3 && self.half_track > 2 {
			self.half_track -= 1;
		}
		self.prev_phase = phase;

		let new_index = self.track_index();
		if old_index != new_index {
			self.refresh_phase_track_metadata();
		}
		let new_length = self.phase_track_length;
		if old_index != new_index && new_length != 0 {
			let new_bit_length = new_length.saturating_mul(8);
			let bit_position = self
				.byte_pos
				.saturating_mul(8)
				.saturating_add(self.bit_pos as usize)
				% new_bit_length;
			self.byte_pos = bit_position / 8;
			self.bit_pos = (bit_position % 8) as u8;
		}
	}
}