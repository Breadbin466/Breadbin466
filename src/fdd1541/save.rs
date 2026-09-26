// =======================================================
// src/fdd1541/save.rs — Disk image persistence and atomic writes
// =======================================================

use super::constants::{G64_HEADER_LEN, G64_MAX_HALF_TRACKS, G64_SIGNATURE};
use super::disk_drive::{DiskMechanism, PendingG64Layout, PendingWrite, TEMP_FILE_COUNTER};
use super::media::{DiskFormat, TrackSpeed, sector_offset, total_sectors};
use super::{d7z, gcr, nib};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

impl DiskMechanism {
	/* Atomic persistence writes and flushes a complete sibling file before replacing the destination, so an interrupted save cannot leave a half-written image. */
	fn write_atomic(path: &Path, buffer: &[u8]) -> bool {
		let parent = path.parent().unwrap_or_else(|| Path::new("."));
		let name = path
			.file_name()
			.and_then(|value| value.to_str())
			.unwrap_or("disk");
		let nonce = SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.map(|value| value.as_nanos())
			.unwrap_or(0);
		let counter = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
		let unique = format!("{}.{}.{}.{}.tmp", name, std::process::id(), nonce, counter);
		let temporary = parent.join(unique);
		let backup = parent.join(format!(
			"{}.{}.{}.{}.bak",
			name,
			std::process::id(),
			nonce,
			counter
		));

		let Ok(mut file) = OpenOptions::new().write(true).create_new(true).open(&temporary) else {
			return false;
		};
		let write_result = (|| -> std::io::Result<()> {
			file.write_all(buffer)?;
			file.sync_all()?;
			drop(file);
			Ok(())
		})();

		if write_result.is_err() {
			let _ = fs::remove_file(&temporary);
			return false;
		}

		if fs::rename(&temporary, path).is_ok() {
			return true;
		}

		let had_original = path.exists();
		if had_original && fs::rename(path, &backup).is_err() {
			let _ = fs::remove_file(&temporary);
			return false;
		}

		if fs::rename(&temporary, path).is_err() {
			if had_original {
				let _ = fs::rename(&backup, path);
			}
			let _ = fs::remove_file(&temporary);
			return false;
		}

		if had_original {
			let _ = fs::remove_file(&backup);
		}

		true
	}
	fn apply_persisted_snapshot(
		&mut self,
		generation: u64,
		original_file: Vec<u8>,
		g64_layout: Option<PendingG64Layout>,
	) {
		self.original_file = Some(original_file);
		if let Some(layout) = g64_layout {
			self.track_offsets_g64 = layout.track_offsets;
			self.speed_offsets_g64 = layout.speed_offsets;
			self.max_track_size_g64 = layout.max_track_size;
		}
		self.persisted_generation
			.store(generation, Ordering::Release);

		if self.current_generation == generation {
			self.write_pending = false;
			self.dirty = false;
			self.dirty_tracks.fill(false);
			self.flush_retry_deferred = false;
		}
	}

	pub(super) fn finish_pending_writer(&mut self, wait: bool) -> bool {
		let Some(pending) = self.pending_writer.as_ref() else {
			return true;
		};
		if !wait && !pending.handle.is_finished() {
			return true;
		}

		let pending = self
			.pending_writer
			.take()
			.expect("pending writer disappeared while owned");
		let success = pending.handle.join().unwrap_or(false);
		if success {
			self.apply_persisted_snapshot(
				pending.generation,
				pending.original_file,
				pending.g64_layout,
			);
		}
		success
	}

