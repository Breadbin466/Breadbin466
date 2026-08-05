// =======================================================
// src/ui/menu_linux.rs — Native GTK menu bar backend
// =======================================================

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use gtk::prelude::*;
use winit::window::Window;

use crate::emulator::Result;
use super::menu::{push_menu_event, MenuEntry, MenuKey, MenuModel, MenuModifier, MenuShortcut};
use super::shell_linux::Shell;

/* PlatformMenu materialises the shared MenuModel as GTK widgets. It retains check-item handles for incremental state changes while full structural changes rebuild the native menu bar. */
pub struct PlatformMenu {
	model: Arc<RwLock<MenuModel>>,
	checks: HashMap<String, gtk::CheckMenuItem>,
}

impl PlatformMenu {
	/* Construction stores the shared model and performs the first complete GTK materialisation. */
/* GTK construction materialises the platform-neutral menu model, registers accelerators and retains check items for later state updates. */
	pub fn new(_window: &Window, model: Arc<RwLock<MenuModel>>) -> Result<Self> {
		let mut platform = Self {
			model,
			checks: HashMap::new(),
		};
		platform.refresh()?;
		Ok(platform)
	}

	/* Refresh rebuilds the GTK tree from an immutable model snapshot and replaces the shell menu bar atomically from the UI thread. */
	pub fn refresh(&mut self) -> Result<()> {
		self.checks.clear();
		let model = self.model.read().map_err(|_| "Menu model lock poisoned")?.clone();
		let menu_bar = gtk::MenuBar::new();
		let accel_group = gtk::AccelGroup::new();
		for section in &model.sections {
			let top_level = gtk::MenuItem::with_label(&section.label);
			top_level.set_sensitive(section.enabled);
			let submenu = gtk::Menu::new();
			self.append_entries(&submenu, &section.entries, &accel_group);
			top_level.set_submenu(Some(&submenu));
			menu_bar.append(&top_level);
		}
		Shell::install_menu_bar(menu_bar, accel_group)
	}

	pub fn set_checked(&mut self, id: &str, checked: bool) -> Result<()> {
		if let Some(item) = self.checks.get(id) {
			item.set_active(checked);
		}
		Ok(())
	}

	pub fn pump(&mut self) {
		let context = glib::MainContext::default();
		while context.pending() {
			context.iteration(false);
		}
	}

/* Recursive materialisation preserves model hierarchy while leaf activation forwards only stable command identifiers. */
	fn append_entries(&mut self, menu: &gtk::Menu, entries: &[MenuEntry], accel_group: &gtk::AccelGroup) {
		for entry in entries {
			match entry {
				MenuEntry::Separator => {
					menu.append(&gtk::SeparatorMenuItem::new());
				}
				MenuEntry::Submenu(section) => {
					let item = gtk::MenuItem::with_label(&section.label);
					item.set_sensitive(section.enabled);
					let submenu = gtk::Menu::new();
					self.append_entries(&submenu, &section.entries, accel_group);
					item.set_submenu(Some(&submenu));
					menu.append(&item);
				}
				MenuEntry::Action { id, label, enabled, shortcut } => {
					let item = gtk::MenuItem::with_label(label);
					item.set_sensitive(*enabled && !id.is_empty());
					if !id.is_empty() {
						let event_id = id.clone();
						item.connect_activate(move |_| push_menu_event(event_id.clone()));
						add_accelerator(&item, *shortcut, accel_group);
					}
					menu.append(&item);
				}
				MenuEntry::Check { id, label, enabled, checked, shortcut } => {
					let item = gtk::CheckMenuItem::with_label(label);
					item.set_active(*checked);
					item.set_sensitive(*enabled && !id.is_empty());
					if !id.is_empty() {
						let event_id = id.clone();
						item.connect_activate(move |_| push_menu_event(event_id.clone()));
						add_accelerator(&item, *shortcut, accel_group);
						self.checks.insert(id.clone(), item.clone());
					}
					menu.append(&item);
				}
			}
		}
	}
}

fn add_accelerator<W: IsA<gtk::Widget>>(item: &W, shortcut: Option<MenuShortcut>, group: &gtk::AccelGroup) {
	let Some(shortcut) = shortcut else {
		return;
	};
	let (key, modifiers) = gtk::accelerator_parse(&gtk_accelerator(shortcut));
	if key != 0 {
		item.add_accelerator("activate", group, key, modifiers, gtk::AccelFlags::VISIBLE);
	}
}

fn gtk_accelerator(shortcut: MenuShortcut) -> String {
	let modifier = match shortcut.modifier {
		MenuModifier::Primary => "<Primary>",
		MenuModifier::Alt => "<Alt>",
		MenuModifier::Unmodified => "",
	};
	let shift = if shortcut.shift { "<Shift>" } else { "" };
	let key = match shortcut.key {
		MenuKey::C => "C",
		MenuKey::D => "D",
		MenuKey::Enter => "Return",
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
	format!("{}{}{}", modifier, shift, key)
}