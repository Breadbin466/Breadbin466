// =======================================================
// src/datassette/deck.rs — Cassette transport and electrical signals
// =======================================================

use super::{
	constants::{MAX_TAPE_PULSES, PULSE_HOLD_CYCLES},
	image::TapeImage,
	odometre::Odometre,
};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TapeState {
	Idle,
	PulseActive,
}

/* A recording replaces transitions only over the tape that actually moved
 * under the head. New transitions are collected separately, allowing one
 * splice at the end instead of shifting the untouched tail on every edge. */
struct Recording {
	start: u64,
	edges: Vec<u64>,
	write_level: bool,
}

/* The 6510 controls motor power and WRITE; the mechanical PLAY/RECORD
 * latches control transport. READ presents the falling edges retained by
 * TAP to CIA1 FLAG. TAP does not contain analogue amplitude or duty cycle,
 * so the short FLAG hold is independent of distance between transitions. */
pub struct Datassette {
	image: Option<TapeImage>,
	tape_path: Option<PathBuf>,
	position: u64,
	cursor: usize,
	pub play_pressed: bool,
	pub record_pressed: bool,
	pub state: TapeState,
	pulse_hold: u8,
	recording: Option<Recording>,
	dirty: bool,
	flush_requested: bool,
	recording_error: Option<String>,
	odometre: Odometre,
}

impl Datassette {
	pub fn new() -> Self {
		Self {
			image: None,
			tape_path: None,
			position: 0,
			cursor: 0,
			play_pressed: false,
			record_pressed: false,
			state: TapeState::Idle,
			pulse_hold: 0,
			recording: None,
			dirty: false,
			flush_requested: false,
			recording_error: None,
			odometre: Odometre::new(),
		}
	}

	/* Invalid or unsupported input cannot disturb the currently mounted tape. */
	pub fn load_tap(&mut self, data: Vec<u8>, path: PathBuf) -> bool {
		let Some(image) = TapeImage::decode(&data) else {
			return false;
		};
		*self = Self::new();
		self.image = Some(image);
		self.tape_path = Some(path);
		true
	}

	pub fn has_tape(&self) -> bool {
		self.image.is_some()
	}
	pub fn get_path(&self) -> Option<&Path> {
		self.tape_path.as_deref()
	}
	pub fn get_odometre_value(&self) -> f64 {
		if self.has_tape() {
			self.odometre.calculate_value(self.position)
		} else {
			0.0
		}
	}

	/* Host transport navigation changes position without fabricating read
	 * pulses. Any recording already made is committed before the seek. */
	pub fn rewind(&mut self) {
		self.seek(0);
	}
	pub fn fast_forward(&mut self) {
		self.finish_recording();
		let end = self
			.image
			.as_ref()
			.and_then(|image| image.edges.last())
			.copied()
			.unwrap_or(0);
		self.seek(end);
	}
	fn seek(&mut self, position: u64) {
		self.finish_recording();
		self.position = position;
		self.cursor = self.image.as_ref().map_or(0, |image| {
			image.edges.partition_point(|&edge| edge <= position)
		});
		self.release_read();
	}
	fn release_read(&mut self) {
		self.state = TapeState::Idle;
		self.pulse_hold = 0;
	}

	/* One call represents one PHI2 cycle, in either normal or warp mode.
	 * Stopping the motor freezes tape position but releases the read pulse;
	 * restarting resumes the remaining interval rather than refetching it. */
	#[inline]
	pub fn tick(&mut self, motor_on: bool, write_level: bool) -> bool {
		if !self.has_tape() || !self.play_pressed || !motor_on {
			self.finish_recording();
			self.release_read();
			return true;
		}
		if self.record_pressed {
			if self.recording.is_none() {
				self.recording = Some(Recording {
					start: self.position,
					edges: Vec::new(),
					write_level,
				});
			}
			self.position += 1;
			let recording = self.recording.as_mut().unwrap();
			/* The 1530 read/write path inverts polarity: rising WRITE edges
			 * become falling READ edges (DATASETTE-SERVICE-1984). Sampling
			 * the initial level avoids inventing a motor-start edge. */
			if !recording.write_level && write_level {
				if recording.edges.len() >= MAX_TAPE_PULSES {
					self.recording_error = Some("Cassette recording reached the supported size; transport stopped without discarding recorded data".into());
					self.record_pressed = false;
					self.play_pressed = false;
					self.finish_recording();
				} else {
					recording.edges.push(self.position);
				}
			}
			if let Some(recording) = self.recording.as_mut() {
				recording.write_level = write_level;
			}
			self.release_read();
			return true;
		}
		self.finish_recording();
		self.position += 1;
		if self.pulse_hold > 0 {
			self.pulse_hold -= 1;
		}
		let image = self.image.as_ref().unwrap();
		if image
			.edges
			.get(self.cursor)
			.is_some_and(|&edge| edge <= self.position)
		{
			self.cursor += 1;
			self.pulse_hold = PULSE_HOLD_CYCLES;
		}
		self.state = if self.pulse_hold == 0 {
			TapeState::Idle
		} else {
			TapeState::PulseActive
		};
		self.state == TapeState::Idle
	}

	fn finish_recording(&mut self) {
		let Some(recording) = self.recording.take() else {
			return;
		};
		if self.position == recording.start {
			return;
		}
		if let Some(image) = self.image.as_mut() {
			let first = image.edges.partition_point(|&edge| edge <= recording.start);
			let end = image.edges.partition_point(|&edge| edge <= self.position);
			image.edges.splice(first..end, recording.edges);
			self.cursor = image.edges.partition_point(|&edge| edge <= self.position);
			self.dirty = true;
			self.flush_requested = true;
		}
	}

	pub fn take_recording_error(&mut self) -> Option<String> {
		self.recording_error.take()
	}

	/* Persistence is requested at transport or motor boundaries, never once
	 * per emulated frame. Unmodified tapes are not rewritten on ejection. */
	pub fn flush_if_requested(&mut self) -> std::io::Result<()> {
		if !self.flush_requested {
			return Ok(());
		}
		self.flush_requested = false;
		self.save_tape_to_host()
	}
	pub fn save_tape_to_host(&mut self) -> std::io::Result<()> {
		self.finish_recording();
		if !self.dirty {
			return Ok(());
		}
		let (Some(image), Some(path)) = (&self.image, &self.tape_path) else {
			return Ok(());
		};
		let data = image.encode()?;
		super::persistence::write(path, &data)?;
		self.dirty = false;
		self.flush_requested = false;
		Ok(())
	}
	pub fn eject(&mut self) -> std::io::Result<()> {
		self.save_tape_to_host()?;
		*self = Self::new();
		Ok(())
	}
}
impl Default for Datassette {
	fn default() -> Self {
		Self::new()
	}
}