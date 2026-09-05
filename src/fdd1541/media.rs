// =======================================================
// src/fdd1541/media.rs — Mounted media metadata and track-speed representation
// =======================================================

use super::gcr;

pub(super) fn total_sectors(num_tracks: u8) -> usize {
	(1..=num_tracks)
		.map(|track| gcr::sectors_per_track(track) as usize)
		.sum()
}

pub(super) fn sector_offset(track: u8, sector: u8) -> usize {
	let mut sectors = 0usize;
	for t in 1..track {
		sectors += gcr::sectors_per_track(t) as usize;
	}
	(sectors + sector as usize) * 256
}

/* A G64 track may have one density for its whole circumference or a packed two-bit density value for each group of four bytes. */
#[derive(Clone)]
pub(super) enum TrackSpeed {
	/* One density zone applies around the entire circular track. */
	Constant(u8),

	/* Packed two-bit density selections allow local timing changes within a G64 track. */
	PerByte(Vec<u8>),
}

impl TrackSpeed {
	pub(super) fn density_for_byte(&self, byte_index: usize) -> Option<u8> {
		match self {
			TrackSpeed::Constant(_) => None,
			TrackSpeed::PerByte(block) => {
				let packed = *block.get(byte_index / 4)?;
				Some((packed >> ((byte_index % 4) * 2)) & 0x03)
			}
		}
	}

	/* The first local density change promotes a constant track to a per-byte speed block while preserving the previous density everywhere else. */
	pub(super) fn set_density_for_byte(
		&mut self,
		byte_index: usize,
		track_length: usize,
		density: u8,
	) {
		let block_length = track_length.div_ceil(4).max(1);
		if let TrackSpeed::Constant(previous) = self {
			if (*previous & 0x03) == (density & 0x03) {
				return;
			}
			let packed = (*previous & 0x03)
				| ((*previous & 0x03) << 2)
				| ((*previous & 0x03) << 4)
				| ((*previous & 0x03) << 6);
			*self = TrackSpeed::PerByte(vec![packed; block_length]);
		}

		if let TrackSpeed::PerByte(block) = self {
			if block.len() < block_length {
				block.resize(block_length, 0);
			}
			let slot = byte_index / 4;
			let shift = (byte_index % 4) * 2;
			if let Some(value) = block.get_mut(slot) {
				*value = (*value & !(0x03 << shift)) | ((density & 0x03) << shift);
			}
		}
	}
}

/* DiskFormat records the mounted container so later writes can preserve its representational limits and compression policy. */
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum DiskFormat {
	/* Sector image reconstructed through standard Commodore DOS headers and data blocks. */
	D64,
	/* D64 byte stream stored in the D7Z raw-LZMA2 container. */
	D7z,
	/* Byte-exact circular GCR tracks with optional per-position density tables. */
	G64,
	/* Raw captured revolutions whose track boundaries must be inferred during loading. */
	Nib,
	/* NIB data preserved through the same physical model and stored with compression. */
	Nbz,
}