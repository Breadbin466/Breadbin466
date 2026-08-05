// =======================================================
// src/emulator/orchestrator.rs — System Master Orchestrator loop
// =======================================================

use std::time::{Instant, Duration};
use winit::event_loop::ActiveEventLoop;
use winit::event::ElementState;
use winit::keyboard::KeyCode;
use winit::window::WindowId;

use super::Result;
use crate::emulator::command_line::{CommandLine, StartupAction};
use super::context::AppContext;
use super::builder::SystemBuilder;
use super::timing::TimeKeeper;
use crate::ui::routing::InputRouter;
use crate::ui::OsdMonitor;
use crate::ui::InspectorWindow;
use crate::ui::constants::INITIAL_WINDOW_SCALE;
use crate::clockchip::constants::CYCLES_PER_FRAME;
use super::constants::STARTUP_COMMAND_DELAY_FRAMES;

/* Orchestrator is the sole coordinator between host-time services and the mutable machine. It owns AppContext, converts host events into deterministic machine actions, advances whole emulated frames, and publishes video, audio and inspector snapshots without exposing direct motherboard access to the event loop. */
pub struct Orchestrator {
	pub context:          AppContext,   pub osd:              OsdMonitor,
	pub(super) timing:          TimeKeeper,
	pub(super) current_scale_factor: f64,
	pub mute_sid_warp:    bool,
	pub(super) osd_cursor_pos:       (f64, f64),
	pub(super) cursor_hidden:         bool,
	pub(super) frame_counter:        u64,
	pub inspector:            Option<InspectorWindow>,
	pub inspector_requested:  bool,
	pub is_dark_mode:         bool,
	pub(super) paused:                   bool,
}

impl Orchestrator {
	/* Construction applies startup configuration in dependency order: build host resources, restore persisted media, apply explicit command-line overrides, schedule deferred machine actions, then synchronise menu state with the resulting runtime state. */
	pub fn new(application: &ActiveEventLoop, command_line: &CommandLine) -> Result<Self> {
		let mut context = SystemBuilder::build(application, command_line)?;

		let cli_has_d64_g64 = command_line.disk.is_some() || command_line.drive == Some(crate::emulator::command_line::DriveSelection::Off);
		let cli_has_tap = command_line.tape.is_some();

		if let Some(joystick) = context.joystick.as_mut() {
			joystick.configure(command_line.joystick, command_line.joystick_port)?;
		} else if command_line.joystick == crate::emulator::command_line::JoystickSelection::Gilrs {
			return Err("GILRS joystick support is unavailable.".into());
		}

		let is_dark_mode = context.window.theme() == Some(winit::window::Theme::Dark);

		let mut driver = Self {
			context,
			osd:                   OsdMonitor::new(),
			timing:           TimeKeeper::new(),
			current_scale_factor:  command_line.scale.map(f64::from).unwrap_or(INITIAL_WINDOW_SCALE),
			mute_sid_warp:         command_line.mute_warp.unwrap_or(true),
			osd_cursor_pos:        (-1.0, -1.0),
			cursor_hidden:          false,
			frame_counter:         0,
			inspector:             None,
			inspector_requested:   command_line.inspector,
			is_dark_mode,
			paused:                  false,
		};

		super::session::restore_media(&mut driver, cli_has_d64_g64, cli_has_tap);

		if let Some(path) = command_line.tape.clone() {
			driver.context.mount_tape(path);
		}
		if let Some(path) = command_line.prg.clone() {
			driver.context.open_prg(path, true);
		}
		if command_line.warp {
			driver.timing.warp_mode = true;
		}
		if command_line.warp_1541 {
			driver.timing.warp_1541 = true;
		}
		match command_line.startup_action {
			StartupAction::None => {}
			StartupAction::LoadDirectory => driver.context.actions.schedule_startup_text_entry("LOAD\"$\",8\r", STARTUP_COMMAND_DELAY_FRAMES),          StartupAction::LoadFirst => driver.context.actions.schedule_startup_text_entry("LOAD\"*\",8,1\r", STARTUP_COMMAND_DELAY_FRAMES),
			StartupAction::LoadFirstRun => driver.context.actions.schedule_startup_load_first_run(STARTUP_COMMAND_DELAY_FRAMES),
		}
		if command_line.freeze {
			let cycle = driver.context.machine.current_cycle();
			driver.context.machine.memory.cartridge.trigger_freeze_button(cycle);
			driver.context.machine.memory.mark_memory_map_dirty();
		}
		if command_line.cartridge_menu {
			let cycle = driver.context.machine.current_cycle();
			let reset_requested = driver.context.machine.memory.cartridge.trigger_menu_button(cycle);
			driver.context.machine.memory.mark_memory_map_dirty();
			if reset_requested { driver.context.machine.soft_reset(); }
		}
		if command_line.fullscreen {
			#[cfg(target_os = "linux")]
			crate::ui::shell::Shell::set_fullscreen(true);
			#[cfg(not(target_os = "linux"))]
			driver.context.window.set_fullscreen(Some(winit::window::Fullscreen::Borderless(None)));
		}
		driver.context.menu.set_checked(&driver.context.menu.ids.warp_mode, driver.timing.warp_mode);
		driver.context.menu.set_checked(&driver.context.menu.ids.warp_1541, driver.timing.warp_1541);
		driver.context.menu.set_checked(&driver.context.menu.ids.debug_mute_warp, driver.mute_sid_warp);
		driver.context.menu.set_checked(&driver.context.menu.ids.debug_c128_2mhz, command_line.mode_8502);
		driver.context.menu.set_checked(&driver.context.menu.ids.debug_reu_1764, command_line.reu);

		Ok(driver)
	}

