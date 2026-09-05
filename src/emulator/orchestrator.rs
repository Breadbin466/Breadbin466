// =======================================================
// src/emulator/orchestrator.rs — System Master Orchestrator loop
// =======================================================

use std::time::{Duration, Instant};
use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, MouseButton};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::KeyCode;
use winit::window::WindowId;

use super::Result;
use super::builder::SystemBuilder;
use super::constants::STARTUP_COMMAND_DELAY_FRAMES;
use super::context::AppContext;
use super::debugger::Debugger;
use super::timing::TimeKeeper;
use crate::clockchip::constants::{CPU_FREQ_HZ, CYCLES_PER_FRAME};
use crate::emulator::command_line::{CommandLine, StartupAction};
use crate::ui::InspectorWindow;
use crate::ui::constants::INITIAL_WINDOW_SCALE;
use crate::ui::routing::InputRouter;
use crate::ui::{MouseHost, OsdMonitor};

/* Orchestrator is the sole coordinator between host-time services and the mutable machine. It owns AppContext, converts host events into deterministic machine actions, advances whole emulated frames, and publishes video, audio and inspector snapshots without exposing direct motherboard access to the event loop. */
pub struct Orchestrator {
	pub context: AppContext,
	pub osd: OsdMonitor,
	pub(super) timing: TimeKeeper,
	pub(super) current_scale_factor: f64,
	pub mute_sid_warp: bool,
	pub(super) silent: bool,
	pub(super) quit_after_cycles: Option<u64>,
	pub(super) emulated_cycles: u64,
	pub(super) osd_cursor_pos: (f64, f64),
	pub(super) mouse_host: MouseHost,
	pub(super) frame_counter: u64,
	pub inspector: Option<InspectorWindow>,
	pub inspector_requested: bool,
	pub is_dark_mode: bool,
	pub(super) paused: bool,
	pub debugger: Option<Debugger>,
}

