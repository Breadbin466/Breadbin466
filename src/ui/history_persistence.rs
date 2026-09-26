// =======================================================
// src/ui/history_persistence.rs — Atomic configuration replacement
// =======================================================

use std::{
	fs::{self, OpenOptions},
	io::{self, Write},
	path::Path,
	sync::atomic::{AtomicU64, Ordering},
};

/* A unique sibling keeps replacement on the same filesystem. The complete
 * file is flushed before rename; failure never truncates the previous copy.
 * create_new prevents concurrent instances from sharing a temporary file. */
pub(super) fn replace(path: &Path, bytes: &[u8]) -> io::Result<()> {
	static SEQUENCE: AtomicU64 = AtomicU64::new(0);
	let parent = path
		.parent()
		.ok_or_else(|| io::Error::other("Missing configuration directory"))?;
	fs::create_dir_all(parent)?;
	let (temporary, mut file) = loop {
		let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
		let temporary = parent.join(format!(
			".breadbin-config-{}-{sequence}.tmp",
			std::process::id()
		));
		match OpenOptions::new()
			.write(true)
			.create_new(true)
			.open(&temporary)
		{
			Ok(file) => break (temporary, file),
			Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
			Err(error) => return Err(error),
		}
	};
	let result = (|| {
		file.write_all(bytes)?;
		file.sync_all()?;
		drop(file);
		fs::rename(&temporary, path)?;
		#[cfg(unix)]
		fs::File::open(parent)?.sync_all()?;
		Ok(())
	})();
	if result.is_err() {
		let _ = fs::remove_file(&temporary);
	}
	result
}