	pub fn handle_cursor_moved(&mut self, x: f64, y: f64) {
		self.osd_cursor_pos = (x, y);
		self.update_cursor_visibility();

		if self.context.renderer.osd_enabled {
			self.context.window.request_redraw();
		}
	}

	pub fn handle_cursor_left(&mut self) {
		self.osd_cursor_pos = (-1.0, -1.0);
		self.set_cursor_hidden(false);

		if self.context.renderer.osd_enabled {
			self.context.window.request_redraw();
		}
	}

	fn set_cursor_hidden(&mut self, hidden: bool) {
		if self.cursor_hidden == hidden {
			return;
		}

		self.context.window.set_cursor_visible(!hidden);

		#[cfg(target_os = "macos")]
		{
			use objc2_app_kit::NSCursor;

			if hidden {
				NSCursor::hide();
			} else {
				NSCursor::unhide();
			}
		}

		self.cursor_hidden = hidden;
	}

	pub fn update_cursor_visibility(&mut self) {
		let (_, y) = self.osd_cursor_pos;
		let window_height = self.context.window.inner_size().height as f64;

		if y < 0.0 || window_height <= 0.0 {
			self.set_cursor_hidden(false);
			return;
		}

		let emulated_height = if self.context.renderer.osd_enabled {
			(crate::ui::renderer::CRT_HEIGHT + crate::ui::renderer::GUI_HEIGHT) as f64
		} else {
			crate::ui::renderer::CRT_HEIGHT as f64
		};
		let display_bottom = window_height
			* crate::ui::renderer::CRT_HEIGHT as f64
			/ emulated_height;

		self.set_cursor_hidden(y < display_bottom);
	}

	pub fn resume_from_pause(&mut self) -> bool {
		if !self.paused {
			return false;
		}

		self.paused = false;
		self.context.machine.set_paused(false);
		self.timing.resynchronise();
		self.context.input.clear_all();
		let pause_id = self.context.menu.ids.pause.clone();
		self.context.menu.set_checked(&pause_id, false);
		true
	}

	pub fn handle_mouse_click(&mut self) {
		InputRouter::handle_mouse_click(self.osd_cursor_pos, &mut self.context);
	}

	pub fn open_inspector(&mut self, application: &ActiveEventLoop) {
		if self.inspector.is_some() { return; }
		let instance = self.context.renderer.gpu_instance();
		let adapter  = self.context.renderer.gpu_adapter().clone();
		let device   = self.context.renderer.gpu_device();
		let queue    = self.context.renderer.gpu_queue();
		match InspectorWindow::new(application, instance, &adapter, device, queue, wgpu::TextureFormat::Bgra8Unorm) {
			Ok(win) => self.inspector = Some(win),
			Err(e) => println!("Inspector window error: {}", e),
		}
	}

	pub fn close_inspector(&mut self) {
		self.inspector = None;
	}

	pub fn is_inspector_window(&self, id: WindowId) -> bool {
		self.inspector.as_ref().map(|i| i.id() == id).unwrap_or(false)
	}

	pub fn redraw_inspector(&mut self) {
		super::snapshot::redraw_inspector(self);
	}

