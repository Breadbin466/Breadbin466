// =======================================================
// src/fdd1541/convert.rs — Disk-image representation conversion
// =======================================================

use super::constants::{G64_HEADER_LEN, G64_MAX_HALF_TRACKS, G64_SIGNATURE};
use super::disk_image::ImageFormat;
use super::{d7z, d64, g64, gcr, nib, reclaim};

/*
CONVERSION SEMANTICS
====================

Conversion never edits the source image. D64 and D7Z are equivalent logical
representations, while NIB and NBZ are equivalent capture representations.
Reclaim Space, when requested, is first applied to an in-memory copy of the
source representation. An invalid or ambiguous BAM aborts the conversion because
that destructive option was explicitly requested; a valid reclaim operation that
finds no bytes to clear does not otherwise prevent conversion.

Logical-to-raw conversion synthesises the physical evidence represented by D64,
including its optional DOS error table. Raw-to-logical conversion is inherently
potentially lossy because D64 cannot represent arbitrary sync lengths, gaps,
half-track data, duplicate sectors, weak or malformed GCR, or detailed timing.
NIB-to-G64 conversion may additionally collapse differences between captured
revolutions. G64-to-NIB can preserve circular track bytes but cannot represent
G64 per-byte speed tables, so those are reduced to one density value per track.

The UI asks for confirmation before a conversion whose destination cannot retain
all information represented by the source. Conversion code still remains strict
about structural validity: permission to lose unsupported representation detail
is not permission to manufacture a malformed destination.
*/

struct RawTracks {
	tracks: Vec<Vec<u8>>,
	densities: Vec<u8>,
}

fn d64_track_count(data: &[u8]) -> Option<u8> {
	for tracks in 35..=42u8 {
		let sectors: usize = (1..=tracks)
			.map(|track| gcr::sectors_per_track(track) as usize)
			.sum();
		let logical = sectors * 256;
		if data.len() == logical || data.len() == logical + sectors {
			return Some(tracks);
		}
	}
	None
}

fn total_sectors(tracks: u8) -> usize {
	(1..=tracks)
		.map(|track| gcr::sectors_per_track(track) as usize)
		.sum()
}

fn d64_to_raw(data: &[u8]) -> Option<RawTracks> {
	let tracks = d64_track_count(data)?;
	let logical_size = total_sectors(tracks) * 256;
	let errors = data.get(logical_size..).unwrap_or(&[]);
	let bam_offset = d64::sector_offset(18, 0);
	let id1 = data.get(bam_offset + 0xa2).copied().unwrap_or(0x41);
	let id2 = data.get(bam_offset + 0xa3).copied().unwrap_or(0x41);
	let mut raw_tracks = vec![Vec::new(); G64_MAX_HALF_TRACKS];
	let mut densities = vec![0u8; G64_MAX_HALF_TRACKS];
	let mut error_index = 0usize;

	for track in 1..=tracks {
		let count = gcr::sectors_per_track(track) as usize;
		let mut sectors = Vec::with_capacity(count);
		let mut track_errors = Vec::with_capacity(count);
		for sector in 0..count {
			let offset = d64::sector_offset(track, sector as u8);
			let mut payload = [0u8; 256];
			payload.copy_from_slice(data.get(offset..offset + 256)?);
			sectors.push(payload);
			track_errors.push(errors.get(error_index).copied().unwrap_or(0x01));
			error_index += 1;
		}
		let half = (usize::from(track) - 1) * 2;
		raw_tracks[half] = gcr::build_track_with_errors(track, &sectors, &track_errors, id1, id2);
		densities[half] = g64::standard_density(track);
		if half + 1 < densities.len() {
			densities[half + 1] = densities[half];
		}
	}
	Some(RawTracks {
		tracks: raw_tracks,
		densities,
	})
}

fn raw_to_d64(raw: &RawTracks) -> Option<Vec<u8>> {
	/* A D64 without a readable BAM would have a legal byte count but would not be a trustworthy DOS image. Require track 18 sector 0 before manufacturing the destination; other missing sectors remain representable as explicitly lossy zero fill. */
	let bam_track = raw
		.tracks
		.get((18 - 1) * 2)
		.filter(|track| !track.is_empty())?;
	let bam_sectors = gcr::decode_track_from(bam_track, 18, 0)?;
	if bam_sectors
		.first()
		.and_then(|sector| sector.as_ref())
		.is_none()
	{
		return None;
	}

	let highest = (0..raw.tracks.len())
		.rev()
		.find(|index| index % 2 == 0 && !raw.tracks[*index].is_empty())
		.map(|index| index / 2 + 1)?;
	if !(35..=42).contains(&highest) {
		return None;
	}
	let tracks = highest as u8;
	let mut output = vec![0u8; total_sectors(tracks) * 256];
	for track in 1..=tracks {
		let half = (usize::from(track) - 1) * 2;
		let Some(track_data) = raw
			.tracks
			.get(half)
			.filter(|track_data| !track_data.is_empty())
		else {
			continue;
		};
		let Some(sectors) = gcr::decode_track_from(track_data, track, 0) else {
			continue;
		};
		for (sector, payload) in sectors.into_iter().enumerate() {
			let Some(payload) = payload else {
				continue;
			};
			let offset = d64::sector_offset(track, sector as u8);
			output
				.get_mut(offset..offset + 256)?
				.copy_from_slice(&payload);
		}
	}
	Some(output)
}

