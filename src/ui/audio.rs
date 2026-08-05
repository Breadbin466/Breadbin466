// =======================================================
// src/ui/audio.rs — Audio host using the native device format and burst-tolerant buffering
// =======================================================

use crate::ui::constants::{AUDIO_SAMPLE_RATE, BUFFER_CAPACITY, BUFFER_MASK};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample, SupportedStreamConfig};
use std::sync::Arc;
use std::sync::atomic::{AtomicU16, AtomicU32, AtomicU64, Ordering};
use crate::emulator::Result;

/* SpscRing is the lock-free boundary between the emulation thread and the real-time audio callback. The producer publishes complete sample slots with Release ordering; the callback observes them with Acquire ordering and never blocks or allocates. */
struct SpscRing {
	buf: Vec<AtomicU32>,
	head: AtomicU16,
	tail: AtomicU16,
	overruns: AtomicU64,
}

impl SpscRing {
	fn new() -> Self {
		Self {
			buf: (0..BUFFER_CAPACITY).map(|_| AtomicU32::new(0)).collect(),
			head: AtomicU16::new(0),
			tail: AtomicU16::new(0),
			overruns: AtomicU64::new(0),
		}
	}

	#[inline]
	/* A burst is truncated rather than blocking when the callback falls behind. The overrun counter records lost samples without adding synchronisation to the real-time path. */
	fn push_slice(&self, samples: &[f32]) {
		if samples.is_empty() {
			return;
		}

		let head = self.head.load(Ordering::Acquire);
		let tail = self.tail.load(Ordering::Relaxed);
		let used = tail.wrapping_sub(head) & BUFFER_MASK;
		let available = usize::from((BUFFER_CAPACITY - 1) - used);
		let accepted = samples.len().min(available);
		let dropped = samples.len() - accepted;

		let mut index = 0usize;
		while index < accepted {
			let slot = (tail.wrapping_add(index as u16) & BUFFER_MASK) as usize;
			self.buf[slot].store(samples[index].to_bits(), Ordering::Relaxed);
			index += 1;
		}

		if accepted != 0 {
			self.tail.store(tail.wrapping_add(accepted as u16), Ordering::Release);
		}
		if dropped != 0 {
			self.overruns.fetch_add(dropped as u64, Ordering::Relaxed);
		}
	}

	#[inline]
	fn take_overruns(&self) -> u64 {
		self.overruns.swap(0, Ordering::AcqRel)
	}

	#[inline(always)]
	/* Underrun produces silence instead of repeating stale output. Advancing head only after reading preserves single-consumer ownership of each slot. */
	fn pop(&self, last_sample: &mut f32) -> f32 {
		let head = self.head.load(Ordering::Relaxed);
		let tail = self.tail.load(Ordering::Acquire);

		if head == tail {
			*last_sample = 0.0;
			return 0.0;
		}

		let idx = (head & BUFFER_MASK) as usize;
		let sample = f32::from_bits(self.buf[idx].load(Ordering::Relaxed));

		self.head.store(head.wrapping_add(1), Ordering::Release);
		*last_sample = sample;
		sample
	}
}

/* AudioHost owns the native stream while exposing only a sample queue to the emulator. The callback converts mono f32 samples into the device format and duplicates them across all output channels. */
pub struct AudioHost {
	_stream: cpal::Stream,
	ring: Arc<SpscRing>,
	sample_rate: u32,
}

impl AudioHost {
	/* Stream selection fixes the emulated output rate at 44.1 kHz, then chooses the closest native format and channel count supported by the default device. */
	pub fn new() -> Result<Self> {
		let host = cpal::default_host();
		let device = host.default_output_device()
			.ok_or("No default audio output device found")?;

		let default_config = device.default_output_config()?;
		let preferred_format = default_config.sample_format();
		let preferred_channels = default_config.channels();

		let mut candidates: Vec<SupportedStreamConfig> = device
			.supported_output_configs()?
			.filter(|range| {
				range.min_sample_rate() <= AUDIO_SAMPLE_RATE
					&& range.max_sample_rate() >= AUDIO_SAMPLE_RATE
			})
			.map(|range| range.with_sample_rate(AUDIO_SAMPLE_RATE))
			.collect();

		candidates.sort_by_key(|config| {
			let format_penalty = u8::from(config.sample_format() != preferred_format);
			let channel_penalty = config.channels().abs_diff(preferred_channels);
			(format_penalty, channel_penalty)
		});

		let final_config = candidates
			.into_iter()
			.next()
			.ok_or("The default audio output device does not support 44,100 Hz")?;

		let sample_format = final_config.sample_format();
		let stream_config = final_config.config();
		let sample_rate = stream_config.sample_rate;
		let channels = stream_config.channels as usize;

		println!(
			"[AUDIO] Stream Format: {} Hz, {} channels, {}",
			sample_rate,
			channels,
			sample_format,
		);

		let ring = Arc::new(SpscRing::new());
		let stream = match sample_format {
			cpal::SampleFormat::I8 => build_output_stream::<i8>(&device, stream_config, &ring)?,
			cpal::SampleFormat::I16 => build_output_stream::<i16>(&device, stream_config, &ring)?,
			cpal::SampleFormat::I24 => build_output_stream::<cpal::I24>(&device, stream_config, &ring)?,
			cpal::SampleFormat::I32 => build_output_stream::<i32>(&device, stream_config, &ring)?,
			cpal::SampleFormat::I64 => build_output_stream::<i64>(&device, stream_config, &ring)?,
			cpal::SampleFormat::U8 => build_output_stream::<u8>(&device, stream_config, &ring)?,
			cpal::SampleFormat::U16 => build_output_stream::<u16>(&device, stream_config, &ring)?,
			cpal::SampleFormat::U24 => build_output_stream::<cpal::U24>(&device, stream_config, &ring)?,
			cpal::SampleFormat::U32 => build_output_stream::<u32>(&device, stream_config, &ring)?,
			cpal::SampleFormat::U64 => build_output_stream::<u64>(&device, stream_config, &ring)?,
			cpal::SampleFormat::F32 => build_output_stream::<f32>(&device, stream_config, &ring)?,
			cpal::SampleFormat::F64 => build_output_stream::<f64>(&device, stream_config, &ring)?,
			format => return Err(format!("Unsupported native audio sample format: {}", format).into()),
		};

		stream.play()?;

		Ok(Self {
			_stream: stream,
			ring,
			sample_rate,
		})
	}

	pub fn push_samples(&self, samples: &[f32]) {
		self.ring.push_slice(samples);
		self.ring.take_overruns();
	}

	pub fn get_sample_rate(&self) -> f32 {
		self.sample_rate as f32
	}
}

fn build_output_stream<T>(
	device: &cpal::Device,
	config: cpal::StreamConfig,
	ring: &Arc<SpscRing>,
) -> Result<cpal::Stream>
where
	T: SizedSample + FromSample<f32>,
{
	let channels = config.channels as usize;
	let ring = Arc::clone(ring);
	let mut last_sample = 0.0f32;
	let stream = device.build_output_stream(
		config,
		move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
			for frame in data.chunks_mut(channels) {
				let sample = T::from_sample(ring.pop(&mut last_sample));
				for output in frame {
					*output = sample;
				}
			}
		},
		|error| eprintln!("[AUDIO] Stream error: {}", error),
		None,
	)?;
	Ok(stream)
}