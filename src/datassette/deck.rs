// =======================================================
// src/datassette/deck.rs — Datassette hardware emulation and tape control
// =======================================================

use std::path::PathBuf;
use std::fs::File;
use std::io::Write;
use super::constants::{
	MAX_TAPE_SIZE,
	PULSE_HOLD_CYCLES,
	TAP_EXTENDED_PULSE_SIZE,
	TAP_HEADER_SIZE,
	TAP_MAX_EXTENDED_PULSE,
	TAP_MAX_SHORT_PULSE,
	TAP_SHORT_PULSE_SCALE,
	TAP_SIGNATURE,
};
use super::odometre::Odometre;

/* TapeState distinguishes the interval between flux transitions from the short active-low pulse currently presented to CIA1 FLAG. */
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TapeState {
	/* No read pulse is being held; the pulse interval counter may still be running. */
	Idle,
	/* A newly decoded transition is being held long enough for the CIA input to sample it. */
	PulseActive,
}

/* Datassette combines three distinct concerns: TAP byte-stream position, transport controls and the electrical read/write signals connected through the 6510 and CIA1. Tape motion advances only while PLAY is pressed and the computer energises the motor line. */
pub struct Datassette {
	/* Complete TAP image, including the original header and all pulse records. */
	pub tape_data:                 Option<Vec<u8>>,
	/* Byte position of the next pulse record in the TAP stream. */
	pub cursor:                    usize,
	/* Remaining machine cycles before the next recorded flux transition. */
	pub cycle_counter:             u32,
	/* Mechanical PLAY latch; the motor line still decides whether tape actually moves. */
	pub play_pressed:              bool,
	/* RECORD latch. Recording remains coupled to PLAY, as on the physical transport. */
	pub record_pressed:            bool,
	/* Read-signal phase. Idle counts the next interval; PulseActive holds the decoded transition on the CIA input. */
	pub state:                     TapeState,
	/* Number of cycles for which the CIA FLAG pulse remains active. */
	pub pulse_hold_timer:          u8,
	/* Host path retained only for persistence; transport timing never depends on host I/O. */
	pub tape_path:                 Option<PathBuf>,
	pub total_pulses:              u64,
	/* Number of pulse records crossed since rewind, used for progress reporting rather than timing. */
	pub current_pulses:            u64,
	/* Total cycles of actual tape motion, used by the mechanical counter model. */
	pub motor_cycles_accumulated:  u64,
	odometre:                      Odometre,
	/* Saturating cycle interval accumulated since the previous rising edge while RECORD waits for the next write-line transition. */
	pub write_cycle_accumulator:   u32,
	/* Previous sampled cassette-write level, retained so only low-to-high transitions terminate intervals. */
	pub last_write_bit:            bool,
}

impl Datassette {
	/* Construction leaves the deck stopped and empty, with the read line idle and no residual write interval. */
	pub fn new() -> Self {
		Self {
			tape_data:                 None,
			cursor:                    0,
			cycle_counter:             0,
			play_pressed:              false,
			record_pressed:            false,
			state:                     TapeState::Idle,
			pulse_hold_timer:          0,
			tape_path:                 None,
			total_pulses:              0,
			current_pulses:            0,
			motor_cycles_accumulated:  0,
			odometre:                  Odometre::new(),
			write_cycle_accumulator:   0,
			last_write_bit:            true,
		}
	}

