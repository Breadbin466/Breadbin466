// =======================================================
// src/ui/menu_macos.rs — Native AppKit menu backend
// =======================================================

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use objc2::rc::Retained;
use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
	NSApplication, NSControlStateValueOff, NSControlStateValueOn, NSEventModifierFlags,
	NSMenu, NSMenuItem,
};
use objc2_foundation::{NSObject, NSObjectProtocol, NSString};
use winit::window::Window;

use crate::emulator::Result;
use super::menu::{push_menu_event, MenuEntry, MenuKey, MenuModel, MenuModifier, MenuSection, MenuShortcut};

static COMMANDS: OnceLock<Mutex<HashMap<isize, String>>> = OnceLock::new();

define_class!(
	#[unsafe(super(NSObject))]
	#[thread_kind = MainThreadOnly]
	struct Breadbin466MenuTarget;

	impl Breadbin466MenuTarget {
		#[unsafe(method(activateBreadbinMenuItem:))]
		fn activate_menu_item(&self, sender: &NSMenuItem) {
			let tag = sender.tag();
			if let Ok(commands) = COMMANDS.get_or_init(|| Mutex::new(HashMap::new())).lock() {
				if let Some(id) = commands.get(&tag) {
					push_menu_event(id.clone());
				}
			}
		}
	}

	unsafe impl NSObjectProtocol for Breadbin466MenuTarget {}
);

impl Breadbin466MenuTarget {
	fn new(main_thread: MainThreadMarker) -> Retained<Self> {
		let allocated = Self::alloc(main_thread).set_ivars(());
		unsafe { msg_send![super(allocated), init] }
	}
}

/* PlatformMenu materialises the shared MenuModel as an AppKit main menu. Stable command identifiers are translated through integer tags because AppKit callbacks deliver native menu items rather than application strings. */
pub struct PlatformMenu {
	model: Arc<RwLock<MenuModel>>,
	target: Retained<Breadbin466MenuTarget>,
	items: HashMap<String, Retained<NSMenuItem>>,
	next_tag: isize,
}

impl PlatformMenu {
	/* Construction must occur on the AppKit main thread and installs one retained target for every generated command item. */
/* AppKit construction replaces the process menu bar from the shared model and retains checkable items for synchronisation. */
	pub fn new(_window: &Window, model: Arc<RwLock<MenuModel>>) -> Result<Self> {
		let main_thread = MainThreadMarker::new().ok_or("AppKit menu must be created on the main thread")?;
		let mut platform = Self {
			model,
			target: Breadbin466MenuTarget::new(main_thread),
			items: HashMap::new(),
			next_tag: 1,
		};
		platform.refresh()?;
		Ok(platform)
	}

	/* Refresh recreates the complete NSMenu tree, resets tag allocation and republishes the tag-to-command map used by the Objective-C callback. */
	pub fn refresh(&mut self) -> Result<()> {
		let main_thread = MainThreadMarker::new().ok_or("AppKit menu refresh must run on the main thread")?;
		let model = self.model.read().map_err(|_| "Menu model lock poisoned")?.clone();
		self.items.clear();
		self.next_tag = 1;
		if let Ok(mut commands) = COMMANDS.get_or_init(|| Mutex::new(HashMap::new())).lock() {
			commands.clear();
		}

		let root = new_menu("", main_thread);
		for section in &model.sections {
			self.append_top_level(&root, section, main_thread);
		}
		NSApplication::sharedApplication(main_thread).setMainMenu(Some(&root));
		Ok(())
	}

	pub fn set_checked(&mut self, id: &str, checked: bool) -> Result<()> {
		if let Some(item) = self.items.get(id) {
			item.setState(if checked { NSControlStateValueOn } else { NSControlStateValueOff });
		}
		Ok(())
	}

	pub fn pump(&mut self) {}

