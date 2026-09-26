// =======================================================
// src/ui/presentation.rs — Bounded frame exchange and GPU worker
// =======================================================

use super::{graphics::GpuRenderer, osd::OsdData};
use crate::emulator::Result;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};

struct Frame {
	pixels: Vec<u8>,
	osd: OsdData,
	osd_enabled: bool,
}

#[derive(Default)]
struct State {
	next: Option<Frame>,
	spare: Vec<u8>,
	resize: Option<(u32, u32)>,
	stop: bool,
	error: Option<String>,
}

/* Only the latest completed frame is retained. No GPU operation runs under
 * the mailbox lock, so presentation cannot hold up the emulation thread.
 * The FIFO surface repeats its last displayed image when no new image is
 * submitted. The worker therefore wakes only for a publication or resize;
 * submitting duplicates would queue stale images ahead of fresh PAL frames. */
pub(super) struct Presentation {
	shared: Arc<(Mutex<State>, Condvar)>,
	worker: Option<JoinHandle<()>>,
}

impl Presentation {
	pub fn new(mut gpu: GpuRenderer) -> std::io::Result<Self> {
		let shared = Arc::new((Mutex::new(State::default()), Condvar::new()));
		let worker_shared = shared.clone();
		let worker = thread::Builder::new()
			.name("presentation".into())
			.spawn(move || {
				let (lock, wake) = &*worker_shared;
				let mut current: Option<Frame> = None;
				loop {
					let mut state = lock.lock().unwrap();
					while !state.stop
						&& state.next.is_none()
						&& state.resize.is_none()
					{
						state = wake.wait(state).unwrap();
					}
					if state.stop {
						break;
					}
					let resize = state.resize.take();
					drop(state);
					if let Some((width, height)) = resize {
						gpu.handle_resize(width, height);
					}
					match gpu.prepare_frame() {
						Ok(true) => {}
						Ok(false) => {
							let mut state = lock.lock().unwrap();
							/* Consume an unavailable surface's pending image before
							 * sleeping; the next publication wakes a fresh attempt. */
							if let Some(frame) = state.next.take() {
								if let Some(previous) = current.replace(frame) {
									state.spare = previous.pixels;
								}
							}
							while !state.stop && state.next.is_none() && state.resize.is_none() {
								state = wake.wait(state).unwrap();
							}
							continue;
						}
						Err(error) => {
							lock.lock().unwrap().error = Some(error.to_string());
							break;
						}
					}
					let mut state = lock.lock().unwrap();
					if state.stop {
						break;
					}
					if state.resize.is_some() {
						continue;
					}
					if let Some(frame) = state.next.take() {
						if let Some(previous) = current.replace(frame) {
							state.spare = previous.pixels;
						}
					}
					drop(state);
					if let Some(frame) = current.as_ref() {
						gpu.set_osd_enabled(frame.osd_enabled);
						if let Err(error) = gpu.draw(&frame.pixels, &frame.osd) {
							lock.lock().unwrap().error = Some(error.to_string());
							break;
						}
					}
				}
			})?;
		Ok(Self {
			shared,
			worker: Some(worker),
		})
	}

	pub fn publish(&self, pixels: &[u8], osd: OsdData, osd_enabled: bool) -> Result<()> {
		/* Reuse retired frame storage, but copy outside the mailbox lock so the
		 * GPU worker remains free to acquire the latest complete publication. */
		let mut buffer = {
			let mut state = self.shared.0.lock().unwrap();
			if let Some(error) = state.error.as_ref() {
				return Err(error.clone().into());
			}
			std::mem::take(&mut state.spare)
		};
		buffer.resize(pixels.len(), 0);
		buffer.copy_from_slice(pixels);
		let frame = Frame {
			pixels: buffer,
			osd,
			osd_enabled,
		};
		let mut state = self.shared.0.lock().unwrap();
		if let Some(error) = state.error.as_ref() {
			return Err(error.clone().into());
		}
		if let Some(previous) = state.next.replace(frame) {
			state.spare = previous.pixels;
		}
		self.shared.1.notify_one();
		Ok(())
	}

	pub fn resize(&self, width: u32, height: u32) {
		self.shared.0.lock().unwrap().resize = Some((width, height));
		self.shared.1.notify_one();
	}
}

impl Drop for Presentation {
	fn drop(&mut self) {
		self.shared.0.lock().unwrap().stop = true;
		self.shared.1.notify_one();
		if let Some(worker) = self.worker.take() {
			let _ = worker.join();
		}
	}
}