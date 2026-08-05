// =======================================================
// src/fdd1541/status.rs — Observable 1541 status snapshot
// =======================================================

use super::drive::Fdd1541;

/* DriveStatus is a detached UI snapshot. Reading it may service pending persistence through the drive accessors, but the snapshot itself contains no live emulation state. */
#[derive(Clone, Default)]
pub struct DriveStatus {
	pub power_led: bool,
	pub busy_led: bool,
	pub dirty: bool,
	pub current_track: Option<u8>,
	pub drive_pc: u16,
	pub error_string: String,
}

impl DriveStatus {
	/* All observable fields are sampled from one owned drive instance so LEDs, track, PC and dirty state describe the same instant. */
	pub fn from_drive(drive: &mut Fdd1541) -> Self {
		Self {
			power_led: drive.power_led(),
			busy_led: drive.busy_led(),
			dirty: drive.is_dirty(),
			current_track: drive.current_track(),
			drive_pc: drive.drive_pc(),
			error_string: drive.get_error_string(),
		}
	}
}