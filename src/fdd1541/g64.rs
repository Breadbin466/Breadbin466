// =======================================================
// src/fdd1541/g64.rs — G64 disk image handling
// =======================================================

use super::constants::{
	D64_DISK_ID, G64_HEADER_LEN, G64_MAX_HALF_TRACKS, G64_MAX_TRACK_SIZE, G64_SIGNATURE,
};
use super::{d64, gcr};

/* Standard 1541 tracks use one of four density zones; outer tracks contain fewer bytes because their bit cells are longer. */
pub(crate) fn standard_density(track: u8) -> u8 {
	match track {
		1..=17 => 3,
		18..=24 => 2,
		25..=30 => 1,
		_ => 0,
	}
}

/* A logical D64 track is expanded into the complete circular GCR stream that the read channel would encounter, including headers, gaps and sync marks. */
pub(crate) fn formatted_track(d64_image: &[u8], track: u8) -> Vec<u8> {
	let sector_count = gcr::sectors_per_track(track) as usize;
	let mut sectors = Vec::with_capacity(sector_count);
	for sector in 0..sector_count {
		let offset = d64::sector_offset(track, sector as u8);
		let mut data = [0u8; 256];
		data.copy_from_slice(&d64_image[offset..offset + 256]);
		sectors.push(data);
	}
	gcr::build_track(track, &sectors, D64_DISK_ID[0], D64_DISK_ID[1])
}

/* G64 stores offsets to variable-length half-track records followed by a speed table. Only the populated full tracks receive records in a freshly formatted image. */
pub(crate) fn create_formatted() -> Vec<u8> {
	let d64_image = d64::create_formatted();
	let table_size = G64_MAX_HALF_TRACKS * 4;
	let data_start = G64_HEADER_LEN + table_size * 2;
	let record_size = 2 + G64_MAX_TRACK_SIZE;
	let populated_tracks = 35usize;
	let total_size = data_start + populated_tracks * record_size;
	let mut image = vec![0u8; total_size];

	image[..G64_SIGNATURE.len()].copy_from_slice(G64_SIGNATURE);
	image[8] = 0;
	image[9] = G64_MAX_HALF_TRACKS as u8;
	image[10..12].copy_from_slice(&(G64_MAX_TRACK_SIZE as u16).to_le_bytes());

	for track in 1..=35u8 {
		let half_track = (track as usize - 1) * 2;
		let record_offset = data_start + (track as usize - 1) * record_size;
		let table_offset = G64_HEADER_LEN + half_track * 4;
		image[table_offset..table_offset + 4]
			.copy_from_slice(&(record_offset as u32).to_le_bytes());

		let speed_offset = G64_HEADER_LEN + table_size + half_track * 4;
		image[speed_offset..speed_offset + 4]
			.copy_from_slice(&u32::from(standard_density(track)).to_le_bytes());

		let track_data = formatted_track(&d64_image, track);
		let track_length = track_data.len().min(G64_MAX_TRACK_SIZE);
		image[record_offset..record_offset + 2]
			.copy_from_slice(&(track_length as u16).to_le_bytes());
		image[record_offset + 2..record_offset + 2 + track_length]
			.copy_from_slice(&track_data[..track_length]);
		image[record_offset + 2 + track_length..record_offset + record_size].fill(0x55);
	}

	image
}