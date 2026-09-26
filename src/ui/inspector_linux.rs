// =======================================================
// src/ui/inspector_linux.rs — GTK Inspector window
// =======================================================

use super::inspector_content::{InspectorSnapshot, TAB_TITLES};
use crate::emulator::Result;
use gtk::{pango, prelude::*};
use std::{cell::Cell, rc::Rc};

/* GTK owns the modeless window, notebook, selectable text and scroll bars.
 * The existing application shell pumps its events on the main thread. */
pub(super) struct NativeInspector {
	window: gtk::Window,
	status: gtk::Label,
	views: Vec<(gtk::TextView, gtk::ScrolledWindow)>,
	previous: [String; 3],
	closed: Rc<Cell<bool>>,
}

impl NativeInspector {
	pub fn new(_parent: &winit::window::Window) -> Result<Self> {
		gtk::init()?;
		let window = gtk::Window::new(gtk::WindowType::Toplevel);
		window.set_title("Breadbin466 — Inspector");
		window.set_default_size(620, 680);
		window.set_size_request(440, 340);
		let root = gtk::Box::new(gtk::Orientation::Vertical, 12);
		root.set_border_width(16);
		let status = gtk::Label::new(Some("Machine state"));
		status.set_xalign(0.0);
		root.pack_start(&status, false, false, 0);
		let notebook = gtk::Notebook::new();
		let mut views = Vec::new();
		for title in TAB_TITLES {
			let scroll =
				gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
			scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
			let view = gtk::TextView::new();
			view.set_editable(false);
			view.set_cursor_visible(false);
			view.set_wrap_mode(gtk::WrapMode::WordChar);
			view.set_left_margin(14);
			view.set_right_margin(14);
			view.set_top_margin(12);
			view.set_bottom_margin(12);
			let buffer = view.buffer().ok_or("Inspector text buffer unavailable")?;
			let tags = buffer
				.tag_table()
				.ok_or("Inspector tag table unavailable")?;
			tags.add(
				&gtk::TextTag::builder()
					.name("body")
					.left_margin(140)
					.indent(-140)
					.pixels_below_lines(7)
					.build(),
			);
			tags.add(
				&gtk::TextTag::builder()
					.name("heading")
					.weight(700)
					.size_points(15.0)
					.left_margin(0)
					.indent(0)
					.pixels_above_lines(12)
					.build(),
			);
			let mut tabs = pango::TabArray::new(1, true);
			tabs.set_tab(0, pango::TabAlign::Left, 140);
			view.set_tabs(&tabs);
			scroll.add(&view);
			notebook.append_page(&scroll, Some(&gtk::Label::new(Some(title))));
			views.push((view, scroll));
		}
		root.pack_start(&notebook, true, true, 0);
		window.add(&root);
		let closed = Rc::new(Cell::new(false));
		let flag = closed.clone();
		window.connect_delete_event(move |_, _| {
			flag.set(true);
			gtk::glib::Propagation::Proceed
		});
		window.show_all();
		Ok(Self {
			window,
			status,
			views,
			previous: std::array::from_fn(|_| String::new()),
			closed,
		})
	}
	pub fn show(&self) {
		self.window.deiconify();
		self.window.present();
	}
	pub fn is_open(&self) -> bool {
		!self.closed.get()
	}
	pub fn is_visible(&self) -> bool {
		self.is_open()
			&& self.window.is_visible()
			&& self
				.window
				.window()
				.is_none_or(|w| !w.state().contains(gtk::gdk::WindowState::ICONIFIED))
	}
	pub fn update(&mut self, snapshot: &InspectorSnapshot) {
		self.status.set_text(&snapshot.status);
		for (index, document) in snapshot.documents().iter().enumerate() {
			let (view, scroll) = &self.views[index];
			let Some(buffer) = view.buffer() else {
				continue;
			};
			if self.previous[index] == document.text || buffer.has_selection() {
				continue;
			}
			let adjustment = scroll.vadjustment();
			let position = adjustment.value();
			buffer.set_text(&document.text);
			buffer.apply_tag_by_name("body", &buffer.start_iter(), &buffer.end_iter());
			for range in &document.headings {
				let start = document.text[..range.start].chars().count() as i32;
				let end = document.text[..range.end].chars().count() as i32;
				buffer.apply_tag_by_name(
					"heading",
					&buffer.iter_at_offset(start),
					&buffer.iter_at_offset(end),
				);
			}
			adjustment.set_value(position);
			self.previous[index].clone_from(&document.text);
		}
	}
}

impl Drop for NativeInspector {
	fn drop(&mut self) {
		if !self.closed.get() {
			self.window.close();
		}
	}
}