	fn commit_buffer(
		&mut self,
		path: &Path,
		buffer: &[u8],
		background: bool,
		original_file: Vec<u8>,
		g64_layout: Option<PendingG64Layout>,
	) -> bool {
		let generation = self.current_generation;
		if !background {
			let success = Self::write_atomic(path, buffer);
			if success {
				self.apply_persisted_snapshot(generation, original_file, g64_layout);
			}
			return success;
		}

		if self.pending_writer.is_some() {
			return false;
		}
		let owned_path = path.to_path_buf();
		let owned_buffer = buffer.to_vec();
		let handle = thread::spawn(move || Self::write_atomic(&owned_path, &owned_buffer));
		self.pending_writer = Some(PendingWrite {
			generation,
			handle,
			original_file,
			g64_layout,
		});
		true
	}
	/* Flush follows the mounted format: logical D64 images require sector reconstruction, while physical formats serialise track state directly. */
	pub(super) fn flush_to_disk(&mut self, background: bool) -> bool {
		if self.write_protect {
			self.write_pending = false;
			self.dirty = false;
			self.dirty_tracks.fill(false);
			self.flush_retry_deferred = false;
			return true;
		}

		let success = match self.format {
			Some(DiskFormat::D64) | Some(DiskFormat::D7z) => self.flush_d64(background),
			Some(DiskFormat::G64) => self.flush_g64(background),
			Some(DiskFormat::Nib) => self.flush_nib(background, false),
			Some(DiskFormat::Nbz) => self.flush_nib(background, true),
			None => true,
		};

		if success && !background {
			self.flush_retry_deferred = false;
		}
		success
	}
	/* Directory persistence is delayed while the drive is between the paired writes commonly used for a directory-sector update. */
	fn directory_sector_is_stable(sector: &[u8; 256]) -> bool {
		for entry in 0..8 {
			let offset = 2 + entry * 32;
			let file_type = sector[offset];
			if file_type & 0x07 != 0 && file_type & 0x80 == 0 {
				return false;
			}
		}
		true
	}
	fn flush_nib(&mut self, background: bool, compressed: bool) -> bool {
		let Some(path) = self.mount_path.clone() else {
			return false;
		};
		let densities: Vec<u8> = self
			.track_speed
			.iter()
			.map(|speed| match speed {
				TrackSpeed::Constant(density) => *density & 0x03,
				TrackSpeed::PerByte(block) => block.first().copied().unwrap_or(0) & 0x03,
			})
			.collect();
		let Some(nib_buffer) = nib::encode_nib(&self.tracks, &densities) else {
			return false;
		};
		let compressed_buffer = if compressed {
			let Some(buffer) = nib::encode_nbz(&nib_buffer) else {
				return false;
			};
			Some(buffer)
		} else {
			None
		};
		let output = compressed_buffer.as_deref().unwrap_or(&nib_buffer);
		let persisted_snapshot = nib_buffer.clone();

		self.commit_buffer(&path, output, background, persisted_snapshot, None)
	}
	/* Saving D64 decodes the current physical tracks back into sectors. Tracks that cannot be represented losslessly remain a persistence failure rather than being silently normalised. */
	fn flush_d64(&mut self, background: bool) -> bool {
		let Some(path) = self.mount_path.clone() else {
			return false;
		};
		let Some(mut buffer) = self.original_file.clone() else {
			return false;
		};
		let mut decoded_dirty_track = false;
		let mut committed_sector = false;
		let mut directory_stable = true;

		for index in 0..self.dirty_tracks.len() {
			if !self.dirty_tracks[index] {
				continue;
			}

			/* D64 has one logical track per whole-track position. A modified half-track or a track outside the mounted D64 geometry cannot be represented without discarding physical state, so persistence must remain pending. */
			if index & 1 != 0 {
				return false;
			}

			let track = (index / 2 + 1) as u8;
			if track > self.num_tracks {
				return false;
			}
			let Some(raw_track) = self.tracks.get(index) else {
				return false;
			};
			let anchor = self
				.dirty_track_anchor_bits
				.get(index)
				.copied()
				.unwrap_or(0);
			let Some(sectors) = gcr::decode_track_from(raw_track, track, anchor) else {
				return false;
			};

			let expected_sectors = gcr::sectors_per_track(track) as usize;
			if sectors.len() != expected_sectors || sectors.iter().any(Option::is_none) {
				return false;
			}

			for (sector, sector_data) in sectors.into_iter().enumerate() {
				let Some(sector_data) = sector_data else {
					return false;
				};

				if track == 18 && sector > 0 && !Self::directory_sector_is_stable(&sector_data) {
					directory_stable = false;
				}
				let offset = sector_offset(track, sector as u8);
				if offset + 256 > buffer.len() {
					return false;
				}
				if buffer[offset..offset + 256] != sector_data {
					buffer[offset..offset + 256].copy_from_slice(&sector_data);
					committed_sector = true;
				}

				let logical_size = total_sectors(self.num_tracks) * 256;
				let error_index = logical_size + offset / 256;
				if error_index < buffer.len() && buffer[error_index] != 1 {
					buffer[error_index] = 1;
					committed_sector = true;
				}
			}

			decoded_dirty_track = true;
		}

		if !directory_stable {
			return false;
		}
		if !committed_sector {
			if decoded_dirty_track {
				self.apply_persisted_snapshot(self.current_generation, buffer, None);
				return true;
			}
			return false;
		}
		let encoded;
		let output = if self.format == Some(DiskFormat::D7z) {
			let Some(value) = d7z::encode(&buffer) else {
				return false;
			};
			encoded = value;
			encoded.as_slice()
		} else {
			buffer.as_slice()
		};
		let persisted_snapshot = buffer.clone();
		self.commit_buffer(&path, output, background, persisted_snapshot, None)
	}
	/* Saving G64 preserves circular track bytes and density maps, including non-DOS layouts and protection data. */
	fn flush_g64(&mut self, background: bool) -> bool {
		let Some(path) = self.mount_path.clone() else {
			return false;
		};
		let Some(mut buffer) = self.original_file.clone() else {
			return false;
		};

		let half_track_count = buffer.get(9).copied().unwrap_or(0) as usize;
		let table_size = half_track_count.saturating_mul(4);
		let speed_table_start = G64_HEADER_LEN.saturating_add(table_size);
		let tables_end = speed_table_start.saturating_add(table_size);
		if half_track_count == 0
			|| half_track_count > G64_MAX_HALF_TRACKS
			|| tables_end > buffer.len()
		{
			return false;
		}

		for index in 0..half_track_count {
			if !self.dirty_tracks.get(index).copied().unwrap_or(false) {
				continue;
			}

			let Some(track) = self.tracks.get(index) else {
				return false;
			};
			let Some(track_start) = self.track_offsets_g64.get(index).copied().flatten() else {
				return self.rebuild_g64(&path, background);
			};
			let Some(track_length_end) = track_start.checked_add(track.len()) else {
				return false;
			};
			if track_start < 2
				|| track.len() > self.max_track_size_g64
				|| track_length_end > buffer.len()
			{
				return false;
			}

			buffer[track_start - 2..track_start]
				.copy_from_slice(&(track.len() as u16).to_le_bytes());
			buffer[track_start..track_length_end].copy_from_slice(track);

			let speed_entry = speed_table_start + index * 4;
			match self.track_speed.get(index) {
				Some(TrackSpeed::Constant(density)) => {
					buffer[speed_entry..speed_entry + 4]
						.copy_from_slice(&u32::from(*density & 0x03).to_le_bytes());
				}
				Some(TrackSpeed::PerByte(block)) => {
					let required = track.len().div_ceil(4).max(1);
					let Some(speed_start) = self.speed_offsets_g64.get(index).copied().flatten()
					else {
						return self.rebuild_g64(&path, background);
					};
					let Some(speed_end) = speed_start.checked_add(required) else {
						return false;
					};
					if block.len() < required || speed_end > buffer.len() {
						return false;
					}
					buffer[speed_start..speed_end].copy_from_slice(&block[..required]);
				}
				None => return false,
			}
		}

		self.commit_buffer(&path, &buffer, background, buffer.clone(), None)
	}
	fn rebuild_g64(&mut self, path: &Path, background: bool) -> bool {
		let half_track_count = match self.tracks.iter().rposition(|track| !track.is_empty()) {
			Some(index) => (index + 1).min(G64_MAX_HALF_TRACKS),
			None => return false,
		};
		let max_track_size = self.tracks[..half_track_count]
			.iter()
			.map(Vec::len)
			.max()
			.unwrap_or(0);
		if max_track_size == 0 || max_track_size > u16::MAX as usize {
			return false;
		}

		let table_size = half_track_count * 4;
		let speed_table_start = G64_HEADER_LEN + table_size;
		let data_start = speed_table_start + table_size;
		let mut buffer = vec![0u8; data_start];
		buffer[..8].copy_from_slice(G64_SIGNATURE);
		buffer[8] = 0;
		buffer[9] = half_track_count as u8;
		buffer[10..12].copy_from_slice(&(max_track_size as u16).to_le_bytes());

		let mut track_offsets = vec![None; G64_MAX_HALF_TRACKS];
		let mut speed_offsets = vec![None; G64_MAX_HALF_TRACKS];
		for index in 0..half_track_count {
			let track = &self.tracks[index];
			if !track.is_empty() {
				let offset = buffer.len();
				buffer.extend_from_slice(&(track.len() as u16).to_le_bytes());
				buffer.extend_from_slice(track);
				buffer.resize(buffer.len() + max_track_size - track.len(), 0xFF);
				buffer[G64_HEADER_LEN + index * 4..G64_HEADER_LEN + index * 4 + 4]
					.copy_from_slice(&(offset as u32).to_le_bytes());
				track_offsets[index] = Some(offset + 2);
			}

			let speed_entry = speed_table_start + index * 4;
			match self.track_speed.get(index) {
				Some(TrackSpeed::PerByte(block)) => {
					let offset = buffer.len();
					let length = track.len().div_ceil(4).max(1);
					let copy_length = block.len().min(length);
					buffer.extend_from_slice(&block[..copy_length]);
					buffer.resize(buffer.len() + length - copy_length, 0);
					buffer[speed_entry..speed_entry + 4]
						.copy_from_slice(&(offset as u32).to_le_bytes());
					speed_offsets[index] = Some(offset);
				}
				Some(TrackSpeed::Constant(density)) => {
					buffer[speed_entry..speed_entry + 4]
						.copy_from_slice(&u32::from(*density & 0x03).to_le_bytes());
				}
				None => return false,
			}
		}

		let layout = PendingG64Layout {
			track_offsets,
			speed_offsets,
			max_track_size,
		};
		self.commit_buffer(path, &buffer, background, buffer.clone(), Some(layout))
	}
}