impl Orchestrator {
	/* Construction applies startup configuration in dependency order: build host resources, restore persisted media, apply explicit command-line overrides, schedule deferred machine actions, then synchronise menu state with the resulting runtime state. */
	pub fn new(application: &ActiveEventLoop, command_line: &CommandLine) -> Result<Self> {
		let mut context = SystemBuilder::build(application, command_line)?;

		let cli_has_d64_g64 = command_line.disk.is_some()
			|| command_line.drive == Some(crate::emulator::command_line::DriveSelection::Off);
		let cli_has_tap = command_line.tape.is_some();

		if let Some(joystick) = context.joystick.as_mut() {
			joystick.configure(command_line.joystick, command_line.joystick_port)?;
		} else if command_line.joystick == crate::emulator::command_line::JoystickSelection::Gilrs {
			return Err("GILRS joystick support is unavailable.".into());
		}

		let is_dark_mode = context.window.theme() == Some(winit::window::Theme::Dark);

		let mut driver = Self {
			context,
			osd: OsdMonitor::new(),
			timing: TimeKeeper::new(),
			current_scale_factor: command_line
				.scale
				.map(f64::from)
				.unwrap_or(INITIAL_WINDOW_SCALE),
			mute_sid_warp: command_line.mute_warp.unwrap_or(true),
			silent: command_line.silent,
			quit_after_cycles: command_line.quit_minutes.map(|minutes| {
				(minutes as f64 * 60.0 * CPU_FREQ_HZ).round() as u64
			}),
			emulated_cycles: 0,
			osd_cursor_pos: (-1.0, -1.0),
			mouse_host: MouseHost::new(),
			frame_counter: 0,
			inspector: None,
			inspector_requested: command_line.inspector,
			is_dark_mode,
			paused: false,
			debugger: command_line
				.debugger
				.then(|| Debugger::new_with_commands(command_line.monitor_commands.clone())),
		};

		super::session::restore_media(&mut driver, cli_has_d64_g64, cli_has_tap);

		if let Some(path) = command_line.tape.clone() {
			if !driver.context.mount_tape(path) && driver.context.history.active_tap.is_some() {
				driver.context.history.active_tap = None;
				driver.context.history.save_forced();
			}
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
			StartupAction::LoadDirectory => driver
				.context
				.actions
				.schedule_startup_text_entry("LOAD\"$\",8\r", STARTUP_COMMAND_DELAY_FRAMES),
			StartupAction::LoadFirst => driver
				.context
				.actions
				.schedule_startup_text_entry("LOAD\"*\",8,1\r", STARTUP_COMMAND_DELAY_FRAMES),
			StartupAction::LoadFirstRun => driver
				.context
				.actions
				.schedule_startup_load_first_run(STARTUP_COMMAND_DELAY_FRAMES),
		}
		if command_line.freeze {
			let cycle = driver.context.machine.current_cycle();
			driver
				.context
				.machine
				.memory
				.cartridge
				.trigger_freeze_button(cycle);
			driver.context.machine.memory.mark_memory_map_dirty();
		}
		if command_line.cartridge_menu {
			let cycle = driver.context.machine.current_cycle();
			let reset_requested = driver
				.context
				.machine
				.memory
				.cartridge
				.trigger_menu_button(cycle);
			driver.context.machine.memory.mark_memory_map_dirty();
			if reset_requested {
				driver.context.machine.soft_reset();
			}
		}
		if command_line.fullscreen {
			#[cfg(target_os = "linux")]
			crate::ui::shell::Shell::set_fullscreen(true);
			#[cfg(not(target_os = "linux"))]
			driver
				.context
				.window
				.set_fullscreen(Some(winit::window::Fullscreen::Borderless(None)));
		}
		driver
			.context
			.menu
			.set_checked(&driver.context.menu.ids.warp_mode, driver.timing.warp_mode);
		driver
			.context
			.menu
			.set_checked(&driver.context.menu.ids.warp_1541, driver.timing.warp_1541);
		driver.context.menu.set_checked(
			&driver.context.menu.ids.debug_mute_warp,
			driver.mute_sid_warp,
		);
		driver.context.menu.set_checked(
			&driver.context.menu.ids.debug_c128_2mhz,
			command_line.mode_8502,
		);
		driver.context.menu.set_checked(
			&driver.context.menu.ids.debug_display_uptime,
			driver.context.history.display_uptime,
		);
		driver
			.context
			.menu
			.set_checked(&driver.context.menu.ids.reu_1764_512k, command_line.reu);
		driver
			.context
			.menu
			.set_checked(&driver.context.menu.ids.mouse_1351, false);

		Ok(driver)
	}

	/* Pointer movement serves both OSD hit-testing and, when explicitly connected,
	 * the 1351. The host cursor is hidden only while it is over the C64 display;
	 * leaving that display restores it immediately. Because the 1351 is relative,
	 * entry recentres the now-hidden host cursor without producing C64 motion. This
	 * prevents an arbitrary host-entry position from exhausting movement range on
	 * one side of the window before the emulated cursor reaches the same edge. */
	pub fn handle_cursor_moved(&mut self, x: f64, y: f64) {
		self.osd_cursor_pos = (x, y);

		if self.context.machine.mouse_1351_connected() {
			let size = self.context.window.inner_size();
			let was_over_display = self.mouse_host.over_display();
			if let Some((dx, dy)) = self.mouse_host.cursor_moved(
				x,
				y,
				size.width,
				size.height,
				self.context.renderer.osd_enabled,
			) {
				self.context.machine.move_mouse_1351(dx, dy);
			}

			let over_display = self.mouse_host.over_display();
			if !was_over_display && over_display {
				self.context.window.set_cursor_visible(false);

				let presentation_height = if self.context.renderer.osd_enabled {
					crate::ui::constants::CRT_HEIGHT + crate::ui::constants::GUI_HEIGHT
				} else {
					crate::ui::constants::CRT_HEIGHT
				};
				let display_height = f64::from(size.height)
					* crate::ui::constants::CRT_HEIGHT as f64
					/ presentation_height as f64;
				let centre =
					PhysicalPosition::new(f64::from(size.width) * 0.5, display_height * 0.5);
				self.mouse_host.reset_motion_baseline();
				let _ = self.context.window.set_cursor_position(centre);
			} else if was_over_display && !over_display {
				self.context.window.set_cursor_visible(true);
				self.context.machine.release_mouse_1351_buttons();
			}
		} else {
			self.mouse_host.reset_tracking();
			self.context.window.set_cursor_visible(true);
		}

		if self.context.renderer.osd_enabled {
			self.context.window.request_redraw();
		}
	}