	/* update is the frame-level service loop. It samples host input, updates CIA-visible controls, decides warp policy, runs the required machine frames, and requests presentation only when TimeKeeper says a host frame is due. */
	pub fn update(&mut self) {
		if self.paused {
			self.context.input.clear_frame();
			return;
		}
		let (j1, j2) = self.context.joystick.as_mut()
			.map(|j| j.poll_manual(&self.context.input))
			.unwrap_or((0xFF, 0xFF));

		{
			let m = &mut self.context.machine;
			m.memory.cia1.joystick_1 = j1;
			m.memory.cia1.joystick_2 = j2;
			m.memory.cia1.clear_keyboard_matrix();

			let virtual_mode = self.context.joystick.as_ref()
				.map(|joystick| joystick.virtual_mode)
				.unwrap_or(crate::ui::joystick::VirtualMode::None);
			self.context.input.map_keyboard_to_cia(&mut m.memory.cia1, virtual_mode);

		}

		let drive_busy = self.context.machine.drive_busy_led();
		let warping = self.timing.is_warping(drive_busy);
		self.context.machine.set_drive_warp_execution(warping);
		if warping {
			let start = Instant::now();
			self.context.machine.set_video_composition(false);
			while start.elapsed() < Duration::from_millis(16) {
				self.run_single_frame();
			}
			self.context.machine.set_video_composition(true);
			self.run_single_frame();
		} else {
			self.context.machine.set_video_composition(true);
			let steps = self.timing.calculate_frames_to_run(drive_busy);
			for _ in 0..steps {
				self.run_single_frame();
			}
		}

		self.context.input.clear_frame();

		if self.timing.should_present(drive_busy) {
			self.context.window.request_redraw();
		}

		if self.inspector.is_some() {
			self.redraw_inspector();
			if let Some(inspector) = self.inspector.as_ref() {
				inspector.request_redraw();
			}
		}
	}

	/* A normal frame advances exactly the PAL cycle count. The const generic keeps the debug clock path outside the inner branch when disabled. */
	#[inline(never)]
	fn run_machine_cycles<const C128_DEBUG: bool>(&mut self) {
		let machine = &mut self.context.machine;
		for _ in 0..CYCLES_PER_FRAME {
			if C128_DEBUG {
				machine.tick_cycle_c128_debug();
			} else {
				machine.tick_cycle();
			}
		}
	}

	/* Tape transport is sampled at machine-cycle granularity. Flux playback drives CIA1 FLAG, recording samples the 6510 cassette-write pin, and motor transitions immediately refresh cassette sense before the next CPU cycle. */
	#[inline(never)]
	fn run_tape_cycles<const C128_DEBUG: bool>(&mut self, play_pressed: bool, mut motor_on: bool) {
		let machine = &mut self.context.machine;
		let datassette = &mut self.context.datassette;
		let record_pressed = datassette.record_pressed;

		for _ in 0..CYCLES_PER_FRAME {
			/* READ and RECORD are mutually exclusive signal paths. Both consume exactly one machine-cycle opportunity while the mechanical transport is moving. */
			if play_pressed && motor_on {
				if record_pressed {
					datassette.record_cycle_tick(machine.cpu.port.cassette_write);
				} else if datassette.clock_tick(motor_on) {
					machine.memory.cia1.set_flag_pin(false);
				} else if datassette.state == crate::datassette::TapeState::Idle {
					machine.memory.cia1.set_flag_pin(true);
				}
			}

			if C128_DEBUG {
				machine.tick_cycle_c128_debug();
			} else {
				machine.tick_cycle();
			}

			/* The motor output may change during the just-completed CPU cycle. Sense is refreshed at that boundary so the following cycle observes the new mechanical condition. */
			let new_motor_on = (machine.cpu.port.output & 0x20) == 0;
			if new_motor_on != motor_on {
				motor_on = new_motor_on;
				let total_cycles = machine.clock.total_cycles;
				let model = machine.cpu.model;
				machine.cpu.port.set_cassette_sense(play_pressed, total_cycles, model);
			}
		}
	}

