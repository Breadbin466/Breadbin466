// =======================================================
// src/ui/osd.rs — OSD with MHz metrics and hardware status
// =======================================================

use crate::ui::constants::{CHAR_ROM, FONT_OFFSET_UPPER, FONT_OFFSET_LOWER, PETSCII_CIRCLE, PETSCII_DIAMOND, PETSCII_REWIND, PETSCII_FWD, PETSCII_STOP, PETSCII_EJECT, COLOR_LED_GREEN_ON, COLOR_LED_GREEN_OFF, COLOR_LED_RED_ON, COLOR_LED_RED_OFF, COLOR_TEXT, COLOR_REVERSE_BG, COLOR_REVERSE_FG, COLOR_TRANSPORT_ON, COLOR_TRANSPORT_OFF, LINE_HEIGHT, OSD_MEDIA_LABEL_OVERHEAD, OSD_TOOLTIP_BACKGROUND, OSD_TOOLTIP_BORDER, OSD_TOOLTIP_PADDING_X, OSD_TOOLTIP_PADDING_Y};
/* OsdMonitor is a stateless façade retained by the orchestrator; all visible state for one frame is carried explicitly in OsdData. */
pub struct OsdMonitor;

impl OsdMonitor {   pub fn new() -> Self {
		Self
	}
}

/* OsdData is an immutable presentation snapshot assembled from machine and host state. Rendering it cannot mutate either source. */
pub struct OsdData {
	pub disk_label:     String,
	pub tape_label:     String,
	pub cart_label:     String,
	pub cart_mapper:    Option<(u16, String)>,
	pub joy_status:     String,
	pub fps:            f64,
	pub mhz:            f64,
	pub has_tape:       bool,
	pub play_on:        bool,
	pub record_on:      bool,
	pub odometre:       f64,
	pub power_led:      bool,
	pub activity_led:   bool,
	pub current_track:  Option<u8>,
	pub hover_cursor:   Option<(usize, usize)>,
}

/* The status bar composes media labels, performance metrics, transport state and drive indicators into the framebuffer extension below the emulated display. */
pub fn draw_status_bar(
	buffer:     &mut [u32],
	width:      usize,
	height:     usize,
	gui_height: usize,
	osd:        &OsdData,
) {
	let bar_start = (height - gui_height) * width;
	if bar_start < buffer.len() {
		buffer[bar_start..].fill(0xFFFFFFFF);
	}
	let sep_end = (bar_start + width).min(buffer.len());
	buffer[bar_start..sep_end].fill(0xFF000000);

	let bar_y1 = height - gui_height;

	let media_line = fit_media_line(
		width.saturating_sub(16) / 8,
		&osd.disk_label,
		&osd.tape_label,
		&osd.cart_label,
	);
	let line2 = format!(
		"{:5.3} FPS - {:5.3} MHz - Joystick: {}",
		osd.fps, osd.mhz, osd.joy_status
	);

	let text_y1 = bar_y1 + 4;
	let text_y2 = bar_y1 + 14;

	draw_string(buffer, width, height, 8, text_y1, &media_line.text, COLOR_TEXT);
	draw_string(buffer, width, height, 8, text_y2, &line2, COLOR_TEXT);

	if let Some((cursor_x, cursor_y)) = osd.hover_cursor {
		if cursor_y >= text_y1 && cursor_y < text_y1 + 8 {
			if let Some(label) = media_line.hovered_label(cursor_x.saturating_sub(8) / 8, osd) {
				draw_tooltip(buffer, width, height, bar_y1, cursor_x, &label);
			}
		}
	}

	let transport_x = width.saturating_sub(134);

	let transport = [
		(PETSCII_CIRCLE,  FONT_OFFSET_UPPER, osd.record_on),
		(PETSCII_DIAMOND, FONT_OFFSET_UPPER, osd.play_on),
		(PETSCII_REWIND,  FONT_OFFSET_LOWER, false),
		(PETSCII_FWD,     FONT_OFFSET_LOWER, false),
		(PETSCII_STOP,    FONT_OFFSET_UPPER, false),
		(PETSCII_EJECT,   FONT_OFFSET_UPPER, !osd.has_tape),
	];

	for (i, &(glyph, font, active)) in transport.iter().enumerate() {
		let col = if active { COLOR_TRANSPORT_ON } else { COLOR_TRANSPORT_OFF };
		draw_char_colored(buffer, width, height, transport_x + i * 8, text_y2, glyph, font, col);
	}

	draw_analog_tape_counter(buffer, width, height, width.saturating_sub(78), text_y2, osd.odometre);

	let track_text = match osd.current_track {
		Some(track) => format!("{:02}", track),
		None => "--".to_string(),
	};
	draw_string(buffer, width, height, width.saturating_sub(46), text_y2, &track_text, COLOR_TEXT);

	let activity_color = if osd.activity_led { COLOR_LED_RED_ON  } else { COLOR_LED_RED_OFF  };
	let power_color    = if osd.power_led    { COLOR_LED_GREEN_ON } else { COLOR_LED_GREEN_OFF };
	draw_char_colored(buffer, width, height, width.saturating_sub(12), text_y2, PETSCII_CIRCLE, FONT_OFFSET_UPPER, activity_color);
	draw_char_colored(buffer, width, height, width.saturating_sub(22), text_y2, PETSCII_CIRCLE, FONT_OFFSET_UPPER, power_color);
}

