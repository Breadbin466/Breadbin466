// =======================================================
// src/fdd1541/thread.rs — 1541 execution ownership and control thread
// =======================================================

use super::constants::{DRIVE_BATCH_LIMIT, DRIVE_TIGHT_WINDOW, DRIVE_SKEW_CHECK_MASK, DRIVE_MAX_SKEW};
use std::hint::spin_loop;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use super::cable::IecCable;
use super::drive::Fdd1541;
use super::status::DriveStatus;

/* Requests are the complete control surface of the worker. Time-critical IEC exchange bypasses this channel and uses the lock-free cable queues instead. */
pub(super) enum Request {
	Status,
	Mount(PathBuf),
	Unmount,
	Reset,
	LoadCustomDosRom(PathBuf),
	ResetDosRom,
	TakeOwnership,
	SetPaused(bool),
	ReturnOwnership {
		drive: Box<Fdd1541>,
		emitted: u64,
		host_state: u32,
		published: u32,
	},
	Shutdown,
}

/* Every synchronous command returns both its result and a status snapshot taken in the same ownership context. */
pub(super) enum Response {
	Status(DriveStatus),
	Boolean(bool, DriveStatus),
	LoadResult(Result<(), String>, DriveStatus),
	Reset(u32, DriveStatus),
	Ownership(Box<Fdd1541>, u64, u32, u32),
}

/* DriveWorker owns the drive thread and exchanges only commands, status snapshots and packed IEC state with the host thread. The emulated 1541 itself never runs concurrently inside host-owned state. */
pub struct DriveWorker {
	pub(super) request_tx: Sender<Request>,
	pub(super) response_rx: Receiver<Response>,
	pub(super) cable: Arc<IecCable>,
	thread: Option<JoinHandle<()>>,
	pub(super) local_drive: Option<Box<Fdd1541>>,
	pub(super) last_host_state: u32,
	pub(super) device_view: u32,
	pub(super) connected: bool,
	tight: u32,
	previous_device_view: u32,
}

impl DriveWorker {
	/* Starting the worker transfers exclusive ownership of a fresh Fdd1541 to the drive thread; cable reads and output changes acquire host ownership at exact emulated boundaries. */
	pub fn new(host_clock_hz: u32) -> Self {
		let (request_tx, request_rx) = mpsc::channel();
		let (response_tx, response_rx) = mpsc::channel();
		let cable = Arc::new(IecCable::new());
		let worker_cable = Arc::clone(&cable);
		let thread = thread::Builder::new()
			.name(String::from("breadbin466-1541"))
			.spawn(move || worker_main(host_clock_hz, request_rx, response_tx, worker_cable))
			.expect("failed to start the 1541 worker");

		Self {
			request_tx,
			response_rx,
			cable,
			thread: Some(thread),
			local_drive: None,
			last_host_state: 0,
			device_view: 0,
			connected: true,
			tight: DRIVE_TIGHT_WINDOW,
			previous_device_view: 0,
		}
	}

	#[inline(always)]
	pub(super) fn publish_host_cycle(&self, cycle: u64) {
		if cycle > self.cable.host_cycle.0.load(Ordering::Relaxed) {
			self.cable.host_cycle.0.store(cycle, Ordering::Relaxed);
		}
	}

	#[inline(always)]
	pub(super) fn wait_for_drive(&mut self, minimum: u64) {
		let mut spins = 0u32;
		while self.cable.drive_cycle.0.load(Ordering::Relaxed) < minimum {
			if !self.cable.device_events.is_empty() {
				self.cable
					.device_events
					.drain_up_to(minimum, &mut self.device_view);
			}
			if spins < 16384 {
				spin_loop();
				spins += 1;
			} else {
				thread::yield_now();
			}
		}
	}