	pub fn handle_cursor_left(&mut self) {
		self.osd_cursor_pos = (-1.0, -1.0);
		self.mouse_host.reset_tracking();
		self.context.window.set_cursor_visible(true);
		self.context.machine.release_mouse_1351_buttons();

		if self.context.renderer.osd_enabled {
			self.context.window.request_redraw();
		}
	}

	pub fn handle_mouse_button(&mut self, button: MouseButton, pressed: bool) {
		let connected = self.context.machine.mouse_1351_connected();
		let over_display = self.mouse_host.over_display();

		match button {
			MouseButton::Left if connected && (over_display || !pressed) => {
				self.context.machine.set_mouse_1351_left_button(pressed);
			}
			MouseButton::Right if connected && (over_display || !pressed) => {
				self.context.machine.set_mouse_1351_right_button(pressed);
			}
			MouseButton::Left if pressed => self.handle_mouse_click(),
			_ => {}
		}
	}

	pub fn set_mouse_1351_connected(&mut self, connected: bool) {
		if let Some(joystick) = self.context.joystick.as_mut() {
			joystick.set_port1_reserved(connected);
		}
		self.context.machine.set_mouse_1351_connected(connected);
		self.mouse_host.reset_tracking();
		self.context.window.set_cursor_visible(true);
		let id = self.context.menu.ids.mouse_1351.clone();
		self.context.menu.set_checked(&id, connected);
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
		if self.inspector.is_some() {
			return;
		}
		let instance = self.context.renderer.gpu_instance();
		let adapter = self.context.renderer.gpu_adapter().clone();
		let device = self.context.renderer.gpu_device();
		let queue = self.context.renderer.gpu_queue();
		match InspectorWindow::new(
			application,
			instance,
			&adapter,
			device,
			queue,
			wgpu::TextureFormat::Bgra8Unorm,
		) {
			Ok(win) => self.inspector = Some(win),
			Err(e) => println!("Inspector window error: {}", e),
		}
	}

	pub fn close_inspector(&mut self) {
		self.inspector = None;
	}

	pub fn is_inspector_window(&self, id: WindowId) -> bool {
		self.inspector
			.as_ref()
			.map(|i| i.id() == id)
			.unwrap_or(false)
	}

	pub fn redraw_inspector(&mut self) {
		super::snapshot::redraw_inspector(self);
	}

