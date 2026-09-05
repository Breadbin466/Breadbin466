// =======================================================
// src/ui/audio.rs — Low-latency audio host using the native device format
// =======================================================

use crate::emulator::Result;
#[cfg(not(target_os = "windows"))]
use crate::ui::constants::AUDIO_SAMPLE_RATE;
use crate::ui::constants::{BUFFER_CAPACITY, BUFFER_MASK};
#[cfg(not(target_os = "windows"))]
use cpal::SupportedStreamConfig;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
#[cfg(target_os = "linux")]
use cpal::{BufferSize, SupportedBufferSize};
use cpal::{FromSample, SizedSample};
use std::sync::Arc;
use std::sync::atomic::{AtomicU16, AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

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
			self.tail
				.store(tail.wrapping_add(accepted as u16), Ordering::Release);
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
	/* Stream selection follows the host contract: Core Audio and Linux prefer the project rate when available, while WASAPI uses the endpoint mix format required by shared mode. */
	pub fn new() -> Result<Self> {
		let host = cpal::default_host();
		let device = host
			.default_output_device()
			.ok_or("No default audio output device found")?;

		let default_config = device.default_output_config()?;

		/* WASAPI shared mode is defined by the Windows mix format. Requesting 44.1 kHz
		 * merely because the endpoint advertises it can create a stream that opens yet
		 * never reaches the active shared engine. Using the exact default format lets
		 * Windows own any device conversion, while the SID resampler follows the selected
		 * native rate through AudioHost::get_sample_rate(). */
		#[cfg(target_os = "windows")]
		let final_config = default_config.clone();

		#[cfg(not(target_os = "windows"))]
		let final_config = {
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

			candidates
				.into_iter()
				.next()
				.ok_or("The default audio output device does not support 44,100 Hz")?
		};

		let sample_format = final_config.sample_format();
		#[cfg(target_os = "linux")]
		let supported_buffer_size = final_config.buffer_size().clone();
		#[cfg(target_os = "linux")]
		let mut stream_config = final_config.config();
		#[cfg(not(target_os = "linux"))]
		let stream_config = final_config.config();
		/* Linux requests a modest fixed period rather than the former 2,048-frame safety
		 * buffer. At 44.1 kHz, 512 frames represent about 11.6 ms: enough to absorb ordinary
		 * scheduler jitter without making keyboard, video and SID output feel disconnected.
		 * The backend default remains the fallback when its period range is unavailable. */
		#[cfg(target_os = "linux")]
		{
			stream_config.buffer_size = match supported_buffer_size {
				SupportedBufferSize::Range { min, max } => {
					BufferSize::Fixed(512u32.clamp(min, max))
				}
				SupportedBufferSize::Unknown => BufferSize::Default,
			};
		}
		let sample_rate = stream_config.sample_rate;
		let channels = stream_config.channels as usize;

		println!(
			"[AUDIO] Stream Format: {} Hz, {} channels, {}",
			sample_rate, channels, sample_format,
		);

		let ring = Arc::new(SpscRing::new());
		let stream = match sample_format {
			cpal::SampleFormat::I8 => build_output_stream::<i8>(&device, stream_config, &ring)?,
			cpal::SampleFormat::I16 => build_output_stream::<i16>(&device, stream_config, &ring)?,
			cpal::SampleFormat::I24 => {
				build_output_stream::<cpal::I24>(&device, stream_config, &ring)?
			}
			cpal::SampleFormat::I32 => build_output_stream::<i32>(&device, stream_config, &ring)?,
			cpal::SampleFormat::I64 => build_output_stream::<i64>(&device, stream_config, &ring)?,
			cpal::SampleFormat::U8 => build_output_stream::<u8>(&device, stream_config, &ring)?,
			cpal::SampleFormat::U16 => build_output_stream::<u16>(&device, stream_config, &ring)?,
			cpal::SampleFormat::U24 => {
				build_output_stream::<cpal::U24>(&device, stream_config, &ring)?
			}
			cpal::SampleFormat::U32 => build_output_stream::<u32>(&device, stream_config, &ring)?,
			cpal::SampleFormat::U64 => build_output_stream::<u64>(&device, stream_config, &ring)?,
			cpal::SampleFormat::F32 => build_output_stream::<f32>(&device, stream_config, &ring)?,
			cpal::SampleFormat::F64 => build_output_stream::<f64>(&device, stream_config, &ring)?,
			format => {
				return Err(format!("Unsupported native audio sample format: {}", format).into());
			}
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
	let mut last_error_report = Instant::now() - Duration::from_secs(5);
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
		move |error| {
			/* Backends can report the same xrun repeatedly while recovering. Rate limiting
			 * keeps a temporary host-side fault visible without flooding the terminal or
			 * making recovery itself more expensive. */
			if last_error_report.elapsed() >= Duration::from_secs(5) {
				eprintln!("[AUDIO] Stream error: {}", error);
				last_error_report = Instant::now();
			}
		},
		None,
	)?;
	Ok(stream)
}