	/* Loading first validates the complete pulse stream. State is committed only after every short or extended record has been proven structurally complete, so a malformed image cannot partially replace the mounted tape. */
	pub fn load_tap(&mut self, data: Vec<u8>, path: PathBuf) -> bool {
		if data.len() < TAP_HEADER_SIZE || data.len() > MAX_TAPE_SIZE || &data[..TAP_SIGNATURE.len()] != TAP_SIGNATURE {
			return false;
		}

		let mut temp_cursor = TAP_HEADER_SIZE;
		let mut pulse_count = 0u64;
		while temp_cursor < data.len() {
			let val = data[temp_cursor];
			temp_cursor += 1;
			if val == 0x00 {
				if temp_cursor + TAP_EXTENDED_PULSE_SIZE > data.len() { return false; }
				let pulse = u32::from_le_bytes([data[temp_cursor], data[temp_cursor + 1], data[temp_cursor + 2], 0]);
				if pulse == 0 { return false; }
				temp_cursor += TAP_EXTENDED_PULSE_SIZE;
			}
			pulse_count += 1;
		}

		self.tape_data                 = Some(data);
		self.tape_path                 = Some(path);
		self.cursor                    = TAP_HEADER_SIZE;
		self.cycle_counter             = 0;
		self.state                     = TapeState::Idle;
		self.total_pulses              = pulse_count;
		self.current_pulses            = 0;
		self.motor_cycles_accumulated  = 0;
		self.write_cycle_accumulator   = 0;
		self.last_write_bit            = true;
		true
	}

	/* Rewind returns both the byte cursor and the mechanical counter to the beginning of recorded data without changing the inserted image or transport buttons. */
	pub fn rewind(&mut self) {
		self.cursor                    = TAP_HEADER_SIZE;
		self.cycle_counter             = 0;
		self.state                     = TapeState::Idle;
		self.current_pulses            = 0;
		self.motor_cycles_accumulated  = 0;
		self.write_cycle_accumulator   = 0;
	}

	/* Ejection commits any recording before releasing the image, then resets both transport and signal state so no stale pulse survives without a cassette. */
	pub fn eject(&mut self) {
		self.save_tape_to_host();
		self.tape_data                 = None;
		self.tape_path                 = None;
		self.cursor                    = 0;
		self.cycle_counter             = 0;
		self.play_pressed              = false;
		self.record_pressed            = false;
		self.state                     = TapeState::Idle;
		self.total_pulses              = 0;
		self.current_pulses            = 0;
		self.motor_cycles_accumulated  = 0;
		self.write_cycle_accumulator   = 0;
		self.last_write_bit            = true;
	}

	/* Media presence is independent of PLAY, motor power and current cursor position. */
	pub fn has_tape(&self) -> bool {
		self.tape_data.is_some()
	}

	/* The mounted path is exposed read-only so UI state cannot retarget persistence behind the deck. */
	pub fn get_path(&self) -> Option<&std::path::Path> {
		self.tape_path.as_deref()
	}

	/* The visible counter follows accumulated motor-on time rather than logical pulse position, matching the transport rather than the file cursor. */
	pub fn get_odometre_value(&self) -> f64 {
		if self.tape_data.is_none() {
			return 0.0;
		}
		self.odometre.calculate_value(self.motor_cycles_accumulated)
	}

	/* Recording is persisted through a temporary file and rename. The backup fallback preserves the previous image on filesystems that cannot replace an existing path atomically. */
	pub fn save_tape_to_host(&self) {
		let Some(path) = self.tape_path.as_ref() else { return; };
		let Some(data) = self.tape_data.as_ref() else { return; };
		let temporary = path.with_extension("tap.tmp");
		let backup = path.with_extension("tap.bak");
		let result = (|| -> std::io::Result<()> {
			let mut file = File::create(&temporary)?;
			file.write_all(data)?;
			file.sync_all()?;
			if std::fs::rename(&temporary, path).is_ok() {
				return Ok(());
			}
			let had_original = path.exists();
			if had_original {
				std::fs::rename(path, &backup)?;
			}
			if let Err(error) = std::fs::rename(&temporary, path) {
				if had_original {
					let _ = std::fs::rename(&backup, path);
				}
				return Err(error);
			}
			if had_original {
				let _ = std::fs::remove_file(&backup);
			}
			Ok(())
		})();
		/* Failure cleanup removes only the unfinished temporary image; the original or restored backup remains authoritative. */
		if result.is_err() {
			let _ = std::fs::remove_file(temporary);
		}
	}

