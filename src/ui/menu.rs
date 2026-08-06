// =======================================================
// src/ui/menu.rs — Cross-platform native menu model
// =======================================================

use std::cell::Cell;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use winit::window::Window;

use crate::emulator::Result;
use crate::motherboard::bus::DriveMode;
use super::history::History;

#[cfg(target_os = "macos")]
use super::menu_macos::PlatformMenu;
#[cfg(target_os = "windows")]
use super::menu_windows::PlatformMenu;
#[cfg(target_os = "linux")]
use super::menu_linux::PlatformMenu;

static MENU_EVENTS: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();

pub(crate) fn push_menu_event(id: String) {
	if let Ok(mut events) = MENU_EVENTS.get_or_init(|| Mutex::new(VecDeque::new())).lock() {
		events.push_back(id);
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuKey {
	C,
	D,
	Enter,
	F,
	F4,
	I,
	J,
	M,
	P,
	Pause,
	Q,
	R,
	T,
	W,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuModifier {
	Primary,
	Alt,
	Unmodified,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuShortcut {
	pub key: MenuKey,
	pub modifier: MenuModifier,
	pub shift: bool,
}

impl MenuShortcut {
	const fn command(key: MenuKey) -> Self {
		#[cfg(target_os = "windows")]
		{
			Self { key, modifier: MenuModifier::Alt, shift: false }
		}
		#[cfg(not(target_os = "windows"))]
		{
			Self { key, modifier: MenuModifier::Primary, shift: false }
		}
	}

	const fn command_shift(key: MenuKey) -> Self {
		#[cfg(target_os = "windows")]
		{
			Self { key, modifier: MenuModifier::Alt, shift: true }
		}
		#[cfg(not(target_os = "windows"))]
		{
			Self { key, modifier: MenuModifier::Primary, shift: true }
		}
	}

	#[cfg(target_os = "windows")]
	const fn alt(key: MenuKey) -> Self {
		Self { key, modifier: MenuModifier::Alt, shift: false }
	}

}

#[derive(Clone, Debug)]
pub enum MenuEntry {
	Action {
		id: String,
		label: String,
		enabled: bool,
		shortcut: Option<MenuShortcut>,
	},
	Check {
		id: String,
		label: String,
		enabled: bool,
		checked: bool,
		shortcut: Option<MenuShortcut>,
	},
	Separator,
	Submenu(MenuSection),
}

#[derive(Clone, Debug)]
pub struct MenuSection {
	pub id: String,
	pub label: String,
	pub enabled: bool,
	pub entries: Vec<MenuEntry>,
}

impl MenuSection {
	fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
		Self {
			id: id.into(),
			label: label.into(),
			enabled: true,
			entries: Vec::new(),
		}
	}
}

/* MenuModel is the platform-neutral tree of sections, actions, checks and shortcuts. Native backends render this shared model instead of defining their own command semantics. */
#[derive(Clone, Debug, Default)]
pub struct MenuModel {
	pub sections: Vec<MenuSection>,
}

impl MenuModel {
	fn set_checked(&mut self, id: &str, checked: bool) -> bool {
		for section in &mut self.sections {
			if Self::set_checked_in_entries(&mut section.entries, id, checked) {
				return true;
			}
		}
		false
	}

	fn set_checked_in_entries(entries: &mut [MenuEntry], id: &str, checked: bool) -> bool {
		for entry in entries {
			match entry {
				MenuEntry::Check { id: entry_id, checked: value, .. } if entry_id == id => {
					*value = checked;
					return true;
				}
				MenuEntry::Submenu(section) => {
					if Self::set_checked_in_entries(&mut section.entries, id, checked) {
						return true;
					}
				}
				_ => {}
			}
		}
		false
	}

	fn replace_submenu_entries(&mut self, submenu_id: &str, entries: Vec<MenuEntry>) {
		for section in &mut self.sections {
			if Self::replace_in_entries(&mut section.entries, submenu_id, &entries) {
				return;
			}
		}
	}

	fn replace_in_entries(entries: &mut [MenuEntry], submenu_id: &str, replacement: &[MenuEntry]) -> bool {
		for entry in entries {
			if let MenuEntry::Submenu(section) = entry {
				if section.id == submenu_id {
					section.entries = replacement.to_vec();
					return true;
				}
				if Self::replace_in_entries(&mut section.entries, submenu_id, replacement) {
					return true;
				}
			}
		}
		false
	}
}

/* MenuIds assigns stable logical identifiers to every command so native menus, keyboard shortcuts and command dispatch can refer to actions independently of visible labels. */
#[derive(Clone)]
pub struct MenuIds {
	pub open_prg: String,
	pub open_prg_run: String,
	pub open_cart: String,
	pub open_d64_g64: String,
	pub unmount_d64_g64: String,
	pub disk_create_d64: String,
	pub disk_create_g64: String,
	pub disk_create_nib: String,
	pub disk_create_nbz: String,
	pub tape_mount: String,
	pub tape_create: String,
	pub tape_eject: String,
	pub tape_rewind: String,
	pub tape_play: String,
	pub tape_record_play: String,
	pub recent_crt_base: String,
	pub recent_prg_base: String,
	pub recent_d64_g64_base: String,
	pub recent_tap_base: String,
	pub reset: String,
	pub soft_reset: String,
	pub reset_detach: String,
	pub pause: String,
	pub cycle_joystick: String,
	pub cartridge_reset: String,
	pub cartridge_freeze: String,
	pub cartridge_menu: String,
	pub warp_mode: String,
	pub warp_1541: String,
	pub cmd_directory: String,
	pub cmd_load_first: String,
	pub drive_off: String,
	pub drive_lle: String,
	pub quit: String,
	pub fullscreen: String,
	pub about: String,
	pub scale_1x: String,
	pub scale_2x: String,
	pub scale_3x: String,
	pub view_osd: String,
	pub show_debug_menu: String,
	pub debug_mute_warp: String,
	pub debug_mute_global: String,
	pub reu_1764_512k: String,
	pub debug_c128_2mhz: String,
	pub rom_char: String,
	pub rom_basic: String,
	pub rom_kernal: String,
	pub rom_drive: String,
	pub rom_use_original: String,
	pub inspector_window: String,
}

impl MenuIds {
	fn new() -> Self {
		Self {
			open_prg: "computer_open_prg".into(),
			open_prg_run: "computer_open_prg_run".into(),
			open_cart: "computer_open_crt".into(),
			open_d64_g64: "computer_open_d64_g64".into(),
			unmount_d64_g64: "computer_unmount_d64_g64".into(),
			disk_create_d64: "1541_create_d64".into(),
			disk_create_g64: "1541_create_g64".into(),
			disk_create_nib: "1541_create_nib".into(),
			disk_create_nbz: "1541_create_nbz".into(),
			tape_mount: "tape_mount_computer".into(),
			tape_create: "tape_create_computer".into(),
			tape_eject: "tape_eject_computer".into(),
			tape_rewind: "tape_rewind_action".into(),
			tape_play: "tape_press_play".into(),
			tape_record_play: "tape_press_record_play".into(),
			recent_crt_base: "rec_crt_".into(),
			recent_prg_base: "rec_prg_".into(),
			recent_d64_g64_base: "rec_d64_g64_".into(),
			recent_tap_base: "rec_tap_".into(),
			reset: "emu_reset".into(),
			soft_reset: "emu_soft_reset".into(),
			reset_detach: "emu_reset_detach".into(),
			pause: "emu_pause".into(),
			cycle_joystick: "emu_cycle_joystick".into(),
			cartridge_reset: "cartridge_reset_button".into(),
			cartridge_freeze: "cartridge_freeze_button".into(),
			cartridge_menu: "cartridge_menu_button".into(),
			warp_mode: "mach_warp".into(),
			warp_1541: "mach_warp_1541".into(),
			cmd_directory: "cmd_load_dir".into(),
			cmd_load_first: "cmd_load_star".into(),
			drive_off: "drive_mode_off".into(),
			drive_lle: "drive_mode_lle".into(),
			quit: "app_quit".into(),
			fullscreen: "view_fullscreen".into(),
			about: "app_about".into(),
			scale_1x: "win_scale_1".into(),
			scale_2x: "win_scale_2".into(),
			scale_3x: "win_scale_3".into(),
			view_osd: "view_osd".into(),
			show_debug_menu: "view_show_debug".into(),
			debug_mute_warp: "dbg_mute_warp".into(),
			debug_mute_global: "dbg_mute_global".into(),
			reu_1764_512k: "reu_1764_512k".into(),
			debug_c128_2mhz: "dbg_c128_2mhz".into(),
			rom_char: "dbg_rom_char".into(),
			rom_basic: "dbg_rom_basic".into(),
			rom_kernal: "dbg_rom_kernal".into(),
			rom_drive: "dbg_rom_drive".into(),
			rom_use_original: "dbg_rom_use_original".into(),
			inspector_window: "c64_inspector".into(),
		}
	}
}

/* MenuManager owns the shared model and one platform backend. Model updates are reflected into the native menu while events travel back as stable identifiers through a process-wide queue. */
pub struct MenuManager {
	pub ids: MenuIds,
	model: Arc<RwLock<MenuModel>>,
	platform: Mutex<PlatformMenu>,
	debug_visible: Cell<bool>,
}

impl MenuManager {
	/* Construction builds the complete platform-neutral model, injects persisted recent-media entries, then gives the shared model to the native backend. */
	pub fn new(window: &Window, history: &History) -> Result<Self> {
		let ids = MenuIds::new();
		let mut initial_model = Self::build_model(&ids, history);
		Self::replace_recent_entries(&mut initial_model, &ids, history);
		let model = Arc::new(RwLock::new(initial_model));
		let platform = PlatformMenu::new(window, model.clone())?;
		let manager = Self {
			ids,
			model,
			platform: Mutex::new(platform),
			debug_visible: Cell::new(history.debug_menu_visible),
		};
		Ok(manager)
	}

	pub fn poll_event() -> Option<String> {
		MENU_EVENTS
			.get_or_init(|| Mutex::new(VecDeque::new()))
			.lock()
			.ok()
			.and_then(|mut events| events.pop_front())
	}

	pub fn shortcut_command(
		&self,
		key: MenuKey,
		modifier: MenuModifier,
		shift: bool,
	) -> Option<String> {
		let shortcut = MenuShortcut { key, modifier, shift };
		let model = self.model.read().ok()?;
		for section in &model.sections {
			if section.enabled {
				if let Some(id) = Self::shortcut_in_entries(&section.entries, shortcut) {
					return Some(id);
				}
			}
		}
		None
	}

	fn shortcut_in_entries(entries: &[MenuEntry], shortcut: MenuShortcut) -> Option<String> {
		for entry in entries {
			match entry {
				MenuEntry::Action { id, enabled: true, shortcut: Some(value), .. }
				| MenuEntry::Check { id, enabled: true, shortcut: Some(value), .. }
					if *value == shortcut => return Some(id.clone()),
				MenuEntry::Submenu(section) if section.enabled => {
					if let Some(id) = Self::shortcut_in_entries(&section.entries, shortcut) {
						return Some(id);
					}
				}
				_ => {}
			}
		}
		None
	}

	pub fn pump(&self) {
		if let Ok(mut platform) = self.platform.lock() {
			platform.pump();
		}
	}

	pub fn set_debug_menu_visible(&self, visible: bool) {
		if self.debug_visible.get() == visible {
			return;
		}
		self.debug_visible.set(visible);
		if let Ok(mut model) = self.model.write() {
			model.sections.retain(|section| section.id != "debug");
			if visible {
				model.sections.insert(Self::debug_insert_index(), Self::build_debug_section(&self.ids));
			}
		}
		if let Ok(mut platform) = self.platform.lock() {
			let _ = platform.refresh();
		}
	}

	pub fn set_radio_selection(&self, active_id: &str, group_ids: &[&str]) {
		let changed = self.model.write().map(|mut model| {
			group_ids.iter().fold(false, |changed, id| {
				model.set_checked(id, *id == active_id) || changed
			})
		}).unwrap_or(false);

		/* Native menu reconstruction is avoided while a resize continues within
		 * the same preset state. This matters during free window dragging, where
		 * many resize notifications may arrive without changing any check mark. */
		if changed {
			if let Ok(mut platform) = self.platform.lock() {
				let _ = platform.refresh();
			}
		}
	}

	pub fn set_checked(&self, id: &str, checked: bool) {
		let changed = self.model.write().map(|mut model| model.set_checked(id, checked)).unwrap_or(false);
		if changed {
			if let Ok(mut platform) = self.platform.lock() {
				let _ = platform.set_checked(id, checked);
			}
		}
	}

	pub fn set_tape_transport(&self, play: bool, record: bool) {
		self.set_checked(&self.ids.tape_play, play);
		self.set_checked(&self.ids.tape_record_play, record);
	}

	pub fn rebuild_recent(&self, history: &History) {
		if let Ok(mut model) = self.model.write() {
			Self::replace_recent_entries(&mut model, &self.ids, history);
		}
		if let Ok(mut platform) = self.platform.lock() {
			let _ = platform.refresh();
		}
	}

	fn replace_recent_entries(model: &mut MenuModel, ids: &MenuIds, history: &History) {
		model.replace_submenu_entries(
			"recent_crt",
			Self::recent_entries(&ids.recent_crt_base, &history.crt_files, "No recent cartridges"),
		);
		model.replace_submenu_entries(
			"recent_prg",
			Self::recent_entries(&ids.recent_prg_base, &history.prg_files, "No recent PRGs"),
		);
		model.replace_submenu_entries(
			"recent_disk",
			Self::recent_entries(&ids.recent_d64_g64_base, &history.d64_g64_files, "No recent disks"),
		);
		model.replace_submenu_entries(
			"recent_tape",
			Self::recent_entries(&ids.recent_tap_base, &history.tap_files, "No recent tapes"),
		);
	}

	fn recent_entries(base: &str, paths: &std::collections::VecDeque<std::path::PathBuf>, empty: &str) -> Vec<MenuEntry> {
		if paths.is_empty() {
			return vec![Self::action("", empty, false, None)];
		}
		paths.iter().enumerate().map(|(index, path)| {
			let label = path.file_name().and_then(|name| name.to_str()).unwrap_or("?");
			Self::action(format!("{}{}", base, index), label, true, None)
		}).collect()
	}

	/* The top-level model fixes section ordering while each section builder owns one coherent command domain. */
	fn build_model(ids: &MenuIds, history: &History) -> MenuModel {
		let mut sections = Vec::new();
		#[cfg(target_os = "macos")]
		sections.push(Self::build_application_section(ids));
		sections.push(Self::build_computer_section(ids));
		sections.push(Self::build_tape_section(ids));
		sections.push(Self::build_cartridge_section(ids));
		sections.push(Self::build_drive_section(ids, history.drive_mode));
		sections.push(Self::build_view_section(ids, history));
		if history.debug_menu_visible {
			sections.push(Self::build_debug_section(ids));
		}
		#[cfg(not(target_os = "macos"))]
		sections.push(Self::build_help_section(ids));
		MenuModel { sections }
	}

	#[cfg(target_os = "macos")]
	/* Application lifecycle commands remain separate from machine controls so native platforms can place them according to their conventions. */
	fn build_application_section(ids: &MenuIds) -> MenuSection {
		let mut section = MenuSection::new("application", "Breadbin466");
		section.entries.push(Self::action(&ids.about, "About Breadbin466", true, None));
		section.entries.push(MenuEntry::Separator);
		section.entries.push(Self::action(&ids.quit, "Quit Breadbin466", true, Some(MenuShortcut::command(MenuKey::Q))));
		section
	}

	/* Computer actions group machine reset, expansion hardware, pause and joystick routing because they alter the running C64 as a whole. */
	fn build_computer_section(ids: &MenuIds) -> MenuSection {
		let mut section = MenuSection::new("computer", "Computer");
		section.entries.push(Self::action(&ids.soft_reset, "Soft Reset", true, Some(MenuShortcut::command_shift(MenuKey::R))));
		section.entries.push(Self::action(&ids.reset, "Hard Reset", true, Some(MenuShortcut::command(MenuKey::R))));
		section.entries.push(MenuEntry::Separator);
		section.entries.push(Self::action(&ids.cycle_joystick, "Cycle Joystick", true, Some(MenuShortcut::command(MenuKey::J))));
		section.entries.push(MenuEntry::Separator);
		section.entries.push(Self::check(&ids.reu_1764_512k, "Enable Commodore 1764 REU (512 KB)", true, false, None));
		section.entries.push(MenuEntry::Separator);
		section.entries.push(Self::check(&ids.pause, "Pause", true, false, Some(MenuShortcut::command(MenuKey::P))));
		#[cfg(target_os = "windows")]
		{
			section.entries.push(MenuEntry::Separator);
			section.entries.push(Self::action(&ids.quit, "Exit", true, Some(MenuShortcut::alt(MenuKey::F4))));
		}
		#[cfg(target_os = "linux")]
		{
			section.entries.push(MenuEntry::Separator);
			section.entries.push(Self::action(&ids.quit, "Quit", true, Some(MenuShortcut::command(MenuKey::Q))));
		}
		section
	}

	/* Tape actions mirror the transport controls and media lifecycle of the datassette. */
	fn build_tape_section(ids: &MenuIds) -> MenuSection {
		let mut section = MenuSection::new("datassette", "Datassette");
		section.entries.push(Self::action(&ids.tape_mount, "Insert Tape (.tap)", true, Some(MenuShortcut::command(MenuKey::T))));
		section.entries.push(Self::action(&ids.tape_create, "Create new Tape (.tap)", true, None));
		section.entries.push(MenuEntry::Separator);
		section.entries.push(Self::action(&ids.tape_eject, "Eject Tape", true, Some(MenuShortcut::command_shift(MenuKey::T))));
		section.entries.push(Self::action(&ids.tape_rewind, "Rewind Tape", true, None));
		section.entries.push(MenuEntry::Submenu(MenuSection::new("recent_tape", "Recent Tape (.tap)")));
		section.entries.push(MenuEntry::Separator);
		section.entries.push(Self::check(&ids.tape_play, "Press PLAY on Tape", true, false, None));
		section.entries.push(Self::check(&ids.tape_record_play, "Press RECORD AND PLAY on Tape", true, false, None));
		section
	}

	/* Cartridge actions expose mount and hardware button operations only when a cartridge can service them. */
	fn build_cartridge_section(ids: &MenuIds) -> MenuSection {
		let mut section = MenuSection::new("cartridge", "Cartridge");
		section.entries.push(Self::action(&ids.open_cart, "Insert Cartridge (.crt)", true, Some(MenuShortcut::command(MenuKey::C))));
		section.entries.push(Self::action(&ids.reset_detach, "Remove Cartridge", true, Some(MenuShortcut::command_shift(MenuKey::C))));
		section.entries.push(MenuEntry::Submenu(MenuSection::new("recent_crt", "Recent Cartridge (.crt)")));
		section.entries.push(MenuEntry::Separator);
		section.entries.push(Self::action(&ids.cartridge_reset, "Reset Button", true, None));
		section.entries.push(Self::action(&ids.cartridge_freeze, "Freeze Button", true, Some(MenuShortcut::command(MenuKey::F))));
		section.entries.push(Self::action(&ids.cartridge_menu, "Menu Button", true, Some(MenuShortcut::command(MenuKey::M))));
		section
	}

	/* Drive actions combine media management, DOS shortcuts and the selected low-level drive mode. */
	fn build_drive_section(ids: &MenuIds, drive_mode: DriveMode) -> MenuSection {
		let mut section = MenuSection::new("drive", "1541");
		section.entries.push(Self::action(&ids.open_d64_g64, "Insert Disk (.d64/.g64/.nib/.nbz)", true, Some(MenuShortcut::command(MenuKey::D))));
		section.entries.push(Self::action(&ids.unmount_d64_g64, "Remove Disk (.d64/.g64/.nib/.nbz)", true, Some(MenuShortcut::command_shift(MenuKey::D))));
		section.entries.push(MenuEntry::Submenu(MenuSection::new("recent_disk", "Recent Disk (.d64/.g64/.nib/.nbz)")));
		section.entries.push(MenuEntry::Separator);
		section.entries.push(Self::action(&ids.disk_create_d64, "Create new Disk (.d64)", true, None));
		section.entries.push(Self::action(&ids.disk_create_g64, "Create new Disk (.g64)", true, None));
		section.entries.push(Self::action(&ids.disk_create_nib, "Create new Disk (.nib)", true, None));
		section.entries.push(Self::action(&ids.disk_create_nbz, "Create new Disk (.nbz)", true, None));
		section.entries.push(MenuEntry::Separator);
		section.entries.push(Self::action(&ids.cmd_directory, "LOAD Directory (LOAD\"$\",8)", true, None));
		section.entries.push(Self::action(&ids.cmd_load_first, "LOAD first File (LOAD\"*\",8,1)", true, None));
		section.entries.push(MenuEntry::Separator);
		section.entries.push(Self::check(&ids.drive_off, "1541 Disabled", true, drive_mode == DriveMode::Off, None));
		section.entries.push(Self::check(&ids.drive_lle, "1541 Enabled", true, drive_mode == DriveMode::Lle, None));
		section
	}

	/* View actions contain host presentation state and persisted UI visibility choices. */
	fn build_view_section(ids: &MenuIds, history: &History) -> MenuSection {
		let mut section = MenuSection::new("view", "View");
		section.entries.push(Self::action(&ids.inspector_window, "Inspector", true, Some(MenuShortcut::command(MenuKey::I))));
		#[cfg(target_os = "windows")]
		section.entries.push(Self::action(&ids.fullscreen, "Toggle Fullscreen", true, Some(MenuShortcut::alt(MenuKey::Enter))));
		#[cfg(not(target_os = "windows"))]
		section.entries.push(Self::action(&ids.fullscreen, "Toggle Fullscreen", true, None));
		section.entries.push(MenuEntry::Separator);
		section.entries.push(Self::check(&ids.show_debug_menu, "Show Debug Menu", true, history.debug_menu_visible, None));
		section.entries.push(MenuEntry::Separator);
		section.entries.push(Self::check(&ids.view_osd, "Enable OSD", true, history.osd_enabled, None));
		section.entries.push(MenuEntry::Separator);
		section.entries.push(Self::check(&ids.scale_1x, "Scale 1x", true, false, None));
		section.entries.push(Self::check(&ids.scale_2x, "Scale 2x", true, true, None));
		section.entries.push(Self::check(&ids.scale_3x, "Scale 3x", true, false, None));
		section
	}

	/* Debug actions expose optional hardware substitutions and diagnostics without entering the normal user command groups. */
	fn build_debug_section(ids: &MenuIds) -> MenuSection {
		let mut section = MenuSection::new("debug", "Debug");
		section.entries.push(Self::action(&ids.open_prg, "Inject .prg", true, None));
		section.entries.push(Self::action(&ids.open_prg_run, "Inject and RUN .prg", true, None));
		section.entries.push(MenuEntry::Submenu(MenuSection::new("recent_prg", "Recent .prg")));
		section.entries.push(MenuEntry::Separator);
		section.entries.push(Self::check(&ids.debug_mute_warp, "Mute SID during Warp", true, true, None));
		section.entries.push(Self::check(&ids.debug_mute_global, "Mute Audio", true, false, None));
		section.entries.push(MenuEntry::Separator);
		section.entries.push(Self::check(&ids.debug_c128_2mhz, "Use 8502 (CPU selectable 2 MHz Mode)", true, false, None));
		section.entries.push(MenuEntry::Separator);
		section.entries.push(Self::check(&ids.warp_mode, "Warp Mode", true, false, Some(MenuShortcut::command(MenuKey::W))));
		section.entries.push(Self::check(&ids.warp_1541, "Warp Mode on 1541 Access", true, false, Some(MenuShortcut::command_shift(MenuKey::W))));
		section.entries.push(MenuEntry::Separator);
		let mut roms = MenuSection::new("custom_roms", "Custom ROMs");
		roms.entries.push(Self::action(&ids.rom_char, "Character ROM (4KB)...", true, None));
		roms.entries.push(Self::action(&ids.rom_basic, "BASIC ROM (8KB)...", true, None));
		roms.entries.push(Self::action(&ids.rom_kernal, "KERNAL ROM (8KB)...", true, None));
		roms.entries.push(Self::action(&ids.rom_drive, "1541 ROM (16KB)...", true, None));
		roms.entries.push(MenuEntry::Separator);
		roms.entries.push(Self::action(&ids.rom_use_original, "Use Original ROMs", true, None));
		section.entries.push(MenuEntry::Submenu(roms));
		section
	}

	#[cfg(not(target_os = "macos"))]
	/* Help remains a small platform-neutral section whose placement is decided by the native backend. */
	fn build_help_section(ids: &MenuIds) -> MenuSection {
		let mut section = MenuSection::new("help", "Help");
		section.entries.push(Self::action(&ids.about, "About Breadbin466", true, None));
		section
	}

	fn debug_insert_index() -> usize {
		#[cfg(target_os = "macos")]
		{ 6 }
		#[cfg(not(target_os = "macos"))]
		{ 5 }
	}

	fn action(id: impl Into<String>, label: impl Into<String>, enabled: bool, shortcut: Option<MenuShortcut>) -> MenuEntry {
		MenuEntry::Action { id: id.into(), label: label.into(), enabled, shortcut }
	}

	fn check(id: impl Into<String>, label: impl Into<String>, enabled: bool, checked: bool, shortcut: Option<MenuShortcut>) -> MenuEntry {
		MenuEntry::Check { id: id.into(), label: label.into(), enabled, checked, shortcut }
	}
}