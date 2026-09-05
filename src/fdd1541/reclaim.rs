// =======================================================
// src/fdd1541/reclaim.rs — Explicit destructive recovery of DOS-free sector contents
// =======================================================

use super::constants::{
	G64_HEADER_LEN, G64_MAX_HALF_TRACKS, G64_SIGNATURE, NIB_HALF_TRACK_COUNT, NIB_HEADER_LENGTH,
	NIB_SIGNATURE, NIB_TRACK_LENGTH,
};
use super::disk_image::ImageFormat;
use super::{d7z, d64, gcr, nib};

/*
RECLAIM SPACE
=============

Reclaim Space is an explicitly destructive filesystem operation. A BAM bit says
that Commodore DOS may allocate a sector; it does not prove that the old bytes in
that sector are semantically irrelevant. Software and disk protections may read
nominally free sectors directly. Reclaiming therefore occurs only after explicit
user request and is never folded into ordinary loading, saving or compression.

The central preservation rule is:

	uncertainty preserves data

The BAM allocation bitmap is the authority for deciding which standard DOS data
sectors are free. Reclaiming validates only the BAM and linked directory structure
needed to interpret that allocation map; it deliberately does not reinterpret file
chains to overrule a BAM-free sector. This distinction is essential because the
operation exists specifically to discard bytes which DOS advertises as reusable.
The cached per-track free-count byte is advisory. Standard BAM entries describe
tracks 1 through 35. Extended tracks in 40- and 42-track images are intentionally
left untouched because D64 extensions do not share one universally authoritative
BAM convention.

A reclaimed logical sector becomes exactly 256 zero bytes. D64 and D7Z therefore
change only those sector payloads. G64 reclaiming locates the existing valid GCR
header/data pair for a free sector and requires exactly one physical occurrence
before changing only the data block and its checksum. NIB captures normally hold
several revolutions in each 8192-byte capture block, so repeated occurrences are
accepted only when every valid copy agrees on the existing sector payload; every
such copy is then changed together. Sync, gaps, unrelated GCR bytes, speed data,
half-tracks and protection structures are never rebuilt merely to reclaim space.

NBZ and D7Z are storage compression layers. They are decoded, transformed using
the exact rules of their underlying NIB or D64 representation, then recompressed.
No reclaim history is written into either format.

Conversion applies reclaiming to a private in-memory copy. Transactional output
replacement and validation remain the responsibility of the common conversion path.
*/

const STANDARD_BAM_TRACKS: u8 = 35;
const ZERO_SECTOR: [u8; 256] = [0; 256];

