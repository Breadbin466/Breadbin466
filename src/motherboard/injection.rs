// =======================================================
// src/motherboard/injection.rs — Hardware Injections and Line Switches
// =======================================================

use super::bus::Motherboard;
use super::constants::{READY_PATTERN, READY_POSITIONS};
use std::collections::VecDeque;

#[derive(Debug, PartialEq, Eq)]
enum InjectionState {
	/* No programme is queued. */
	Idle,
	/* A programme is queued while the KERNAL/BASIC startup is incomplete. */
	Waiting,
	/* The machine is considered stable enough for direct RAM injection. */
	Ready,
	/* The queued injection has been consumed. */
	Done,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoadFirstRunState {
	/* No LOAD-first autorun sequence is active. */
	Idle,
	/* The LOAD command is still being fed through the KERNAL keyboard buffer. */
	WaitingForLoadCommand,
	/* LOAD has been submitted; wait for a newly appearing READY. prompt. */
	WaitingForNewReady,
	/* RUN has been scheduled and the sequence is complete. */
	Done,
}

/* ActionManager models host-assisted input as a small frame-driven state machine rather than mutating the emulated machine at arbitrary host times. Text is inserted through the KERNAL keyboard buffer, direct PRG injection waits for a stable post-boot point, and LOAD-first autorun records the existing READY. positions before waiting for a new prompt. The sequence is therefore idle -> waiting -> ready -> done, with READY. edge detection preventing an old prompt from being mistaken for completion of the new load. */
pub struct ActionManager {
	boot_frames: u64,
	inject_state: InjectionState,
	pending_prg: Option<Vec<u8>>,
	pending_autorun: bool,
	type_buffer: VecDeque<u8>,
	type_not_before_frame: u64,
	load_first_run_state: LoadFirstRunState,
	ready_baseline: [bool; READY_POSITIONS],
}

impl ActionManager {
	/* A fresh manager has no pending host action and begins counting frames from the current machine boot. */
	pub fn new() -> Self {
		Self {
			boot_frames: 0,
			inject_state: InjectionState::Idle,
			pending_prg: None,
			pending_autorun: false,
			type_buffer: VecDeque::new(),
			type_not_before_frame: 0,
			load_first_run_state: LoadFirstRunState::Idle,
			ready_baseline: [false; READY_POSITIONS],
		}
	}

	/* Resetting host actions discards pending typing and detection state without altering the emulated motherboard itself. */
	pub fn reset_state(&mut self) {
		self.boot_frames = 0;
		self.inject_state = InjectionState::Idle;
		self.type_buffer.clear();
		self.type_not_before_frame = 0;
		self.load_first_run_state = LoadFirstRunState::Idle;
		self.ready_baseline.fill(false);
	}

	/* Direct injection waits for both a conservative frame threshold and a
	 * visible BASIC READY. prompt. Host speed therefore cannot make injection
	 * race the emulated KERNAL startup. */
	pub fn schedule_injection(&mut self, data: Vec<u8>, autorun: bool) {
		println!("Scheduling PRG injection...");
		self.pending_prg = Some(data);
		self.pending_autorun = autorun;

		self.inject_state = InjectionState::Waiting;
	}

	/* Normal text entry becomes eligible on the current frame and is drained through the ten-byte KERNAL keyboard queue. */
	pub fn schedule_text_entry(&mut self, text: &str) {
		self.schedule_text_entry_at_frame(text, self.boot_frames);
	}

	/* Startup text may be queued immediately but remains blocked until the requested minimum boot frame. */
	pub fn schedule_startup_text_entry(&mut self, text: &str, minimum_boot_frames: u64) {
		self.schedule_text_entry_at_frame(text, minimum_boot_frames);
	}

	/* LOAD-first autorun types LOAD"*",8,1, captures all READY. matches once that command has left the queue, then waits for a new match before typing RUN. */
	pub fn schedule_startup_load_first_run(&mut self, minimum_boot_frames: u64) {
		self.schedule_text_entry_at_frame("LOAD\"*\",8,1\r", minimum_boot_frames);
		self.load_first_run_state = LoadFirstRunState::WaitingForLoadCommand;
		self.ready_baseline.fill(false);
	}

