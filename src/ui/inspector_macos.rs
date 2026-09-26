// =======================================================
// src/ui/inspector_macos.rs — AppKit Inspector window
// =======================================================

use super::inspector_content::{Document, InspectorSnapshot, TAB_TITLES};
use crate::emulator::Result;
use objc2::{AnyThread, MainThreadMarker, MainThreadOnly, rc::Retained, runtime::AnyObject};
use objc2_app_kit::{
	NSAutoresizingMaskOptions as Resize, NSBackingStoreType, NSColor, NSFont, NSFontAttributeName,
	NSForegroundColorAttributeName, NSMutableParagraphStyle, NSParagraphStyleAttributeName,
	NSScrollView, NSTabView, NSTabViewItem, NSTextField, NSTextView, NSView, NSWindow,
	NSWindowStyleMask,
};
use objc2_foundation::{
	NSArray, NSDictionary, NSMutableAttributedString, NSPoint, NSRange, NSRect, NSSize, NSString,
};

/* All AppKit objects are retained on the application's main thread. Cocoa
 * supplies typography, accessibility, selection, context menus and DPI scaling. */
pub(super) struct NativeInspector {
	window: Retained<NSWindow>,
	status: Retained<NSTextField>,
	views: Vec<(Retained<NSTextView>, Retained<NSScrollView>)>,
	previous: [String; 3],
}

impl NativeInspector {
	pub fn new(_parent: &winit::window::Window) -> Result<Self> {
		let mtm = MainThreadMarker::new().ok_or("Inspector requires the AppKit main thread")?;
		let rect = NSRect::new(NSPoint::ZERO, NSSize::new(620.0, 680.0));
		let window = unsafe {
			NSWindow::initWithContentRect_styleMask_backing_defer(
				NSWindow::alloc(mtm),
				rect,
				NSWindowStyleMask::Titled
					| NSWindowStyleMask::Closable
					| NSWindowStyleMask::Miniaturizable
					| NSWindowStyleMask::Resizable,
				NSBackingStoreType::Buffered,
				false,
			)
		};
		unsafe {
			window.setReleasedWhenClosed(false);
		}
		window.setTitle(&NSString::from_str("Breadbin466 — Inspector"));
		window.setContentMinSize(NSSize::new(440.0, 340.0));
		let root = NSView::initWithFrame(NSView::alloc(mtm), rect);
		let status = NSTextField::labelWithString(&NSString::from_str("Machine state"), mtm);
		status.setFrame(NSRect::new(
			NSPoint::new(20.0, 640.0),
			NSSize::new(580.0, 22.0),
		));
		status.setAutoresizingMask(Resize::ViewWidthSizable | Resize::ViewMinYMargin);
		root.addSubview(&status);
		let tabs = NSTabView::initWithFrame(
			NSTabView::alloc(mtm),
			NSRect::new(NSPoint::new(16.0, 16.0), NSSize::new(588.0, 612.0)),
		);
		tabs.setAutoresizingMask(Resize::ViewWidthSizable | Resize::ViewHeightSizable);
		let mut views = Vec::new();
		for title in TAB_TITLES {
			let scroll = NSScrollView::initWithFrame(NSScrollView::alloc(mtm), tabs.contentRect());
			scroll.setHasVerticalScroller(true);
			scroll.setHasHorizontalScroller(false);
			scroll.setAutohidesScrollers(true);
			scroll.setAutoresizingMask(Resize::ViewWidthSizable | Resize::ViewHeightSizable);
			let text = NSTextView::initWithFrame(
				NSTextView::alloc(mtm),
				NSRect::new(NSPoint::ZERO, scroll.contentSize()),
			);
			text.setEditable(false);
			text.setSelectable(true);
			text.setRichText(true);
			text.setVerticallyResizable(true);
			text.setHorizontallyResizable(false);
			text.setMinSize(NSSize::ZERO);
			text.setMaxSize(NSSize::new(f64::MAX, f64::MAX));
			text.setAutoresizingMask(Resize::ViewWidthSizable);
			text.setTextContainerInset(NSSize::new(14.0, 12.0));
			if let Some(container) = unsafe { text.textContainer() } {
				container.setContainerSize(NSSize::new(scroll.contentSize().width, f64::MAX));
				container.setWidthTracksTextView(true);
			}
			scroll.setDocumentView(Some(&text));
			let item = unsafe { NSTabViewItem::initWithIdentifier(NSTabViewItem::alloc(), None) };
			item.setLabel(&NSString::from_str(title));
			item.setView(Some(&scroll));
			tabs.addTabViewItem(&item);
			views.push((text, scroll));
		}
		root.addSubview(&tabs);
		window.setContentView(Some(&root));
		window.center();
		window.makeKeyAndOrderFront(None);
		Ok(Self {
			window,
			status,
			views,
			previous: std::array::from_fn(|_| String::new()),
		})
	}
	pub fn show(&self) {
		self.window.deminiaturize(None);
		self.window.makeKeyAndOrderFront(None);
	}
	pub fn is_open(&self) -> bool {
		self.window.isVisible() || self.window.isMiniaturized()
	}
	pub fn is_visible(&self) -> bool {
		self.window.isVisible() && !self.window.isMiniaturized()
	}
	pub fn update(&mut self, snapshot: &InspectorSnapshot) {
		self.status
			.setStringValue(&NSString::from_str(&snapshot.status));
		for (index, document) in snapshot.documents().iter().enumerate() {
			let (view, scroll) = &self.views[index];
			/* Keep selected text stable until the user finishes copying it. */
			if self.previous[index] == document.text || view.selectedRange().length != 0 {
				continue;
			}
			let origin = scroll.contentView().bounds().origin;
			if let Some(storage) = unsafe { view.textStorage() } {
				storage.setAttributedString(&styled(document));
			}
			scroll.contentView().scrollToPoint(origin);
			scroll.reflectScrolledClipView(&scroll.contentView());
			self.previous[index].clone_from(&document.text);
		}
	}
}

