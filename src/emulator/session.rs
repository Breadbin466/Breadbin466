// =======================================================
// src/emulator/session.rs — Startup media restoration
// =======================================================

use super::orchestrator::Orchestrator;

/* Startup restoration is deliberately subordinate to explicit command-line media. Persisted paths are retried only when the user did not select a replacement, and stale disk history is cleared after a failed mount so subsequent launches do not repeat a known-bad request. */

pub(super) fn restore_media(driver: &mut Orchestrator, cli_has_disk: bool, cli_has_tap: bool) {
	if !cli_has_disk {
		if let Some(path) = driver.context.history.active_d64_g64.clone() {
			if !driver.context.machine.mount_drive(&path) {
				driver.context.history.active_d64_g64 = None;
				driver.context.history.clear_last_disk();
				driver.context.history.save_forced();
			}
		}
	}
	if !cli_has_tap {
		if let Some(path) = driver.context.history.active_tap.clone() {
			let restored = std::fs::read(&path)
				.ok()
				.is_some_and(|data| driver.context.datassette.load_tap(data, path));
			if !restored {
				eprintln!("[TAPE] Failed to restore TAP image");
				driver.context.history.active_tap = None;
				driver.context.history.save_forced();
			}
		}
	}
}