	#[inline(always)]
	/* Between cable accesses the independent drive may run on its worker.
	Host output changes acquire the preceding boundary before advancing the
	drive. Reads explicitly acquire that boundary too; a silent cable never
	permits a CPU-visible read to use an unfinished worker result. */
	pub fn tick_host(&mut self, cycle: u64, host_state: u32) -> u32 {
		let completed = cycle.wrapping_sub(1);
		if self.local_drive.is_some() && self.tight == 0 && host_state == self.last_host_state {
			self.return_ownership(completed);
		}
		if self.local_drive.is_none() && (self.tight > 0 || host_state != self.last_host_state) {
			self.take_ownership(completed);
		}
		if let Some(drive) = self.local_drive.as_mut() {
			self.previous_device_view = self.device_view;
			let state = if host_state != self.last_host_state {
				self.last_host_state = host_state;
				self.tight = DRIVE_TIGHT_WINDOW;
				drive.run_committed_host_cycle(host_state, self.connected)
			} else {
				drive.run_stable_host_cycle()
			};
			if state != self.device_view { self.tight = DRIVE_TIGHT_WINDOW; }
			else { self.tight = self.tight.saturating_sub(1); }
			self.device_view = state;
			return state;
		}
		if cycle & DRIVE_SKEW_CHECK_MASK == 0 {
			self.publish_host_cycle(completed);
			self.wait_for_drive(completed.saturating_sub(DRIVE_MAX_SKEW));
			self.cable.device_events.drain_up_to(completed, &mut self.device_view);
		}
		self.device_view
	}

	/* CIA input sampling precedes this cycle's drive step. Preserve that
	phase when a CPU or DMA read ends a parallel interval, then advance the
	drive once for the current cycle. The second state is published for the
	next motherboard cycle. Repeated reads in one cycle do not clock twice. */
	pub fn synchronise_input(&mut self, cycle: u64) -> (u32, u32) {
		if self.local_drive.is_none() {
			self.take_ownership(cycle.wrapping_sub(1));
			self.previous_device_view = self.device_view;
			self.device_view = self.local_drive.as_mut().expect("1541 ownership unavailable during cable read").run_stable_host_cycle();
		}
		self.tight = DRIVE_TIGHT_WINDOW;
		(self.previous_device_view, self.device_view)
	}

	/* Connection state is applied on the next committed host cycle so it becomes an electrical event, not an asynchronous mutation of the drive. */
	pub fn set_connected(&mut self, connected: bool) {
		self.connected = connected;
	}

	pub fn set_paused(&self, paused: bool) {
		self.send(Request::SetPaused(paused));
	}

	/* Status is read directly when the host owns the drive, otherwise it is sampled by the worker to avoid racing mutable emulation state. */
	pub fn status(&mut self) -> DriveStatus {
		if let Some(drive) = self.local_drive.as_mut() {
			return DriveStatus::from_drive(drive);
		}
		self.send(Request::Status);
		self.receive_status()
	}

	pub fn mount(&mut self, path: PathBuf) -> (bool, DriveStatus) {
		if let Some(drive) = self.local_drive.as_mut() {
			let mounted = drive.mount(&path);
			return (mounted, DriveStatus::from_drive(drive));
		}
		self.send(Request::Mount(path));
		self.receive_boolean()
	}

	pub fn unmount(&mut self) -> (bool, DriveStatus) {
		if let Some(drive) = self.local_drive.as_mut() {
			let unmounted = drive.unmount();
			return (unmounted, DriveStatus::from_drive(drive));
		}
		self.send(Request::Unmount);
		self.receive_boolean()
	}

	/* Explicit host shutdown persistence temporarily acquires the drive only when necessary, so a failed final flush can be reported before worker destruction while preserving the normal ownership model. */
	pub fn flush_media(&mut self) -> bool {
		let already_owned = self.local_drive.is_some();
		let completed_cycle = self.cable.host_cycle.0.load(Ordering::Relaxed);
		if !already_owned {
			self.take_ownership(completed_cycle);
		}
		let flushed = self
			.local_drive
			.as_mut()
			.expect("1541 ownership unavailable during explicit flush")
			.flush_now();
		if !already_owned {
			self.return_ownership(completed_cycle);
		}
		flushed
	}

