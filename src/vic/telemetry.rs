// =======================================================
// src/vic/telemetry.rs — Telemetry
// =======================================================

use std::time::Instant;

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
	/* Host timestamp anchoring the current reporting interval. */
	last_report: Instant,
	/* Master-cycle counter sampled at the previous report. */
	last_master_cycles: u64,

	/* Raster IRQ edges accumulated for diagnostic output during the interval. */
	pub irq_edge_count: u64,
}

impl VicTelemetry {
	/* Construction seeds display values near PAL nominal rates so the UI has meaningful telemetry before the first reporting interval completes. */
	pub fn new() -> Self {
		Self {
			frame_count: 0,
			last_fps: 50.125,
			current_mhz: 0.985,
			frames_since_report: 0,
			last_report: Instant::now(),
			last_master_cycles: 0,
			irq_edge_count: 0,
		}
	}

	/* Reporting is amortised over fifty frames. Host elapsed time yields FPS and effective master-clock throughput, while the sampled machine values are diagnostic context only and cannot alter emulation. */
	pub fn update_report(&mut self, ctrl1: u8, _sprite_en: u8, current_pc: u16, master_cycles: u64, drive_pc: Option<u16>) {
		self.frame_count += 1;
		self.frames_since_report += 1;

		/* A full PAL-second-sized window smooths scheduler jitter without retaining an unbounded history. */
		if self.frames_since_report >= 50 {
			let now = Instant::now();
			let elapsed = now.duration_since(self.last_report);

			self.last_fps = 50.0 / elapsed.as_secs_f64();

			let delta_cycles = master_cycles.saturating_sub(self.last_master_cycles);
			self.current_mhz = (delta_cycles as f64 / elapsed.as_secs_f64()) / 1_000_000.0;

			self.last_master_cycles = master_cycles;

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
			self.last_report = now;
		}
	}
}