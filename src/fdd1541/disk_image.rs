// =======================================================
// src/fdd1541/disk_image.rs — Disk-image format identity and transactional replacement
// =======================================================

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/* Even 84 maximum-length G64 records and their speed maps fit below this
 * container bound; compressed formats retain their own decoded-size checks. */
pub(crate) const MAX_IMAGE_SIZE: usize = 16 * 1024 * 1024;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ImageFormat {
	D64,
	D7z,
	G64,
	Nib,
	Nbz,
}

impl ImageFormat {
	pub(crate) fn is_logical(self) -> bool {
		matches!(self, Self::D64 | Self::D7z)
	}

	pub(crate) fn is_raw(self) -> bool {
		matches!(self, Self::G64 | Self::Nib | Self::Nbz)
	}

	pub(crate) fn extension(self) -> &'static str {
		match self {
			Self::D64 => "d64",
			Self::D7z => "d7z",
			Self::G64 => "g64",
			Self::Nib => "nib",
			Self::Nbz => "nbz",
		}
	}
}

pub(crate) fn format_from_path(path: &Path) -> Option<ImageFormat> {
	match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
		"d64" => Some(ImageFormat::D64),
		"d7z" => Some(ImageFormat::D7z),
		"g64" => Some(ImageFormat::G64),
		"nib" => Some(ImageFormat::Nib),
		"nbz" => Some(ImageFormat::Nbz),
		_ => None,
	}
}

fn temp_path(path: &Path, purpose: &str) -> Option<PathBuf> {
	let parent = path.parent().unwrap_or_else(|| Path::new("."));
	let name = path.file_name()?.to_string_lossy();
	let stamp = SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.ok()?
		.as_nanos();
	let count = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
	Some(parent.join(format!(".{name}.breadbin-{purpose}-{stamp}-{count}.tmp")))
}

/* A persistent image is never edited in place. The complete replacement is written and synced beside the target first; platforms whose rename cannot replace an existing file use a temporary backup so failure can restore the previous image. */
pub(crate) fn replace_atomically(path: &Path, bytes: &[u8]) -> bool {
	let Some(temp) = temp_path(path, "image") else {
		return false;
	};
	let Ok(mut file) = OpenOptions::new().write(true).create_new(true).open(&temp) else {
		return false;
	};
	let write_result = (|| -> std::io::Result<()> {
		file.write_all(bytes)?;
		file.sync_all()?;
		drop(file);
		Ok(())
	})();
	if write_result.is_err()
		|| fs::metadata(&temp)
			.map(|metadata| metadata.len() as usize)
			.ok()
			!= Some(bytes.len())
	{
		let _ = fs::remove_file(&temp);
		return false;
	}
	if fs::rename(&temp, path).is_ok() {
		return true;
	}

	let backup = temp.with_extension("bak");
	let had_original = path.exists();
	if had_original && fs::rename(path, &backup).is_err() {
		let _ = fs::remove_file(&temp);
		return false;
	}
	if fs::rename(&temp, path).is_err() {
		if had_original {
			let _ = fs::rename(&backup, path);
		}
		let _ = fs::remove_file(&temp);
		return false;
	}
	if had_original {
		let _ = fs::remove_file(&backup);
	}
	true
}