	/* Hard reset acquires the drive, commits pending media writes and re-bases both cable cycle counters around the host's current IEC state. */
	pub fn hard_reset(&mut self, completed_cycle: u64, host_state: u32) -> DriveStatus {
		self.take_ownership(completed_cycle);
		let drive = self
			.local_drive
			.as_mut()
			.expect("1541 ownership unavailable during hard reset");
		let _ = drive.flush_now();
		drive.reset_with_iec_state(host_state, self.connected);
		self.last_host_state = host_state;
		self.device_view = drive.device_iec_state();
		self.previous_device_view = self.device_view;
		self.tight = DRIVE_TIGHT_WINDOW;
		self.cable.device_events.clear();
		self.cable.host_cycle.0.store(0, Ordering::Relaxed);
		self.cable.drive_cycle.0.store(0, Ordering::Relaxed);
		DriveStatus::from_drive(drive)
	}

	pub fn reset(&mut self) -> DriveStatus {
		self.tight = DRIVE_TIGHT_WINDOW;
		if let Some(drive) = self.local_drive.as_mut() {
			let _ = drive.flush_now();
			drive.reset();
			self.device_view = drive.device_iec_state();
		self.previous_device_view = self.device_view;
		self.tight = DRIVE_TIGHT_WINDOW;
			self.cable.device_events.clear();
			return DriveStatus::from_drive(drive);
		}
		self.send(Request::Reset);
		match self.receive() {
			Response::Reset(state, status) => {
				self.device_view = state;
				self.cable.device_events.clear();
				status
			}
			_ => panic!("1541 worker returned an unexpected reset response"),
		}
	}

	pub fn load_custom_dos_rom(&mut self, path: PathBuf) -> (Result<(), String>, DriveStatus) {
		if let Some(drive) = self.local_drive.as_mut() {
			let result = drive
				.load_custom_dos_rom(&path)
				.map_err(|error| error.to_string());
			return (result, DriveStatus::from_drive(drive));
		}
		self.send(Request::LoadCustomDosRom(path));
		match self.receive() {
			Response::LoadResult(result, status) => (result, status),
			_ => panic!("1541 worker returned an unexpected response to LoadCustomDosRom"),
		}
	}

	pub fn reset_dos_rom(&mut self) -> DriveStatus {
		if let Some(drive) = self.local_drive.as_mut() {
			drive.reset_dos_rom();
			return DriveStatus::from_drive(drive);
		}
		self.send(Request::ResetDosRom);
		self.receive_status()
	}

	pub(super) fn send(&self, request: Request) {
		self.request_tx
			.send(request)
			.expect("1541 worker request channel disconnected");
	}

	pub(super) fn receive(&self) -> Response {
		self.response_rx
			.recv()
			.expect("1541 worker response channel disconnected")
	}

	fn receive_status(&self) -> DriveStatus {
		match self.receive() {
			Response::Status(status) => status,
			_ => panic!("1541 worker returned an unexpected status response"),
		}
	}

	fn receive_boolean(&self) -> (bool, DriveStatus) {
		match self.receive() {
			Response::Boolean(value, status) => (value, status),
			_ => panic!("1541 worker returned an unexpected boolean response"),
		}
	}
}

/* Shutdown returns any locally owned drive before joining the worker, ensuring media persistence and ownership have one final serialised path. */
impl Drop for DriveWorker {
	fn drop(&mut self) {
		if self.local_drive.is_some() {
			let completed_cycle = self.cable.host_cycle.0.load(Ordering::Relaxed);
			self.return_ownership(completed_cycle);
		}
		self.cable.stopping.store(true, Ordering::Release);
		let _ = self.request_tx.send(Request::Shutdown);
		if let Some(thread) = self.thread.take() {
			let _ = thread.join();
		}
	}
}

