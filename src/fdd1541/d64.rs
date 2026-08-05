// =======================================================
// src/fdd1541/d64.rs — D64 disk image handling
// =======================================================

use super::constants::{D64_BAM_OFFSET, D64_DISK_ID, D64_DISK_NAME, D64_IMAGE_SIZE};
use super::gcr;

/* A freshly formatted BAM marks every sector on a data track free; directory track 18 is reserved separately below. */
fn build_bam_bitmap(sector_count: u8) -> (u8, u8, u8) {
	let bits = (1u32 << sector_count) - 1;
	(
		(bits & 0xFF) as u8,
		((bits >> 8) & 0xFF) as u8,
		((bits >> 16) & 0xFF) as u8,
	)
}

/* D64 stores sectors consecutively by logical track, so a track offset is the sum of all preceding zone-dependent sector counts. */
pub(crate) fn track_offset(track: u8) -> usize {
	let mut sectors = 0usize;
	for current in 1..track {
		sectors += gcr::sectors_per_track(current) as usize;
	}
	sectors * 256
}

pub(crate) fn sector_offset(track: u8, sector: u8) -> usize {
	track_offset(track) + sector as usize * 256
}

/* The blank image contains a valid BAM and an empty directory chain. PETSCII padding and the DOS type bytes are written exactly where 1541 DOS expects them. */
pub(crate) fn create_formatted() -> Vec<u8> {
	let mut disk = vec![0u8; D64_IMAGE_SIZE];

	disk[D64_BAM_OFFSET] = 18;
	disk[D64_BAM_OFFSET + 1] = 1;
	disk[D64_BAM_OFFSET + 2] = 0x41;
	disk[D64_BAM_OFFSET + 3] = 0x00;

	let mut bam_entry = D64_BAM_OFFSET + 4;
	for track in 1..=35u8 {
		let sector_count = gcr::sectors_per_track(track);
		if track == 18 {
			disk[bam_entry..bam_entry + 4].fill(0);
		} else {
			let (bitmap0, bitmap1, bitmap2) = build_bam_bitmap(sector_count);
			disk[bam_entry] = sector_count;
			disk[bam_entry + 1] = bitmap0;
			disk[bam_entry + 2] = bitmap1;
			disk[bam_entry + 3] = bitmap2;
		}
		bam_entry += 4;
	}

	disk[D64_BAM_OFFSET + 144..D64_BAM_OFFSET + 160].fill(0xA0);
	disk[D64_BAM_OFFSET + 144..D64_BAM_OFFSET + 144 + D64_DISK_NAME.len()]
		.copy_from_slice(D64_DISK_NAME);
	disk[D64_BAM_OFFSET + 160] = 0xA0;
	disk[D64_BAM_OFFSET + 161] = 0xA0;
	disk[D64_BAM_OFFSET + 162] = D64_DISK_ID[0];
	disk[D64_BAM_OFFSET + 163] = D64_DISK_ID[1];
	disk[D64_BAM_OFFSET + 164] = 0xA0;
	disk[D64_BAM_OFFSET + 165] = 0x32;
	disk[D64_BAM_OFFSET + 166] = 0x41;
	disk[D64_BAM_OFFSET + 167..D64_BAM_OFFSET + 171].fill(0xA0);

	let directory_offset = sector_offset(18, 1);
	disk[directory_offset] = 0x00;
	disk[directory_offset + 1] = 0xFF;

	disk
}