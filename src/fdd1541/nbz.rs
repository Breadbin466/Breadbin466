// =======================================================
// src/fdd1541/nbz.rs — NBZ stream encoding and decoding
// =======================================================

use super::constants::{
	NIB_HEADER_LENGTH, NIB_MAX_DECOMPRESSED_LENGTH, NIB_SIGNATURE, NIB_TRACK_LENGTH,
};

const NBZ_MIN_MATCH: usize = 4;
const NBZ_SEARCH_DEPTH: usize = 96;
const NBZ_WINDOW: usize = 100_000;
const NBZ_HASH_BUCKETS: usize = 65_536;

/* NBZ compresses a NIB capture with an escape byte, literal runs and backward references while retaining the exact decompressed byte stream. */
pub fn encode_nbz(input: &[u8]) -> Option<Vec<u8>> {
	if input.is_empty() || input.len() > NIB_MAX_DECOMPRESSED_LENGTH {
		return None;
	}

	let escape = least_frequent_byte(input);
	let mut output = Vec::with_capacity(input.len().saturating_add(1));
	output.push(escape);

	let mut chains = vec![Vec::<usize>::new(); NBZ_HASH_BUCKETS];
	let mut cursor = 0usize;
	while cursor < input.len() {
		let candidate = best_nbz_match(input, cursor, &chains);
		if let Some((distance, length)) = candidate.filter(|&(distance, length)| {
			reference_cost(length, distance) < literal_cost(&input[cursor..cursor + length], escape)
		}) {
			output.push(escape);
			write_nbz_integer(length, &mut output);
			write_nbz_integer(distance, &mut output);

			let end = cursor + length;
			while cursor < end {
				index_nbz_position(input, cursor, &mut chains);
				cursor += 1;
			}
		} else {
			let value = input[cursor];
			output.push(value);
			if value == escape {
				output.push(0);
			}
			index_nbz_position(input, cursor, &mut chains);
			cursor += 1;
		}
	}

	Some(output)
}

/* Decoding is bounded by the maximum legal NIB size so malformed references cannot expand without limit. */
pub fn decode_nbz(input: &[u8]) -> Option<Vec<u8>> {
	let (&escape, encoded) = input.split_first()?;
	let mut reader = NbzReader::new(encoded);
	let mut output = Vec::with_capacity(NIB_HEADER_LENGTH + 42 * NIB_TRACK_LENGTH);

	while let Some(value) = reader.read_byte() {
		if value != escape {
			push_nbz_byte(&mut output, value)?;
			continue;
		}

		if reader.peek_byte()? == 0 {
			reader.read_byte()?;
			push_nbz_byte(&mut output, escape)?;
			continue;
		}

		let length = reader.read_integer()?;
		let distance = reader.read_integer()?;
		if length == 0 || distance == 0 || distance > output.len() {
			return None;
		}
		let final_length = output.len().checked_add(length)?;
		if final_length > NIB_MAX_DECOMPRESSED_LENGTH {
			return None;
		}

		while output.len() < final_length {
			let source = output.len() - distance;
			output.push(output[source]);
		}
	}

	(output.get(..NIB_SIGNATURE.len()) == Some(NIB_SIGNATURE)).then_some(output)
}

struct NbzReader<'a> {
	data: &'a [u8],
	position: usize,
}

impl<'a> NbzReader<'a> {
	fn new(data: &'a [u8]) -> Self {
		Self { data, position: 0 }
	}

	fn read_byte(&mut self) -> Option<u8> {
		let value = self.data.get(self.position).copied()?;
		self.position += 1;
		Some(value)
	}

	fn peek_byte(&self) -> Option<u8> {
		self.data.get(self.position).copied()
	}

	fn read_integer(&mut self) -> Option<usize> {
		let mut value = 0usize;
		for _ in 0..5 {
			let byte = self.read_byte()?;
			value = value
				.checked_mul(128)?
				.checked_add(usize::from(byte & 0x7f))?;
			if byte & 0x80 == 0 {
				return Some(value);
			}
		}
		None
	}
}

fn push_nbz_byte(output: &mut Vec<u8>, value: u8) -> Option<()> {
	if output.len() >= NIB_MAX_DECOMPRESSED_LENGTH {
		return None;
	}
	output.push(value);
	Some(())
}

fn least_frequent_byte(input: &[u8]) -> u8 {
	let mut counts = [0usize; 256];
	for &value in input {
		counts[usize::from(value)] += 1;
	}
	counts
		.iter()
		.enumerate()
		.min_by_key(|&(value, count)| (*count, value))
		.map(|(value, _)| value as u8)
		.unwrap_or(0)
}

fn index_nbz_position(input: &[u8], position: usize, chains: &mut [Vec<usize>]) {
	let Some(hash) = nbz_hash(input, position) else {
		return;
	};
	let chain = &mut chains[hash];
	chain.push(position);

	let oldest_allowed = position.saturating_sub(NBZ_WINDOW);
	let stale = chain.partition_point(|&candidate| candidate < oldest_allowed);
	if stale > 0 {
		chain.drain(..stale);
	}
}

/* Match selection compares encoded cost rather than raw length, preventing a longer reference from replacing cheaper literals. */
fn best_nbz_match(input: &[u8], position: usize, chains: &[Vec<usize>]) -> Option<(usize, usize)> {
	let hash = nbz_hash(input, position)?;
	let remaining = input.len() - position;
	let mut best = None;

	for &candidate in chains[hash].iter().rev().take(NBZ_SEARCH_DEPTH) {
		let distance = position - candidate;
		if distance == 0 || distance > NBZ_WINDOW {
			continue;
		}

		let mut length = 0usize;
		while length < remaining && input[candidate + length] == input[position + length] {
			length += 1;
		}
		if length < NBZ_MIN_MATCH {
			continue;
		}

		if best.is_none_or(|(_, best_length)| length > best_length) {
			best = Some((distance, length));
			if length == remaining {
				break;
			}
		}
	}

	best
}

fn nbz_hash(input: &[u8], position: usize) -> Option<usize> {
	let bytes = input.get(position..position + 3)?;
	let mixed =
		(usize::from(bytes[0]) * 251) ^ (usize::from(bytes[1]) * 31) ^ usize::from(bytes[2]);
	Some(mixed & (NBZ_HASH_BUCKETS - 1))
}

fn literal_cost(data: &[u8], escape: u8) -> usize {
	data.len() + data.iter().filter(|&&value| value == escape).count()
}

fn reference_cost(length: usize, distance: usize) -> usize {
	1 + nbz_integer_length(length) + nbz_integer_length(distance)
}

fn nbz_integer_length(mut value: usize) -> usize {
	let mut length = 1usize;
	while value >= 128 {
		value >>= 7;
		length += 1;
	}
	length
}

/* Variable-length integers emit seven payload bits per byte and set the continuation bit on every non-final group. */
fn write_nbz_integer(mut value: usize, output: &mut Vec<u8>) {
	let mut groups = [0u8; 10];
	let mut first = groups.len() - 1;
	groups[first] = (value & 0x7f) as u8;
	value >>= 7;
	while value != 0 {
		first -= 1;
		groups[first] = ((value & 0x7f) as u8) | 0x80;
		value >>= 7;
	}
	output.extend_from_slice(&groups[first..]);
}