struct MediaLineLayout {
	text: String,
	ranges: [(usize, usize); 3],
	truncated: [bool; 3],
}

impl MediaLineLayout {
	fn hovered_label(&self, char_x: usize, osd: &OsdData) -> Option<String> {
		for index in 0..3 {
			let (start, end) = self.ranges[index];
			let has_additional_cart_details = index == 2 && osd.cart_mapper.is_some();
			if (self.truncated[index] || has_additional_cart_details) && char_x >= start && char_x < end {
				return Some(match index {
					0 => osd.disk_label.clone(),
					1 => osd.tape_label.clone(),
					_ => match &osd.cart_mapper {
						Some((id, name)) => format!("{}\nMapper {}: {}", osd.cart_label, id, name),
						None => osd.cart_label.clone(),
					},
				});
			}
		}
		None
	}
}

/* Space is shared fairly between disk, tape and cartridge labels. Short labels release unused characters to the remaining active labels before any ellipsis is applied. */
fn fit_media_line(max_chars: usize, disk: &str, tape: &str, cart: &str) -> MediaLineLayout {
	let available = max_chars.saturating_sub(OSD_MEDIA_LABEL_OVERHEAD);
	let lengths = [disk.chars().count(), tape.chars().count(), cart.chars().count()];
	let mut limits = [0usize; 3];
	let mut remaining = available;  let mut active = [true; 3];
	let mut active_count = 3usize;

	while active_count > 0 && remaining > 0 {
		let share = remaining / active_count;
		let extra = remaining % active_count;
		let mut changed = false;
		let mut active_index = 0usize;

		for index in 0..3 {
			if !active[index] {
				continue;
			}

			let allowance = share + usize::from(active_index < extra);
			let needed = lengths[index].saturating_sub(limits[index]);
			let granted = allowance.min(needed);
			limits[index] += granted;
			remaining -= granted;
			active_index += 1;

			if limits[index] >= lengths[index] {
				active[index] = false;
				active_count -= 1;
				changed = true;
			}
		}

		if !changed && share == 0 {
			break;
		}
	}

	let displayed = [
		truncate_with_ellipsis(disk, limits[0]),
		truncate_with_ellipsis(tape, limits[1]),
		truncate_with_ellipsis(cart, limits[2]),
	];
	let disk_start = "Disk: ".len();
	let disk_end = disk_start + displayed[0].chars().count();
	let tape_start = disk_end + "  -  Tape: ".len();
	let tape_end = tape_start + displayed[1].chars().count();
	let cart_start = tape_end + "  -  Cartridge: ".len();
	let cart_end = cart_start + displayed[2].chars().count();

	MediaLineLayout {
		text: format!(
			"Disk: {}  -  Tape: {}  -  Cartridge: {}",
			displayed[0], displayed[1], displayed[2],
		),
		ranges: [(disk_start, disk_end), (tape_start, tape_end), (cart_start, cart_end)],
		truncated: [lengths[0] > limits[0], lengths[1] > limits[1], lengths[2] > limits[2]],
	}
}

fn truncate_with_ellipsis(text: &str, max_chars: usize) -> String {
	let char_count = text.chars().count();
	if char_count <= max_chars {
		return text.to_string();
	}
	if max_chars <= 3 {
		return ".".repeat(max_chars);
	}

	let mut truncated = text.chars().take(max_chars - 3).collect::<String>();
	truncated.push_str("...");
	truncated
}

