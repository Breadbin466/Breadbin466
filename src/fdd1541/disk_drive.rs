// =======================================================
// src/fdd1541/disk_drive.rs — 1541 drive medium, mechanics and serial read channel
// =======================================================

use super::constants::{
	DISK_ABSENT_CYCLES, DISK_CHANGE_CYCLES, DISK_INSERTING_CYCLES,
	DRIVE_MASTER_CYCLES_PER_ROTATION, DRIVE_MASTER_PER_CPU, DRIVE_RESET_HALF_TRACK,
	G64_CELL_CYCLES, NOISE_PRNG_MASK, POST_FLUX_SETTLING_MIN, POST_FLUX_SETTLING_SPAN,
	SPURIOUS_FLUX_INTERVAL_MIN, SPURIOUS_FLUX_INTERVAL_SPAN,
};
use super::media::{DiskFormat, TrackSpeed};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::JoinHandle;

pub(super) static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(super) struct PendingG64Layout {
	pub(super) track_offsets: Vec<Option<usize>>,
	pub(super) speed_offsets: Vec<Option<usize>>,
	pub(super) max_track_size: usize,
}

pub(super) struct PendingWrite {
	pub(super) generation: u64,
	pub(super) handle: JoinHandle<bool>,
	pub(super) original_file: Vec<u8>,
	pub(super) g64_layout: Option<PendingG64Layout>,
}

/* DiskMechanism keeps rotational position, head position, read-channel state and writable track data in one timeline. Byte-ready and sync are consequences of flux-cell timing, not sector-level shortcuts. */
pub struct DiskMechanism {
	/* Circular GCR images and optional per-byte speed maps form the medium currently passing under the head. */
	pub(super) tracks: Vec<Vec<u8>>,
	pub(super) track_speed: Vec<TrackSpeed>,
	pub(super) num_tracks: u8,
	pub(super) disk_present: bool,
	pub(super) write_protect: bool,
	pub(super) dirty: bool,
	pub(super) dirty_tracks: Vec<bool>,
	pub(super) dirty_track_anchor_bits: Vec<usize>,
	pub(super) flush_retry_deferred: bool,

	/* Persistence metadata is kept separate from live rotational state so flushing cannot redefine what the head is currently seeing. */
	pub(super) mount_path: Option<PathBuf>,
	pub(super) original_file: Option<Vec<u8>>,
	pub(super) track_offsets_g64: Vec<Option<usize>>,
	pub(super) speed_offsets_g64: Vec<Option<usize>>,
	pub(super) max_track_size_g64: usize,
	pub(super) format: Option<DiskFormat>,
	pub(super) write_pending: bool,
	pub(super) pending_writer: Option<PendingWrite>,
	pub(super) current_generation: u64,
	pub(super) persisted_generation: AtomicU64,

	/* Mechanical state survives media replacement and advances independently of whether a formatted track exists at that position. */
	pub(super) half_track: u8,
	pub(super) prev_phase: u8,
	pub(super) phase_output: u8,
	pub(super) motor_latched: bool,
	pub(super) head_settling_cycles: u32,

	/* Decoder state follows the serial bit stream continuously; byte-ready is produced only after eight qualified bit cells. */
	pub(super) byte_pos: usize,
	pub(super) bit_pos: u8,
	pub(super) rotation_numerator: u64,
	pub(super) phase_track_length: usize,
	pub(super) position_track_length: usize,
	pub(super) phase_track_variable_speed: bool,
	pub(super) bit_cell_divider: u8,
	pub(super) decoder_phase: u8,
	pub(super) byte_bit_count: u8,

	pub(super) read_shift_register: u16,
	pub(super) sync_active: bool,
	pub(super) write_read_shift_register: u8,
	pub(super) was_writing: bool,

	pub(super) latched: u8,
	pub(super) byte_ready: bool,

	/* Disk-change timing and deterministic read-amplifier noise model the period in which no trustworthy flux is available. */
	pub(super) disk_change_cycles: u32,
	pub(super) next_spurious_flux_cycles: u32,
	pub(super) noise_prng_state: u32,
}

