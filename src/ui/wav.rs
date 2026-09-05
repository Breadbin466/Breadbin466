// =======================================================
// src/ui/wav.rs — Session-long WAV audio capture
// =======================================================

use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::Path;

const WAV_WRITE_BUFFER_BYTES: usize = 1024 * 1024;

/* WavRecorder captures the exact mono host-rate sample stream produced by the
 * motherboard audio-rate converter. It deliberately supports one stable export
 * format only: 16-bit little-endian PCM WAV at the active host sample rate. */
pub struct WavRecorder {
	file: BufWriter<File>,
	pcm_bytes: Vec<u8>,
	data_bytes: u32,
	sample_rate: u32,
	failed: bool,
}

impl WavRecorder {
	pub fn create(path: &Path, sample_rate: u32) -> std::io::Result<Self> {
		let mut file = BufWriter::with_capacity(WAV_WRITE_BUFFER_BYTES, File::create(path)?);
		write_header(&mut file, sample_rate, 0)?;
		Ok(Self {
			file,
			pcm_bytes: Vec::with_capacity(2048),
			data_bytes: 0,
			sample_rate,
			failed: false,
		})
	}

	/* Samples are already the final Breadbin466 host-rate floating-point stream.
	 * WAV capture therefore performs only the representation conversion required
	 * by PCM16 and never resamples or reconstructs SID state independently. */
	pub fn push_samples(&mut self, samples: &[f32]) {
		if self.failed || samples.is_empty() {
			return;
		}

		self.pcm_bytes.clear();
		self.pcm_bytes.reserve(samples.len() * 2);
		for &sample in samples {
			let pcm = (sample.clamp(-1.0, 1.0) * 32767.0).round() as i16;
			self.pcm_bytes.extend_from_slice(&pcm.to_le_bytes());
		}

		if self.file.write_all(&self.pcm_bytes).is_err() {
			self.failed = true;
			eprintln!("[WAV] Recording stopped after an output write failed.");
			return;
		}
		self.data_bytes = self.data_bytes.saturating_add(self.pcm_bytes.len() as u32);
	}

	fn finalise(&mut self) -> std::io::Result<()> {
		if self.failed {
			return Ok(());
		}
		self.file.flush()?;
		self.file.seek(SeekFrom::Start(0))?;
		write_header(&mut self.file, self.sample_rate, self.data_bytes)?;
		self.file.flush()
	}
}

impl Drop for WavRecorder {
	fn drop(&mut self) {
		if let Err(error) = self.finalise() {
			eprintln!("[WAV] Failed to finalise recording: {error}");
		}
	}
}

fn write_header<W: Write>(file: &mut W, sample_rate: u32, data_bytes: u32) -> std::io::Result<()> {
	let riff_size = 36u32.saturating_add(data_bytes);
	let byte_rate = sample_rate.saturating_mul(2);

	file.write_all(b"RIFF")?;
	file.write_all(&riff_size.to_le_bytes())?;
	file.write_all(b"WAVE")?;
	file.write_all(b"fmt ")?;
	file.write_all(&16u32.to_le_bytes())?;
	file.write_all(&1u16.to_le_bytes())?;
	file.write_all(&1u16.to_le_bytes())?;
	file.write_all(&sample_rate.to_le_bytes())?;
	file.write_all(&byte_rate.to_le_bytes())?;
	file.write_all(&2u16.to_le_bytes())?;
	file.write_all(&16u16.to_le_bytes())?;
	file.write_all(b"data")?;
	file.write_all(&data_bytes.to_le_bytes())
}