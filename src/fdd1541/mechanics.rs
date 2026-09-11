// =======================================================
// src/fdd1541/mechanics.rs — DiskMechanism head position and stepper motor mechanics
// =======================================================

use super::constants::{DRIVE_MASTER_CYCLES_PER_ROTATION, G64_MAX_HALF_TRACKS, HEAD_SETTLING_CYCLES};
use super::disk_drive::DiskMechanism;
use super::media::TrackSpeed;

impl DiskMechanism {
	/* Coil commands change the field immediately, while the carriage follows
		on a millisecond time scale. A new pattern supersedes an unsettled
		command; a rapid round trip of the field need not move the head.
		Spindle rotation continues independently throughout this response.
		(1541-HEAD-MOTION-OBSERVATIONS) */
	pub(super) fn clock_stepper(&mut self, motor: bool, phase: u8) {
		if phase != self.phase_output {
			self.phase_output = phase;
			if self.motor_latched {
				self.head_settling_cycles = HEAD_SETTLING_CYCLES;
			}
		}
		if self.head_settling_cycles != 0 {
			self.head_settling_cycles -= 1;
			if self.head_settling_cycles == 0 && motor {
				self.update_stepper(self.phase_output);
			}
		}
		self.motor_latched = motor;
	}

	/* Refreshing the cached track view is the boundary between stepper motion and rotation. A head move selects a new half-track, then this helper snapshots the circular byte length and whether that track carries a per-byte G64 speed map. */
	pub(super) fn refresh_phase_track_metadata(&mut self) {
		let track_index = self.track_index();
		let length = self.tracks.get(track_index).map(Vec::len).unwrap_or(0);
		let variable_speed = matches!(self.track_speed.get(track_index), Some(TrackSpeed::PerByte(_)));
		/* A radial movement changes the number of recorded cells, not the
		spindle angle. Retain the coordinate scale on blank half-tracks and
		convert the position when a different circumference is selected. */
		if length != 0 && self.position_track_length != 0 && length != self.position_track_length {
			let old_bits = self.position_track_length as u128 * 8;
			let new_bits = length as u128 * 8;
			let position = (self.byte_pos as u128 * 8 + u128::from(self.bit_pos)) % old_bits;
			let period = u128::from(DRIVE_MASTER_CYCLES_PER_ROTATION);
			let fraction = if !self.phase_track_variable_speed && !variable_speed {
				u128::from(self.rotation_numerator)
			} else {
				0
			};
			let scaled = (position * period + fraction) * new_bits / old_bits;
			let bit_position = (scaled / period) as usize;
			self.byte_pos = bit_position / 8;
			self.bit_pos = (bit_position % 8) as u8;
			if !self.phase_track_variable_speed && !variable_speed {
				self.rotation_numerator = (scaled % period) as u64;
			}
		}
		if length != 0 {
			self.position_track_length = length;
		}
		self.phase_track_length = length;
		self.phase_track_variable_speed = variable_speed;
	}
	/* Internal half-tracks start at physical half-track 2, which represents DOS track 1. Subtracting two converts that hardware numbering into the zero-based vector index used by the mounted media. */
	pub(super) fn track_index(&self) -> usize {
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
	pub(super) fn update_stepper(&mut self, phase: u8) {
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
	}
}