	/* Recording measures the interval between rising edges of the 6510 cassette-write signal. Short intervals use the compact one-byte TAP form; longer intervals are emitted as a 24-bit extended pulse. */
	#[inline]
	pub fn record_cycle_tick(&mut self, current_write_bit: bool) {
		self.motor_cycles_accumulated += 1;
		self.write_cycle_accumulator = self.write_cycle_accumulator.saturating_add(1);

		/* Only the low-to-high transition closes the preceding interval and creates a flux pulse record. */
		if !self.last_write_bit && current_write_bit {
			if let Some(ref mut data) = self.tape_data {
				/* Once the bounded image is full, recording stops growing it but input edge tracking continues so transport state remains coherent. */
				if data.len() >= MAX_TAPE_SIZE {
					self.last_write_bit = current_write_bit;
					return;
				}

				/* The recording path first quantises the interval to eight-cycle short-pulse units; values above one byte are then stored in the extended three-byte form. */
				let mut pulse_val = self.write_cycle_accumulator / TAP_SHORT_PULSE_SCALE;
				if pulse_val > 0 {
					if pulse_val > TAP_MAX_EXTENDED_PULSE {
						pulse_val = TAP_MAX_EXTENDED_PULSE;
					}

					if pulse_val <= TAP_MAX_SHORT_PULSE {
						data.push(pulse_val as u8);
					} else {
						data.push(0x00);
						data.push((pulse_val & 0xFF) as u8);
						data.push(((pulse_val >> 8) & 0xFF) as u8);
						data.push(((pulse_val >> 16) & 0xFF) as u8);
					}
					self.total_pulses += 1;
					self.current_pulses = self.total_pulses;

				}
				self.write_cycle_accumulator = 0;
			}
		}
		self.last_write_bit = current_write_bit;
	}

	/* Playback advances by one C64 machine cycle. A completed interval emits one active-low FLAG pulse, while the hold state keeps that pulse observable for the configured number of cycles. */
	#[inline]
	pub fn clock_tick(&mut self, motor_on: bool) -> bool {
		/* PLAY alone does not move tape: the 6510 motor output must also energise the transport. */
		if !self.play_pressed || !motor_on {
			return false;
		}
		let Some(ref data) = self.tape_data else { return false; };
		if self.cursor >= data.len() {
			return false;
		}

		self.motor_cycles_accumulated += 1;

		/* PulseActive is a presentation phase, not extra tape travel: the next TAP interval is not fetched until the held FLAG pulse has ended. */
		if self.state == TapeState::PulseActive {
			if self.pulse_hold_timer > 0 {
				self.pulse_hold_timer -= 1;
				return false;
			}
			self.state = TapeState::Idle;
		}
		/* The interval counter represents distance to the next transition. Decrementing before fetching prevents adjacent records from collapsing into the same machine cycle. */
		if self.cycle_counter > 0 {
			self.cycle_counter -= 1;
			return false;
		}
		/* A non-zero byte stores an interval in units of eight cycles; zero selects the following 24-bit interval verbatim. */
		let raw_val = data[self.cursor];
		self.cursor += 1;
		self.current_pulses += 1;
		/* Extended records are consumed atomically. A truncated tail is treated as end-of-media rather than exposing a partial duration. */
		if raw_val == 0x00 {
			if self.cursor + TAP_EXTENDED_PULSE_SIZE <= data.len() {
				let b1 = data[self.cursor] as u32;
				let b2 = data[self.cursor + 1] as u32;
				let b3 = data[self.cursor + 2] as u32;
				self.cursor += TAP_EXTENDED_PULSE_SIZE;
				self.cycle_counter = b1 | (b2 << 8) | (b3 << 16);
			} else {
				self.cursor = data.len();
				return false;
			}
		} else {
			self.cycle_counter = (raw_val as u32) * TAP_SHORT_PULSE_SCALE;
		}
		/* Fetching a record schedules the transition immediately, then leaves its encoded interval to time the following transition. */
		self.state            = TapeState::PulseActive;
		self.pulse_hold_timer = PULSE_HOLD_CYCLES;
		true
	}
}

impl Default for Datassette {
	fn default() -> Self {
		Self::new()
	}
}