impl Drop for NativeInspector {
	fn drop(&mut self) {
		self.window.close();
	}
}

/* Attribute dictionaries contain retained AppKit objects of the types
 * required by each key. UTF-16 ranges match Cocoa's NSString indexing. */
fn styled(document: &Document) -> Retained<NSMutableAttributedString> {
	let font = NSFont::systemFontOfSize(13.0);
	let colour = NSColor::labelColor();
	let paragraph = NSMutableParagraphStyle::new();
	paragraph.setHeadIndent(140.0);
	paragraph.setFirstLineHeadIndent(0.0);
	paragraph.setTabStops(Some(&NSArray::new()));
	paragraph.setDefaultTabInterval(140.0);
	paragraph.setParagraphSpacing(7.0);
	let keys = unsafe {
		[
			NSFontAttributeName,
			NSForegroundColorAttributeName,
			NSParagraphStyleAttributeName,
		]
	};
	let values: [&AnyObject; 3] = [&font, &colour, &paragraph];
	let attrs = NSDictionary::from_slices(&keys, &values);
	let text = unsafe {
		NSMutableAttributedString::initWithString_attributes(
			NSMutableAttributedString::alloc(),
			&NSString::from_str(&document.text),
			Some(&attrs),
		)
	};
	let heading = NSFont::boldSystemFontOfSize(15.0);
	let heading_paragraph = NSMutableParagraphStyle::new();
	heading_paragraph.setParagraphSpacingBefore(12.0);
	heading_paragraph.setParagraphSpacing(8.0);
	for range in &document.headings {
		let range = NSRange::new(
			document.text[..range.start].encode_utf16().count(),
			document.text[range.clone()].encode_utf16().count(),
		);
		unsafe {
			text.addAttribute_value_range(NSFontAttributeName, &heading, range);
			text.addAttribute_value_range(NSParagraphStyleAttributeName, &heading_paragraph, range);
		}
	}
	text
}