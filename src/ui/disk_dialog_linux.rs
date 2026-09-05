// =======================================================
// src/ui/disk_dialog_linux.rs — Native GTK disk dialogs
// =======================================================

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use gtk::prelude::*;

use crate::fdd1541::disk_image::ImageFormat;

/*
Convert Image deliberately separates source selection from conversion options.
The first native file chooser only identifies the existing source image. The second
small GTK dialog shows the source and destination explicitly and gathers only the
remaining conversion choices. A folder chooser is opened only when Change… is used.
*/

#[derive(Clone, Debug)]
pub(crate) struct ConvertRequest {
	pub source: PathBuf,
	pub destination: PathBuf,
	pub destination_format: ImageFormat,
	pub reclaim_space: bool,
	pub delete_source: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct CreateRequest {
	pub destination: PathBuf,
	pub format: ImageFormat,
}

fn format_combo(default: ImageFormat) -> gtk::ComboBoxText {
	let combo = gtk::ComboBoxText::new();
	for format in ["D64", "D7Z", "G64", "NIB", "NBZ"] {
		combo.append_text(format);
	}
	combo.set_active(Some(match default {
		ImageFormat::D64 => 0,
		ImageFormat::D7z => 1,
		ImageFormat::G64 => 2,
		ImageFormat::Nib => 3,
		ImageFormat::Nbz => 4,
	}));
	combo
}

fn selected_format(combo: &gtk::ComboBoxText) -> ImageFormat {
	match combo.active().unwrap_or(0) {
		0 => ImageFormat::D64,
		1 => ImageFormat::D7z,
		2 => ImageFormat::G64,
		3 => ImageFormat::Nib,
		_ => ImageFormat::Nbz,
	}
}

fn basename(path: &Path) -> Option<String> {
	Some(path.file_stem()?.to_string_lossy().into_owned())
}

fn display_name(path: &Path) -> String {
	path.file_name()
		.map(|name| name.to_string_lossy().into_owned())
		.unwrap_or_else(|| path.display().to_string())
}

pub(crate) fn convert_image() -> Option<ConvertRequest> {
	/* Step 1: select only the source image. */
	let open_dialog = gtk::FileChooserDialog::with_buttons(
		Some("Select Image to Convert"),
		None::<&gtk::Window>,
		gtk::FileChooserAction::Open,
		&[
			("Cancel", gtk::ResponseType::Cancel),
			("Next", gtk::ResponseType::Accept),
		],
	);
	open_dialog.set_select_multiple(false);
	if open_dialog.run() != gtk::ResponseType::Accept {
		open_dialog.close();
		return None;
	}
	let source = open_dialog.filename()?;
	open_dialog.close();

	let destination_dir = Arc::new(Mutex::new(source.parent()?.to_path_buf()));
	let dialog = gtk::Dialog::with_buttons(
		Some("Convert Image"),
		None::<&gtk::Window>,
		gtk::DialogFlags::MODAL,
		&[
			("Cancel", gtk::ResponseType::Cancel),
			("Convert", gtk::ResponseType::Accept),
		],
	);
	dialog.set_default_response(gtk::ResponseType::Accept);

	let content = dialog.content_area();
	content.set_spacing(12);
	content.set_margin_top(16);
	content.set_margin_bottom(12);
	content.set_margin_start(18);
	content.set_margin_end(18);

	let grid = gtk::Grid::new();
	grid.set_column_spacing(12);
	grid.set_row_spacing(10);

	let source_title = gtk::Label::new(Some("Source:"));
	source_title.set_halign(gtk::Align::End);
	let source_value = gtk::Label::new(Some(&display_name(&source)));
	source_value.set_halign(gtk::Align::Start);

	let destination_title = gtk::Label::new(Some("Destination:"));
	destination_title.set_halign(gtk::Align::End);
	let destination_value =
		gtk::Label::new(Some(&destination_dir.lock().ok()?.display().to_string()));
	destination_value.set_halign(gtk::Align::Start);
	destination_value.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
	destination_value.set_hexpand(true);
	let change = gtk::Button::with_label("Change…");
	{
		let destination_dir = Arc::clone(&destination_dir);
		let destination_value = destination_value.clone();
		change.connect_clicked(move |_| {
			let chooser = gtk::FileChooserDialog::with_buttons(
				Some("Choose Destination Folder"),
				None::<&gtk::Window>,
				gtk::FileChooserAction::SelectFolder,
				&[
					("Cancel", gtk::ResponseType::Cancel),
					("Choose", gtk::ResponseType::Accept),
				],
			);
			if chooser.run() == gtk::ResponseType::Accept {
				if let Some(folder) = chooser.filename() {
					if let Ok(mut current) = destination_dir.lock() {
						*current = folder.clone();
					}
					destination_value.set_text(&folder.display().to_string());
				}
			}
			chooser.close();
		});
	}

	let save_title = gtk::Label::new(Some("Save as:"));
	save_title.set_halign(gtk::Align::End);
	let save_as = gtk::Entry::new();
	save_as.set_text(&basename(&source)?);
	save_as.set_hexpand(true);

	let format_title = gtk::Label::new(Some("Target format:"));
	format_title.set_halign(gtk::Align::End);
	let format = format_combo(ImageFormat::D7z);
	let reclaim = gtk::CheckButton::with_label("Reclaim space");
	let delete_source =
		gtk::CheckButton::with_label("Delete source image after successful conversion");

	grid.attach(&source_title, 0, 0, 1, 1);
	grid.attach(&source_value, 1, 0, 2, 1);
	grid.attach(&destination_title, 0, 1, 1, 1);
	grid.attach(&destination_value, 1, 1, 1, 1);
	grid.attach(&change, 2, 1, 1, 1);
	grid.attach(&save_title, 0, 2, 1, 1);
	grid.attach(&save_as, 1, 2, 2, 1);
	grid.attach(&format_title, 0, 3, 1, 1);
	grid.attach(&format, 1, 3, 1, 1);
	grid.attach(&reclaim, 1, 4, 2, 1);
	grid.attach(&delete_source, 1, 5, 2, 1);
	content.add(&grid);
	dialog.show_all();

	loop {
		let response = dialog.run();
		if response == gtk::ResponseType::Accept {
			let name = save_as.text().trim().to_string();
			if name.is_empty() {
				continue;
			}
			let destination_format = selected_format(&format);
			let current_dir = destination_dir.lock().ok()?.clone();
			let mut destination = current_dir.join(name);
			destination.set_extension(destination_format.extension());
			let request = ConvertRequest {
				source,
				destination,
				destination_format,
				reclaim_space: reclaim.is_active(),
				delete_source: delete_source.is_active(),
			};
			dialog.close();
			return Some(request);
		}
		if response == gtk::ResponseType::Cancel || response == gtk::ResponseType::DeleteEvent {
			dialog.close();
			return None;
		}
	}
}

pub(crate) fn create_image() -> Option<CreateRequest> {
	let dialog = gtk::FileChooserDialog::with_buttons(
		Some("Create New Disk"),
		None::<&gtk::Window>,
		gtk::FileChooserAction::Save,
		&[
			("Cancel", gtk::ResponseType::Cancel),
			("Create", gtk::ResponseType::Accept),
		],
	);
	dialog.set_do_overwrite_confirmation(true);
	dialog.set_current_name("untitled.d64");

	let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
	let label = gtk::Label::new(Some("Format:"));
	let format = format_combo(ImageFormat::D64);
	row.pack_start(&label, false, false, 0);
	row.pack_start(&format, true, true, 0);
	row.set_margin_top(8);
	row.set_margin_bottom(8);
	row.set_margin_start(8);
	row.set_margin_end(8);
	row.show_all();
	dialog.set_extra_widget(&row);

	if dialog.run() != gtk::ResponseType::Accept {
		dialog.close();
		return None;
	}
	let format = selected_format(&format);
	let mut destination = dialog.filename()?;
	destination.set_extension(format.extension());
	dialog.close();
	Some(CreateRequest {
		destination,
		format,
	})
}