// =======================================================
// src/ui/disk_dialog_macos.rs — Native macOS disk dialogs
// =======================================================

use std::path::PathBuf;

use objc2::{MainThreadMarker, MainThreadOnly, sel};
use objc2_app_kit::{
	NSAlert, NSAlertFirstButtonReturn, NSAlertStyle, NSApplication, NSBackingStoreType, NSButton,
	NSButtonType, NSControlStateValueOff, NSControlStateValueOn, NSModalResponseAbort,
	NSModalResponseOK, NSOpenPanel, NSPopUpButton, NSSavePanel, NSTextField, NSView, NSWindow,
	NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

use crate::fdd1541::disk_image::ImageFormat;

/*
Convert Image deliberately separates source selection from conversion options.
NSOpenPanel chooses only the existing source image. A normal AppKit window then
shows the source and destination explicitly and gathers the remaining choices. The
destination is deliberately fixed to the source image directory so the conversion flow
never opens a second filesystem browser.
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

fn frame(x: f64, y: f64, width: f64, height: f64) -> NSRect {
	NSRect::new(NSPoint::new(x, y), NSSize::new(width, height))
}

fn label(text: &str, rect: NSRect, mtm: MainThreadMarker) -> objc2::rc::Retained<NSTextField> {
	let field = NSTextField::labelWithString(&NSString::from_str(text), mtm);
	field.setFrame(rect);
	field
}

fn format_popup(
	rect: NSRect,
	default: ImageFormat,
	mtm: MainThreadMarker,
) -> objc2::rc::Retained<NSPopUpButton> {
	let popup = NSPopUpButton::initWithFrame_pullsDown(NSPopUpButton::alloc(mtm), rect, false);
	for title in ["D64", "D7Z", "G64", "NIB", "NBZ"] {
		popup.addItemWithTitle(&NSString::from_str(title));
	}
	popup.selectItemAtIndex(match default {
		ImageFormat::D64 => 0,
		ImageFormat::D7z => 1,
		ImageFormat::G64 => 2,
		ImageFormat::Nib => 3,
		ImageFormat::Nbz => 4,
	});
	popup
}

fn selected_format(popup: &NSPopUpButton) -> ImageFormat {
	match popup.indexOfSelectedItem() {
		0 => ImageFormat::D64,
		1 => ImageFormat::D7z,
		2 => ImageFormat::G64,
		3 => ImageFormat::Nib,
		_ => ImageFormat::Nbz,
	}
}

fn checkbox(text: &str, rect: NSRect, mtm: MainThreadMarker) -> objc2::rc::Retained<NSButton> {
	/* AppKit's checkbox is an NSButton with NSButtonTypeSwitch. objc2 0.3 exposes
	 * the legacy NSSwitchButton alias as deprecated, while the underlying enum value
	 * remains the native checkbox button type. Constructing the typed value directly
	 * avoids both the deprecated alias and any raw Objective-C callback API. */
	const CHECKBOX_BUTTON_TYPE: NSButtonType = NSButtonType(3);
	let button = NSButton::initWithFrame(NSButton::alloc(mtm), rect);
	button.setButtonType(CHECKBOX_BUTTON_TYPE);
	button.setTitle(&NSString::from_str(text));
	button.setState(NSControlStateValueOff);
	button
}

pub(crate) fn info(title: &str, description: &str) {
	let Some(mtm) = MainThreadMarker::new() else {
		return;
	};
	let alert = NSAlert::new(mtm);
	alert.setAlertStyle(NSAlertStyle::Informational);
	alert.setMessageText(&NSString::from_str(title));
	alert.setInformativeText(&NSString::from_str(description));
	alert.addButtonWithTitle(&NSString::from_str("OK"));
	alert.runModal();
}

pub(crate) fn ask(title: &str, description: &str) -> bool {
	let Some(mtm) = MainThreadMarker::new() else {
		return false;
	};
	let alert = NSAlert::new(mtm);
	alert.setAlertStyle(NSAlertStyle::Warning);
	alert.setMessageText(&NSString::from_str(title));
	alert.setInformativeText(&NSString::from_str(description));
	alert.addButtonWithTitle(&NSString::from_str("Continue"));
	alert.addButtonWithTitle(&NSString::from_str("Cancel"));
	alert.runModal() == NSAlertFirstButtonReturn
}

fn url_path(panel: &NSSavePanel) -> Option<PathBuf> {
	let url = panel.URL()?;
	let path = url.path()?;
	Some(PathBuf::from(path.to_string()))
}