/* The worker serialises reset, media and ROM commands with drive execution, preventing host file operations from racing the 1541 CPU. */
fn worker_main(
	host_clock_hz: u32,
	request_rx: Receiver<Request>,
	response_tx: Sender<Response>,
	cable: Arc<IecCable>,
) {
	let mut drive = Some(Box::new(Fdd1541::new(host_clock_hz)));
	let mut emitted = 0u64;
	let mut host_state = 0u32;
	let mut published = 0u32;
	let mut idle_polls = 0u32;
	let mut paused = false;

	loop {
		let request = if paused {
			match request_rx.recv_timeout(Duration::from_millis(100)) {
				Ok(request) => Ok(request),
				Err(mpsc::RecvTimeoutError::Timeout) => continue,
				Err(mpsc::RecvTimeoutError::Disconnected) => break,
			}
		} else {
			request_rx.try_recv()
		};

		match request {
			Ok(request) => {
				let response = match request {
					Request::Status => {
						let active_drive =
							drive.as_mut().expect("1541 worker does not own the drive");
						Some(Response::Status(DriveStatus::from_drive(active_drive)))
					}
					Request::Mount(path) => {
						let active_drive =
							drive.as_mut().expect("1541 worker does not own the drive");
						let mounted = active_drive.mount(&path);
						Some(Response::Boolean(
							mounted,
							DriveStatus::from_drive(active_drive),
						))
					}
					Request::Unmount => {
						let active_drive =
							drive.as_mut().expect("1541 worker does not own the drive");
						let unmounted = active_drive.unmount();
						Some(Response::Boolean(
							unmounted,
							DriveStatus::from_drive(active_drive),
						))
					}
					Request::Reset => {
						let active_drive =
							drive.as_mut().expect("1541 worker does not own the drive");
						let _ = active_drive.flush_now();
						active_drive.reset();
						cable.device_events.clear();
						emitted = cable.host_cycle.0.load(Ordering::Relaxed);
						published = active_drive.device_iec_state();
						cable.drive_cycle.0.store(emitted, Ordering::Relaxed);
						Some(Response::Reset(
							published,
							DriveStatus::from_drive(active_drive),
						))
					}
					Request::LoadCustomDosRom(path) => {
						let active_drive =
							drive.as_mut().expect("1541 worker does not own the drive");
						let result = active_drive
							.load_custom_dos_rom(&path)
							.map_err(|error| error.to_string());
						Some(Response::LoadResult(
							result,
							DriveStatus::from_drive(active_drive),
						))
					}
					Request::ResetDosRom => {
						let active_drive =
							drive.as_mut().expect("1541 worker does not own the drive");
						active_drive.reset_dos_rom();
						Some(Response::Status(DriveStatus::from_drive(active_drive)))
					}
					Request::SetPaused(value) => {
						paused = value;
						None
					}
					Request::TakeOwnership => {
						let owned_drive = drive.take().expect("1541 worker does not own the drive");
						Some(Response::Ownership(
							owned_drive,
							emitted,
							host_state,
							published,
						))
					}
					Request::ReturnOwnership {
						drive: returned_drive,
						emitted: returned_emitted,
						host_state: returned_host_state,
						published: returned_published,
					} => {
						drive = Some(returned_drive);
						emitted = returned_emitted;
						host_state = returned_host_state;
						published = returned_published;
						cable.device_events.clear();
						cable.drive_cycle.0.store(emitted, Ordering::Relaxed);
						None
					}
					Request::Shutdown => {
						if let Some(active_drive) = drive.as_mut() {
							let _ = active_drive.flush_now();
						}
						break;
					}
				};

				if let Some(response) = response {
					if response_tx.send(response).is_err() {
						break;
					}
				}
				continue;
			}
			Err(TryRecvError::Disconnected) => break,
			Err(TryRecvError::Empty) => {}
		}

		let Some(active_drive) = drive.as_mut() else {
			thread::park_timeout(Duration::from_micros(100));
			continue;
		};

		let target = cable.host_cycle.0.load(Ordering::Relaxed);
		if emitted > target {
			emitted = target;
			cable.drive_cycle.0.store(emitted, Ordering::Relaxed);
			continue;
		}
		if emitted == target {
			idle_polls = idle_polls.saturating_add(1);
			if idle_polls < 4096 {
				spin_loop();
			} else if idle_polls < 65536 {
				thread::yield_now();
			} else {
				thread::sleep(Duration::from_micros(100));
			}
			continue;
		}
		idle_polls = 0;

		let batch_end = target.min(emitted.wrapping_add(DRIVE_BATCH_LIMIT));
		while emitted < batch_end {
			emitted = emitted.wrapping_add(1);
			let device_state = active_drive.run_stable_host_cycle();
			if device_state != published {
				if !cable
					.device_events
					.push(emitted, device_state, &cable.stopping)
				{
					break;
				}
				published = device_state;
			}
		}
		cable.drive_cycle.0.store(emitted, Ordering::Relaxed);
	}
}