/* Tooltip placement is centred on the hovered label, clamped to the framebuffer and wrapped into fixed-width character rows. */
fn draw_tooltip(
	buffer: &mut [u32],
	width: usize,
	height: usize,
	bar_y: usize,
	cursor_x: usize,
	label: &str,
) {
	let max_line_chars = width.saturating_sub((OSD_TOOLTIP_PADDING_X + 1) * 2) / 8;
	if max_line_chars == 0 {
		return;
	}
	let lines = wrap_tooltip_text(label, max_line_chars);
	let longest_line = lines.iter().map(|line| line.chars().count()).max().unwrap_or(0);
	let tooltip_width = longest_line * 8 + OSD_TOOLTIP_PADDING_X * 2;
	let tooltip_height = lines.len() * LINE_HEIGHT + OSD_TOOLTIP_PADDING_Y * 2;
	let max_x = width.saturating_sub(tooltip_width + 1);
	let x = cursor_x.saturating_sub(tooltip_width / 2).min(max_x);
	let y = bar_y.saturating_sub(tooltip_height + 4);

	fill_rect(buffer, width, height, x, y, tooltip_width, tooltip_height, OSD_TOOLTIP_BACKGROUND);
	draw_rect_outline(buffer, width, height, x, y, tooltip_width, tooltip_height, OSD_TOOLTIP_BORDER);
	for (index, line) in lines.iter().enumerate() {
		draw_string(
			buffer,
			width,
			height,
			x + OSD_TOOLTIP_PADDING_X,
			y + OSD_TOOLTIP_PADDING_Y + index * LINE_HEIGHT,
			line,
			COLOR_TEXT,
		);  }
}

fn wrap_tooltip_text(text: &str, max_chars: usize) -> Vec<String> {
	let mut lines = Vec::new();
	for source_line in text.split('\n') {
		let chars = source_line.chars().collect::<Vec<_>>();
		if chars.is_empty() {
			lines.push(String::new());
		} else {
			lines.extend(
				chars
					.chunks(max_chars)
					.map(|chunk| chunk.iter().collect::<String>()),
			);
		}
	}
	lines
}

fn fill_rect(
	buffer: &mut [u32],
	width: usize,
	height: usize,
	x: usize,
	y: usize,
	rect_width: usize,
	rect_height: usize,
	color: u32,
) {
	let x_end = (x + rect_width).min(width);
	let y_end = (y + rect_height).min(height);
	for row in y..y_end {
		let start = row * width + x;
		let end = row * width + x_end;
		buffer[start..end].fill(color);
	}
}

fn draw_rect_outline(
	buffer: &mut [u32],
	width: usize,
	height: usize,
	x: usize,
	y: usize,
	rect_width: usize,
	rect_height: usize,
	color: u32,
) {
	if rect_width == 0 || rect_height == 0 || x >= width || y >= height {
		return;
	}
	let x_end = (x + rect_width).min(width);
	let y_end = (y + rect_height).min(height);
	if x_end <= x || y_end <= y {
		return;
	}
	buffer[y * width + x..y * width + x_end].fill(color);
	buffer[(y_end - 1) * width + x..(y_end - 1) * width + x_end].fill(color);
	for row in y..y_end {
		buffer[row * width + x] = color;
		buffer[row * width + x_end - 1] = color;
	}
}

pub fn draw_string(buffer: &mut [u32], w: usize, h: usize, x: usize, y: usize, text: &str, color: u32) {
	if y + 8 > h { return; }
	let mut cur_x = x;
	for c in text.chars() {
		if cur_x + 8 > w { break; }
		draw_char_unchecked(buffer, w, cur_x, y, c, color);
		cur_x += 8;
	}
}

