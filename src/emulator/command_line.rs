// =======================================================
// src/emulator/command_line.rs — Minimal Breadbin466 command-line
// =======================================================

use std::path::{Path, PathBuf};

/* DriveSelection is an explicit command-line override. Absence means that persisted configuration remains authoritative. */
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveSelection {
	On,
	Off,
}

/* JoystickSelection chooses which host input source is allowed to drive the two active-low CIA joystick bytes. */
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoystickSelection {
	Auto,
	Keyboard,
	Gilrs,
	None,
}

/* StartupAction represents deferred text entry rather than direct machine mutation, so startup commands follow the same keyboard-visible path as user input. */
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupAction {
	None,
	LoadDirectory,
	LoadFirst,
	LoadFirstRun,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateKind {
	Disk,
	Tape,
}

#[derive(Debug, Clone)]
pub enum Command {
	Run,
	Create { kind: CreateKind, path: PathBuf },
	Help,
}

/* CommandLine is the single translation boundary from process arguments to runtime configuration. Parsing records intent only; validation rejects combinations that cannot produce a coherent initial machine state. */
#[derive(Debug, Clone)]
pub struct CommandLine {
	pub command: Command,
	pub disk: Option<PathBuf>,
	pub tape: Option<PathBuf>,
	pub cartridge: Option<PathBuf>,
	pub no_cartridge: bool,
	pub prg: Option<PathBuf>,
	pub startup_action: StartupAction,
	pub drive: Option<DriveSelection>,
	pub joystick: JoystickSelection,
	pub joystick_port: u8,
	pub warp: bool,
	pub warp_1541: bool,
	pub freeze: bool,
	pub cartridge_menu: bool,
	pub fullscreen: bool,
	pub inspector: bool,
	pub osd: Option<bool>,
	pub display_uptime: bool,
	pub scale: Option<u8>,
	pub mute: bool,
	pub silent: bool,
	pub wav: Option<PathBuf>,
	pub mute_warp: Option<bool>,
	pub quit_minutes: Option<u64>,
	pub mode_8502: bool,
	/* Machine expansion selection remains independent from debugger startup: the
	 * REU is ordinary emulated hardware, while the debugger changes only host-side
	 * execution control and observation policy. */
	pub reu: bool,
	pub debugger: bool,
	/* Monitor commands supplied by the host command line are executed through the
	 * same parser and dispatcher as interactive input.  They are host-side debugger
	 * policy only and never bypass emulated bus or CPU behaviour. */
	pub monitor_commands: Vec<String>,
}

impl Default for CommandLine {
	fn default() -> Self {
		Self {
			command: Command::Run,
			disk: None,
			tape: None,
			cartridge: None,
			no_cartridge: false,
			prg: None,
			startup_action: StartupAction::None,
			drive: None,
			joystick: JoystickSelection::Auto,
			joystick_port: 2,
			warp: false,
			warp_1541: false,
			freeze: false,
			cartridge_menu: false,
			fullscreen: false,
			inspector: false,
			osd: None,
			display_uptime: false,
			scale: None,
			mute: false,
			silent: false,
			wav: None,
			mute_warp: None,
			quit_minutes: None,
			mode_8502: false,
			reu: false,
			debugger: false,
			monitor_commands: Vec::new(),
		}
	}
}

impl CommandLine {
	/* Parsing accepts both explicit options and one positional file per supported media class, then validates cross-option invariants after the complete argument vector is known. */
	pub fn parse(args: &[String]) -> Result<Self, String> {
		let mut result = Self::default();
		let mut index = 1;

		if args.get(index).map(String::as_str) == Some("create") {
			index += 1;
			let kind = match args.get(index).map(String::as_str) {
				Some("disk") => CreateKind::Disk,
				Some("tape") => CreateKind::Tape,
				_ => return Err("Expected 'disk' or 'tape' after 'create'.".into()),
			};
			index += 1;
			let path = args
				.get(index)
				.map(PathBuf::from)
				.ok_or_else(|| "Missing output file for create command.".to_string())?;
			if index + 1 != args.len() {
				return Err("Unexpected arguments after create output file.".into());
			}
			result.command = Command::Create { kind, path };
			return Ok(result);
		}

		while index < args.len() {
			let argument = &args[index];
			match argument.as_str() {
				"--help" | "-h" => result.command = Command::Help,
				"--disk" => result.disk = Some(path_value(args, &mut index, "--disk")?),
				"--tape" => result.tape = Some(path_value(args, &mut index, "--tape")?),
				"--cartridge" => {
					result.cartridge = Some(path_value(args, &mut index, "--cartridge")?)
				}
				"--no-cartridge" => result.no_cartridge = true,
				"--prg" => result.prg = Some(path_value(args, &mut index, "--prg")?),
				"--load-directory" => {
					set_startup_action(&mut result, StartupAction::LoadDirectory)?
				}
				"--load-first" => set_startup_action(&mut result, StartupAction::LoadFirst)?,
				"--load-first-run" => set_startup_action(&mut result, StartupAction::LoadFirstRun)?,
				"--drive" => result.drive = Some(parse_drive(value(args, &mut index, "--drive")?)?),
				"--joystick" => {
					result.joystick = parse_joystick(value(args, &mut index, "--joystick")?)?
				}
				"--joystick-port" => {
					let port = parse_u8(
						value(args, &mut index, "--joystick-port")?,
						"--joystick-port",
					)?;
					if port != 1 && port != 2 {
						return Err("--joystick-port accepts only 1 or 2.".into());
					}
					result.joystick_port = port;
				}
				"--warp" => result.warp = true,
				"--warp-1541" => result.warp_1541 = true,
				"--freeze" => result.freeze = true,
				"--cartridge-menu" => result.cartridge_menu = true,
				"--fullscreen" => result.fullscreen = true,
				"--inspector" => result.inspector = true,
				"--osd" => result.osd = Some(true),
				"--no-osd" => result.osd = Some(false),
				"--display-uptime" => result.display_uptime = true,
				"--scale" => {
					let scale = parse_u8(value(args, &mut index, "--scale")?, "--scale")?;
					if !(1..=3).contains(&scale) {
						return Err("--scale accepts only 1, 2 or 3.".into());
					}
					result.scale = Some(scale);
				}
				"--mute" => result.mute = true,
				"--silent" => result.silent = true,
				"--wav" => result.wav = Some(path_value(args, &mut index, "--wav")?),
				"--mute-warp" => result.mute_warp = Some(true),
				"--no-mute-warp" => result.mute_warp = Some(false),
				"--quit" => {
					let minutes = parse_u64(value(args, &mut index, "--quit")?, "--quit")?;
					if minutes == 0 {
						return Err("--quit requires a positive number of C64 minutes.".into());
					}
					result.quit_minutes = Some(minutes);
				}
				"--8502" => result.mode_8502 = true,
				"--reu" => result.reu = true,
				"--debugger" | "--monitor" => result.debugger = true,
				"--monitor-command" => {
					let command = value(args, &mut index, "--monitor-command")?.trim();
					if command.is_empty() {
						return Err(
							"--monitor-command requires a non-empty debugger command.".into()
						);
					}
					result.debugger = true;
					result.monitor_commands.push(command.to_string());
				}
				"--monitor-commands" => {
					let commands = value(args, &mut index, "--monitor-commands")?;
					let mut added = 0usize;
					for command in commands
						.split(';')
						.map(str::trim)
						.filter(|command| !command.is_empty())
					{
						result.monitor_commands.push(command.to_string());
						added += 1;
					}
					if added == 0 {
						return Err(
							"--monitor-commands requires at least one debugger command.".into()
						);
					}
					result.debugger = true;
				}
				_ if argument.starts_with('-') => {
					return Err(format!("Unknown option: {argument}"));
				}
				_ => assign_positional_media(&mut result, PathBuf::from(argument))?,
			}
			index += 1;
		}

		result.validate()?;
		Ok(result)
	}

	/* Validation keeps the builder free from contradictory startup states such as a disk image with the drive disabled or mutually exclusive warp modes. */
	fn validate(&self) -> Result<(), String> {
		if self.disk.is_some() && self.drive == Some(DriveSelection::Off) {
			return Err("--disk cannot be combined with --drive off.".into());
		}
		if self.warp_1541 && self.drive == Some(DriveSelection::Off) {
			return Err("--warp-1541 requires the 1541 drive.".into());
		}
		if self.startup_action != StartupAction::None && self.disk.is_none() {
			return Err(
				"--load-directory, --load-first and --load-first-run require a disk image.".into(),
			);
		}
		if self.warp && self.warp_1541 {
			return Err("--warp and --warp-1541 are mutually exclusive.".into());
		}
		if self.no_cartridge && self.cartridge.is_some() {
			return Err("--no-cartridge and --cartridge are mutually exclusive.".into());
		}
		if (self.freeze || self.cartridge_menu) && self.cartridge.is_none() {
			return Err("--freeze and --cartridge-menu require a cartridge.".into());
		}
		if let Some(path) = &self.wav {
			if path
				.extension()
				.and_then(|item| item.to_str())
				.map(str::to_ascii_lowercase)
				.as_deref()
				!= Some("wav")
			{
				return Err("--wav output must use .wav.".into());
			}
		}
		for path in [&self.disk, &self.tape, &self.cartridge, &self.prg]
			.into_iter()
			.flatten()
		{
			if !path.exists() {
				return Err(format!("File not found: {}", path.display()));
			}
		}
		Ok(())
	}

	pub fn help() -> &'static str {
		"Breadbin466 [MEDIA] [OPTIONS]\n\nMedia:\n  --disk FILE\n  --tape FILE\n  --cartridge FILE\n  --no-cartridge         Start without the cartridge saved in history\n  --prg FILE\n  --load-directory\n  --load-first\n  --load-first-run\n\nMachine:\n  --drive on|off\n  --8502\n  --reu\n  --debugger             Start paused with the terminal debugger\n  --monitor-command CMD  Execute one monitor command at startup; repeatable\n  --monitor-commands CMDS\n                         Execute a semicolon-separated monitor command sequence\n\nJoystick:\n  --joystick auto|keyboard|gilrs|none\n  --joystick-port 1|2\n\nExecution:\n  --warp\n  --warp-1541\n  --quit MINUTES         Exit after this many minutes of emulated C64 time\n  --freeze\n  --cartridge-menu\n\nDisplay:\n  --fullscreen\n  --inspector\n  --osd\n  --no-osd\n  --display-uptime      Display elapsed emulated C64 time in the OSD\n  --scale 1|2|3\n\nAudio:\n  --mute\n  --silent               Disable host audio output without disabling WAV capture\n  --wav FILE             Record the complete session as PCM WAV\n  --mute-warp\n  --no-mute-warp\n\nCommands:\n  create disk FILE\n  create tape FILE\n\nGeneral:\n  --help\n"
	}
}