	fn append_top_level(&mut self, root: &NSMenu, section: &MenuSection, main_thread: MainThreadMarker) {
		let item = new_item(&section.label, section.enabled, main_thread);
		let submenu = new_menu(&section.label, main_thread);
		self.append_entries(&submenu, &section.entries, main_thread);
		item.setSubmenu(Some(&submenu));
		root.addItem(&item);
	}

/* Recursive conversion preserves hierarchy while native targets forward only stable command identifiers. */
	fn append_entries(&mut self, menu: &NSMenu, entries: &[MenuEntry], main_thread: MainThreadMarker) {
		for entry in entries {
			match entry {
				MenuEntry::Separator => menu.addItem(&NSMenuItem::separatorItem(main_thread)),
				MenuEntry::Submenu(section) => {
					let item = new_item(&section.label, section.enabled, main_thread);
					let submenu = new_menu(&section.label, main_thread);
					self.append_entries(&submenu, &section.entries, main_thread);
					item.setSubmenu(Some(&submenu));
					menu.addItem(&item);
				}
				MenuEntry::Action { id, label, enabled, shortcut } => {
					let item = self.command_item(id, label, *enabled, false, *shortcut, main_thread);
					menu.addItem(&item);
				}
				MenuEntry::Check { id, label, enabled, checked, shortcut } => {
					let item = self.command_item(id, label, *enabled, *checked, *shortcut, main_thread);
					menu.addItem(&item);
				}
			}
		}
	}

	fn command_item(
		&mut self,
		id: &str,
		label: &str,
		enabled: bool,
		checked: bool,
		shortcut: Option<MenuShortcut>,
		main_thread: MainThreadMarker,
	) -> Retained<NSMenuItem> {
		let item = new_item(label, enabled, main_thread);
		let tag = self.next_tag;
		self.next_tag += 1;
		item.setTag(tag);
		unsafe {
			item.setTarget(Some(&*self.target));
			item.setAction(Some(sel!(activateBreadbinMenuItem:)));
		}
		item.setState(if checked { NSControlStateValueOn } else { NSControlStateValueOff });
		if let Some(shortcut) = shortcut {
			let equivalent = NSString::from_str(key_equivalent(shortcut.key));
			item.setKeyEquivalent(&equivalent);
			let mut mask = match shortcut.modifier {
				MenuModifier::Primary => NSEventModifierFlags::Command,
				MenuModifier::Alt => NSEventModifierFlags::Option,
				MenuModifier::Unmodified => NSEventModifierFlags::empty(),
			};
			if shortcut.shift {
				mask |= NSEventModifierFlags::Shift;
			}
			item.setKeyEquivalentModifierMask(mask);
		}
		if let Ok(mut commands) = COMMANDS.get_or_init(|| Mutex::new(HashMap::new())).lock() {
			commands.insert(tag, id.to_string());
		}
		self.items.insert(id.to_string(), item.clone());
		item
	}
}

fn new_menu(title: &str, main_thread: MainThreadMarker) -> Retained<NSMenu> {
	let title = NSString::from_str(title);
	let menu = NSMenu::initWithTitle(NSMenu::alloc(main_thread), &title);
	menu.setAutoenablesItems(false);
	menu
}

fn new_item(title: &str, enabled: bool, main_thread: MainThreadMarker) -> Retained<NSMenuItem> {
	let title = NSString::from_str(title);
	let empty = NSString::from_str("");
	let item = unsafe {
		NSMenuItem::initWithTitle_action_keyEquivalent(
			NSMenuItem::alloc(main_thread),
			&title,
			None,
			&empty,
		)
	};
	item.setEnabled(enabled);
	item
}

fn key_equivalent(key: MenuKey) -> &'static str {
	match key {
		MenuKey::C => "c",
		MenuKey::D => "d",
		MenuKey::Enter => "\r",
		MenuKey::F => "f",
		MenuKey::F4 => "\u{f707}",
		MenuKey::I => "i",
		MenuKey::J => "j",
		MenuKey::M => "m",
		MenuKey::P => "p",
		MenuKey::Pause => "",
		MenuKey::Q => "q",
		MenuKey::R => "r",
		MenuKey::T => "t",
		MenuKey::W => "w",
	}
}