fn draw_char_unchecked(buffer: &mut [u32], w: usize, x: usize, y: usize, c: char, color: u32) {
	let char_code = c as u8;
	let rom_index = match c {
		'@'      => 0,
		'A'..='Z' => (char_code - b'A' + 65) as usize,
		'['      => 27,
		']'      => 29,
		' '      => 32,
		'!'      => 33,
		'\"'     => 34,
		'#'      => 35,
		'$'      => 36,
		'%'      => 37,
		'&'      => 38,
		'\''     => 39,
		'('      => 40,
		')'      => 41,
		'*'      => 42,
		'+'      => 43,
		','      => 44,
		'-'      => 45,
		'.'      => 46,
		'/'      => 47,
		'0'..='9' => (char_code - b'0' + 48) as usize,
		':'      => 58,
		';'      => 59,
		'<'      => 60,
		'='      => 61,
		'>'      => 62,
		'?'      => 63,
		'a'..='z' => (char_code - b'a' + 1) as usize,
		_        => 32,
	};
	draw_glyph_unchecked(buffer, w, x, y, rom_index, FONT_OFFSET_LOWER, color);
}

#[inline(always)]
fn draw_char_colored(
	buffer:      &mut [u32],
	w:           usize,
	h:           usize,
	x:           usize,
	y:           usize,
	rom_index:   usize,
	font_offset: usize,
	color:       u32,
) {
	if x + 8 > w || y + 8 > h { return; }
	draw_glyph_unchecked(buffer, w, x, y, rom_index, font_offset, color);
}

#[inline(always)]
fn draw_glyph_unchecked(
	buffer:      &mut [u32],
	w:           usize,
	x:           usize,
	y:           usize,
	rom_index:   usize,
	font_offset: usize,
	color:       u32,
) {
	let offset = font_offset + rom_index * 8;
	if offset + 8 > CHAR_ROM.len() { return; }
	let glyph = &CHAR_ROM[offset..offset + 8];
	for row in 0..8 {
		let line = glyph[row];
		let idx_base = (y + row) * w + x;
		let row_slice = &mut buffer[idx_base..idx_base + 8];
		let mut mask = 0x80u8;
		for px in row_slice.iter_mut() {
			if (line & mask) != 0 {
				*px = color;
			}
			mask >>= 1;
		}
	}
}

pub fn draw_analog_tape_counter(
	buffer:         &mut [u32],
	w:              usize,
	h:              usize,
	x_start:        usize,
	y_start:        usize,
	odometre_value: f64,
) {
	if x_start + 24 > w || y_start + 8 > h { return; }
	let counter_clamped = odometre_value.min(999.99);
	let val_u32         = counter_clamped as u32;

	let d3 = val_u32 / 100;
	let d2 = (val_u32 / 10) % 10;
	let d1 = val_u32 % 10;

	let f1 = counter_clamped - val_u32 as f64;
	let f2 = if d1 == 9 { f1 } else { 0.0 };
	let f3 = if d1 == 9 && d2 == 9 { f1 } else { 0.0 };

	render_rolling_digit(buffer, w, h, x_start,      y_start, d3, f3);
	render_rolling_digit(buffer, w, h, x_start + 8,  y_start, d2, f2);
	render_rolling_digit(buffer, w, h, x_start + 16, y_start, d1, f1);
}

/* A rolling digit vertically interpolates between adjacent ROM glyphs. Carry motion propagates to higher digits only while all lower digits are rolling through nine. */
fn render_rolling_digit(
	buffer:        &mut [u32],
	w:             usize,
	h:             usize,
	x:             usize,
	y:             usize,
	current_digit: u32,
	fraction:      f64,
) {
	if x + 8 > w || y + 8 > h { return; }
	let next_digit = (current_digit + 1) % 10;

	let current_offset = FONT_OFFSET_LOWER + (current_digit + 48) as usize * 8;
	let next_offset    = FONT_OFFSET_LOWER + (next_digit    + 48) as usize * 8;

	let glyph_curr = &CHAR_ROM[current_offset..current_offset + 8];
	let glyph_next = &CHAR_ROM[next_offset..next_offset + 8];

	let shift_pixels = (fraction * 8.0) as i32;

	for row in 0..8i32 {
		let out_y = y + row as usize;
		let virtual_y = row + shift_pixels;
		let line = if virtual_y < 8 {
			glyph_curr[virtual_y as usize]
		} else {
			glyph_next[(virtual_y - 8) as usize]
		};

		let row_start = out_y * w;
		let mut mask = 0x80u8;
		for col in 0..8usize {
			let out_x = x + col;
			let pixel_active = (line & mask) != 0;
			buffer[row_start + out_x] = if pixel_active { COLOR_REVERSE_FG } else { COLOR_REVERSE_BG };
			mask >>= 1;
		}
	}
}