fn g64_track_ranges(data: &[u8]) -> Option<Vec<Option<(usize, usize)>>> {
	if data.len() < G64_HEADER_LEN
		|| data.get(..G64_SIGNATURE.len()) != Some(G64_SIGNATURE.as_slice())
	{
		return None;
	}
	let half_tracks = usize::from(data[9]);
	let max_track_size = u16::from_le_bytes([data[10], data[11]]) as usize;
	if half_tracks == 0 || half_tracks > G64_MAX_HALF_TRACKS || max_track_size == 0 {
		return None;
	}
	let tables_end = G64_HEADER_LEN.checked_add(half_tracks.checked_mul(8)?)?;
	if tables_end > data.len() {
		return None;
	}
	let mut ranges = vec![None; G64_MAX_HALF_TRACKS];
	for half in 0..half_tracks {
		let entry = G64_HEADER_LEN + half * 4;
		let offset = u32::from_le_bytes(data.get(entry..entry + 4)?.try_into().ok()?) as usize;
		if offset == 0 {
			continue;
		}
		let length = u16::from_le_bytes(data.get(offset..offset + 2)?.try_into().ok()?) as usize;
		let start = offset.checked_add(2)?;
		let end = start.checked_add(length)?;
		if length == 0 || length > max_track_size || end > data.len() {
			return None;
		}
		ranges[half] = Some((start, length));
	}
	Some(ranges)
}

fn g64_to_raw(data: &[u8]) -> Option<(RawTracks, bool)> {
	let ranges = g64_track_ranges(data)?;
	let half_tracks = usize::from(data[9]);
	let speed_table = G64_HEADER_LEN + half_tracks * 4;
	let max_track_size = u16::from_le_bytes([data[10], data[11]]) as usize;
	let speed_block_length = max_track_size.div_ceil(4).max(1);
	let mut tracks = vec![Vec::new(); G64_MAX_HALF_TRACKS];
	let mut densities = vec![0u8; G64_MAX_HALF_TRACKS];
	let mut variable_speed = false;

	for half in 0..half_tracks {
		if let Some((start, length)) = ranges[half] {
			tracks[half] = data[start..start + length].to_vec();
		}
		let entry = speed_table + half * 4;
		let value = u32::from_le_bytes(data.get(entry..entry + 4)?.try_into().ok()?) as usize;
		if value < 4 {
			densities[half] = value as u8;
		} else {
			if value.checked_add(speed_block_length)? > data.len() {
				return None;
			}
			variable_speed = true;
			let track = (half / 2 + 1).min(u8::MAX as usize) as u8;
			densities[half] = g64::standard_density(track);
		}
	}
	Some((RawTracks { tracks, densities }, variable_speed))
}

fn raw_to_g64(raw: &RawTracks) -> Option<Vec<u8>> {
	let half_tracks = raw
		.tracks
		.iter()
		.rposition(|track| !track.is_empty())?
		.checked_add(1)?;
	if half_tracks == 0 || half_tracks > G64_MAX_HALF_TRACKS {
		return None;
	}
	let max_track_size = raw.tracks[..half_tracks].iter().map(Vec::len).max()?;
	if max_track_size == 0 || max_track_size > u16::MAX as usize {
		return None;
	}
	let table_size = half_tracks * 4;
	let data_start = G64_HEADER_LEN + table_size * 2;
	let mut output = vec![0u8; data_start];
	output[..G64_SIGNATURE.len()].copy_from_slice(G64_SIGNATURE);
	output[8] = 0;
	output[9] = half_tracks as u8;
	output[10..12].copy_from_slice(&(max_track_size as u16).to_le_bytes());

	for half in 0..half_tracks {
		let track = &raw.tracks[half];
		if !track.is_empty() {
			let offset = output.len();
			output.extend_from_slice(&(track.len() as u16).to_le_bytes());
			output.extend_from_slice(track);
			output.resize(output.len() + max_track_size - track.len(), 0x55);
			let table = G64_HEADER_LEN + half * 4;
			output[table..table + 4].copy_from_slice(&(offset as u32).to_le_bytes());
		}
		let speed = G64_HEADER_LEN + table_size + half * 4;
		let density = raw.densities.get(half).copied().unwrap_or(0) & 0x03;
		output[speed..speed + 4].copy_from_slice(&u32::from(density).to_le_bytes());
	}
	Some(output)
}

