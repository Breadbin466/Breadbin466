// =======================================================
// src/fdd1541/gcr.rs — Group Code Recording: encode, decode, track layout
// =======================================================

use super::constants::GCR_ENCODE;

/* Commodore zone recording reduces sectors per revolution on outer-numbered tracks as the bit-cell density changes. */
pub fn sectors_per_track(track: u8) -> u8 {
	match track {
		1..=17 => 21,
		18..=24 => 19,
		25..=30 => 18,
		_ => 17,
	}
}

/* Commodore GCR maps each four-bit nibble to a five-bit code with bounded zero runs, allowing the read separator to retain clock recovery. */
/* Four payload bytes become five GCR bytes by translating each nibble into a legal five-bit symbol with bounded zero runs. */
pub fn encode_group(input: &[u8; 4]) -> [u8; 5] {
	let mut bits = 0u64;
	for &byte in input {
		bits = (bits << 5) | u64::from(GCR_ENCODE[(byte >> 4) as usize]);
		bits = (bits << 5) | u64::from(GCR_ENCODE[(byte & 0x0F) as usize]);
	}
	[
		(bits >> 32) as u8,
		(bits >> 24) as u8,
		(bits >> 16) as u8,
		(bits >> 8) as u8,
		bits as u8,
	]
}

fn gcr_decode(code: u8) -> Option<u8> {
	GCR_ENCODE.iter().position(|&entry| entry == code).map(|value| value as u8)
}

/* Decoding rejects symbols absent from the Commodore 4-to-5 table instead of silently manufacturing sector data. */
pub fn decode_group(input: &[u8; 5]) -> Option<[u8; 4]> {
	let bits = (u64::from(input[0]) << 32)
		| (u64::from(input[1]) << 24)
		| (u64::from(input[2]) << 16)
		| (u64::from(input[3]) << 8)
		| u64::from(input[4]);
	let nibbles = [
		gcr_decode(((bits >> 35) & 0x1F) as u8)?,
		gcr_decode(((bits >> 30) & 0x1F) as u8)?,
		gcr_decode(((bits >> 25) & 0x1F) as u8)?,
		gcr_decode(((bits >> 20) & 0x1F) as u8)?,
		gcr_decode(((bits >> 15) & 0x1F) as u8)?,
		gcr_decode(((bits >> 10) & 0x1F) as u8)?,
		gcr_decode(((bits >> 5) & 0x1F) as u8)?,
		gcr_decode((bits & 0x1F) as u8)?,
	];
	Some([
		(nibbles[0] << 4) | nibbles[1],
		(nibbles[2] << 4) | nibbles[3],
		(nibbles[4] << 4) | nibbles[5],
		(nibbles[6] << 4) | nibbles[7],
	])
}

fn decode_gcr_block(gcr: &[u8], output_length: usize) -> Option<Vec<u8>> {
	let mut output = Vec::with_capacity(output_length);
	for chunk in gcr.chunks_exact(5) {
		let group: [u8; 5] = chunk.try_into().ok()?;
		output.extend_from_slice(&decode_group(&group)?);
		if output.len() >= output_length {
			output.truncate(output_length);
			return Some(output);
		}
	}
	None
}

fn append_gcr(output: &mut Vec<u8>, data: &[u8]) {
	for chunk in data.chunks(4) {
		let mut group = [0u8; 4];
		group[..chunk.len()].copy_from_slice(chunk);
		output.extend_from_slice(&encode_group(&group));
	}
}

fn track_capacity(track: u8) -> usize {
	match track {
		1..=17 => 7692,
		18..=24 => 7142,
		25..=30 => 6666,
		_ => 6250,
	}
}

pub fn build_track(track: u8, sectors: &[[u8; 256]], id1: u8, id2: u8) -> Vec<u8> {
	build_track_with_errors(track, sectors, &[], id1, id2)
}

/* A logical sector is emitted as sync, header, gap, data and checksum fields. Error codes alter only the corresponding on-disk field so protection and DOS error behaviour remain representable. */
/* Track construction emits sync, header, gap and data blocks in rotational order; optional DOS error codes alter the encoded on-disk evidence rather than a host-side status flag. */
pub fn build_track_with_errors(
	track: u8,
	sectors: &[[u8; 256]],
	errors: &[u8],
	id1: u8,
	id2: u8,
) -> Vec<u8> {
	let count = sectors_per_track(track) as usize;
	let capacity = track_capacity(track);
	let fixed_per_sector = 5 + 10 + 9 + 5 + 325;
	let total_gap = capacity.saturating_sub(fixed_per_sector * count);
	let gap_base = total_gap / count;
	let gap_remainder = total_gap % count;
	let empty = [0u8; 256];
	let mut output = Vec::with_capacity(capacity);

	for sector in 0..count {
		let gap = gap_base + usize::from(sector < gap_remainder);
		let sector_size = fixed_per_sector + gap;
		let source = sectors.get(sector).unwrap_or(&empty);
		let error = errors.get(sector).copied().unwrap_or(0x01);
		let mut encoded = Vec::with_capacity(sector_size);

		if error == 0x03 {
			encoded.resize(sector_size, 0x55);
			output.extend_from_slice(&encoded);
			continue;
		}

		let mut header_id1 = id1;
		let mut header_id2 = id2;
		if error == 0x0B {
			header_id1 ^= 0xFF;
			header_id2 ^= 0xFF;
		}

		if error == 0x02 {
			encoded.resize(24, 0x55);
		} else {
			encoded.extend_from_slice(&[0xFF; 5]);
			let mut header_checksum = (sector as u8) ^ track ^ header_id2 ^ header_id1;
			if error == 0x09 {
				header_checksum ^= 0xFF;
			}
			let header = [
				0x08,
				header_checksum,
				sector as u8,
				track,
				header_id2,
				header_id1,
				0x0F,
				0x0F,
			];
			append_gcr(&mut encoded, &header);
			encoded.extend_from_slice(&[0x55; 9]);
		}

		if error != 0x04 {
			encoded.extend_from_slice(&[0xFF; 5]);
			let mut block = [0u8; 260];
			block[0] = 0x07;
			block[1..257].copy_from_slice(source);
			block[257] = source.iter().fold(0u8, |checksum, byte| checksum ^ byte);
			if error == 0x05 {
				block[257] ^= 0xFF;
			}
			append_gcr(&mut encoded, &block);
		}

		encoded.resize(sector_size, 0x55);
		encoded.truncate(sector_size);
		output.extend_from_slice(&encoded);
	}

	output.resize(capacity, 0x55);
	output.truncate(capacity);
	output
}

