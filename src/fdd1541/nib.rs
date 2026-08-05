// =======================================================
// src/fdd1541/nib.rs — NIB disk image encoding and decoding
// =======================================================

use crate::fdd1541::constants::{
	NIB_TRACK_BYTES_MAX, NIB_TRACK_BYTES_MIN, NIB_TRACK_BYTES_TOLERANCE, NIB_HALF_TRACK_COUNT,
	NIB_HEADER_LENGTH, NIB_CYCLE_SIGNATURE_BYTES, NIB_FORMATTED_GCR_RUN_BYTES,
	NIB_SIGNATURE, NIB_TRACK_LENGTH,
};
use super::{d64, g64};
pub use super::nbz::{decode_nbz, encode_nbz};

/* NibImage exposes recovered circular tracks and their density zones after removing the fixed 8192-byte capture padding. */
pub struct NibImage {
	pub tracks: Vec<Vec<u8>>,
	pub densities: Vec<u8>,
}

/* The NIB header maps capture blocks to half-tracks. Each block is reduced to the most plausible repeating revolution before entering the drive mechanism. */
pub fn decode_nib(data: &[u8]) -> Option<NibImage> {
	if data.len() < NIB_HEADER_LENGTH || data.get(..NIB_SIGNATURE.len()) != Some(NIB_SIGNATURE) {
		return None;
	}

	let mut entries = Vec::new();
	let mut header_offset = 0x10usize;
	while header_offset + 1 < NIB_HEADER_LENGTH {
		let encoded_track = data[header_offset];
		if encoded_track == 0 {
			break;
		}
		if encoded_track < 2 {
			return None;
		}
		let track_index = usize::from(encoded_track - 2);
		if track_index >= NIB_HALF_TRACK_COUNT
			|| entries.iter().any(|(index, _)| *index == track_index)
		{
			return None;
		}
		entries.push((track_index, data[header_offset + 1] & 0x03));
		header_offset += 2;
	}

	if entries.is_empty() {
		return None;
	}

	let expected_length = NIB_HEADER_LENGTH.checked_add(entries.len().checked_mul(NIB_TRACK_LENGTH)?)?;
	if data.len() < expected_length {
		return None;
	}

	let mut tracks = vec![Vec::new(); NIB_HALF_TRACK_COUNT];
	let mut densities = vec![0; NIB_HALF_TRACK_COUNT];
	for (block_index, (track_index, density)) in entries.into_iter().enumerate() {
		let start = NIB_HEADER_LENGTH + block_index * NIB_TRACK_LENGTH;
		let source = &data[start..start + NIB_TRACK_LENGTH];
		tracks[track_index] = extract_track(source, density);
		densities[track_index] = density;
	}

	Some(NibImage { tracks, densities })
}

/* Encoding repeats each circular track to fill the fixed capture block while preserving half-track numbering and density metadata. */
pub fn encode_nib(tracks: &[Vec<u8>], densities: &[u8]) -> Option<Vec<u8>> {
	let populated: Vec<usize> = tracks
		.iter()
		.enumerate()
		.filter_map(|(index, track)| (!track.is_empty()).then_some(index))
		.collect();
	if populated.is_empty() || populated.len() > NIB_HALF_TRACK_COUNT {
		return None;
	}

	let mut output = vec![0u8; NIB_HEADER_LENGTH + populated.len() * NIB_TRACK_LENGTH];
	output[..NIB_SIGNATURE.len()].copy_from_slice(NIB_SIGNATURE);
	output[NIB_SIGNATURE.len()] = 1;

	for (block_index, track_index) in populated.into_iter().enumerate() {
		let encoded_track = u8::try_from(track_index.checked_add(2)?).ok()?;
		let header_offset = 0x10 + block_index * 2;
		output[header_offset] = encoded_track;
		output[header_offset + 1] = densities.get(track_index).copied().unwrap_or(0) & 0x03;

		let track = tracks.get(track_index)?;
		let block_offset = NIB_HEADER_LENGTH + block_index * NIB_TRACK_LENGTH;
		for offset in 0..NIB_TRACK_LENGTH {
			output[block_offset + offset] = track[offset % track.len()];
		}
	}

	Some(output)
}

fn extract_track(source: &[u8], density: u8) -> Vec<u8> {
	if source.len() != NIB_TRACK_LENGTH || !has_formatted_data(source) {
		return Vec::new();
	}

	let minimum = NIB_TRACK_BYTES_MIN[usize::from(density)].saturating_sub(NIB_TRACK_BYTES_TOLERANCE);
	let maximum = NIB_TRACK_BYTES_MAX[usize::from(density)];
	let mut cycle = find_track_cycle(source, minimum);
	if cycle.1 < minimum || cycle.1 > maximum {
		cycle = find_nondos_track_cycle(source, minimum);
	}

	let (start, length) = cycle;
	let Some(end) = start.checked_add(length).filter(|end| *end <= source.len()) else {
		return source.to_vec();
	};
	source.get(start..end).unwrap_or(source).to_vec()
}