impl DiskMechanism {
	/* A new mechanism has no medium, starts at the reset half-track and seeds its analogue-noise sequence deterministically for reproducible emulation. */
	pub fn new() -> Self {
		Self {
			tracks: Vec::new(),
			track_speed: Vec::new(),
			num_tracks: 0,
			disk_present: false,
			write_protect: false,
			dirty: false,
			dirty_tracks: Vec::new(),
			dirty_track_anchor_bits: Vec::new(),
			flush_retry_deferred: false,
			mount_path: None,
			original_file: None,
			track_offsets_g64: Vec::new(),
			speed_offsets_g64: Vec::new(),
			max_track_size_g64: 0,
			format: None,
			write_pending: false,
			pending_writer: None,
			current_generation: 0,
			persisted_generation: AtomicU64::new(0),
			half_track: DRIVE_RESET_HALF_TRACK,
			prev_phase: 0,
			phase_output: 0,
			motor_latched: false,
			head_settling_cycles: 0,
			byte_pos: 0,
			bit_pos: 0,
			rotation_numerator: 0,
			phase_track_length: 0,
			position_track_length: 0,
			phase_track_variable_speed: false,
			bit_cell_divider: 3,
			decoder_phase: 0,
			byte_bit_count: 0,
			read_shift_register: 0,
			sync_active: false,
			write_read_shift_register: 0,
			was_writing: false,
			latched: 0,
			byte_ready: false,
			disk_change_cycles: 0,
			next_spurious_flux_cycles: POST_FLUX_SETTLING_MIN,
			noise_prng_state: 0x811C_9DC5,
		}
	}
	/* Replacing media preserves spindle phase and head position so disk insertion changes the medium without teleporting the mechanism. */
	pub(super) fn inherit_runtime_state(&mut self, previous: &Self) {
		self.half_track = previous.half_track;
		self.prev_phase = previous.prev_phase;
		self.phase_output = previous.phase_output;
		self.motor_latched = previous.motor_latched;
		self.head_settling_cycles = previous.head_settling_cycles;
		self.byte_pos = previous.byte_pos;
		self.bit_pos = previous.bit_pos;
		self.rotation_numerator = previous.rotation_numerator;
		self.position_track_length = previous.position_track_length;
		self.bit_cell_divider = previous.bit_cell_divider;
		self.decoder_phase = previous.decoder_phase;
		self.byte_bit_count = previous.byte_bit_count;
		self.read_shift_register = previous.read_shift_register;
		self.sync_active = previous.sync_active;
		self.write_read_shift_register = previous.write_read_shift_register;
		self.was_writing = previous.was_writing;
		self.latched = previous.latched;
		self.byte_ready = previous.byte_ready;
		self.next_spurious_flux_cycles = previous.next_spurious_flux_cycles;
		self.noise_prng_state = previous.noise_prng_state;
	}
	/* Track replacement may change the circular bit length; normalisation preserves the nearest equivalent angular position in the new image. */
	pub(super) fn normalise_runtime_position(&mut self) {
		self.refresh_phase_track_metadata();
		if self.phase_track_length == 0 {
			return;
		}

		let bit_length = self.phase_track_length.saturating_mul(8);
		let bit_position = self
			.byte_pos
			.saturating_mul(8)
			.saturating_add(self.bit_pos as usize)
			% bit_length;
		self.byte_pos = bit_position / 8;
		self.bit_pos = (bit_position % 8) as u8;
	}
	/* Decoder reset clears byte assembly and sync detection without moving the head or spindle to an artificial index position. */
	pub(super) fn reset_decoder(&mut self) {
		self.rotation_numerator = 0;
		self.refresh_phase_track_metadata();
		if self.phase_track_length != 0 {
			let bit_length = self.phase_track_length.saturating_mul(8);
			let bit_position = self
				.byte_pos
				.saturating_mul(8)
				.saturating_add(self.bit_pos as usize)
				% bit_length;
			self.byte_pos = bit_position / 8;
			self.bit_pos = (bit_position % 8) as u8;
		}
		self.bit_cell_divider = 3;
		self.decoder_phase = 0;
		self.byte_bit_count = 0;
		self.read_shift_register = 0;
		self.sync_active = false;
		self.write_read_shift_register = 0;
		self.was_writing = false;
		self.latched = 0;
		self.byte_ready = false;
		let next_spurious_flux_cycles =
			self.next_noise_interval(POST_FLUX_SETTLING_MIN, POST_FLUX_SETTLING_SPAN);
		self.next_spurious_flux_cycles = next_spurious_flux_cycles;
	}
	#[inline(always)]
	fn next_noise_interval(&mut self, minimum: u32, span: u32) -> u32 {
		self.noise_prng_state = self
			.noise_prng_state
			.wrapping_mul(1_103_515_245)
			.wrapping_add(12_345)
			& NOISE_PRNG_MASK;
		let range = span.max(1) as u64;
		minimum + (((self.noise_prng_state as u64 * range) >> 31) as u32)
	}
	#[inline(always)]
	fn schedule_post_flux_settling(&mut self) {
		let next_spurious_flux_cycles =
			self.next_noise_interval(POST_FLUX_SETTLING_MIN, POST_FLUX_SETTLING_SPAN);
		self.next_spurious_flux_cycles = next_spurious_flux_cycles;
	}
	#[inline(always)]
	/* With no trustworthy flux transition, the analogue read path eventually produces density-dependent noise rather than an endless deterministic zero stream. */
	fn clock_unstable_read_amplifier(&mut self, density: u8) {
		if self.next_spurious_flux_cycles > 0 {
			self.next_spurious_flux_cycles -= 1;
		}
		if self.next_spurious_flux_cycles == 0 {
			self.restart_bit_cell_clock(density);
			let next_spurious_flux_cycles =
				self.next_noise_interval(SPURIOUS_FLUX_INTERVAL_MIN, SPURIOUS_FLUX_INTERVAL_SPAN);
			self.next_spurious_flux_cycles = next_spurious_flux_cycles;
		}
	}
	/* Reset returns the stepper electronics and serial decoder to power-on state while keeping mounted media and unsaved modifications intact. */
	pub fn reset(&mut self) {
		self.half_track = DRIVE_RESET_HALF_TRACK;
		self.prev_phase = 0;
		self.phase_output = 0;
		self.motor_latched = false;
		self.head_settling_cycles = 0;
		self.disk_change_cycles = DISK_CHANGE_CYCLES;
		self.reset_decoder();
	}
}