	/* update is the frame-level service loop. It samples host input, updates CIA-visible controls, decides warp policy, runs the required machine frames, and requests presentation only when TimeKeeper says a host frame is due. */
	pub fn update(&mut self) {
		/* Debugger commands are consumed only at the host service boundary, where the
		 * motherboard is quiescent. Temporarily taking ownership avoids aliasing the
		 * debugger with AppContext while a command inspects or mutates machine state. */
		if let Some(mut debugger) = self.debugger.take() {
			debugger.poll_commands(&mut self.context);
			let debugger_paused = debugger.paused;
			self.debugger = Some(debugger);
			if debugger_paused {
				self.context.input.clear_frame();
				return;
			}
		}
		if self.paused {
			self.context.input.clear_frame();
			return;
		}
		let (j1, j2) = self
			.context
			.joystick
			.as_mut()
			.map(|j| j.poll_manual(&self.context.input))
			.unwrap_or((0xFF, 0xFF));

		{
			let m = &mut self.context.machine;
			let mouse_connected = m.mouse_1351_connected();
			m.memory.cia1.joystick_1 = if mouse_connected {
				m.mouse_1351_digital_mask()
			} else {
				j1
			};
			m.memory.cia1.joystick_2 = j2;
			m.memory.cia1.clear_keyboard_matrix();

			let virtual_mode = self
				.context
				.joystick
				.as_ref()
				.map(|joystick| joystick.virtual_mode)
				.unwrap_or(crate::ui::joystick::VirtualMode::None);
			self.context
				.input
				.map_keyboard_to_cia(&mut m.memory.cia1, virtual_mode);
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

	/*
	 * A frame selects its execution path once before entering the PAL cycle loop.
	 * Keeping the optional debugger test outside the hot loop is essential in Warp:
	 * even a perfectly predictable Option branch repeated nearly twenty thousand
	 * times per frame measurably reduces throughput.  The normal path therefore
	 * contains only the machine tick selected by the const generic.
	 */
	/*
	 * Optional hardware is selected once at the frame boundary.  The REU-disabled
	 * instantiation calls the motherboard entry point that contains no REC
	 * arbitration or REU interrupt work, while the enabled instantiation preserves
	 * the complete DMA contract.  Menu and command-line changes are applied only at
	 * host service boundaries, so the selection remains stable for the frame.
	 */
	#[inline(never)]
	fn run_machine_cycles<const C128_DEBUG: bool, const REU_ENABLED: bool>(&mut self) {
		if self.debugger.is_some() {
			for _ in 0..CYCLES_PER_FRAME {
				if !self.run_debugger_cycle(C128_DEBUG, REU_ENABLED) {
					break;
				}
			}
		} else if C128_DEBUG {
			for _ in 0..CYCLES_PER_FRAME {
				if REU_ENABLED {
					self.context.machine.tick_cycle_c128_debug_reu();
				} else {
					self.context.machine.tick_cycle_c128_debug();
				}
			}
		} else {
			for _ in 0..CYCLES_PER_FRAME {
				if REU_ENABLED {
					self.context.machine.tick_cycle_reu();
				} else {
					self.context.machine.tick_cycle();
				}
			}
		}
	}

	/*
	 * A debugger-controlled cycle brackets exactly one motherboard cycle with an
	 * instruction-boundary gate and a completed-bus-cycle observation. The machine
	 * is never stopped halfway through a CPU micro-operation merely because the host
	 * console delivered a command.
	 */
	fn run_debugger_cycle(&mut self, c128_debug: bool, reu_enabled: bool) -> bool {
		let Some(mut debugger) = self.debugger.take() else {
			return true;
		};
		if !debugger.before_cycle(&mut self.context) {
			self.debugger = Some(debugger);
			return false;
		}
		match (c128_debug, reu_enabled) {
			(true, true) => self.context.machine.tick_cycle_c128_debugger_reu(),
			(true, false) => self.context.machine.tick_cycle_c128_debugger(),
			(false, true) => self.context.machine.tick_cycle_debugger_reu(),
			(false, false) => self.context.machine.tick_cycle_debugger(),
		}
		debugger.after_cycle(&mut self.context);
		let keep_running = !debugger.paused;
		self.debugger = Some(debugger);
		keep_running
	}

	/* Tape transport is sampled at machine-cycle granularity. Flux playback drives CIA1 FLAG, recording samples the 6510 cassette-write pin, and motor transitions immediately refresh cassette sense before the next CPU cycle. */
	#[inline(never)]
	fn run_tape_cycles<const C128_DEBUG: bool, const REU_ENABLED: bool>(
		&mut self,
		play_pressed: bool,
		mut motor_on: bool,
	) {
		let record_pressed = self.context.datassette.record_pressed;
		let debugger_active = self.debugger.is_some();

		for _ in 0..CYCLES_PER_FRAME {
			if play_pressed && motor_on {
				if record_pressed {
					let cassette_write = self.context.machine.cpu.port.cassette_write;
					self.context.datassette.record_cycle_tick(cassette_write);
				} else if self.context.datassette.clock_tick(motor_on) {
					self.context.machine.memory.cia1.set_flag_pin(false);
				} else if self.context.datassette.state == crate::datassette::TapeState::Idle {
					self.context.machine.memory.cia1.set_flag_pin(true);
				}
			}

			if debugger_active {
				if !self.run_debugger_cycle(C128_DEBUG, REU_ENABLED) {
					break;
				}
			} else if C128_DEBUG {
				if REU_ENABLED {
					self.context.machine.tick_cycle_c128_debug_reu();
				} else {
					self.context.machine.tick_cycle_c128_debug();
				}
			} else if REU_ENABLED {
				self.context.machine.tick_cycle_reu();
			} else {
				self.context.machine.tick_cycle();
			}

			let new_motor_on = (self.context.machine.cpu.port.output & 0x20) == 0;
			if new_motor_on != motor_on {
				motor_on = new_motor_on;
				let total_cycles = self.context.machine.clock.total_cycles;
				let model = self.context.machine.cpu.model;
				self.context
					.machine
					.cpu
					.port
					.set_cassette_sense(play_pressed, total_cycles, model);
			}
		}
	}

	/* A single frame keeps machine execution, datassette transport, audio production and post-frame actions in one ordered unit so host scheduling cannot interleave them inconsistently. */
	fn run_single_frame(&mut self) {
		let drive_busy = self.context.machine.drive_busy_led();
		let global_mute = self.context.history.mute_enabled;
		let warp_mute = self.timing.is_warping(drive_busy) && self.mute_sid_warp;
		let render_audio = !self.silent && !global_mute && !warp_mute;
		let capture_audio = self.context.wav.is_some();
		let produce_audio = render_audio || capture_audio;

		self.context.machine.set_audio_rendering(produce_audio);
		self.context
			.machine
			.set_sid_clocking(!warp_mute || capture_audio);
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
				cpu.port
					.set_cassette_sense(play_pressed, total_cycles, model);
			}
		}

		let c128_debug = self.context.machine.memory.c128_2mhz_debug_enabled;
		let reu_enabled = self.context.machine.memory.reu.enabled;
		if has_tape {
			let motor_on = (self.context.machine.cpu.port.output & 0x20) == 0;
			match (c128_debug, reu_enabled) {
				(true, true) => self.run_tape_cycles::<true, true>(play_pressed, motor_on),
				(true, false) => self.run_tape_cycles::<true, false>(play_pressed, motor_on),
				(false, true) => self.run_tape_cycles::<false, true>(play_pressed, motor_on),
				(false, false) => self.run_tape_cycles::<false, false>(play_pressed, motor_on),
			}
		} else {
			match (c128_debug, reu_enabled) {
				(true, true) => self.run_machine_cycles::<true, true>(),
				(true, false) => self.run_machine_cycles::<true, false>(),
				(false, true) => self.run_machine_cycles::<false, true>(),
				(false, false) => self.run_machine_cycles::<false, false>(),
			}
		}

		if has_tape && is_recording {
			if let Err(error) = self.context.datassette.save_tape_to_host() {
				eprintln!("[TAPE] Failed to persist TAP image: {error}");
			}
		}

		{
			let m = &mut self.context.machine;
			m.refresh_drive_status();
			let ctrl1 = m.vic.regs.ctrl1;
			let sprite_en = m.vic.regs.sprite_en;
			let current_pc = m.cpu.pc;
			let master_cycles = m.cpu_clock_cycles;
			let dpc = m.drive_pc();
			m.vic
				.telemetry
				.update_report(ctrl1, sprite_en, current_pc, master_cycles, dpc);
		}

		self.context.actions.tick(&mut self.context.machine);

		if capture_audio {
			if let Some(wav) = &mut self.context.wav {
				wav.push_samples(self.context.machine.audio_buffer());
			}
		}

		if render_audio {
			if let Some(audio) = &self.context.audio {
				audio.push_samples(self.context.machine.audio_buffer());
			}
		}

		self.emulated_cycles = self.emulated_cycles.saturating_add(CYCLES_PER_FRAME);
		self.frame_counter = self.frame_counter.wrapping_add(1);
		if self.frame_counter % 60 == 0 {
			crate::vic::state::normalise_clock(&mut self.context.machine.vic);
		}
	}

