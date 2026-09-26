// =======================================================
// src/datassette/persistence.rs — Transactional TAP persistence
// =======================================================

use std::{
	fs::{self, OpenOptions},
	io::{Error, Result, Write},
	path::Path,
	sync::atomic::{AtomicU64, Ordering},
};

/* Sibling temporary files keep replacement on the same filesystem.
 * Exclusive creation prevents collisions with existing files; failed
 * replacement preserves either the original path or its named backup. */
pub(super) fn write(path: &Path, data: &[u8]) -> Result<()> {
	static SERIAL: AtomicU64 = AtomicU64::new(0);
	let serial = SERIAL.fetch_add(1, Ordering::Relaxed);
	let suffix = format!("tap.{}.{}", std::process::id(), serial);
	let temporary = path.with_extension(format!("{suffix}.tmp"));
	let backup = path.with_extension(format!("{suffix}.bak"));
	let mut file = OpenOptions::new()
		.write(true)
		.create_new(true)
		.open(&temporary)?;
	let result = (|| -> Result<()> {
		file.write_all(data)?;
		file.sync_all()?;
		drop(file);
		if fs::rename(&temporary, path).is_ok() {
			return Ok(());
		}
		if !path.is_file() || backup.exists() {
			return Err(Error::other("Cannot replace the cassette image"));
		}
		fs::rename(path, &backup)?;
		if let Err(error) = fs::rename(&temporary, path) {
			if let Err(restore) = fs::rename(&backup, path) {
				return Err(Error::other(format!(
					"{error}; restoration failed: {restore}; original cassette retained at {}",
					backup.display()
				)));
			}
			return Err(error);
		}
		let _ = fs::remove_file(&backup);
		Ok(())
	})();
	if result.is_err() {
		let _ = fs::remove_file(&temporary);
	}
	result
}