fn value<'a>(args: &'a [String], index: &mut usize, option: &str) -> Result<&'a str, String> {
	*index += 1;
	args.get(*index)
		.map(String::as_str)
		.ok_or_else(|| format!("Missing value for {option}."))
}

fn path_value(args: &[String], index: &mut usize, option: &str) -> Result<PathBuf, String> {
	Ok(PathBuf::from(value(args, index, option)?))
}

fn parse_u64(value: &str, option: &str) -> Result<u64, String> {
	value
		.parse::<u64>()
		.map_err(|_| format!("{option} requires a non-negative integer."))
}

fn parse_u8(value: &str, option: &str) -> Result<u8, String> {
	value
		.parse()
		.map_err(|_| format!("Invalid numeric value for {option}: {value}"))
}

fn parse_drive(value: &str) -> Result<DriveSelection, String> {
	match value {
		"on" => Ok(DriveSelection::On),
		"off" => Ok(DriveSelection::Off),
		_ => Err("--drive accepts only on or off.".into()),
	}
}

fn parse_joystick(value: &str) -> Result<JoystickSelection, String> {
	match value {
		"auto" => Ok(JoystickSelection::Auto),
		"keyboard" => Ok(JoystickSelection::Keyboard),
		"gilrs" => Ok(JoystickSelection::Gilrs),
		"none" => Ok(JoystickSelection::None),
		_ => Err("--joystick accepts auto, keyboard, gilrs or none.".into()),
	}
}