	/* Command-line duration limits are measured from emulated PAL machine cycles, so Warp changes only how quickly the limit is reached in host time. */
	pub fn quit_time_reached(&self) -> bool {
		self.quit_after_cycles
			.is_some_and(|limit| self.emulated_cycles >= limit)
	}

	/* draw_frame presents the latest completed machine framebuffer and overlays. Rendering never advances emulation, which keeps host repaint frequency independent from C64 timing. */
	pub fn draw_frame(&mut self) -> Result<()> {
		let disk_label = self
			.context
			.history
			.active_d64_g64
			.as_ref()
			.and_then(|p| p.file_name())
			.and_then(|n| n.to_str())
			.unwrap_or("None")
			.to_string();
		let tape_label = self
			.context
			.history
			.active_tap
			.as_ref()
			.and_then(|p| p.file_name())
			.and_then(|n| n.to_str())
			.unwrap_or("None")
			.to_string();
		let cart_label = self
			.context
			.history
			.active_crt
			.as_ref()
			.and_then(|p| p.file_name())
			.and_then(|n| n.to_str())
			.unwrap_or("None")
			.to_string();
		let cart_mapper = if self.context.history.active_crt.is_some() {
			let info = self.context.machine.memory.cartridge.mapper.get_info();
			Some((
				info.mapper_type.crt_id(),
				info.mapper_type.display_name().to_string(),
			))
		} else {
			None
		};
		let joy_status = self
			.context
			.joystick
			.as_ref()
			.map(|j| j.get_status_string())
			.unwrap_or_else(|| "None".to_string());

		let window_size = self.context.window.inner_size();
		let hover_cursor = if self.osd_cursor_pos.0 >= 0.0
			&& self.osd_cursor_pos.1 >= 0.0
			&& window_size.width > 0
			&& window_size.height > 0
		{
			Some((
				((self.osd_cursor_pos.0 / window_size.width as f64)
					* self.context.renderer.src_width as f64) as usize,
				((self.osd_cursor_pos.1 / window_size.height as f64)
					* self.context.renderer.src_height as f64) as usize,
			))
		} else {
			None
		};

		let osd_data = crate::ui::osd::OsdData {
			disk_label,
			tape_label,
			cart_label,
			cart_mapper,
			joy_status,
			hover_cursor,
			mouse_active: self.context.machine.mouse_1351_connected(),
			display_uptime: self.context.history.display_uptime,
			uptime_seconds: self.emulated_cycles as f64 / CPU_FREQ_HZ,
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

		self.context
			.renderer
			.draw(self.context.machine.vic.get_framebuffer(), &osd_data)
	}

	/* User resizing is constrained before the GPU surface is reconfigured. This keeps the host window and the emulated presentation in the same proportion instead of merely letterboxing a wrongly shaped window. The corrective request produces one follow-up resize event, which is accepted once its integer dimensions are within the renderer's one-pixel tolerance. */
	pub fn handle_resize(&mut self, width: u32, height: u32) {
		if let Some((locked_width, locked_height)) =
			self.context.renderer.constrain_resize(width, height)
		{
			#[cfg(target_os = "linux")]
			crate::ui::shell::Shell::resize_content(locked_width, locked_height);
			#[cfg(not(target_os = "linux"))]
			let _ = self
				.context
				.window
				.request_inner_size(winit::dpi::PhysicalSize::new(locked_width, locked_height));
			return;
		}

		let scale = self.context.renderer.handle_resize(width, height);
		if scale > 0.0 {
			self.current_scale_factor = scale;
		}

		/* Scale menu checks represent exact presets, not the nearest arbitrary zoom.
		 * Free resizing clears the whole radio group; landing exactly on a native
		 * 1x, 2x or 3x size selects only that corresponding entry. */
		let ids = self.context.menu.ids.clone();
		let scale_ids = [
			ids.scale_1x.as_str(),
			ids.scale_2x.as_str(),
			ids.scale_3x.as_str(),
		];
		match self
			.context
			.renderer
			.integer_scale_for_window_size(width, height)
		{
			Some(1) => self
				.context
				.menu
				.set_radio_selection(&ids.scale_1x, &scale_ids),
			Some(2) => self
				.context
				.menu
				.set_radio_selection(&ids.scale_2x, &scale_ids),
			Some(3) => self
				.context
				.menu
				.set_radio_selection(&ids.scale_3x, &scale_ids),
			_ => self.context.menu.set_radio_selection("", &scale_ids),
		}
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
	}

	pub fn get_next_frame_time(&self) -> Instant {
		self.timing.get_next_frame_time()
	}
}