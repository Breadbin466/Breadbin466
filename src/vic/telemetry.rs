// =======================================================
// src/vic/telemetry.rs — Telemetry
// =======================================================

use std::{collections::VecDeque, time::Instant};

const RATE_SAMPLE_COUNT: usize = 128;

/* VicTelemetry is deliberately outside the emulated chip state. It samples completed frames and host time for diagnostics, but none of its counters feed back into raster timing, IRQ generation or memory arbitration. */
pub struct VicTelemetry {
	/* Total completed frames since construction. */
	pub frame_count: u64,
	/* Host-observed frame rate from the most recent reporting window. */
	pub last_fps: f64,
	/* Effective emulated master-clock throughput over that same window. */
	pub current_mhz: f64,
	/* Frames accumulated since the previous host-time sample. */
	frames_since_report: u64,
	/* Actual frame-start timestamps measure pacing independently of the
	 * varying cost of completing each frame. Median rates over overlapping
	 * half-windows reject isolated host stalls but follow sustained changes. */
	rate_samples: VecDeque<(Instant, u64)>,
	frame_started: Option<Instant>,

	/* Raster IRQ edges accumulated for diagnostic output during the interval. */
	pub irq_edge_count: u64,
}

impl VicTelemetry {
	/* The OSD waits for a completed measurement interval before displaying rates. */
	pub fn new() -> Self {
		Self {
			frame_count: 0,
			last_fps: 0.0,
			current_mhz: 0.0,
			frames_since_report: 0,
			rate_samples: VecDeque::with_capacity(RATE_SAMPLE_COUNT),
			frame_started: None,
			irq_edge_count: 0,
		}
	}

	pub fn begin_frame(&mut self) {
		self.frame_started = Some(Instant::now());
	}

	/* Pauses and execution-mode changes begin a new measurement interval;
	 * a stopped clock must not dilute the rate reported after resuming. */
	pub fn restart_rate_window(&mut self) {
		self.rate_samples.clear();
		self.frame_started = None;
		self.frames_since_report = 0;
		self.irq_edge_count = 0;
	}

	/* Reporting is amortised over fifty frames. Host elapsed time yields FPS and effective master-clock throughput, while the sampled machine values are diagnostic context only and cannot alter emulation. */
	pub fn update_report(
		&mut self,
		ctrl1: u8,
		_sprite_en: u8,
		current_pc: u16,
		master_cycles: u64,
		drive_pc: Option<u16>,
	) {
		let sample_time = self.frame_started.take().unwrap_or_else(Instant::now);
		if self
			.rate_samples
			.back()
			.is_some_and(|&(_, cycles)| master_cycles < cycles)
		{
			self.restart_rate_window();
		}
		if self.rate_samples.len() == RATE_SAMPLE_COUNT {
			self.rate_samples.pop_front();
		}
		self.rate_samples.push_back((sample_time, master_cycles));
		self.frame_count += 1;
		self.frames_since_report += 1;

		if self.frames_since_report >= 50 {
			let lag = self.rate_samples.len() / 2;
			let mut fps = [0.0; RATE_SAMPLE_COUNT / 2];
			let mut mhz = [0.0; RATE_SAMPLE_COUNT / 2];
			let mut count = 0;
			for index in 0..lag {
				let (start, start_cycles) = self.rate_samples[index];
				let (end, end_cycles) = self.rate_samples[index + lag];
				let elapsed = end.duration_since(start).as_secs_f64();
				if elapsed > 0.0 {
					fps[count] = lag as f64 / elapsed;
					mhz[count] = (end_cycles - start_cycles) as f64 / elapsed / 1_000_000.0;
					count += 1;
				}
			}
			if count > 0 {
				self.last_fps = median(&mut fps[..count]);
				self.current_mhz = median(&mut mhz[..count]);
			}

			let den = (ctrl1 & 0x10) != 0;

			match drive_pc {
				Some(dpc) => {
					println!(
						"[TEL] FPS:{:.3} | {:.3} MHz | C64_PC:${:04X} | DRIVE_PC:${:04X} | IRQs:{} | DEN:{}",
						self.last_fps,
						self.current_mhz,
						current_pc,
						dpc,
						self.irq_edge_count,
						if den { "ON" } else { "OFF" }
					);
				}
				None => {
					println!(
						"[TEL] FPS:{:.3} | {:.3} MHz | C64_PC:${:04X} | IRQs:{} | DEN:{}",
						self.last_fps,
						self.current_mhz,
						current_pc,
						self.irq_edge_count,
						if den { "ON" } else { "OFF" }
					);
				}
			}

			self.frames_since_report = 0;
			self.irq_edge_count = 0;
		}
	}
}

fn median(values: &mut [f64]) -> f64 {
	values.sort_unstable_by(f64::total_cmp);
	let middle = values.len() / 2;
	if values.len() % 2 == 0 {
		(values[middle - 1] + values[middle]) * 0.5
	} else {
		values[middle]
	}
}