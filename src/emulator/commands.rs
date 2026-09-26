// =======================================================
// src/emulator/commands.rs — Emulator menu command handling
// =======================================================

use super::orchestrator::Orchestrator;
use crate::ui::menu_actions::MenuHandler;

/* Menu handling keeps commands that affect orchestrator-owned state local, then delegates ordinary machine, media and view actions to the shared MenuHandler. */
pub(super) fn handle_menu_event(orchestrator: &mut Orchestrator, id: &str) {
	let ids = orchestrator.context.menu.ids.clone();
	/* Input and execution-state commands need direct access to orchestrator timing or host devices. */
	if id == ids.cycle_joystick {
		if let Some(joystick) = orchestrator.context.joystick.as_mut() {
			joystick.cycle();
		}
	} else if id == ids.mouse_1351 {
		let connected = !orchestrator.context.machine.mouse_1351_connected();
		orchestrator.set_mouse_1351_connected(connected);
	} else if id == ids.pause {
		orchestrator.paused = !orchestrator.paused;
		orchestrator.context.machine.set_paused(orchestrator.paused);
		orchestrator.resynchronise_host();
		orchestrator
			.context
			.menu
			.set_checked(&ids.pause, orchestrator.paused);
	/* Cartridge buttons are hardware events: they operate at the current machine cycle and may request a reset after changing cartridge mapping. */
	} else if id == ids.cartridge_reset {
		if orchestrator.context.machine.memory.cartridge.is_present() {
			orchestrator.context.machine.soft_reset();
			orchestrator.context.input.clear_all();
			orchestrator.context.actions.reset_state();
		}
	} else if id == ids.cartridge_freeze {
		let cycle = orchestrator.context.machine.current_cycle();
		let machine = &mut orchestrator.context.machine;
		machine.memory.cartridge.trigger_freeze_button(cycle);
		machine.memory.mark_memory_map_dirty();
	} else if id == ids.cartridge_menu {
		let cycle = orchestrator.context.machine.current_cycle();
		let reset_requested = orchestrator
			.context
			.machine
			.memory
			.cartridge
			.trigger_menu_button(cycle);
		orchestrator.context.machine.memory.mark_memory_map_dirty();
		if reset_requested {
			orchestrator.context.machine.soft_reset();
			orchestrator.context.input.clear_all();
			orchestrator.context.actions.reset_state();
		}
	/* Host presentation controls remain outside MenuHandler because they modify orchestrator-owned audio or window state. */
	} else if id == ids.debug_mute_warp {
		orchestrator.mute_sid_warp = !orchestrator.mute_sid_warp;
		if let Some(audio) = orchestrator.context.audio.as_mut() { audio.discard_pending(); }
		orchestrator
			.context
			.menu
			.set_checked(&ids.debug_mute_warp, orchestrator.mute_sid_warp);
	} else if id == ids.inspector_window {
		if orchestrator.inspector.is_some() {
			orchestrator.close_inspector();
		} else {
			orchestrator.inspector_requested = true;
		}
	} else {
		/* All remaining identifiers are stable logical actions shared by native menu backends and keyboard shortcuts. */
		MenuHandler::dispatch(
			id,
			&mut orchestrator.context,
			&mut orchestrator.timing,
			&mut orchestrator.current_scale_factor,
		);
	}
}