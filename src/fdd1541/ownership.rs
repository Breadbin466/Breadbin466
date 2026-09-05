// =======================================================
// src/fdd1541/ownership.rs — Temporary 1541 ownership transfer
// =======================================================

use std::sync::atomic::Ordering;

use super::thread::DriveWorker;
use super::thread::{Request, Response};

impl DriveWorker {
	/* Tight IEC activity temporarily moves the complete drive object to the host thread. The transfer is accepted only at the exact completed cycle so neither thread can execute the same emulated interval. */
	pub(super) fn take_ownership(&mut self, completed_cycle: u64) {
		if self.local_drive.is_some() {
			return;
		}
		self.publish_host_cycle(completed_cycle);
		self.wait_for_drive(completed_cycle);
		self.send(Request::TakeOwnership);
		match self.receive() {
			Response::Ownership(drive, emitted, host_state, published) => {
				assert_eq!(
					emitted, completed_cycle,
					"1541 ownership transferred at the wrong cycle"
				);
				self.local_drive = Some(drive);
				self.last_host_state = host_state;
				self.device_view = published;
				self.cable.device_events.clear();
			}
			_ => panic!("1541 worker returned an unexpected ownership response"),
		}
	}

	/* Returning ownership publishes the host and device views together with the drive, allowing the worker to resume from the same electrical and temporal boundary. */
	pub(super) fn return_ownership(&mut self, completed_cycle: u64) {
		let Some(drive) = self.local_drive.take() else {
			return;
		};
		self.cable
			.host_cycle
			.0
			.store(completed_cycle, Ordering::Relaxed);
		self.cable
			.drive_cycle
			.0
			.store(completed_cycle, Ordering::Relaxed);
		self.send(Request::ReturnOwnership {
			drive,
			emitted: completed_cycle,
			host_state: self.last_host_state,
			published: self.device_view,
		});
	}
}