// =======================================================
// src/emulator/builder.rs — Application Initialisation and Assembly Factory
// =======================================================

use winit::event_loop::ActiveEventLoop;
use winit::window::Window;
#[cfg(not(target_os = "linux"))]
use winit::dpi::LogicalSize;
#[cfg(target_os = "linux")]
use winit::dpi::PhysicalSize;

use super::context::AppContext;
use crate::emulator::command_line::{CommandLine, DriveSelection};
use crate::motherboard::Motherboard;
use crate::motherboard::injection::ActionManager;
use crate::datassette::Datassette;
use crate::ui::{Renderer, InputState, MenuManager, History, AudioHost, JoystickHost};
use crate::ui::shell::Shell;
use crate::ui::constants::{CRT_WIDTH, CRT_HEIGHT, GUI_HEIGHT, INITIAL_WINDOW_SCALE};

pub type Result<T, E = Box<dyn std::error::Error>> = std::result::Result<T, E>;

/* SystemBuilder assembles host resources and the emulated machine in dependency order. It restores durable preferences first, creates optional host services without making them fatal, initialises ROM-backed hardware, mounts requested media, and returns one fully coherent AppContext. */
pub struct SystemBuilder;

impl SystemBuilder {
	/* Construction proceeds from host shell resources to optional audio and joystick services, then renderer and menus, then the motherboard and ROMs, and finally media and reset. This prevents later stages from observing a partly initialised machine. */
	pub fn build(application: &ActiveEventLoop, command_line: &CommandLine) -> Result<AppContext> {
		let mut history = History::load();

		if let Some(osd) = command_line.osd {
			history.osd_enabled = osd;
		}
		if command_line.mute {
			history.mute_enabled = true;
		}
		let initial_scale = command_line.scale.map(f64::from).unwrap_or(INITIAL_WINDOW_SCALE);
		let width  = CRT_WIDTH as f64 * initial_scale;
		let height = if history.osd_enabled {
			(CRT_HEIGHT + GUI_HEIGHT) as f64 * initial_scale
		} else {
			CRT_HEIGHT as f64 * initial_scale
		};

		let window_attrs = Window::default_attributes()
			.with_title("Breadbin466 0.11.0 – PAL Assy 250466 Commodore 64 emulator")
			.with_resizable(true);
		#[cfg(target_os = "linux")]
		let window_attrs = window_attrs.with_inner_size(PhysicalSize::new(width as u32, height as u32));
		#[cfg(not(target_os = "linux"))]
		let window_attrs = window_attrs.with_inner_size(LogicalSize::new(width, height));

		let window = Shell::create_window(application, window_attrs)?;

		println!("System Initialised.");

		/* Audio and joystick failures degrade the corresponding host feature without invalidating the emulated computer itself. */
		let audio = match AudioHost::new() {
			Ok(host) => Some(host),
			Err(error) => {
				eprintln!("Audio initialisation error: {}", error);
				None
			}
		};
		let joystick = match JoystickHost::new() {
			Ok(host) => Some(host),
			Err(error) => {
				eprintln!("Joystick initialisation error: {}", error);
				None
			}
		};

		let sample_rate = audio.as_ref().map(|a: &AudioHost| a.get_sample_rate()).unwrap_or(44100.0);

		let mut initial_crt = command_line.cartridge.clone().or_else(|| history.active_crt.clone());
		if let Some(path) = command_line.cartridge.clone() {
			history.active_crt = Some(path);
			history.save_forced();
		}

		if let Some(ref path) = initial_crt {
			if !path.exists() {
				initial_crt        = None;
				history.active_crt = None;
				history.save_forced();
			}
		}

		let mut renderer = Renderer::new(&window)?;
		renderer.set_osd_enabled(history.osd_enabled);
		let menu     = MenuManager::new(&window, &history)?;
		renderer.resize_window_to_fit(initial_scale);

		/* The machine is created only after the actual host sample rate is known, because SID resampling coefficients are part of machine construction. */
		let mut machine = Motherboard::new(sample_rate, initial_crt);
		if command_line.mode_8502 {
			machine.set_c128_debug_enabled(true);
		}
		if command_line.reu {
			machine.memory.reu.enabled = true;
			machine.memory.reu.storage = Some(Box::new([0u8; 512 * 1024]));
		}
		if let Err(e) = machine.init_roms() {
			return Err(e);
		}

		if let Some(ref path) = history.custom_char_rom {
			if let Err(e) = machine.load_custom_char_rom(path) {
				println!("Custom Character ROM error: {}", e);
				history.custom_char_rom = None;
				history.save_forced();
			}
		}
		if let Some(ref path) = history.custom_basic_rom {
			if let Err(e) = machine.load_custom_basic_rom(path) {
				println!("Custom BASIC ROM error: {}", e);
				history.custom_basic_rom = None;
				history.save_forced();
			}
		}
		if let Some(ref path) = history.custom_kernal_rom {
			if let Err(e) = machine.load_custom_kernal_rom(path) {
				println!("Custom KERNAL ROM error: {}", e);
				history.custom_kernal_rom = None;
				history.save_forced();
			}
		}
		if let Some(ref path) = history.custom_drive_rom {
			if let Err(e) = machine.load_custom_drive_rom(path) {
				println!("Custom 1541 ROM error: {}", e);
				history.custom_drive_rom = None;
				history.save_forced();
			}
		}

		let drive_mode = match command_line.drive {
			Some(DriveSelection::On) => crate::motherboard::bus::DriveMode::Lle,
			Some(DriveSelection::Off) => crate::motherboard::bus::DriveMode::Off,
			None => history.drive_mode,
		};
		machine.set_drive_mode(drive_mode);

		if let Some(arg_path) = command_line.disk.as_ref() {
			if machine.mount_drive(arg_path) {
				history.active_d64_g64 = Some(arg_path.clone());
				history.set_last_disk(arg_path.clone());
			}
		}

		/* Reset is deliberately last: mounted media, selected ROMs and expansion hardware must already define the machine that enters its power-on sequence. */
		machine.reset();

		Ok(AppContext {
			machine,
			renderer,
			window,
			input:      InputState::new(),
			actions:    ActionManager::new(),
			menu,
			history,
			audio,
			joystick,
			datassette: Datassette::new(),
		})
	}
}