impl DiskMechanism {
	/* An immediate flush first resolves any older writer so generations reach the host file in the same order they were produced. */
	pub fn flush_now(&mut self) -> bool {
		let previous_succeeded = self.finish_pending_writer(true);
		if !self.write_pending {
			return previous_succeeded;
		}
		self.flush_retry_deferred = false;
		self.flush_to_disk(false)
	}
	pub fn flush_pending(&self) -> bool {
		self.write_pending
	}
	/* Deferred flushing waits until the write gate is inactive. A completed background writer is reconciled on the drive thread before another generation can be launched. */
	pub fn service_flush(&mut self) -> bool {
		if self.pending_writer.is_some() {
			if !self.finish_pending_writer(false) {
				self.flush_retry_deferred = true;
				return false;
			}
			if self.pending_writer.is_some() {
				return true;
			}
		}
		if !self.write_pending || self.was_writing || self.flush_retry_deferred {
			return true;
		}
		let success = self.flush_to_disk(true);
		if !success {
			self.flush_retry_deferred = true;
		}
		success
	}
	/* Ending write mode closes the current burst before persistence, preserving the exact circular track image produced by firmware. */
	fn finish_write_burst(&mut self) {
		self.was_writing = false;
	}
	/* One mechanism step applies motor and stepper outputs, advances angular position at the selected density, and clocks either the read separator or write shifter. */
	pub fn step(&mut self, motor: bool, density: u8, phase: u8, write_mode: bool, write_byte: u8) {
		self.byte_ready = false;

		if self.disk_change_cycles > 0 {
			self.disk_change_cycles -= 1;
		}

		self.clock_stepper(motor, phase);

		if self.disk_change_cycles > 0 {
			self.sync_active = false;
			self.finish_write_burst();
			return;
		}

		if !self.disk_present || self.tracks.is_empty() || !motor {
			self.sync_active = false;
			self.finish_write_burst();
			return;
		}

		let track_index = self.track_index();
		if track_index >= self.tracks.len() {
			self.sync_active = false;
			self.finish_write_burst();
			return;
		}

		let selected_density = density & 0x03;
		let requested_write_mode = write_mode;
		if self.tracks[track_index].is_empty() && !requested_write_mode {
			self.finish_write_burst();
			for _ in 0..DRIVE_MASTER_PER_CPU {
				self.advance_empty_track_rotation();
				self.clock_unstable_read_amplifier(selected_density);
				self.clock_bit_cell(track_index, selected_density, false, write_byte);
			}
			return;
		}

		let effective_write_mode = requested_write_mode
			&& (!self.tracks[track_index].is_empty()
				|| self.initialise_empty_track(track_index, selected_density));
		if effective_write_mode != self.was_writing {
			self.sync_active = false;
			self.rotation_numerator = 0;
			if !effective_write_mode {
				self.finish_write_burst();
			}
		}
		if effective_write_mode {
			self.was_writing = true;
		}

		let variable_speed = self.phase_track_variable_speed;
		let constant_track_bits = self.phase_track_length as u64 * 8;

		if effective_write_mode {
			self.clock_bit_cell_span(
				track_index,
				selected_density,
				true,
				write_byte,
				DRIVE_MASTER_PER_CPU as u8,
			);
			return;
		}

		if !variable_speed && self.next_spurious_flux_cycles > DRIVE_MASTER_PER_CPU {
			let rotation_advance = constant_track_bits * u64::from(DRIVE_MASTER_PER_CPU);
			if self.rotation_numerator.wrapping_add(rotation_advance)
				< DRIVE_MASTER_CYCLES_PER_ROTATION
			{
				self.rotation_numerator = self.rotation_numerator.wrapping_add(rotation_advance);
				self.next_spurious_flux_cycles -= DRIVE_MASTER_PER_CPU;
				self.clock_bit_cell_span(
					track_index,
					selected_density,
					false,
					write_byte,
					DRIVE_MASTER_PER_CPU as u8,
				);
				return;
			}
		}

		for _ in 0..DRIVE_MASTER_PER_CPU {
			if !effective_write_mode {
				if variable_speed {
					if let Some(playback_density) = self.playback_density_for_next_bit(track_index)
					{
						self.rotation_numerator = self.rotation_numerator.wrapping_add(1);
						let cell_cycles = G64_CELL_CYCLES[playback_density as usize];
						if self.rotation_numerator >= cell_cycles {
							self.rotation_numerator -= cell_cycles;
							if self.read_track_bit(track_index) {
								self.restart_bit_cell_clock(selected_density);
								self.schedule_post_flux_settling();
							}
						}
					}
				} else {
					self.rotation_numerator =
						self.rotation_numerator.wrapping_add(constant_track_bits);
					while self.rotation_numerator >= DRIVE_MASTER_CYCLES_PER_ROTATION {
						self.rotation_numerator -= DRIVE_MASTER_CYCLES_PER_ROTATION;
						if self.read_track_bit(track_index) {
							self.restart_bit_cell_clock(selected_density);
							self.schedule_post_flux_settling();
						}
					}
				}
				self.clock_unstable_read_amplifier(selected_density);
			}
			self.clock_bit_cell(
				track_index,
				selected_density,
				effective_write_mode,
				write_byte,
			);
		}
	}
	pub fn head_byte(&self) -> u8 {
		self.latched
	}
	pub fn byte_ready(&self) -> bool {
		self.byte_ready
	}
	pub fn sync(&self) -> bool {
		self.sync_active
	}
	pub fn write_protect(&self) -> bool {
		if self.disk_change_cycles == 0 {
			return self.disk_present && self.write_protect;
		}

		let no_disk_threshold = DISK_INSERTING_CYCLES;
		let ejecting_threshold = DISK_ABSENT_CYCLES + DISK_INSERTING_CYCLES;
		if self.disk_change_cycles > ejecting_threshold {
			true
		} else if self.disk_change_cycles > no_disk_threshold {
			false
		} else {
			true
		}
	}
	pub fn disk_present(&self) -> bool {
		self.disk_present
	}
	pub fn is_dirty(&self) -> bool {
		self.dirty || self.persisted_generation.load(Ordering::Acquire) != self.current_generation
	}
}

impl Default for DiskMechanism {
	fn default() -> Self {
		Self::new()
	}
}