fn nib_to_raw(data: &[u8]) -> Option<RawTracks> {
	let image = nib::decode_nib(data)?;
	Some(RawTracks {
		tracks: image.rings,
		densities: image.zones,
	})
}

fn raw_to_nib(raw: &RawTracks) -> Option<Vec<u8>> {
	nib::encode_nib(&raw.tracks, &raw.densities)
}

fn decode_logical(format: ImageFormat, bytes: &[u8]) -> Option<Vec<u8>> {
	match format {
		ImageFormat::D64 => d64_track_count(bytes).map(|_| bytes.to_vec()),
		ImageFormat::D7z => d7z::decode(bytes),
		_ => None,
	}
}

fn encode_logical(format: ImageFormat, bytes: &[u8]) -> Option<Vec<u8>> {
	match format {
		ImageFormat::D64 => Some(bytes.to_vec()),
		ImageFormat::D7z => d7z::encode(bytes),
		_ => None,
	}
}

fn decode_raw(format: ImageFormat, bytes: &[u8]) -> Option<RawTracks> {
	match format {
		ImageFormat::G64 => g64_to_raw(bytes).map(|value| value.0),
		ImageFormat::Nib => nib_to_raw(bytes),
		ImageFormat::Nbz => nib::decode_nbz(bytes).and_then(|decoded| nib_to_raw(&decoded)),
		ImageFormat::D64 | ImageFormat::D7z => {
			decode_logical(format, bytes).and_then(|logical| d64_to_raw(&logical))
		}
	}
}

fn encode_raw(format: ImageFormat, raw: &RawTracks) -> Option<Vec<u8>> {
	match format {
		ImageFormat::G64 => raw_to_g64(raw),
		ImageFormat::Nib => raw_to_nib(raw),
		ImageFormat::Nbz => raw_to_nib(raw).and_then(|nib_data| nib::encode_nbz(&nib_data)),
		_ => None,
	}
}

pub(crate) fn warning(
	source: ImageFormat,
	destination: ImageFormat,
	source_bytes: &[u8],
) -> Option<&'static str> {
	if source == destination {
		return None;
	}
	if source.is_raw() && destination.is_logical() {
		return Some(match source {
			ImageFormat::G64 => {
				"G64 raw track layout, timing, sync/gap structure, non-standard sectors and protection data cannot all be represented by D64 and may be lost."
			}
			ImageFormat::Nib | ImageFormat::Nbz => {
				"NIB capture-level track data, non-standard sectors and protection information cannot all be represented by D64 and may be lost."
			}
			_ => unreachable!(),
		});
	}
	if matches!(source, ImageFormat::Nib | ImageFormat::Nbz) && destination == ImageFormat::G64 {
		return Some(
			"NIB may contain differences across captured revolutions that G64 cannot preserve; the recovered circular track representation will be used.",
		);
	}
	if source == ImageFormat::G64 && matches!(destination, ImageFormat::Nib | ImageFormat::Nbz) {
		if g64_to_raw(source_bytes).is_some_and(|(_, variable_speed)| variable_speed) {
			return Some(
				"This G64 uses per-byte speed-zone data that NIB cannot represent; track bytes are preserved where possible but speed information will be reduced to track density.",
			);
		}
	}
	None
}

pub(crate) fn bytes(
	source: ImageFormat,
	destination: ImageFormat,
	bytes: &[u8],
	reclaim_space: bool,
) -> Option<Vec<u8>> {
	/* Same-format output is intentional. It supports an ordinary image copy and,
	 * more importantly, lets Reclaim Space rewrite an image in its existing
	 * representation. The destructive transformation remains explicit and is
	 * applied only to this private conversion copy. */
	if source == destination {
		return if reclaim_space {
			reclaim::transform_for_conversion(source, bytes)
		} else {
			Some(bytes.to_vec())
		};
	}
	let reclaimed;
	let input = if reclaim_space {
		reclaimed = reclaim::transform_for_conversion(source, bytes)?;
		reclaimed.as_slice()
	} else {
		bytes
	};

	if source.is_logical() && destination.is_logical() {
		return encode_logical(destination, &decode_logical(source, input)?);
	}
	if source == ImageFormat::Nib && destination == ImageFormat::Nbz {
		return nib::encode_nbz(input);
	}
	if source == ImageFormat::Nbz && destination == ImageFormat::Nib {
		return nib::decode_nbz(input);
	}
	if source.is_logical() && destination.is_raw() {
		let logical = decode_logical(source, input)?;
		return encode_raw(destination, &d64_to_raw(&logical)?);
	}
	if source.is_raw() && destination.is_logical() {
		let raw = decode_raw(source, input)?;
		return encode_logical(destination, &raw_to_d64(&raw)?);
	}
	if source.is_raw() && destination.is_raw() {
		return encode_raw(destination, &decode_raw(source, input)?);
	}
	None
}