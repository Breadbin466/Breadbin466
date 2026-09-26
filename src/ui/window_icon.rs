// =======================================================
// src/ui/window_icon.rs — Native application icon loading
// =======================================================

use std::io::Read;
use winit::window::Icon;

use crate::emulator::Result;

const ICON_WIDTH: u32 = 1024;
const ICON_HEIGHT: u32 = 1024;
const ICON_DICTIONARY_SIZE: u32 = 1024 * 1024;
const APPLICATION_ICON: &[u8] = include_bytes!("../../assets/Breadbin466.rgba-delta.lzma2");

/*
 * The embedded asset preserves the original PNG's RGBA pixels without requiring a
 * second compression library. Its raw LZMA2 stream uses a 1 MiB dictionary and
 * contains 1024 × 1024 RGBA8 pixels, with each channel stored as the wrapping
 * difference from the previous pixel in the same row. The first pixel of each
 * row is unchanged. Dimensions and decompression bounds are fixed here because
 * this is a bundled application resource, not an external image format.
 */
fn icon_pixels() -> Result<Vec<u8>> {
	let mut reader = lzma_rust2::Lzma2Reader::new(APPLICATION_ICON, ICON_DICTIONARY_SIZE, None);
	let mut pixels = vec![0; (ICON_WIDTH * ICON_HEIGHT * 4) as usize];
	reader.read_exact(&mut pixels)?;
	if reader.read(&mut [0])? != 0 {
		return Err("The embedded application icon has an incorrect decoded size".into());
	}
	for row in pixels.chunks_exact_mut(ICON_WIDTH as usize * 4) {
		for channel in 4..row.len() {
			row[channel] = row[channel].wrapping_add(row[channel - 4]);
		}
	}
	Ok(pixels)
}

pub fn load_window_icon() -> Result<Icon> {
	Ok(Icon::from_rgba(icon_pixels()?, ICON_WIDTH, ICON_HEIGHT)?)
}

#[cfg(target_os = "linux")]
pub fn load_gtk_icon() -> Result<gtk::gdk_pixbuf::Pixbuf> {
	/* GTK owns the decorated toplevel; winit owns only its rendering child. */
	let pixels = gtk::glib::Bytes::from_owned(icon_pixels()?);
	Ok(gtk::gdk_pixbuf::Pixbuf::from_bytes(
		&pixels,
		gtk::gdk_pixbuf::Colorspace::Rgb,
		true,
		8,
		ICON_WIDTH as i32,
		ICON_HEIGHT as i32,
		(ICON_WIDTH * 4) as i32,
	))
}