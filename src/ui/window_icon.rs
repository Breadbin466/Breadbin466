// =======================================================
// src/ui/window_icon.rs — Native application icon loading
// =======================================================

use std::io::Cursor;
use winit::window::Icon;

use crate::emulator::Result;

const APPLICATION_ICON_PNG: &[u8] = include_bytes!("../../assets/Breadbin466.png");

/*
 * The desktop icon is embedded in the executable rather than resolved relative to
 * the current working directory. This keeps packaged, command-line and development
 * launches identical, and avoids silently falling back to a toolkit placeholder when
 * the process is started outside the repository root.
 */
pub fn load_window_icon() -> Result<Icon> {
	let decoder = png::Decoder::new(Cursor::new(APPLICATION_ICON_PNG));
	let mut reader = decoder.read_info()?;
	let required = reader
		.output_buffer_size()
		.ok_or("The embedded application icon has no finite decoded size")?;
	let mut pixels = vec![0; required];
	let info = reader.next_frame(&mut pixels)?;

	if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
		return Err("The embedded application icon must be an 8-bit RGBA PNG".into());
	}

	pixels.truncate(info.buffer_size());
	Ok(Icon::from_rgba(pixels, info.width, info.height)?)
}

#[cfg(target_os = "linux")]
pub fn load_gtk_icon() -> Result<gdk_pixbuf::Pixbuf> {
	use gdk_pixbuf::prelude::PixbufLoaderExt;

	/*
	 * GTK owns the decorated Linux toplevel while winit owns only the embedded X11
	 * rendering child. The icon must therefore also be applied to the GTK window;
	 * setting it on the child alone cannot affect the desktop shell or task switcher.
	 */
	let loader = gdk_pixbuf::PixbufLoader::new();
	loader.write(APPLICATION_ICON_PNG)?;
	loader.close()?;
	loader
		.pixbuf()
		.ok_or_else(|| "GTK could not decode the embedded application icon".into())
}