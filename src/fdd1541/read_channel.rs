// =======================================================
// src/fdd1541/read_channel.rs — Magnetic read/write channel and bit-cell clock
// =======================================================

use super::constants::NOMINAL_TRACK_BYTES;
use super::disk_drive::DiskMechanism;
use super::media::{DiskFormat, TrackSpeed};

impl DiskMechanism {
	#[inline(always)]
	pub(super) fn read_track_bit(&mut self, track_index: usize) -> bool {
		let track_length = self.tracks[track_index].len();
		if track_length == 0 {
			return false;
		}
		if self.byte_pos >= track_length {
			self.byte_pos = 0;
		}
		self.advance_track_position(track_length);
		let bit = (self.tracks[track_index][self.byte_pos] >> (7 - self.bit_pos)) & 1;
		bit != 0
	}
	pub(super) fn initialise_empty_g64_track(&mut self, track_index: usize, density: u8) -> bool {
		if self.format != Some(DiskFormat::G64) || track_index >= self.tracks.len() {
			return false;
		}

		let nominal_length = NOMINAL_TRACK_BYTES[density as usize];
		let track_length = nominal_length.min(self.max_track_size_g64);
		if track_length == 0 {
			return false;
		}

		let bit_length = track_length.saturating_mul(8);
		let bit_position = self
			.byte_pos
			.saturating_mul(8)
			.saturating_add(self.bit_pos as usize)
			% bit_length;
		self.tracks[track_index] = vec![0; track_length];
		if let Some(speed) = self.track_speed.get_mut(track_index) {
			*speed = TrackSpeed::Constant(density);
		}
		self.byte_pos = bit_position / 8;
		self.bit_pos = (bit_position % 8) as u8;
		self.phase_track_length = track_length;
		true
	}
	#[inline(always)]
	fn write_track_bit(&mut self, track_index: usize, bit: bool, density: u8) {
		let track_length = self.tracks[track_index].len();
		if track_length == 0 {
			return;
		}
		if self.byte_pos >= track_length {
			self.byte_pos = 0;
		}
		self.advance_track_position(track_length);
		if self.format == Some(DiskFormat::G64) {
			if let Some(speed) = self.track_speed.get_mut(track_index) {
				speed.set_density_for_byte(self.byte_pos, track_length, density);
			}
		}
		let mask = 1u8 << (7 - self.bit_pos);
		if bit {
			self.tracks[track_index][self.byte_pos] |= mask;
		} else {
			self.tracks[track_index][self.byte_pos] &= !mask;
		}
		self.current_generation = self.current_generation.wrapping_add(1);
		self.dirty = true;
		if let Some(track_dirty) = self.dirty_tracks.get_mut(track_index) {
			*track_dirty = true;
		}
		if let Some(anchor) = self.dirty_track_anchor_bits.get_mut(track_index) {
			*anchor = self.byte_pos * 8 + usize::from(self.bit_pos);
		}
		self.flush_retry_deferred = false;
		self.write_pending = true;
	}
	#[inline(always)]
	pub(super) fn restart_bit_cell_clock(&mut self, density: u8) {
		self.bit_cell_divider = density & 0x03;
		self.decoder_phase = 0;
	}
	#[inline(always)]
	/* The fractional bit-cell phase allows different density zones to share one drive clock while retaining continuous angular motion across density changes. */
	fn advance_bit_cell_phase(
		&mut self,
		track_index: usize,
		density: u8,
		write_mode: bool,
		write_byte: u8,
	) {
		self.decoder_phase = self.decoder_phase.wrapping_add(1) & 0x0F;
		if (self.decoder_phase & 0x03) == 2 {
			self.read_shift_register <<= 1;
			if self.decoder_phase == 2 {
				self.read_shift_register |= 1;
			}
			if write_mode {
				let bit = (self.write_read_shift_register & 0x80) != 0;
				self.write_track_bit(track_index, bit, density);
				self.sync_active = false;
				self.byte_bit_count = self.byte_bit_count.wrapping_add(1);
			} else if (self.read_shift_register & 0x03FF) == 0x03FF {
				self.sync_active = true;
				self.byte_bit_count = 0;
			} else {
				self.sync_active = false;
				self.byte_bit_count = self.byte_bit_count.wrapping_add(1);
			}
			self.write_read_shift_register <<= 1;
		} else if (self.decoder_phase & 0x02) == 0 && self.byte_bit_count == 8 {
			self.byte_bit_count = 0;
			self.byte_ready = true;
			if write_mode {
				self.write_read_shift_register = write_byte;
			} else {
				self.write_read_shift_register = self.read_shift_register as u8;
				self.latched = self.write_read_shift_register;
			}
		}
	}
	#[inline(always)]
	/* A bit-cell clock first samples or writes the circular track, then advances sync and byte assembly state from that physical bit. */
	pub(super) fn clock_bit_cell(
		&mut self,
		track_index: usize,
		density: u8,
		write_mode: bool,
		write_byte: u8,
	) {
		self.bit_cell_divider = self.bit_cell_divider.wrapping_add(1);
		if self.bit_cell_divider != 0x10 {
			return;
		}
		self.bit_cell_divider = density & 0x03;
		self.advance_bit_cell_phase(track_index, density, write_mode, write_byte);
	}
	#[inline(always)]
	/* Large elapsed spans are decomposed into individual cells so sync marks and byte-ready pulses cannot be skipped. */
	pub(super) fn clock_bit_cell_span(
		&mut self,
		track_index: usize,
		density: u8,
		write_mode: bool,
		write_byte: u8,
		mut master_cycles: u8,
	) {
		while master_cycles > 0 {
			let until_event = 0x10u8.saturating_sub(self.bit_cell_divider);
			if master_cycles < until_event {
				self.bit_cell_divider = self.bit_cell_divider.wrapping_add(master_cycles);
				return;
			}
			master_cycles -= until_event;
			self.bit_cell_divider = density & 0x03;
			self.advance_bit_cell_phase(track_index, density, write_mode, write_byte);
		}
	}
}