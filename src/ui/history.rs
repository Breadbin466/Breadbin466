// =======================================================
// src/ui/history.rs — Split history lists and configuration
// =======================================================

use crate::ui::constants::{MAX_RECENT, SAVE_DEBOUNCE_SECS};
use std::path::PathBuf;
use std::collections::VecDeque;
use std::fs;
use std::time::{Instant, Duration};
use serde::{Serialize, Deserialize};
use crate::motherboard::bus::DriveMode;

fn default_true() -> bool { true }

/* History combines recent-file lists with persistent UI and media preferences. Runtime bookkeeping fields are excluded from serialisation so disk state describes user choices rather than the debounce mechanism itself. */
#[derive(Serialize, Deserialize)]
pub struct History {
	pub crt_files: VecDeque<PathBuf>,
	pub prg_files: VecDeque<PathBuf>,
	pub d64_g64_files: VecDeque<PathBuf>,
	pub tap_files: VecDeque<PathBuf>,
	pub last_mounted_disk: Option<PathBuf>,
	pub active_crt: Option<PathBuf>,
	pub active_d64_g64: Option<PathBuf>,
	pub active_tap: Option<PathBuf>,
	#[serde(default)]
	pub mute_enabled: bool,
	#[serde(default = "default_true")]
	pub osd_enabled: bool,
	#[serde(default = "default_true")]
	pub debug_menu_visible: bool,
	#[serde(default)]
	pub drive_mode: DriveMode,
	#[serde(default)]
	pub custom_char_rom: Option<PathBuf>,
	#[serde(default)]
	pub custom_basic_rom: Option<PathBuf>,
	#[serde(default)]
	pub custom_kernal_rom: Option<PathBuf>,
	#[serde(default)]
	pub custom_drive_rom: Option<PathBuf>,
	#[serde(skip)]
	dirty: bool,
	#[serde(skip)]
	last_save: Option<Instant>,
}

impl Default for History {
	fn default() -> Self {
		Self {
			crt_files: VecDeque::new(),
			prg_files: VecDeque::new(),
			d64_g64_files: VecDeque::new(),
			tap_files: VecDeque::new(),
			last_mounted_disk: None,
			active_crt: None,
			active_d64_g64: None,
			active_tap: None,
			mute_enabled: false,
			osd_enabled: true,
			debug_menu_visible: true,
			drive_mode: DriveMode::default(),
			custom_char_rom: None,
			custom_basic_rom: None,
			custom_kernal_rom: None,
			custom_drive_rom: None,
			dirty: false,
			last_save: None,
		}
	}
}

impl History {
	/* Loading is deliberately fail-soft: an absent, unreadable or incompatible configuration falls back to defaults rather than preventing emulator startup. */
	pub fn load() -> Self {
		if let Some(config_dir) = Self::resolve_config_dir() {
			let config_path = config_dir.join("recent_v2.json");
			if config_path.exists() {
				if let Ok(content) = fs::read_to_string(config_path) {
					if let Ok(mut history) = serde_json::from_str::<Self>(&content) {
						history.dirty = false;
						history.last_save = Some(Instant::now());
						return history;
					}
				}
			}
		}
		Self::default()
	}

	/* Recent files are partitioned by media type, deduplicated and moved to the front. Persistence is debounced because several related UI actions may update history in quick succession. */
	pub fn add(&mut self, path: PathBuf) {
		let ext = path.extension()
			.and_then(|e| e.to_str())
			.unwrap_or("")
			.to_lowercase();

		match ext.as_str() {
			"crt" => Self::add_to_list(&mut self.crt_files, path),
			"prg" => Self::add_to_list(&mut self.prg_files, path),
			"d64" | "g64" | "nib" | "nbz" => Self::add_to_list(&mut self.d64_g64_files, path),
			"tap" => Self::add_to_list(&mut self.tap_files, path),
			_ => {}
		}

		self.save_debounced();
	}

	/* Recency lists are move-to-front sets with a fixed capacity, so reopening a file updates order without creating duplicates. */
	fn add_to_list(list: &mut VecDeque<PathBuf>, path: PathBuf) {
		list.retain(|p| p != &path);
		list.push_front(path);
		if list.len() > MAX_RECENT {
			list.pop_back();
		}
	}

	pub fn set_last_disk(&mut self, path: PathBuf) {
		self.last_mounted_disk = Some(path);
		self.save_debounced();
	}

	pub fn clear_last_disk(&mut self) {
		self.last_mounted_disk = None;
		self.save_debounced();
	}

	/* Configuration location follows each platform convention and returns None when the host exposes no writable per-user directory. */
	fn resolve_config_dir() -> Option<PathBuf> {
		#[cfg(target_os = "windows")]
		{
			std::env::var_os("APPDATA")
				.map(PathBuf::from)
				.map(|p| p.join("Foursixsix").join("Breadbin466"))
		}

		#[cfg(any(target_os = "linux", target_os = "macos"))]
		{
			std::env::var_os("XDG_CONFIG_HOME")
				.map(PathBuf::from)
				.or_else(|| {
					std::env::var_os("HOME")
						.map(PathBuf::from)
						.map(|p| p.join(".config"))
				})
				.map(|p| p.join("Breadbin466"))
		}

		#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
		{
			None
		}
	}

	/* Debouncing marks state dirty immediately but avoids rewriting the configuration file more often than the configured interval. Explicit media-state changes can still force a write. */
	fn save_debounced(&mut self) {
		self.dirty = true;
		let elapsed = self.last_save
			.map(|t| t.elapsed())
			.unwrap_or(Duration::MAX);
		if elapsed >= Duration::from_secs(SAVE_DEBOUNCE_SECS) {
			self.write_to_disk();
		}
	}

	pub fn save_forced(&mut self) {
		self.dirty = true;
		self.write_to_disk();
	}

	fn write_to_disk(&mut self) {
		if let Some(config_dir) = Self::resolve_config_dir() {
			if std::fs::create_dir_all(&config_dir).is_ok() {
				let config_path = config_dir.join("recent_v2.json");
				let json = serde_json::to_string_pretty(self).unwrap_or_default();
				if fs::write(config_path, json).is_ok() {
					self.dirty = false;
					self.last_save = Some(Instant::now());
				}
			}
		}
	}

	pub fn save(&mut self) {
		self.save_forced();
	}

	pub fn get_crt(&self, index: usize) -> Option<PathBuf> { self.crt_files.get(index).cloned() }
	pub fn get_prg(&self, index: usize) -> Option<PathBuf> { self.prg_files.get(index).cloned() }
	pub fn get_d64_g64(&self, index: usize) -> Option<PathBuf> { self.d64_g64_files.get(index).cloned() }
	pub fn get_tap(&self, index: usize) -> Option<PathBuf> { self.tap_files.get(index).cloned() }
}