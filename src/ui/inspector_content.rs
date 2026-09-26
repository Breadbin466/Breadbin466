// =======================================================
// src/ui/inspector_content.rs — Detached native Inspector documents
// =======================================================

use std::ops::Range;

pub(crate) struct InspectorSection {
	pub title: &'static str,
	pub rows: Vec<(&'static str, String)>,
}

#[derive(Default)]
pub(crate) struct InspectorSnapshot {
	pub status: String,
	pub overview: Vec<InspectorSection>,
	pub chips: Vec<InspectorSection>,
	pub media: Vec<InspectorSection>,
}

/* Text ranges use UTF-8 byte offsets. Each native backend converts these to
 * its own string indices without exposing live emulated state to widgets. */
pub(super) struct Document {
	pub text: String,
	pub headings: Vec<Range<usize>>,
}

impl Document {
	pub fn new(sections: &[InspectorSection]) -> Self {
		let mut text = String::new();
		let mut headings = Vec::new();
		for section in sections {
			if !text.is_empty() {
				text.push('\n');
			}
			let start = text.len();
			text.push_str(section.title);
			headings.push(start..text.len());
			text.push('\n');
			for (label, value) in &section.rows {
				text.push_str(label);
				text.push('\t');
				text.push_str(&value.replace('\n', "\n\t"));
				text.push('\n');
			}
		}
		Self { text, headings }
	}
}

impl InspectorSnapshot {
	pub(super) fn documents(&self) -> [Document; 3] {
		[
			Document::new(&self.overview),
			Document::new(&self.chips),
			Document::new(&self.media),
		]
	}
}

pub(super) const TAB_TITLES: [&str; 3] = ["Overview", "Chips", "Media & ROMs"];