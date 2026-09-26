// =======================================================
// src/host_files.rs — Bounded host-file input
// =======================================================

use std::{
	fs::File,
	io::{self, Read},
	path::Path,
};

/* Reject oversized files before allocating their contents. The read itself is
 * also bounded, since a file can grow after its metadata has been inspected.
 * Device nodes and pipes are not media images and must not be read indefinitely.
 */
pub(crate) fn read(path: &Path, maximum: usize) -> io::Result<Vec<u8>> {
	let invalid = || {
		io::Error::new(
			io::ErrorKind::InvalidData,
			"File exceeds the supported size or is not a regular file",
		)
	};
	let metadata = std::fs::metadata(path)?;
	if !metadata.is_file() || metadata.len() > maximum as u64 {
		return Err(invalid());
	}
	let file = File::open(path)?;
	let metadata = file.metadata()?;
	if !metadata.is_file() || metadata.len() > maximum as u64 {
		return Err(invalid());
	}
	let bound = (maximum as u64).checked_add(1).ok_or_else(invalid)?;
	let mut bytes = Vec::new();
	file.take(bound).read_to_end(&mut bytes)?;
	if bytes.len() > maximum {
		return Err(invalid());
	}
	Ok(bytes)
}