	/* A single frame keeps machine execution, datassette transport, audio production and post-frame actions in one ordered unit so host scheduling cannot interleave them inconsistently. */
	fn run_single_frame(&mut self) {
		let drive_busy = self.context.machine.drive_busy_led();
		let global_mute = self.context.history.mute_enabled;
		let warp_mute = self.timing.is_warping(drive_busy) && self.mute_sid_warp;
		let render_audio = !global_mute && !warp_mute;

		self.context.machine.set_audio_rendering(render_audio);
		self.context.machine.set_sid_clocking(!warp_mute);
		self.context.machine.audio_buffer_storage.clear();

		let has_tape = self.context.datassette.has_tape();
		let play_pressed = self.context.datassette.play_pressed;
		let is_recording = self.context.datassette.record_pressed && play_pressed;

		{
			let total_cycles = self.context.machine.clock.total_cycles;
			let cpu = &mut self.context.machine.cpu;
			let model = cpu.model;
			let current_sense = !cpu.port.cassette_sense;
			if current_sense != play_pressed {
				cpu.port.set_cassette_sense(play_pressed, total_cycles, model);
			}
		}

		let c128_debug = self.context.machine.memory.c128_2mhz_debug_enabled;
		if has_tape {
			let motor_on = (self.context.machine.cpu.port.output & 0x20) == 0;
			if c128_debug {
				self.run_tape_cycles::<true>(play_pressed, motor_on);
			} else {
				self.run_tape_cycles::<false>(play_pressed, motor_on);
			}
		} else if c128_debug {
			self.run_machine_cycles::<true>();
		} else {
			self.run_machine_cycles::<false>();
		}

		if has_tape && is_recording {
			self.context.datassette.save_tape_to_host();
		}

		{
			let m = &mut self.context.machine;
			m.refresh_drive_status();
			let ctrl1 = m.vic.regs.ctrl1;
			let sprite_en = m.vic.regs.sprite_en;
			let current_pc = m.cpu.pc;
			let master_cycles = m.cpu_clock_cycles;
			let dpc = m.drive_pc();
			m.vic.telemetry.update_report(ctrl1, sprite_en, current_pc, master_cycles, dpc);
		}

		self.context.actions.tick(&mut self.context.machine);

		if render_audio {
			if let Some(audio) = &self.context.audio {
				audio.push_samples(self.context.machine.audio_buffer());
			}
		}

		self.frame_counter = self.frame_counter.wrapping_add(1);
		if self.frame_counter % 60 == 0 {
			crate::vic::state::normalise_clock(&mut self.context.machine.vic);
		}
	}

	/* draw_frame presents the latest completed machine framebuffer and overlays. Rendering never advances emulation, which keeps host repaint frequency independent from C64 timing. */
	pub fn draw_frame(&mut self) -> Result<()> {
		let disk_label = self.context.history.active_d64_g64.as_ref().and_then(|p| p.file_name()).and_then(|n| n.to_str()).unwrap_or("None").to_string();
		let tape_label = self.context.history.active_tap.as_ref().and_then(|p| p.file_name()).and_then(|n| n.to_str()).unwrap_or("None").to_string();
		let cart_label = self.context.history.active_crt.as_ref().and_then(|p| p.file_name()).and_then(|n| n.to_str()).unwrap_or("None").to_string();
		let cart_mapper = if self.context.history.active_crt.is_some() {
			let info = self.context.machine.memory.cartridge.mapper.get_info();
			Some((info.mapper_type.crt_id(), info.mapper_type.display_name().to_string()))
		} else {
			None
		};
		let joy_status = self.context.joystick.as_ref().map(|j| j.get_status_string()).unwrap_or_else(|| "None".to_string());

		let window_size = self.context.window.inner_size();
		let hover_cursor = if self.osd_cursor_pos.0 >= 0.0
			&& self.osd_cursor_pos.1 >= 0.0
			&& window_size.width > 0
			&& window_size.height > 0
		{
			Some((
				((self.osd_cursor_pos.0 / window_size.width as f64) * self.context.renderer.src_width as f64) as usize,
				((self.osd_cursor_pos.1 / window_size.height as f64) * self.context.renderer.src_height as f64) as usize,
			))
		} else {
			None
		};

		let osd_data = crate::ui::osd::OsdData {
			disk_label, tape_label, cart_label, cart_mapper, joy_status, hover_cursor,
			fps: self.context.machine.vic.telemetry.last_fps,
			mhz: self.context.machine.vic.telemetry.current_mhz,
			has_tape: self.context.datassette.has_tape(),
			play_on: self.context.datassette.play_pressed,
			record_on: self.context.datassette.record_pressed,
			odometre: self.context.datassette.get_odometre_value(),
			power_led: self.context.machine.drive_power_led(),
			activity_led: self.context.machine.drive_busy_led(),
			current_track: self.context.machine.drive_current_track(),
		};

		self.context.renderer.draw(
			self.context.machine.vic.get_framebuffer(),
			&osd_data,
		)
	}

	pub fn handle_resize(&mut self, width: u32, height: u32) {
		let scale = self.context.renderer.handle_resize(width, height);
		if scale > 0.0 {
			self.current_scale_factor = scale;
		}
		self.update_cursor_visibility();
	}

	pub fn handle_input_event(&mut self, key: KeyCode, state: ElementState) {
		self.context.input.update_key(key, state);
	}

	/* Drag-and-drop is reduced to the same AppContext operations used by menus and command-line startup, preventing separate media-loading semantics for each UI surface. */
	pub fn handle_drop(&mut self, path: std::path::PathBuf) {
		InputRouter::handle_drop(path, &mut self.context);
	}

	pub fn handle_menu_event(&mut self, id: &str) {
		super::commands::handle_menu_event(self, id);
		self.update_cursor_visibility();
	}

	pub fn get_next_frame_time(&self) -> Instant {
		self.timing.get_next_frame_time()
	}
}