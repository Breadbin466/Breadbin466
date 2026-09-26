// =======================================================
// src/datassette/image.rs — TAP pulse timeline
// =======================================================

use super::constants::*;
use std::io::{Error, ErrorKind, Result};

/* Absolute transition positions preserve the unrecorded tail when a new
 * recording overwrites only part of a cassette. TAP stores intervals;
 * conversion takes place at mounting and persistence, never per cycle. */
pub(super) struct TapeImage {
	pub edges: Vec<u64>,
}

impl TapeImage {
	pub fn decode(data: &[u8]) -> Option<Self> {
		if data.len() < TAP_HEADER_SIZE
			|| data.len() > MAX_TAPE_SIZE
			|| &data[..12] != TAP_SIGNATURE
			|| data[13..16] != [0, 0, 0]
			|| !matches!(data[TAP_VERSION_OFFSET], TAP_VERSION_0 | TAP_VERSION_1)
			|| u32::from_le_bytes(data[16..20].try_into().ok()?) as usize
				!= data.len() - TAP_HEADER_SIZE
		{
			return None;
		}
		let mut edges = Vec::new();
		let mut cursor = TAP_HEADER_SIZE;
		let mut position = 0u64;
		while cursor < data.len() {
			let byte = data[cursor];
			cursor += 1;
			let cycles = if byte != 0 {
				u32::from(byte) * TAP_SHORT_PULSE_SCALE
			} else if data[TAP_VERSION_OFFSET] == TAP_VERSION_0 {
				/* Legacy overflow records have no exact duration. */
				(TAP_MAX_SHORT_PULSE + 1) * TAP_SHORT_PULSE_SCALE
			} else {
				let bytes = data.get(cursor..cursor + TAP_EXTENDED_PULSE_SIZE)?;
				cursor += TAP_EXTENDED_PULSE_SIZE;
				u32::from_le_bytes([bytes[0], bytes[1], bytes[2], 0])
			};
			if cycles == 0 || edges.len() >= MAX_TAPE_PULSES {
				return None;
			}
			position = position.checked_add(u64::from(cycles))?;
			edges.push(position);
		}
		Some(Self { edges })
	}

	/* Modified media uses TAP v1. Eight-cycle records are used only when
	 * exact; extended records retain all other cycle counts, including
	 * sub-eight-cycle intervals. No rounding accumulates in turbo saves. */
	pub fn encode(&self) -> Result<Vec<u8>> {
		let mut data = b"C64-TAPE-RAW\x01\0\0\0\0\0\0\0".to_vec();
		let mut previous = 0u64;
		for &edge in &self.edges {
			let duration = edge - previous;
			if duration == 0 || duration > u64::from(TAP_MAX_EXTENDED_PULSE) {
				return Err(Error::new(
					ErrorKind::InvalidData,
					"The recorded interval cannot be represented exactly in TAP v1; the recording remains in memory",
				));
			}
			let short = duration / u64::from(TAP_SHORT_PULSE_SCALE);
			if duration % u64::from(TAP_SHORT_PULSE_SCALE) == 0
				&& short <= u64::from(TAP_MAX_SHORT_PULSE)
			{
				data.push(short as u8);
			} else {
				data.push(0);
				data.extend_from_slice(&(duration as u32).to_le_bytes()[..3]);
			}
			previous = edge;
		}
		if data.len() > MAX_TAPE_SIZE {
			return Err(Error::other("TAP image exceeds the supported size"));
		}
		let size = (data.len() - TAP_HEADER_SIZE) as u32;
		data[TAP_DATA_SIZE_OFFSET..TAP_DATA_SIZE_OFFSET + 4].copy_from_slice(&size.to_le_bytes());
		Ok(data)
	}
}