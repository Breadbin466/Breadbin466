// =======================================================
// src/ui/menu_windows.rs — Native Win32 menu backend
// =======================================================

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
	AppendMenuW, CallWindowProcW, CreateMenu, CreatePopupMenu, DestroyMenu, DrawMenuBar,
	GWLP_WNDPROC, GetWindowLongPtrW, HMENU, MF_CHECKED, MF_DISABLED, MF_ENABLED, MF_POPUP,
	MF_SEPARATOR, MF_STRING, MF_UNCHECKED, SetMenu, SetWindowLongPtrW, WM_COMMAND, WNDPROC,
};
use windows::core::PCWSTR;
use winit::window::Window;

use super::menu::{MenuEntry, MenuKey, MenuModel, MenuModifier, MenuShortcut, push_menu_event};
use crate::emulator::Result;

struct WindowHook {
	previous: isize,
	commands: HashMap<u16, String>,
}

static WINDOWS: OnceLock<Mutex<HashMap<isize, WindowHook>>> = OnceLock::new();

unsafe extern "system" fn menu_window_proc(
	hwnd: HWND,
	message: u32,
	wparam: WPARAM,
	lparam: LPARAM,
) -> LRESULT {
	if message == WM_COMMAND {
		let command = (wparam.0 & 0xffff) as u16;
		if let Ok(windows) = WINDOWS.get_or_init(|| Mutex::new(HashMap::new())).lock() {
			if let Some(hook) = windows.get(&(hwnd.0 as isize)) {
				if let Some(id) = hook.commands.get(&command) {
					push_menu_event(id.clone());
					return LRESULT(0);
				}
			}
		}
	}
	let previous = WINDOWS
		.get_or_init(|| Mutex::new(HashMap::new()))
		.lock()
		.ok()
		.and_then(|windows| windows.get(&(hwnd.0 as isize)).map(|hook| hook.previous))
		.unwrap_or(0);
	let procedure: WNDPROC = if previous == 0 {
		None
	} else {
		Some(unsafe { std::mem::transmute(previous) })
	};
	unsafe { CallWindowProcW(procedure, hwnd, message, wparam, lparam) }
}

/* PlatformMenu materialises the shared MenuModel as a Win32 HMENU and subclasses the window procedure so WM_COMMAND identifiers return to the platform-neutral event queue. */
pub struct PlatformMenu {
	model: Arc<RwLock<MenuModel>>,
	hwnd: HWND,
	menu: Option<HMENU>,
}

impl PlatformMenu {
	/* Construction binds the backend to one HWND, installs the command hook and builds the first native menu tree. */
	/* Win32 construction allocates native command identifiers for the shared model and records checkable entries for later refresh. */
	pub fn new(window: &Window, model: Arc<RwLock<MenuModel>>) -> Result<Self> {
		let handle = window.window_handle()?;
		let hwnd = match handle.as_raw() {
			RawWindowHandle::Win32(handle) => HWND(handle.hwnd.get() as *mut _),
			_ => return Err("The winit window does not expose a Win32 handle".into()),
		};
		let mut platform = Self {
			model,
			hwnd,
			menu: None,
		};
		unsafe {
			let previous = GetWindowLongPtrW(hwnd, GWLP_WNDPROC);
			SetWindowLongPtrW(
				hwnd,
				GWLP_WNDPROC,
				menu_window_proc as *const () as usize as isize,
			);
			WINDOWS
				.get_or_init(|| Mutex::new(HashMap::new()))
				.lock()
				.map_err(|_| "Windows menu lock poisoned")?
				.insert(
					hwnd.0 as isize,
					WindowHook {
						previous,
						commands: HashMap::new(),
					},
				);
		}
		platform.refresh()?;
		Ok(platform)
	}

	/* Refresh replaces the complete HMENU, rebuilds the transient numeric-command map and destroys the previous native tree only after the replacement is installed. */
	/* Refresh rebuilds native labels and enabled states while preserving the logical identifiers used by command dispatch. */
	pub fn refresh(&mut self) -> Result<()> {
		let model = self
			.model
			.read()
			.map_err(|_| "Menu model lock poisoned")?
			.clone();
		let mut next_command = 0x4000u16;
		let mut commands = HashMap::new();
		let root = unsafe {
			let root = CreateMenu()?;
			for section in &model.sections {
				let popup = CreatePopupMenu()?;
				append_entries(popup, &section.entries, &mut next_command, &mut commands)?;
				append_popup(root, popup, &section.label, section.enabled)?;
			}
			SetMenu(self.hwnd, Some(root))?;
			DrawMenuBar(self.hwnd)?;
			root
		};
		if let Some(previous) = self.menu.replace(root) {
			unsafe {
				let _ = DestroyMenu(previous);
			}
		}
		if let Ok(mut windows) = WINDOWS.get_or_init(|| Mutex::new(HashMap::new())).lock() {
			if let Some(hook) = windows.get_mut(&(self.hwnd.0 as isize)) {
				hook.commands = commands;
			}
		}
		Ok(())
	}