pub(crate) fn convert_image() -> Option<ConvertRequest> {
	let mtm = MainThreadMarker::new()?;

	/* Step 1 uses the standard macOS open panel only to identify the source image. */
	let open_panel = NSOpenPanel::openPanel(mtm);
	open_panel.setTitle(Some(&NSString::from_str("Select Image to Convert")));
	open_panel.setPrompt(Some(&NSString::from_str("Next")));
	open_panel.setCanChooseDirectories(false);
	open_panel.setCanChooseFiles(true);
	open_panel.setAllowsMultipleSelection(false);
	if open_panel.runModal() != NSModalResponseOK {
		return None;
	}
	let source = url_path(&open_panel)?;
	let source_name = source.file_name()?.to_string_lossy().into_owned();
	let stem = source.file_stem()?.to_string_lossy().into_owned();
	let destination_dir = source.parent()?.to_path_buf();

	/* Step 2 is a normal AppKit utility window, not an alert and not another file
	 * browser. All controls are standard AppKit controls. The destination is fixed
	 * to the source directory and is shown explicitly before conversion. */
	let style = NSWindowStyleMask::Titled | NSWindowStyleMask::Closable;
	let window = unsafe {
		NSWindow::initWithContentRect_styleMask_backing_defer(
			NSWindow::alloc(mtm),
			frame(0.0, 0.0, 620.0, 330.0),
			style,
			NSBackingStoreType::Buffered,
			false,
		)
	};
	window.setTitle(&NSString::from_str("Convert Image"));
	window.center();
	let content = window.contentView()?;

	let source_label = label("Source:", frame(28.0, 272.0, 100.0, 22.0), mtm);
	let source_value = label(&source_name, frame(138.0, 272.0, 450.0, 22.0), mtm);
	let destination_label = label("Destination:", frame(28.0, 232.0, 100.0, 22.0), mtm);
	let destination_value = label(
		&destination_dir.display().to_string(),
		frame(138.0, 232.0, 450.0, 22.0),
		mtm,
	);
	let save_label = label("Save as:", frame(28.0, 190.0, 100.0, 22.0), mtm);
	let save_as = NSTextField::textFieldWithString(&NSString::from_str(&stem), mtm);
	save_as.setFrame(frame(138.0, 186.0, 360.0, 26.0));
	let format_label = label("Target format:", frame(28.0, 148.0, 100.0, 22.0), mtm);
	let format = format_popup(frame(138.0, 144.0, 180.0, 26.0), ImageFormat::D7z, mtm);
	let reclaim = checkbox("Reclaim space", frame(138.0, 104.0, 230.0, 24.0), mtm);
	let delete_source = checkbox(
		"Delete source image after successful conversion",
		frame(138.0, 70.0, 420.0, 24.0),
		mtm,
	);
	let cancel = NSButton::initWithFrame(NSButton::alloc(mtm), frame(370.0, 20.0, 100.0, 32.0));
	cancel.setTitle(&NSString::from_str("Cancel"));
	let convert = NSButton::initWithFrame(NSButton::alloc(mtm), frame(480.0, 20.0, 110.0, 32.0));
	convert.setTitle(&NSString::from_str("Convert"));
	convert.setKeyEquivalent(&NSString::from_str("\r"));

	content.addSubview(&source_label);
	content.addSubview(&source_value);
	content.addSubview(&destination_label);
	content.addSubview(&destination_value);
	content.addSubview(&save_label);
	content.addSubview(&save_as);
	content.addSubview(&format_label);
	content.addSubview(&format);
	content.addSubview(&reclaim);
	content.addSubview(&delete_source);
	content.addSubview(&cancel);
	content.addSubview(&convert);

	let app = NSApplication::sharedApplication(mtm);
	unsafe {
		convert.setTarget(Some(&app));
		convert.setAction(Some(sel!(stopModal)));
		cancel.setTarget(Some(&app));
		cancel.setAction(Some(sel!(abortModal)));
	}
	window.makeKeyAndOrderFront(None);

	loop {
		let response = app.runModalForWindow(&window);
		window.orderOut(None);
		if response == NSModalResponseAbort {
			return None;
		}

		let name = save_as.stringValue().to_string();
		if name.trim().is_empty() {
			window.makeKeyAndOrderFront(None);
			continue;
		}

		let destination_format = selected_format(&format);
		let mut destination = destination_dir.join(name.trim());
		destination.set_extension(destination_format.extension());
		return Some(ConvertRequest {
			source,
			destination,
			destination_format,
			reclaim_space: reclaim.state() == NSControlStateValueOn,
			delete_source: delete_source.state() == NSControlStateValueOn,
		});
	}
}

pub(crate) fn create_image() -> Option<CreateRequest> {
	let mtm = MainThreadMarker::new()?;
	let panel = NSSavePanel::savePanel(mtm);
	panel.setTitle(Some(&NSString::from_str("Create New Disk")));
	panel.setPrompt(Some(&NSString::from_str("Create")));
	panel.setNameFieldStringValue(&NSString::from_str("untitled.d64"));

	let accessory = NSView::initWithFrame(NSView::alloc(mtm), frame(0.0, 0.0, 300.0, 42.0));
	let format_label = label("Format:", frame(0.0, 10.0, 58.0, 22.0), mtm);
	let format = format_popup(frame(66.0, 7.0, 176.0, 26.0), ImageFormat::D64, mtm);
	accessory.addSubview(&format_label);
	accessory.addSubview(&format);
	panel.setAccessoryView(Some(&accessory));

	if panel.runModal() != NSModalResponseOK {
		return None;
	}

	let format = selected_format(&format);
	let mut destination = url_path(&panel)?;
	destination.set_extension(format.extension());
	Some(CreateRequest {
		destination,
		format,
	})
}