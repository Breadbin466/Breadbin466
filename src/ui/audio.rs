// =======================================================
// src/ui/audio.rs — Low-latency audio host using the native device format
// =======================================================

use super::audio_resampler::OutputResampler;
use crate::clockchip::constants::{CPU_FREQ_HZ, CYCLES_PER_FRAME};
use crate::emulator::Result;
use crate::ui::constants::AUDIO_SAMPLE_RATE;
use crate::ui::constants::BUFFER_CAPACITY;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, SupportedBufferSize};
use cpal::{FromSample, SizedSample};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

/* SpscRing is the lock-free boundary between the emulation thread and the real-time audio callback. The producer publishes complete sample slots with Release ordering; the callback observes them with Acquire ordering and never blocks or allocates. */
struct SpscRing {
	buf: Vec<AtomicU32>,
	mask: u16,
	head: AtomicU16,
	tail: AtomicU16,
	overruns: AtomicU64,
	discard_to: AtomicU32,
}

impl SpscRing {
	fn new() -> Self {
		Self::for_rate(AUDIO_SAMPLE_RATE)
	}

	/* Preserve the queue's time capacity at fallback rates, including outputs
	 * above 192 kHz whose single emulated frame exceeds the base queue size. */
	fn for_rate(rate: u32) -> Self {
		let capacity = (u64::from(BUFFER_CAPACITY) * u64::from(rate))
			.div_ceil(u64::from(AUDIO_SAMPLE_RATE))
			.next_power_of_two()
			.clamp(2, 32768) as u16;
		Self {
			mask: capacity - 1,
			buf: (0..capacity).map(|_| AtomicU32::new(0)).collect(),
			head: AtomicU16::new(0),
			tail: AtomicU16::new(0),
			overruns: AtomicU64::new(0),
			discard_to: AtomicU32::new(0),
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
		let used = tail.wrapping_sub(head) & self.mask;
		let available = usize::from(self.mask - used);
		let accepted = samples.len().min(available);
		let dropped = samples.len() - accepted;

		let mut index = 0usize;
		while index < accepted {
			let slot = (tail.wrapping_add(index as u16) & self.mask) as usize;
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

	/* Only the consumer advances head. A discard request snapshots the producer
	 * boundary so samples published afterwards remain available for playback. */
	fn discard_pending(&self) {
		self.discard_to.store(0x10000 | u32::from(self.tail.load(Ordering::Acquire)), Ordering::Release);
	}

	fn apply_discard(&self) -> bool {
		let request = self.discard_to.swap(0, Ordering::AcqRel);
		if request & 0x10000 != 0 {
			let head = self.head.load(Ordering::Relaxed);
			let tail = self.tail.load(Ordering::Acquire);
			let target = request as u16;
			if target.wrapping_sub(head) <= tail.wrapping_sub(head) {
				self.head.store(target, Ordering::Release);
			}
		}
		request & 0x10000 != 0
	}

	/* The callback owns head throughout a block. Publish consumed slots together
	 * rather than transferring ownership for each sample. If a producer burst
	 * arrives during a short block, consume it before filling the rest with silence. */
	fn write_output<T: SizedSample + FromSample<f32>>(&self, mut data: &mut [T], channels: usize) {
		while !data.is_empty() {
			let head = self.head.load(Ordering::Relaxed);
			let tail = self.tail.load(Ordering::Acquire);
			let count = usize::from(tail.wrapping_sub(head)).min(data.len().div_ceil(channels));
			if count == 0 {
				data.fill(T::from_sample(0.0));
				return;
			}
			let length = (count * channels).min(data.len());
			let (ready, remaining) = data.split_at_mut(length);
			for (index, frame) in ready.chunks_mut(channels).enumerate() {
				let slot = usize::from(head.wrapping_add(index as u16) & self.mask);
				let sample = T::from_sample(f32::from_bits(self.buf[slot].load(Ordering::Relaxed)));
				frame.fill(sample);
			}
			self.head.store(head.wrapping_add(count as u16), Ordering::Release);
			data = remaining;
		}
	}

}

/* Stream callbacks only move samples and publish health flags. Device queries,
 * retries and fallback resampling belong to the host service boundary. The SID
 * and WAV rate stays at 44.1 kHz even if the output device changes rate. */
pub struct AudioHost {
	stream: Option<cpal::Stream>,
	ring: Arc<SpscRing>,
	health: Arc<StreamHealth>,
	device_id: Option<cpal::DeviceId>,
	last_check: Option<Instant>,
	last_progress: Instant,
	last_callbacks: u64,
	resampler: Option<OutputResampler>,
	converted: Vec<f32>,
	last_error: Option<String>,
}

#[derive(Default)]
struct StreamHealth {
	failed: AtomicBool,
	callbacks: AtomicU64,
}

impl AudioHost {
	/* An absent device is a recoverable state, including at application startup.
	 * Keeping the host alive permits hot-plug recovery without restarting the C64. */
	pub fn new() -> Self {
		let mut host = Self {
			stream: None,
			ring: Arc::new(SpscRing::new()),
			health: Arc::new(StreamHealth::default()),
			device_id: None,
			last_check: None,
			last_progress: Instant::now(),
			last_callbacks: 0,
			resampler: None,
			converted: Vec::new(),
			last_error: None,
		};
		host.service();
		host
	}

	/* One check per second observes default-device changes and fatal errors.
	 * A five-second callback watchdog also catches streams lost during sleep.
	 * A replacement always gets a fresh queue, so disconnected audio is not replayed. */
	pub fn service(&mut self) {
		let now = Instant::now();
		if self
			.last_check
			.is_some_and(|last| now.duration_since(last) < Duration::from_secs(1))
		{
			return;
		}
		self.last_check = Some(now);
		let callbacks = self.health.callbacks.load(Ordering::Relaxed);
		if callbacks != self.last_callbacks {
			self.last_callbacks = callbacks;
			self.last_progress = now;
		}
		let host = cpal::default_host();
		let device = host.default_output_device();
		let id = device.as_ref().and_then(|device| device.id().ok());
		if device.is_some()
			&& self.stream.is_some()
			&& id == self.device_id
			&& !self.health.failed.load(Ordering::Relaxed)
			&& now.duration_since(self.last_progress) < Duration::from_secs(5)
		{
			return;
		}
		self.stream = None;
		self.device_id = id;
		self.ring = Arc::new(SpscRing::new());
		self.health = Arc::new(StreamHealth::default());
		self.resampler = None;
		self.converted.clear();
		let result = device
			.ok_or_else(|| "No default audio output device found".into())
			.and_then(|device| open_preferred(&device, &self.health));
		match result {
			Ok((stream, ring, rate)) => {
				self.ring = ring;
				self.resampler = (rate != AUDIO_SAMPLE_RATE)
					.then(|| OutputResampler::new(AUDIO_SAMPLE_RATE, rate));
				self.stream = Some(stream);
				self.last_progress = Instant::now();
				self.last_callbacks = 0;
				self.last_error = None;
				println!(
					"[AUDIO] Output connected at {rate} Hz; SID source at {AUDIO_SAMPLE_RATE} Hz"
				);
			}
			Err(error) => {
				let message = error.to_string();
				if self.last_error.as_ref() != Some(&message) {
					eprintln!("[AUDIO] Output unavailable; reconnecting automatically: {message}");
					self.last_error = Some(message);
				}
			}
		}
	}

	pub fn discard_pending(&mut self) {
		self.ring.discard_pending();
		if let Some(resampler) = self.resampler.as_mut() { resampler.reset(); }
		self.converted.clear();
	}

	pub fn push_samples(&mut self, samples: &[f32]) {
		if self.stream.is_none() {
			return;
		}
		if let Some(resampler) = &mut self.resampler {
			resampler.process(samples, &mut self.converted);
			self.ring.push_slice(&self.converted);
		} else {
			self.ring.push_slice(samples);
		}
		self.ring.take_overruns();
	}
	pub fn is_connected(&self) -> bool {
		self.stream.is_some()
	}
	pub fn get_sample_rate(&self) -> f32 {
		AUDIO_SAMPLE_RATE as f32
	}
}

/* Prefer 44.1 kHz in the endpoint's own format, then any advertised 44.1 kHz
 * format. Only exhausted preferred candidates permit the native default rate.
 * CPAL's WASAPI shared stream enables AUTOCONVERTPCM, so 44.1 kHz can also be
 * negotiated when the Windows mix engine itself runs at a different rate. */
fn open_preferred(
	device: &cpal::Device,
	health: &Arc<StreamHealth>,
) -> Result<(cpal::Stream, Arc<SpscRing>, u32)> {
	let default = device.default_output_config()?;
	let mut preferred = default.config();
	preferred.sample_rate = AUDIO_SAMPLE_RATE;
	let mut candidates = Vec::new();
	add_candidate(&mut candidates, preferred, default.sample_format(), *default.buffer_size());
	if let Ok(ranges) = device.supported_output_configs() {
		let mut ranges: Vec<_> = ranges
			.filter(|range| {
				range.min_sample_rate() <= AUDIO_SAMPLE_RATE
					&& range.max_sample_rate() >= AUDIO_SAMPLE_RATE
			})
			.collect();
		ranges.sort_by_key(|range| {
			(
				u8::from(range.sample_format() != default.sample_format()),
				range.channels().abs_diff(default.channels()),
			)
		});
		for range in ranges {
			let supported = range.with_sample_rate(AUDIO_SAMPLE_RATE);
			add_candidate(&mut candidates, supported.config(), supported.sample_format(), *supported.buffer_size());
		}
	}
	if default.sample_rate() != AUDIO_SAMPLE_RATE {
		add_candidate(&mut candidates, default.config(), default.sample_format(), *default.buffer_size());
	}
	let mut last_error: Box<dyn std::error::Error> = "No usable audio output format".into();
	for (config, format) in candidates {
		let rate = config.sample_rate;
		health.failed.store(false, Ordering::Relaxed);
		let ring = Arc::new(SpscRing::for_rate(rate));
		match open_stream(device, config, format, &ring, health) {
			Ok(stream) => return Ok((stream, ring, rate)),
			Err(error) => last_error = error,
		}
	}
	Err(last_error)
}

/* Request roughly 5.8 ms per callback, bounded by advertised device limits.
 * Failure falls back to that format's default buffer before trying another
 * format or rate; shortening the period never displaces the 44.1 kHz priority. */
fn add_candidate(candidates: &mut Vec<(cpal::StreamConfig, cpal::SampleFormat)>, config: cpal::StreamConfig, format: cpal::SampleFormat, sizes: SupportedBufferSize) {
	let frames = (u64::from(config.sample_rate) * 256).div_ceil(u64::from(AUDIO_SAMPLE_RATE)) as u32;
	let frames = match sizes {
		SupportedBufferSize::Range { min, max } => frames.clamp(min, max),
		SupportedBufferSize::Unknown => frames,
	};
	let mut bounded = config.clone();
	bounded.buffer_size = BufferSize::Fixed(frames);
	candidates.push((bounded, format));
	candidates.push((config, format));
}

fn open_stream(
	device: &cpal::Device,
	stream_config: cpal::StreamConfig,
	sample_format: cpal::SampleFormat,
	ring: &Arc<SpscRing>,
	health: &Arc<StreamHealth>,
) -> Result<cpal::Stream> {
	let stream = match sample_format {
		cpal::SampleFormat::I8 => build_output_stream::<i8>(device, stream_config, ring, health)?,
		cpal::SampleFormat::I16 => build_output_stream::<i16>(device, stream_config, ring, health)?,
		cpal::SampleFormat::I24 => {
			build_output_stream::<cpal::I24>(device, stream_config, ring, health)?
		}
		cpal::SampleFormat::I32 => build_output_stream::<i32>(device, stream_config, ring, health)?,
		cpal::SampleFormat::I64 => build_output_stream::<i64>(device, stream_config, ring, health)?,
		cpal::SampleFormat::U8 => build_output_stream::<u8>(device, stream_config, ring, health)?,
		cpal::SampleFormat::U16 => build_output_stream::<u16>(device, stream_config, ring, health)?,
		cpal::SampleFormat::U24 => {
			build_output_stream::<cpal::U24>(device, stream_config, ring, health)?
		}
		cpal::SampleFormat::U32 => build_output_stream::<u32>(device, stream_config, ring, health)?,
		cpal::SampleFormat::U64 => build_output_stream::<u64>(device, stream_config, ring, health)?,
		cpal::SampleFormat::F32 => build_output_stream::<f32>(device, stream_config, ring, health)?,
		cpal::SampleFormat::F64 => build_output_stream::<f64>(device, stream_config, ring, health)?,
		format => {
			return Err(format!("Unsupported native audio sample format: {}", format).into());
		}
	};

	stream.play()?;
	Ok(stream)
}

fn build_output_stream<T>(
	device: &cpal::Device,
	config: cpal::StreamConfig,
	ring: &Arc<SpscRing>,
	health: &Arc<StreamHealth>,
) -> Result<cpal::Stream>
where
	T: SizedSample + FromSample<f32>,
{
	let channels = config.channels as usize;
	let frame_samples = (f64::from(config.sample_rate) * CYCLES_PER_FRAME as f64 / CPU_FREQ_HZ).ceil() as usize;
	let ring = Arc::clone(ring);
	let mut priming = true;
	let progress = Arc::clone(health);
	let failure = Arc::clone(health);
	let stream = device.build_output_stream(
		config,
		move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
			progress.callbacks.fetch_add(1, Ordering::Relaxed);
			if ring.apply_discard() { priming = true; }
			/* Samples arrive in PAL-frame bursts. Prime one producer frame plus a
			 * device block so callback phase cannot exhaust a freshly started queue. */
			if priming {
				let available = ring.tail.load(Ordering::Acquire).wrapping_sub(ring.head.load(Ordering::Relaxed)) & ring.mask;
				let threshold = (frame_samples + data.len() / channels).min(usize::from(ring.mask));
				if usize::from(available) < threshold {
					data.fill(T::from_sample(0.0));
					return;
				}
				priming = false;
			}
			ring.write_output(data, channels);
		},
		move |error| {
			/* Xruns and scheduling warnings need no stream reconstruction. */
			if !matches!(
				error.kind(),
				cpal::ErrorKind::Xrun | cpal::ErrorKind::RealtimeDenied
			) {
				failure.failed.store(true, Ordering::Relaxed);
			}
		},
		None,
	)?;
	Ok(stream)
}