fn set_startup_action(result: &mut CommandLine, action: StartupAction) -> Result<(), String> {
	if result.startup_action != StartupAction::None {
		return Err(
			"--load-directory, --load-first and --load-first-run are mutually exclusive.".into(),
		);
	}
	result.startup_action = action;
	Ok(())
}

/* Positional media is classified solely by extension and fills only an unassigned media slot, preserving deterministic precedence with explicit options. */
fn assign_positional_media(result: &mut CommandLine, path: PathBuf) -> Result<(), String> {
	let extension = extension(&path)?;
	match extension.as_str() {
		"d64" | "d7z" | "g64" | "nib" | "nbz" if result.disk.is_none() => result.disk = Some(path),
		"tap" if result.tape.is_none() => result.tape = Some(path),
		"crt" if result.cartridge.is_none() => result.cartridge = Some(path),
		"prg" if result.prg.is_none() => result.prg = Some(path),
		"d64" | "d7z" | "g64" | "nib" | "nbz" => {
			return Err("Only one disk image may be specified.".into());
		}
		"tap" => return Err("Only one tape image may be specified.".into()),
		"crt" => return Err("Only one cartridge may be specified.".into()),
		"prg" => return Err("Only one PRG may be specified.".into()),
		_ => return Err(format!("Unsupported media type: .{extension}")),
	}
	Ok(())
}

fn extension(path: &Path) -> Result<String, String> {
	path.extension()
		.and_then(|item| item.to_str())
		.map(str::to_ascii_lowercase)
		.ok_or_else(|| format!("Missing file extension: {}", path.display()))
}