	fn schedule_text_entry_at_frame(&mut self, text: &str, not_before_frame: u64) {
		println!("Scheduling command: {}", text.trim());
		self.type_not_before_frame = self.type_not_before_frame.max(not_before_frame);
		for byte in text.bytes() {
			self.type_buffer.push_back(byte);
		}
	}

	/* One call represents one completed video frame. It services at most one keyboard byte when space is available, advances READY. detection, and commits a deferred PRG injection once the boot gate opens. */
	pub fn tick(&mut self, machine: &mut Motherboard) {
		self.boot_frames += 1;

		if self.boot_frames >= self.type_not_before_frame && !self.type_buffer.is_empty() {
			let ndx = machine.memory.read_ram(0x00C6);
			if ndx < 10 {
				if let Some(char_code) = self.type_buffer.pop_front() {
					let target_addr = 0x0277 + (ndx as u16);
					machine.memory.ram.write(target_addr, char_code);
					machine.memory.ram.write(0x00C6, ndx + 1);
				}
			}
		}

		self.tick_load_first_run(machine);

		match self.inject_state {
			InjectionState::Waiting => {
				if self.boot_frames > 120 && Self::machine_ready(machine) {
					self.inject_state = InjectionState::Ready;
				}
			}
			InjectionState::Ready => {
				if let Some(data) = self.pending_prg.take() {
					println!("Injecting PRG into RAM...");
					match machine.inject_prg(&data) {
						Ok(()) => {
							if self.pending_autorun {
								self.perform_autorun(machine);
								println!("Auto-Run: Typed 'RUN' into keyboard buffer.");
							}
						}
						Err(error) => eprintln!("PRG injection failed: {error}"),
					}
				}
				self.inject_state = InjectionState::Done;
				self.pending_autorun = false;
			}
			_ => {}
		}
	}

	/* BASIC is ready either when its prompt is visible or when the CPU is in
	 * the KERNAL keyboard-wait loop used by the PAL 901227-03 ROM. Combining
	 * both observations makes direct injection independent of VIC visibility. */
	fn machine_ready(machine: &Motherboard) -> bool {
		(0xE5CF..=0xE5D4).contains(&machine.cpu.pc)
			|| Self::ready_positions(machine).iter().any(|ready| *ready)
	}

	/* The baseline is captured only after the LOAD command has fully entered the keyboard buffer. A later false-to-true READY. transition then identifies completion of that load rather than the prompt that existed before it. */
	fn tick_load_first_run(&mut self, machine: &mut Motherboard) {
		match self.load_first_run_state {
			LoadFirstRunState::WaitingForLoadCommand if self.type_buffer.is_empty() => {
				self.ready_baseline = Self::ready_positions(machine);
				self.load_first_run_state = LoadFirstRunState::WaitingForNewReady;
			}
			LoadFirstRunState::WaitingForNewReady => {
				let current = Self::ready_positions(machine);
				let new_ready = current
					.iter()
					.zip(self.ready_baseline.iter())
					.any(|(now, before)| *now && !*before);
				if new_ready {
					self.schedule_text_entry("RUN\r");
					self.load_first_run_state = LoadFirstRunState::Done;
					println!("Auto-Run: New READY. detected; scheduling RUN.");
				}
			}
			_ => {}
		}
	}

	/* READY. is searched in the VIC-visible screen matrix, not at a fixed CPU address, so the detector follows the active VIC bank and screen-memory base. */
	fn ready_positions(machine: &Motherboard) -> [bool; READY_POSITIONS] {
		let mut result = [false; READY_POSITIONS];
		let bank_base = (machine.memory.get_vic_bank() as u16) << 14;
		let screen_base = bank_base.wrapping_add(machine.vic.regs.vm_base());

		for offset in 0..READY_POSITIONS {
			let matches = READY_PATTERN.iter().enumerate().all(|(index, expected)| {
				machine
					.memory
					.read_ram(screen_base.wrapping_add((offset + index) as u16))
					== *expected
			});
			result[offset] = matches;
		}
		result
	}

	/* Direct autorun reproduces a four-character RUN plus RETURN entry in the KERNAL keyboard buffer. */
	fn perform_autorun(&self, machine: &mut Motherboard) {
		machine.memory.ram.write(0x00C6, 4);
		machine.memory.ram.write(0x0277, b'R');
		machine.memory.ram.write(0x0278, b'U');
		machine.memory.ram.write(0x0279, b'N');
		machine.memory.ram.write(0x027A, 0x0D);
	}
}