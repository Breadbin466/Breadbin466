// =======================================================
// src/fdd1541/d7z.rs — D7Z compressed D64 container
// =======================================================

use lzma_rust2::{Lzma2Options, Lzma2Reader, Lzma2Writer};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};

/*
D7Z DISK IMAGE FORMAT
=====================

D7Z is a compressed storage representation of exactly one D64 byte stream.
It deliberately contains no historical metadata: no source filename, time,
creator, operating system, reclaim marker or conversion history. The complete
semantic payload is the D64 byte stream produced by decompression.

Version 1.0 layout, little-endian for the only multi-byte integers:

	Offset  Size  Field
	------  ----  ---------------------------------------------------------
	0x00      8   magic: 44 37 5A 1A 0D 0A 00 00 ("D7Z" + sentinel)
	0x08      1   major version (1)
	0x09      1   minor version (0)
	0x0A      2   header size (48)
	0x0C      4   raw LZMA2 dictionary size in bytes
	0x10     32   SHA-256 of the complete uncompressed D64 byte stream
	0x30    ...   one raw LZMA2 stream, ending at end-of-file

Every field exists because a decoder needs it. Raw LZMA2 does not carry its
required dictionary size in-band, so D7Z stores that value. D64 geometry and
error-table presence are deliberately not duplicated in the header: they are
already determined unambiguously by the size of the decoded D64 stream. The
compressed and uncompressed sizes are likewise not metadata because the former
is the remainder of the file and the latter is validated against legal D64
sizes after bounded decompression.

Compression has no Commodore filesystem semantics. Encoding never inspects the
BAM, clears deleted data, normalises free sectors or otherwise changes the D64.
Reclaim Space is a separate user-requested transformation. If reclaiming was
performed before D7Z encoding, the resulting D64 bytes are simply the input to
the codec and no history of that operation is retained.

Version 1 uses raw LZMA2. Breadbin encodes with preset 9 and a 1 MiB dictionary,
prioritising the smallest practical file over compression time. The encoding
strategy is not canonical: another compliant encoder may choose different
search parameters or a different valid dictionary size. Decoding is canonical
because the stored dictionary size and SHA-256 identify how to decode and which
D64 byte stream must result.

A decoder MUST bound decompression before allocating arbitrary output, MUST
accept only D64 sizes already supported by the 1541 subsystem, MUST reject an
unsupported D7Z version, and MUST verify SHA-256 before exposing the disk image
to the emulator.
*/

const MAGIC: [u8; 8] = [b'D', b'7', b'Z', 0x1A, 0x0D, 0x0A, 0x00, 0x00];
const VERSION_MAJOR: u8 = 1;
const VERSION_MINOR: u8 = 0;
const HEADER_SIZE: usize = 48;
const DICTIONARY_SIZE: u32 = 1024 * 1024;
const MAX_D64_SIZE: usize = 256 * 1024;

pub(crate) fn is_d7z(data: &[u8]) -> bool {
	data.get(..MAGIC.len()) == Some(MAGIC.as_slice())
}

fn supported_d64_size(length: usize) -> bool {
	for tracks in 35..=42u8 {
		let sectors: usize = (1..=tracks)
			.map(|track| super::gcr::sectors_per_track(track) as usize)
			.sum();
		let payload = sectors * 256;
		if length == payload || length == payload + sectors {
			return true;
		}
	}
	false
}

pub(crate) fn encode(d64: &[u8]) -> Option<Vec<u8>> {
	if !supported_d64_size(d64.len()) {
		return None;
	}

	let mut options = Lzma2Options::with_preset(9);
	options.lzma_options.dict_size = DICTIONARY_SIZE;
	let mut writer = Lzma2Writer::new(Vec::new(), options);
	writer.write_all(d64).ok()?;
	let payload = writer.finish().ok()?;

	let digest = Sha256::digest(d64);
	let mut output = Vec::with_capacity(HEADER_SIZE + payload.len());
	output.extend_from_slice(&MAGIC);
	output.push(VERSION_MAJOR);
	output.push(VERSION_MINOR);
	output.extend_from_slice(&(HEADER_SIZE as u16).to_le_bytes());
	output.extend_from_slice(&DICTIONARY_SIZE.to_le_bytes());
	output.extend_from_slice(&digest);
	output.extend_from_slice(&payload);
	Some(output)
}

pub(crate) fn decode(data: &[u8]) -> Option<Vec<u8>> {
	if data.len() <= HEADER_SIZE || !is_d7z(data) {
		return None;
	}
	if data[8] != VERSION_MAJOR || data[9] != VERSION_MINOR {
		return None;
	}
	let header_size = u16::from_le_bytes([data[10], data[11]]) as usize;
	if header_size != HEADER_SIZE {
		return None;
	}
	let dictionary_size = u32::from_le_bytes(data[12..16].try_into().ok()?);
	if !(4096..=64 * 1024 * 1024).contains(&dictionary_size) {
		return None;
	}
	let expected_digest = data.get(16..48)?;
	let mut reader = Lzma2Reader::new(&data[header_size..], dictionary_size, None);
	let mut output = Vec::new();
	reader
		.by_ref()
		.take((MAX_D64_SIZE + 1) as u64)
		.read_to_end(&mut output)
		.ok()?;
	if output.len() > MAX_D64_SIZE || !supported_d64_size(output.len()) {
		return None;
	}
	if !reader.into_inner().is_empty() {
		return None;
	}
	if Sha256::digest(&output).as_slice() != expected_digest {
		return None;
	}
	Some(output)
}