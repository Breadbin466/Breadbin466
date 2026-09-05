// =======================================================
// src/ui/about_macos.rs — Native AppKit About panel backend
// =======================================================

#![allow(unused_unsafe)]

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{
	NSAboutPanelOptionApplicationName, NSAboutPanelOptionApplicationVersion,
	NSAboutPanelOptionCredits, NSAboutPanelOptionVersion, NSApplication,
};
use objc2_foundation::{NSAttributedString, NSDictionary, NSString};
use winit::window::Window;

use super::about::{APP_NAME, COPYRIGHT, VERSION, description};
use crate::emulator::Result;

/* AppKit requires About-panel objects to be created and presented on the main thread. Rust-owned strings are converted into retained Objective-C objects before the options dictionary is handed to NSApplication. */

/* AppKit presentation remains on the main thread and uses the standard application About panel rather than a custom event loop. */
pub fn show(_window: &Window) -> Result<()> {
	let main_thread = MainThreadMarker::new()
		.ok_or("The About panel must be presented on the macOS main thread")?;

	let credits_text = format!("{}\n\n{}", description("\n\n"), COPYRIGHT);
	let credits = NSAttributedString::from_nsstring(&NSString::from_str(&credits_text));

	let name = NSString::from_str(APP_NAME);
	let version = NSString::from_str(VERSION);
	let build = NSString::from_str("");

	let keys: [&NSString; 4] = [
		unsafe { NSAboutPanelOptionApplicationName },
		unsafe { NSAboutPanelOptionApplicationVersion },
		unsafe { NSAboutPanelOptionVersion },
		unsafe { NSAboutPanelOptionCredits },
	];

	let values: [Retained<AnyObject>; 4] = [
		into_any(name),
		into_any(version),
		into_any(build),
		into_any_attributed(credits),
	];

	let options = NSDictionary::from_retained_objects(&keys, &values);

	unsafe {
		NSApplication::sharedApplication(main_thread)
			.orderFrontStandardAboutPanelWithOptions(&options);
	}

	Ok(())
}

fn into_any(value: Retained<NSString>) -> Retained<AnyObject> {
	Retained::into_super(Retained::into_super(value))
}

fn into_any_attributed(value: Retained<NSAttributedString>) -> Retained<AnyObject> {
	Retained::into_super(Retained::into_super(value))
}