struct ReclaimResult {
	output: Vec<u8>,
	free_sectors: usize,
	changed_sectors: usize,
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

/* The bitmap is authoritative for allocation. The per-track free-count byte is a
 * DOS convenience value and real D64 images sometimes leave it stale after tools or
 * unusual write sequences. Rejecting an otherwise coherent image solely because that
 * cached count disagrees made Reclaim Space fail on ordinary disks. Safety validation
 * is limited to the BAM and linked directory structure; BAM-free data sectors remain
 * intentionally reclaimable. Unused bitmap bits remain ignored. */
fn free_sector_map_from_bam(bam: &[u8; 256], highest_track: u8) -> Option<Vec<(u8, u8)>> {
	let mut free = Vec::new();
	for track in 1..=highest_track.min(STANDARD_BAM_TRACKS) {
		let entry = 4 + (usize::from(track) - 1) * 4;
		let count = gcr::sectors_per_track(track);
		let bitmap = u32::from(bam[entry + 1])
			| (u32::from(bam[entry + 2]) << 8)
			| (u32::from(bam[entry + 3]) << 16);
		for sector in 0..count {
			if bitmap & (1u32 << sector) != 0 {
				free.push((track, sector));
			}
		}
	}
	Some(free)
}

fn valid_sector_address(track: u8, sector: u8, highest_track: u8) -> bool {
	track != 0 && track <= highest_track && sector < gcr::sectors_per_track(track)
}

/* Reclaiming uses the BAM allocation bitmap as the user's explicit authority for
 * data sectors, but it still protects the filesystem structures required to interpret
 * that BAM. The BAM sector itself and every linked directory sector must therefore be
 * allocated and readable. File data chains are intentionally NOT cross-validated
 * against the BAM: software may deliberately keep useful bytes in sectors which DOS
 * advertises as free, and Reclaim Space is specifically the user-requested operation
 * that discards those bytes. Treating such a sector as evidence that the BAM is
 * corrupt made ordinary and intentionally non-standard disks impossible to reclaim.
 *
 * This is the boundary between structural validity and content policy:
 *
 *     invalid BAM/directory structure -> refuse the whole operation
 *     BAM-free data sector            -> eligible for reclaim
 *
 * The warning shown to the user exists precisely because the latter may destroy
 * software-visible data which lives outside normal DOS allocation. */
fn protected_filesystem_sectors(
	bam: &[u8; 256],
	highest_track: u8,
	mut read_sector: impl FnMut(u8, u8) -> Option<[u8; 256]>,
) -> Option<Vec<(u8, u8)>> {
	/*
	The BAM bitmap is the authority for ordinary data-sector allocation, but the
	filesystem sectors used to interpret that bitmap are never reclaimable. Real
	images produced by old tools can contain stale BAM bits for 18/0 or directory
	sectors while remaining perfectly usable. Treating that bookkeeping mismatch
	as fatal made D7Z -> D7Z reclaim fail on otherwise ordinary images.

	We therefore protect the BAM sector and every sector reached through the live
	directory chain even if a stale BAM bit says that one of them is free. This is
	not a repair of the BAM and it writes no metadata: it is only the minimum
	non-destructive boundary required before clearing BAM-free payload sectors.

	Malformed directory links are still fatal. A link outside the supported disk,
	a loop, or a sector which cannot be read means the set of structural sectors is
	ambiguous, so the whole reclaim operation is refused.
	*/
	let mut protected = vec![(18, 0)];
	let mut directory_track = bam[0];
	let mut directory_sector = bam[1];
	if !valid_sector_address(directory_track, directory_sector, highest_track) {
		return None;
	}

	while directory_track != 0 {
		if !valid_sector_address(directory_track, directory_sector, highest_track)
			|| protected.contains(&(directory_track, directory_sector))
		{
			return None;
		}
		protected.push((directory_track, directory_sector));
		let directory = read_sector(directory_track, directory_sector)?;
		directory_track = directory[0];
		directory_sector = directory[1];
	}

	Some(protected)
}

fn reclaimable_free_sectors(
	bam: &[u8; 256],
	highest_track: u8,
	read_sector: impl FnMut(u8, u8) -> Option<[u8; 256]>,
) -> Option<Vec<(u8, u8)>> {
	let mut free = free_sector_map_from_bam(bam, highest_track)?;
	let protected = protected_filesystem_sectors(bam, highest_track, read_sector)?;
	free.retain(|address| !protected.contains(address));
	Some(free)
}

fn reclaim_d64(mut disk: Vec<u8>) -> Option<ReclaimResult> {
	let tracks = d64_track_count(&disk)?;
	let bam_offset = d64::sector_offset(18, 0);
	let mut bam = [0u8; 256];
	bam.copy_from_slice(disk.get(bam_offset..bam_offset + 256)?);
	let free = reclaimable_free_sectors(&bam, tracks, |track, sector| {
		let offset = d64::sector_offset(track, sector);
		let mut payload = [0u8; 256];
		payload.copy_from_slice(disk.get(offset..offset + 256)?);
		Some(payload)
	})?;
	let mut changed = 0usize;
	for &(track, sector) in &free {
		let offset = d64::sector_offset(track, sector);
		let bytes = disk.get_mut(offset..offset.checked_add(256)?)?;
		if bytes.iter().any(|&byte| byte != 0) {
			bytes.fill(0);
			changed += 1;
		}
	}
	Some(ReclaimResult {
		output: disk,
		free_sectors: free.len(),
		changed_sectors: changed,
	})
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
		let table = G64_HEADER_LEN + half * 4;
		let offset = u32::from_le_bytes(data.get(table..table + 4)?.try_into().ok()?) as usize;
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

fn reclaim_g64(mut image: Vec<u8>) -> Option<ReclaimResult> {
	let ranges = g64_track_ranges(&image)?;
	let (bam_start, bam_length) = ranges.get((18 - 1) * 2).copied().flatten()?;
	let bam = gcr::decode_sector_unique(&image[bam_start..bam_start + bam_length], 18, 0)?;
	let free = reclaimable_free_sectors(&bam, STANDARD_BAM_TRACKS, |track, sector| {
		let (start, length) = ranges
			.get((usize::from(track) - 1) * 2)
			.copied()
			.flatten()?;
		gcr::decode_sector_unique(&image[start..start + length], track, sector)
	})?;
	let mut changed = 0usize;

	for &(track, sector) in &free {
		let Some((start, length)) = ranges.get((usize::from(track) - 1) * 2).copied().flatten()
		else {
			continue;
		};
		let Some(existing) =
			gcr::decode_sector_unique(&image[start..start + length], track, sector)
		else {
			continue;
		};
		if existing.iter().all(|&byte| byte == 0) {
			continue;
		}
		if gcr::replace_sector_data_unique(
			&mut image[start..start + length],
			track,
			sector,
			&ZERO_SECTOR,
		) {
			changed += 1;
		}
	}

	Some(ReclaimResult {
		output: image,
		free_sectors: free.len(),
		changed_sectors: changed,
	})
}

fn nib_track_ranges(data: &[u8]) -> Option<Vec<Option<(usize, usize)>>> {
	if data.len() < NIB_HEADER_LENGTH
		|| data.get(..NIB_SIGNATURE.len()) != Some(NIB_SIGNATURE.as_slice())
	{
		return None;
	}
	let mut entries = Vec::new();
	let mut header = 0x10usize;
	while header + 1 < NIB_HEADER_LENGTH {
		let encoded = data[header];
		if encoded == 0 {
			break;
		}
		if encoded < 2 {
			return None;
		}
		let index = usize::from(encoded - 2);
		if index >= NIB_HALF_TRACK_COUNT || entries.contains(&index) {
			return None;
		}
		entries.push(index);
		header += 2;
	}
	if entries.is_empty() {
		return None;
	}
	let expected = NIB_HEADER_LENGTH.checked_add(entries.len().checked_mul(NIB_TRACK_LENGTH)?)?;
	if expected > data.len() {
		return None;
	}
	let mut ranges = vec![None; NIB_HALF_TRACK_COUNT];
	for (block, index) in entries.into_iter().enumerate() {
		ranges[index] = Some((
			NIB_HEADER_LENGTH + block * NIB_TRACK_LENGTH,
			NIB_TRACK_LENGTH,
		));
	}
	Some(ranges)
}

fn reclaim_nib(mut image: Vec<u8>) -> Option<ReclaimResult> {
	let ranges = nib_track_ranges(&image)?;
	let (bam_start, bam_length) = ranges.get((18 - 1) * 2).copied().flatten()?;
	let bam = gcr::decode_sector_consistent(&image[bam_start..bam_start + bam_length], 18, 0)?;
	let free = reclaimable_free_sectors(&bam, STANDARD_BAM_TRACKS, |track, sector| {
		let (start, length) = ranges
			.get((usize::from(track) - 1) * 2)
			.copied()
			.flatten()?;
		gcr::decode_sector_consistent(&image[start..start + length], track, sector)
	})?;
	let mut changed = 0usize;

	for &(track, sector) in &free {
		let Some((start, length)) = ranges.get((usize::from(track) - 1) * 2).copied().flatten()
		else {
			continue;
		};
		let Some(existing) =
			gcr::decode_sector_consistent(&image[start..start + length], track, sector)
		else {
			continue;
		};
		if existing.iter().all(|&byte| byte == 0) {
			continue;
		}
		if gcr::replace_sector_data_consistent(
			&mut image[start..start + length],
			track,
			sector,
			&ZERO_SECTOR,
		) {
			changed += 1;
		}
	}

	Some(ReclaimResult {
		output: image,
		free_sectors: free.len(),
		changed_sectors: changed,
	})
}

fn transform(format: ImageFormat, bytes: &[u8]) -> Option<ReclaimResult> {
	match format {
		ImageFormat::D64 => reclaim_d64(bytes.to_vec()),
		ImageFormat::D7z => {
			let decoded = d7z::decode(bytes)?;
			let result = reclaim_d64(decoded)?;
			let output = d7z::encode(&result.output)?;
			Some(ReclaimResult {
				output,
				free_sectors: result.free_sectors,
				changed_sectors: result.changed_sectors,
			})
		}
		ImageFormat::G64 => reclaim_g64(bytes.to_vec()),
		ImageFormat::Nib => reclaim_nib(bytes.to_vec()),
		ImageFormat::Nbz => {
			let decoded = nib::decode_nbz(bytes)?;
			let result = reclaim_nib(decoded)?;
			let output = nib::encode_nbz(&result.output)?;
			Some(ReclaimResult {
				output,
				free_sectors: result.free_sectors,
				changed_sectors: result.changed_sectors,
			})
		}
	}
}

/* Conversion calls this on a private copy. A valid BAM with nothing left to clear is still a successful reclaim request; malformed BAM metadata returns None and therefore aborts a conversion that explicitly requested reclaiming. */
pub(crate) fn transform_for_conversion(format: ImageFormat, bytes: &[u8]) -> Option<Vec<u8>> {
	Some(transform(format, bytes)?.output)
}