fn bit_at(track: &[u8], bit_index: usize) -> u8 {
	let total_bits = track.len() * 8;
	let wrapped = bit_index % total_bits;
	let byte = track[wrapped / 8];
	let shift = 7 - (wrapped % 8);
	(byte >> shift) & 1
}

fn read_circular_bytes(track: &[u8], start_bit: usize, length: usize) -> Vec<u8> {
	let mut output = vec![0u8; length];

	for (byte_index, slot) in output.iter_mut().enumerate() {
		let mut value = 0u8;
		for bit in 0..8 {
			value = (value << 1) | bit_at(track, start_bit + byte_index * 8 + bit);
		}
		*slot = value;
	}

	output
}

fn find_sync_end(track: &[u8], start_bit: usize, limit_bits: usize) -> Option<usize> {
	let total_bits = track.len() * 8;
	if total_bits == 0 {
		return None;
	}

	let mut ones = 0usize;
	for offset in 0..limit_bits {
		let position = start_bit + offset;
		if bit_at(track, position) != 0 {
			ones += 1;
		} else {
			if ones >= 10 {
				return Some(position);
			}
			ones = 0;
		}
	}

	None
}

/* Decoding searches the circular bitstream for sync and validates headers, checksums and sector identity instead of assuming byte-aligned canonical tracks. */
/* Decoding begins at an arbitrary bit position and walks the circular track, allowing sector recovery without assuming a physical index hole. */
pub fn decode_track_from(
	track: &[u8],
	track_num: u8,
	start_bit: usize,
) -> Option<Vec<Option<[u8; 256]>>> {
	if track.is_empty() {
		return None;
	}

	let total_bits = track.len() * 8;
	let count = sectors_per_track(track_num) as usize;
	let mut sectors = vec![None; count];
	let scan_start = start_bit % total_bits;
	let scan_end = scan_start + total_bits;
	let mut scan_bit = scan_start;

	while scan_bit < scan_end {
		let Some(header_start) = find_sync_end(track, scan_bit, scan_end + 16 - scan_bit) else {
			break;
		};

		let header_gcr = read_circular_bytes(track, header_start, 10);
		let Some(header) = decode_gcr_block(&header_gcr, 8) else {
			scan_bit = header_start + 1;
			continue;
		};

		if header[0] != 0x08
			|| header[3] != track_num
			|| header[1] != header[2] ^ header[3] ^ header[4] ^ header[5]
		{
			scan_bit = header_start + 1;
			continue;
		}

		let sector = header[2] as usize;
		if sector >= count {
			scan_bit = header_start + 1;
			continue;
		}

		let data_search_start = header_start + 80;
		let data_search_limit = 8 * 96;
		let Some(data_start) = find_sync_end(track, data_search_start, data_search_limit) else {
			scan_bit = header_start + 1;
			continue;
		};

		let marker_gcr = read_circular_bytes(track, data_start, 5);
		let Some(marker) = decode_gcr_block(&marker_gcr, 4) else {
			scan_bit = header_start + 1;
			continue;
		};
		if marker[0] == 0x08 {
			scan_bit = data_start + 1;
			continue;
		}

		let data_gcr = read_circular_bytes(track, data_start, 325);
		let Some(block) = decode_gcr_block(&data_gcr, 260) else {
			scan_bit = header_start + 1;
			continue;
		};

		if block[0] != 0x07 {
			scan_bit = header_start + 1;
			continue;
		}

		let checksum = block[1..257].iter().fold(0u8, |value, byte| value ^ byte);
		if checksum != block[257] {
			scan_bit = header_start + 1;
			continue;
		}

		let mut data = [0u8; 256];
		data.copy_from_slice(&block[1..257]);
		sectors[sector] = Some(data);

		if sectors.iter().all(Option::is_some) {
			break;
		}

		scan_bit = data_start + 325 * 8;
	}

	Some(sectors)
}