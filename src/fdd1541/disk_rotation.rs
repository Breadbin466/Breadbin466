// =======================================================
// src/fdd1541/disk_rotation.rs — Disk angular position and track timing
// =======================================================

/* Rotation is tracked as a circular byte-and-bit position. Density controls when that position advances, while the stored track length defines the revolution boundary. */
impl DiskMechanism {
	#[inline(always)]
	/* Advancing past the final bit wraps to the first bit without resetting decoder state, matching continuous spindle rotation. */
	fn advance_track_position(&mut self, track_length: usize) {
		self.bit_pos += 1;
		if self.bit_pos >= 8 {
			self.bit_pos = 0;
			self.byte_pos += 1;
			if self.byte_pos >= track_length {
				self.byte_pos = 0;
			}
		}
	}
	#[inline(always)]
	/* Look-ahead is side-effect free so variable-speed tracks can choose the next cell timing before committing angular movement. */
	fn next_track_position(&self, track_length: usize) -> (usize, u8) {
		let mut byte_pos = self.byte_pos;
		let mut bit_pos = self.bit_pos + 1;
		if bit_pos >= 8 {
			bit_pos = 0;
			byte_pos += 1;
			if byte_pos >= track_length {
				byte_pos = 0;
			}
		}
		(byte_pos, bit_pos)
	}
	#[inline(always)]
	/* G64 speed tables describe the density of the byte about to be read, not merely the byte currently under the head. */
	fn playback_density_for_next_bit(&self, track_index: usize) -> Option<u8> {
		let track_length = self.tracks.get(track_index)?.len();
		if track_length == 0 {
			return None;
		}
		let (byte_pos, _) = self.next_track_position(track_length);
		self.track_speed.get(track_index)?.density_for_byte(byte_pos)
	}
	#[inline(always)]
	/* An unformatted or missing half-track still rotates mechanically. The synthetic track length preserves angular continuity while the read channel supplies instability separately. */
	fn advance_empty_track_rotation(&mut self) {
		if self.phase_track_length == 0 {
			return;
		}

		let track_bits = self.phase_track_length as u64 * 8;
		self.rotation_numerator = self.rotation_numerator.wrapping_add(track_bits);
		while self.rotation_numerator >= DRIVE_MASTER_CYCLES_PER_ROTATION {
			self.rotation_numerator -= DRIVE_MASTER_CYCLES_PER_ROTATION;
			self.advance_track_position(self.phase_track_length);
		}
	}
}