/* DOS-formatted captures are aligned by matching sync-delimited signatures separated by at least one nominal revolution. */
fn find_track_cycle(source: &[u8], minimum: usize) -> (usize, usize) {
	let stop = source.len().saturating_sub(NIB_CYCLE_SIGNATURE_BYTES);
	let mut start = 0usize;

	loop {
		if start + minimum >= stop {
			break;
		}

		let mut data_position = start + minimum;
		while let Some(candidate) = find_sync(source, data_position, stop) {
			let mut left = start;
			let mut right = candidate;
			let mut matched = true;

			loop {
				if left + NIB_CYCLE_SIGNATURE_BYTES > stop || right + NIB_CYCLE_SIGNATURE_BYTES > stop
					|| source[left..left + NIB_CYCLE_SIGNATURE_BYTES] != source[right..right + NIB_CYCLE_SIGNATURE_BYTES]
				{
					matched = false;
					break;
				}

				let next_left = find_sync(source, left, stop);
				let next_right = find_sync(source, right, stop);
				match (next_left, next_right) {
					(Some(new_left), Some(new_right)) => {
						left = new_left;
						right = new_right;
					}
					_ => break,
				}
			}

			if matched && valid_data(source, candidate) {
				return (start, candidate - start);
			}
			data_position = candidate.saturating_add(1);
		}

		let Some(next_start) = find_sync(source, start, stop) else {
			break;
		};
		start = next_start;
	}

	(0, source.len())
}

/* Protection tracks may lack ordinary sync structure, so the fallback searches for any sufficiently distant repeated signature. */
fn find_nondos_track_cycle(source: &[u8], minimum: usize) -> (usize, usize) {
	let stop = source.len().saturating_sub(NIB_CYCLE_SIGNATURE_BYTES);
	for left in 0..stop {
		let first_right = left.saturating_add(minimum);
		if first_right >= stop {
			break;
		}
		for right in first_right..stop {
			if source[left..left + NIB_CYCLE_SIGNATURE_BYTES] == source[right..right + NIB_CYCLE_SIGNATURE_BYTES]
				&& valid_data(source, right)
			{
				return (left, right - left);
			}
		}
	}
	(0, source.len())
}

fn find_sync(source: &[u8], position: usize, stop: usize) -> Option<usize> {
	let mut current = position.saturating_add(1);
	while current < stop {
		if source[current] == 0xff && source[current - 1] != 0xff {
			return Some(current);
		}
		current += 1;
	}
	None
}

fn valid_data(source: &[u8], start: usize) -> bool {
	let Some(end) = start.checked_add(NIB_CYCLE_SIGNATURE_BYTES + 4) else {
		return false;
	};
	if end > source.len() {
		return false;
	}
	let mut redundant = 0usize;
	for index in 0..NIB_CYCLE_SIGNATURE_BYTES {
		let value = source[start + index];
		if source[start + index + 1..=start + index + 4].contains(&value) {
			redundant += 1;
		}
	}
	redundant <= 1
}

fn has_formatted_data(source: &[u8]) -> bool {
	let mut run = 0usize;
	for index in 0..source.len() {
		if bad_gcr(source, index) {
			run = 0;
		} else {
			run += 1;
			if run >= NIB_FORMATTED_GCR_RUN_BYTES {
				return true;
			}
		}
	}
	false
}

/* A run containing three consecutive zero bits cannot occur in valid Commodore 4-to-5 GCR and is used to reject unformatted noise. */
fn bad_gcr(source: &[u8], index: usize) -> bool {
	let previous = source[(index + source.len() - 1) % source.len()];
	let data = (u16::from(previous & 0x03) << 8) | u16::from(source[index]);
	let mut mask = 7u16 << 7;
	while mask >= 7 {
		if data & mask == 0 {
			return true;
		}
		mask >>= 1;
	}
	false
}

pub(crate) fn create_formatted() -> Vec<u8> {
	let d64_image = d64::create_formatted();
	let populated_tracks = 35usize;
	let mut image = vec![0u8; NIB_HEADER_LENGTH + populated_tracks * NIB_TRACK_LENGTH];
	image[..NIB_SIGNATURE.len()].copy_from_slice(NIB_SIGNATURE);
	image[NIB_SIGNATURE.len()] = 1;

	for track in 1..=35u8 {
		let half_track = (usize::from(track) - 1) * 2;
		let header_offset = 0x10 + (usize::from(track) - 1) * 2;
		image[header_offset] = (half_track + 2) as u8;
		image[header_offset + 1] = g64::standard_density(track);

		let track_data = g64::formatted_track(&d64_image, track);
		let block_offset = NIB_HEADER_LENGTH + (usize::from(track) - 1) * NIB_TRACK_LENGTH;
		for index in 0..NIB_TRACK_LENGTH {
			image[block_offset + index] = track_data[index % track_data.len()];
		}
	}

	image
}

pub(crate) fn create_formatted_nbz() -> Vec<u8> {
	encode_nbz(&create_formatted()).unwrap_or_default()
}