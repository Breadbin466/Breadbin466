// =======================================================
// src/fdd1541/load.rs — Disk image mounting and format decoding
// =======================================================

impl DiskMechanism {
	/* Mounting converts each supported container into circular track bytes plus per-byte density metadata, then installs the medium without resetting mechanical position. */
/* Mount parses a complete replacement medium before committing it, so a malformed image cannot destroy the currently inserted disk. */
	pub fn mount(&mut self, path: &Path) -> bool {
		let data = match fs::read(path) {
			Ok(data) => data,
			Err(_) => return false,
		};

		let extension = path.extension().and_then(|value| value.to_str()).unwrap_or("").to_ascii_lowercase();
		let is_g64 = data.get(0..8) == Some(G64_SIGNATURE.as_slice());
		let nib_data = if extension == "nbz" { nib::decode_nbz(&data) } else { None };
		let mut replacement = Self::new();

		let (mounted, format, original) = if let Some(decoded) = nib_data {
			(replacement.mount_nib(&decoded), DiskFormat::Nbz, decoded)
		} else {
			let (mounted, format) = if extension == "nib" {
				(replacement.mount_nib(&data), DiskFormat::Nib)
			} else if is_g64 {
				(replacement.mount_g64(&data), DiskFormat::G64)
			} else {
				(replacement.mount_d64(&data), DiskFormat::D64)
			};
			(mounted, format, data)
		};

		if !mounted {
			return false;
		}

		replacement.inherit_runtime_state(self);

		let _ = self.flush_now();

		replacement.write_protect = fs::metadata(path)
			.map(|metadata| metadata.permissions().readonly())
			.unwrap_or(true);
		replacement.mount_path = Some(path.to_path_buf());
		replacement.write_pending = false;
		replacement.flush_retry_deferred = false;
		replacement.format = Some(format);
		replacement.original_file = Some(original);
		replacement.normalise_runtime_position();
		*self = replacement;
		true
	}
	fn standard_density_for_track(track: u8) -> u8 {
		match track {
			1..=17 => 3,
			18..=24 => 2,
			25..=30 => 1,
			_ => 0,
		}
	}
	fn d64_track_count(data_length: usize) -> Option<u8> {
		for tracks in 35..=42 {
			let sectors = total_sectors(tracks);
			let payload_length = sectors.checked_mul(256)?;
			if data_length == payload_length || data_length == payload_length.checked_add(sectors)? {
				return Some(tracks);
			}
		}
		None
	}
	/* D64 stores logical sectors, so mounting synthesises the headers, gaps, checksums and GCR stream that a real 1541 would encounter. */
/* D64 sectors are expanded into canonical GCR tracks so the running drive always consumes a physical-track representation. */
	fn mount_d64(&mut self, data: &[u8]) -> bool {
		let Some(num_tracks) = Self::d64_track_count(data.len()) else {
			return false;
		};

		let bam_off = sector_offset(18, 0);
		let id1 = data.get(bam_off + 0xA2).copied().unwrap_or(0x41);
		let id2 = data.get(bam_off + 0xA3).copied().unwrap_or(0x41);

		let logical_size = total_sectors(num_tracks) * 256;
		let error_table = data.get(logical_size..).unwrap_or(&[]);
		let mut sector_number = 0usize;
		let mut tracks = Vec::with_capacity(num_tracks as usize * 2);
		let mut speeds = Vec::with_capacity(num_tracks as usize * 2);
		for t in 1..=num_tracks {
			let count = gcr::sectors_per_track(t) as usize;
			let mut sectors = Vec::with_capacity(count);
			let mut errors = Vec::with_capacity(count);
			for s in 0..count {
				let off = sector_offset(t, s as u8);
				let mut buf = [0u8; 256];
				if off + 256 <= data.len() {
					buf.copy_from_slice(&data[off..off + 256]);
				}
				sectors.push(buf);
				errors.push(error_table.get(sector_number).copied().unwrap_or(0x01));
				sector_number += 1;
			}
			tracks.push(gcr::build_track_with_errors(t, &sectors, &errors, id1, id2));
			speeds.push(TrackSpeed::Constant(Self::standard_density_for_track(t)));

			tracks.push(Vec::new());
			speeds.push(TrackSpeed::Constant(Self::standard_density_for_track(t)));
		}

		tracks.resize(G64_MAX_HALF_TRACKS, Vec::new());
		speeds.resize(G64_MAX_HALF_TRACKS, TrackSpeed::Constant(0));

		self.tracks = tracks;
		self.track_speed = speeds;
		self.track_offsets_g64.clear();
		self.speed_offsets_g64.clear();
		self.max_track_size_g64 = 0;
		self.num_tracks = num_tracks;
		self.disk_present = true;
		self.dirty = false;
		self.dirty_tracks = vec![false; G64_MAX_HALF_TRACKS];
		self.dirty_track_anchor_bits = vec![0; G64_MAX_HALF_TRACKS];
		self.flush_retry_deferred = false;
		self.half_track = self.half_track.clamp(2, G64_MAX_HALF_TRACKS as u8 + 1);
		self.reset_decoder();
		self.disk_change_cycles = DISK_CHANGE_CYCLES;
		true
	}
	fn mount_nib(&mut self, data: &[u8]) -> bool {
		let Some(image) = nib::decode_nib(data) else {
			return false;
		};
		let Some(last_track) = image.tracks.iter().rposition(|track| !track.is_empty()) else {
			return false;
		};
		self.tracks = image.tracks;
		self.track_speed = image.densities.into_iter().map(TrackSpeed::Constant).collect();
		self.track_offsets_g64.clear();
		self.speed_offsets_g64.clear();
		self.max_track_size_g64 = 0;
		self.num_tracks = ((last_track + 2) / 2).min(u8::MAX as usize) as u8;
		self.disk_present = true;
		self.write_protect = true;
		self.dirty = false;
		self.dirty_tracks = vec![false; G64_MAX_HALF_TRACKS];
		self.dirty_track_anchor_bits = vec![0; G64_MAX_HALF_TRACKS];
		self.flush_retry_deferred = false;
		self.half_track = self.half_track.clamp(2, G64_MAX_HALF_TRACKS as u8 + 1);
		self.reset_decoder();
		self.disk_change_cycles = DISK_CHANGE_CYCLES;
		true
	}
	/* G64 supplies raw circular track bytes and speed information; no sector normalisation is performed during mount. */
/* G64 mounting preserves supplied track bytes and per-byte speed information instead of normalising through DOS sectors. */
	fn mount_g64(&mut self, data: &[u8]) -> bool {
		if data.len() < G64_HEADER_LEN {
			return false;
		}

		let half_track_count = data[9] as usize;
		let max_track_size = u16::from_le_bytes([data[10], data[11]]) as usize;
		if half_track_count == 0 || half_track_count > G64_MAX_HALF_TRACKS {
			return false;
		}

		let track_table_start = G64_HEADER_LEN;
		let track_table_len = half_track_count * 4;
		let speed_table_start = track_table_start + track_table_len;
		let speed_table_len = half_track_count * 4;
		if data.len() < speed_table_start + speed_table_len {
			return false;
		}

		let speed_block_len = max_track_size.div_ceil(4).max(1);

		let mut tracks: Vec<Vec<u8>> = Vec::with_capacity(half_track_count);
		let mut track_offsets: Vec<Option<usize>> = Vec::with_capacity(half_track_count);
		let mut speed_offsets: Vec<Option<usize>> = Vec::with_capacity(half_track_count);
		let mut speeds: Vec<TrackSpeed> = Vec::with_capacity(half_track_count);

		for half in 0..half_track_count {
			let entry = track_table_start + half * 4;
			let offset = u32::from_le_bytes([
				data[entry],
				data[entry + 1],
				data[entry + 2],
				data[entry + 3],
			]) as usize;

			if offset == 0 || offset + 2 > data.len() {
				tracks.push(Vec::new());
				track_offsets.push(None);
			} else {
				let len = u16::from_le_bytes([data[offset], data[offset + 1]]) as usize;
				let start = offset + 2;
				let Some(end) = start.checked_add(len) else {
					return false;
				};
				if len > max_track_size || end > data.len() {
					return false;
				}
				tracks.push(data[start..end].to_vec());
				track_offsets.push(Some(start));
			}

			let speed_entry = speed_table_start + half * 4;
			let speed_val = u32::from_le_bytes([
				data[speed_entry],
				data[speed_entry + 1],
				data[speed_entry + 2],
				data[speed_entry + 3],
			]) as usize;

			if speed_val < 4 {
				speeds.push(TrackSpeed::Constant(speed_val as u8));
				speed_offsets.push(None);
			} else if speed_val + speed_block_len <= data.len() {
				speeds.push(TrackSpeed::PerByte(data[speed_val..speed_val + speed_block_len].to_vec()));
				speed_offsets.push(Some(speed_val));
			} else {
				return false;
			}
		}

		let num_half_tracks = match tracks.iter().rposition(|t| !t.is_empty()) {
			Some(i) => i + 1,
			None => return false,
		};

		if tracks.iter().any(|t| t.len() > max_track_size) {
			return false;
		}

		tracks.resize(G64_MAX_HALF_TRACKS, Vec::new());
		track_offsets.resize(G64_MAX_HALF_TRACKS, None);
		speeds.resize(G64_MAX_HALF_TRACKS, TrackSpeed::Constant(0));
		speed_offsets.resize(G64_MAX_HALF_TRACKS, None);

		self.tracks = tracks;
		self.track_speed = speeds;
		self.track_offsets_g64 = track_offsets;
		self.speed_offsets_g64 = speed_offsets;
		self.max_track_size_g64 = max_track_size;
		self.num_tracks = ((num_half_tracks + 1) / 2) as u8;
		self.disk_present = true;
		self.dirty = false;
		self.dirty_tracks = vec![false; G64_MAX_HALF_TRACKS];
		self.dirty_track_anchor_bits = vec![0; G64_MAX_HALF_TRACKS];
		self.flush_retry_deferred = false;
		self.half_track = self.half_track.clamp(2, G64_MAX_HALF_TRACKS as u8 + 1);
		self.reset_decoder();
		self.disk_change_cycles = DISK_CHANGE_CYCLES;
		true
	}
	pub fn unmount(&mut self) -> bool {
		let _ = self.flush_now();

		self.tracks.clear();
		self.track_speed.clear();
		self.dirty_tracks.clear();
		self.dirty_track_anchor_bits.clear();
		self.num_tracks = 0;
		self.disk_present = false;
		self.write_protect = false;
		self.dirty = false;
		self.flush_retry_deferred = false;
		self.mount_path = None;
		self.original_file = None;
		self.track_offsets_g64.clear();
		self.speed_offsets_g64.clear();
		self.max_track_size_g64 = 0;
		self.format = None;
		self.write_pending = false;
		self.current_generation = 0;
		self.persisted_generation.store(0, Ordering::Release);
		self.reset_decoder();
		self.disk_change_cycles = DISK_CHANGE_CYCLES;
		true
	}
}