	pub fn set_checked(&mut self, _id: &str, _checked: bool) -> Result<()> {
		self.refresh()
	}

	pub fn pump(&mut self) {}
}

unsafe fn append_entries(
	menu: HMENU,
	entries: &[MenuEntry],
	next_command: &mut u16,
	commands: &mut HashMap<u16, String>,
) -> Result<()> {
	unsafe {
		for entry in entries {
			match entry {
				MenuEntry::Separator => {
					AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null())?;
				}
				MenuEntry::Submenu(section) => {
					let popup = CreatePopupMenu()?;
					append_entries(popup, &section.entries, next_command, commands)?;
					append_popup(menu, popup, &section.label, section.enabled)?;
				}
				MenuEntry::Action {
					id,
					label,
					enabled,
					shortcut,
				} => {
					append_command(
						menu,
						id,
						label,
						*enabled,
						false,
						*shortcut,
						next_command,
						commands,
					)?;
				}
				MenuEntry::Check {
					id,
					label,
					enabled,
					checked,
					shortcut,
				} => {
					append_command(
						menu,
						id,
						label,
						*enabled,
						*checked,
						*shortcut,
						next_command,
						commands,
					)?;
				}
			}
		}
		Ok(())
	}
}

unsafe fn append_popup(menu: HMENU, popup: HMENU, label: &str, enabled: bool) -> Result<()> {
	unsafe {
		let wide = wide(label);
		let flags = MF_POPUP | if enabled { MF_ENABLED } else { MF_DISABLED };
		AppendMenuW(menu, flags, popup.0 as usize, PCWSTR(wide.as_ptr()))?;
		Ok(())
	}
}

unsafe fn append_command(
	menu: HMENU,
	id: &str,
	label: &str,
	enabled: bool,
	checked: bool,
	shortcut: Option<MenuShortcut>,
	next_command: &mut u16,
	commands: &mut HashMap<u16, String>,
) -> Result<()> {
	unsafe {
		if id.is_empty() {
			let wide = wide(label);
			AppendMenuW(menu, MF_STRING | MF_DISABLED, 0, PCWSTR(wide.as_ptr()))?;
			return Ok(());
		}
		let command = *next_command;
		*next_command = next_command.wrapping_add(1);
		commands.insert(command, id.to_string());
		let mut flags = MF_STRING | if enabled { MF_ENABLED } else { MF_DISABLED };
		flags |= if checked { MF_CHECKED } else { MF_UNCHECKED };
		let display = shortcut_label(label, shortcut);
		let wide = wide(&display);
		AppendMenuW(menu, flags, command as usize, PCWSTR(wide.as_ptr()))?;
		Ok(())
	}
}

fn wide(value: &str) -> Vec<u16> {
	value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn shortcut_label(label: &str, shortcut: Option<MenuShortcut>) -> String {
	let Some(shortcut) = shortcut else {
		return label.to_string();
	};
	let key = match shortcut.key {
		MenuKey::C => "C",
		MenuKey::D => "D",
		MenuKey::Enter => "Enter",
		MenuKey::F => "F",
		MenuKey::F4 => "F4",
		MenuKey::I => "I",
		MenuKey::J => "J",
		MenuKey::M => "M",
		MenuKey::P => "P",
		MenuKey::Pause => "Pause",
		MenuKey::Q => "Q",
		MenuKey::R => "R",
		MenuKey::T => "T",
		MenuKey::W => "W",
	};
	let modifier = match shortcut.modifier {
		MenuModifier::Primary => "Ctrl+",
		MenuModifier::Alt => "Alt+",
		MenuModifier::Unmodified => "",
	};
	let shift = if shortcut.shift { "Shift+" } else { "" };
	format!("{}\t{}{}{}", label, modifier, shift, key)
}

impl Drop for PlatformMenu {
	fn drop(&mut self) {
		unsafe {
			let _ = SetMenu(self.hwnd, None);
			if let Some(menu) = self.menu.take() {
				let _ = DestroyMenu(menu);
			}
			if let Ok(mut windows) = WINDOWS.get_or_init(|| Mutex::new(HashMap::new())).lock() {
				if let Some(hook) = windows.remove(&(self.hwnd.0 as isize)) {
					SetWindowLongPtrW(self.hwnd, GWLP_WNDPROC, hook.previous);
				}
			}
		}
	}
}