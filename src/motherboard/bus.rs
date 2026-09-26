// =======================================================
// src/motherboard/bus.rs — Core Motherboard Bus and Chip Interconnection
// =======================================================

use crate::emulator::Result;
use crate::motherboard::constants::{CPU_FREQ_HZ, NEGATIVE_RANGE, POSITIVE_RANGE, THRESHOLD};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::cia::{Cia1, Cia2};
use crate::clockchip::constants::CYCLES_PER_FRAME;
use crate::clockchip::tick_cia;
use crate::cpu::bus::SystemBus;
use crate::cpu::{Cpu, CpuModel};
use crate::fdd1541::{DriveStatus, DriveWorker};
use crate::iec::IecBus;
use crate::memory::Memory;
use crate::sid::{AudioRateConverter, constants::CLOCK_FREQUENCY_HZ};
use crate::vic::VicII;
use serde::{Deserialize, Serialize};

use super::scheduler::Scheduler;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
pub enum DriveMode {
	#[default]
	Lle,
	Off,
}

/* CpuBus is the narrow adapter that lets the CPU issue bus cycles without owning the VIC-II or the complete motherboard. Memory performs PLA routing and device dispatch. */

/*
 * The debugger observes the final CPU bus transaction rather than instrumenting
 * individual memory devices. One completed access is retained per motherboard
 * cycle, which is sufficient for read/write watchpoints and does not turn normal
 * execution into an unbounded trace.
 */
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugBusAccessKind {
	Read,
	Write,
}

#[derive(Debug, Clone, Copy)]
pub struct DebugBusAccess {
	pub kind: DebugBusAccessKind,
	pub addr: u16,
	pub value: u8,
	pub cycle: u64,
}

pub struct CpuBus<'a, const CAPTURE_ACCESS: bool> {
	pub memory: &'a mut Memory,
	pub vic: &'a mut VicII,
	pub debug_access: &'a mut Option<DebugBusAccess>,
	drive_worker: &'a mut DriveWorker,
	iec: &'a IecBus,
	drive_enabled: bool,
	last_drive_device_state: &'a mut u32,
}

/* DMA pulls both RDY and AEC low. Internal write cycles still complete,
 * but the disconnected CPU cannot place their writes on the external bus. */
struct DmaDisconnectedBus;

impl SystemBus for DmaDisconnectedBus {
	fn address_enabled(&self) -> bool { false }
	fn read(&mut self, _addr: u16, _cycle: u64) -> u8 { 0xFF }
	fn write(&mut self, _addr: u16, _value: u8, _cycle: u64) {}
}

impl<const CAPTURE_ACCESS: bool> SystemBus for CpuBus<'_, CAPTURE_ACCESS> {
	fn address_enabled(&self) -> bool { !self.vic.aec_low }

	/* The 6510 does not drive its internal port value onto the data bus.
	 * DRAM instead captures the byte retained from the preceding VIC phase
	 * (CPU-PORT-RAM-MEASUREMENTS). */
	fn write_port(&mut self, addr: u16, cycle: u64) {
		let value = self.memory.bus_state.latched_value();
		self.write(addr, value, cycle);
	}

	#[inline(always)]
	fn read(&mut self, addr: u16, cycle: u64) -> u8 {
		if self.drive_enabled && addr & 0xFF0F == 0xDD00 {
			let (previous, current) = self.drive_worker.synchronise_input(cycle);
			let (clk, data) = self.iec.input_lines_for_device(previous);
			self.memory.cia2.update_iec_inputs(clk, data);
			self.iec.set_device_state(current, cycle);
			*self.last_drive_device_state = current;
		}
		let value = self.memory.cpu_read(addr, cycle, self.vic);
		if CAPTURE_ACCESS {
			*self.debug_access = Some(DebugBusAccess {
				kind: DebugBusAccessKind::Read,
				addr,
				value,
				cycle,
			});
		}
		value
	}

	#[inline(always)]
	fn write(&mut self, addr: u16, value: u8, cycle: u64) {
		self.memory.cpu_write(addr, value, cycle, self.vic);
		if CAPTURE_ACCESS {
			*self.debug_access = Some(DebugBusAccess {
				kind: DebugBusAccessKind::Write,
				addr,
				value,
				cycle,
			});
		}
	}
}

/* Motherboard owns the master-cycle narrative. Each cycle advances the VIC-II first, samples the resulting BA/IRQ state, updates the 6510 port-driven map, clocks both CIAs and the external drive, presents combined interrupt lines to the CPU, grants the bus to REU or CPU, and finally clocks audio. This order makes bus ownership and interrupt visibility explicit rather than relying on host-language call order by accident. */
pub struct Motherboard {
	pub cpu: Cpu,
	pub memory: Memory,
	pub vic: VicII,
	pub clock: Scheduler,
	pub cpu_clock_cycles: u64,
	pub iec: Rc<IecBus>,
	drive_worker: DriveWorker,
	drive_status: DriveStatus,
	last_drive_device_state: u32,
	drive_mode: DriveMode,
	audio_rate_converter: AudioRateConverter,
	audio_output_enabled: bool,
	sid_clocking_enabled: bool,
	pub audio_buffer_storage: Vec<f32>,
	/* Consumed after each motherboard cycle by the optional debugger. */
	debug_last_cpu_access: Option<DebugBusAccess>,
}

/*
 * BA is the cartridge-port arbitration input sampled by both the processor and
 * the REC.  The VIC-II already resolves its internal AEC timing before this
 * value is published. REU reads also retain the PHI1 arbitration state.
 */
#[derive(Clone, Copy)]
struct BusAvailability {
	ba_high: bool,
}

impl Motherboard {
	/* Construction wires the two CIAs, VIC-II, 6510, shared IEC cable, memory router and independently running 1541 into one cycle domain. */
	pub fn new(audio_sample_rate: f32, active_crt: Option<PathBuf>) -> Self {
		let iec = Rc::new(IecBus::new());
		let cia1 = Cia1::new();
		let mut cia2 = Cia2::new();
		cia2.attach_iec_bus(Rc::clone(&iec));
		let vic = VicII::new();
		let memory = Memory::new(cia1, cia2, Rc::clone(&iec), active_crt);
		let drive_worker = DriveWorker::new(CPU_FREQ_HZ);

		let mut motherboard = Self {
			cpu: Cpu::new(CpuModel::Mos6510),
			memory,
			vic,
			clock: Scheduler::new(),
			cpu_clock_cycles: 0,
			iec,
			drive_worker,
			drive_status: DriveStatus::default(),
			last_drive_device_state: 0,
			drive_mode: DriveMode::Lle,
			audio_rate_converter: AudioRateConverter::new(
				CLOCK_FREQUENCY_HZ,
				f64::from(audio_sample_rate.round().clamp(8_000.0, 192_000.0) as u32),
			),
			audio_output_enabled: true,
			sid_clocking_enabled: true,
			audio_buffer_storage: Vec::with_capacity(
				((audio_sample_rate.max(8_000.0) as usize + 49) / 50) + 64,
			),
			debug_last_cpu_access: None,
		};
		motherboard.drive_status = motherboard.drive_worker.reset();
		motherboard
	}

	pub fn current_cycle(&self) -> u64 {
		self.clock.total_cycles
	}

	/* The 1351 is an external control-port peripheral. Desktop code can attach it
	 * and feed relative motion without acquiring direct access to SID or CIA state. */
	pub fn set_mouse_1351_connected(&mut self, connected: bool) {
		let cycle = self.clock.total_cycles;
		let port_a_pins = self.memory.cia1.port_a_pin_levels();
		self.memory
			.mouse1351
			.set_connected(connected, cycle, port_a_pins);
		if !connected {
			self.memory.sid.pot_x = 0xFF;
			self.memory.sid.pot_y = 0xFF;
		}
	}

	pub fn mouse_1351_connected(&self) -> bool {
		self.memory.mouse1351.is_connected()
	}

	pub fn move_mouse_1351(&mut self, dx: i32, dy: i32) {
		self.memory.mouse1351.move_relative(dx, dy);
	}

	pub fn set_mouse_1351_left_button(&mut self, pressed: bool) {
		self.memory.mouse1351.set_left_pressed(pressed);
	}

	pub fn set_mouse_1351_right_button(&mut self, pressed: bool) {
		self.memory.mouse1351.set_right_pressed(pressed);
	}

	pub fn release_mouse_1351_buttons(&mut self) {
		self.memory.mouse1351.release_buttons();
	}

	pub fn mouse_1351_digital_mask(&self) -> u8 {
		self.memory.mouse1351.digital_port_mask()
	}

	pub fn init_roms(&mut self) -> Result<()> {
		self.memory.load_system_roms()?;
		Ok(())
	}

	pub fn load_custom_char_rom(&mut self, path: &Path) -> Result<()> {
		self.memory.rom.load_custom_char_rom(path)
	}

	pub fn load_custom_basic_rom(&mut self, path: &Path) -> Result<()> {
		self.memory.rom.load_custom_basic_rom(path)
	}

	pub fn load_custom_kernal_rom(&mut self, path: &Path) -> Result<()> {
		self.memory.rom.load_custom_kernal_rom(path)
	}

	pub fn set_drive_warp_execution(&mut self, _enabled: bool) {}

	pub fn set_paused(&self, paused: bool) {
		self.drive_worker.set_paused(paused);
	}

	pub fn refresh_drive_status(&mut self) {
		if self.drive_mode == DriveMode::Off {
			return;
		}

		self.drive_status = self.drive_worker.status();
	}

	pub fn load_custom_drive_rom(&mut self, path: &Path) -> Result<()> {
		let (result, status) = self.drive_worker.load_custom_dos_rom(path.to_path_buf());
		self.drive_status = status;
		result.map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error).into())
	}

	pub fn reset_char_rom(&mut self) {
		self.memory.rom.reset_char_rom();
	}

	pub fn reset_basic_rom(&mut self) {
		self.memory.rom.reset_basic_rom();
	}

	pub fn reset_kernal_rom(&mut self) {
		self.memory.rom.reset_kernal_rom();
	}

	pub fn reset_drive_rom(&mut self) {
		self.drive_status = self.drive_worker.reset_dos_rom();
	}

	/* A hard reset recreates volatile memory state, resets every motherboard device and drive, restarts the scheduler, then lets the CPU perform its hardware reset-vector sequence through the normal bus. */
	pub fn reset(&mut self) {
		let completed_cycle = self.clock.total_cycles;
		self.memory.reset(true);
		self.memory.cia1.reset();
		self.memory.cia2.reset();
		self.memory
			.mouse1351
			.reset_bus_selection(completed_cycle, self.memory.cia1.port_a_pin_levels());
		self.vic.reset();
		self.iec.reset();
		self.last_drive_device_state = 0;
		let host_state = self.iec.host_state();
		self.drive_status = self.drive_worker.hard_reset(completed_cycle, host_state);
		self.clock.reset();
		self.cpu_clock_cycles = 0;
		self.iec
			.set_device_connected(self.drive_mode != DriveMode::Off, 0);
		self.audio_rate_converter.reset();
		{
			let cpu = &mut self.cpu;
			let mut bus = CpuBus::<false> {
				memory: &mut self.memory,
				vic: &mut self.vic,
				debug_access: &mut self.debug_last_cpu_access,
				drive_worker: &mut self.drive_worker,
				iec: &self.iec,
				drive_enabled: self.drive_mode != DriveMode::Off,
				last_drive_device_state: &mut self.last_drive_device_state,
			};
			cpu.reset(&mut bus);
		}
		self.audio_buffer_storage.clear();
	}

	/* A soft reset preserves RAM and cartridge state but resets the CIAs and asserts the CPU RESET input, matching a reset-button style restart rather than power cycling the board. */
	pub fn soft_reset(&mut self) {
		self.memory.reset(false);
		self.memory.cia1.reset();
		self.memory.cia2.reset();
		let cycle = self.clock.total_cycles;
		self.memory
			.mouse1351
			.reset_bus_selection(cycle, self.memory.cia1.port_a_pin_levels());
		self.cpu.assert_reset();
		self.audio_rate_converter.reset();
		self.audio_buffer_storage.clear();
	}

	/* Changing drive mode updates both the electrical presence of device 8 on the IEC bus and the worker connection state. Resetting the worker prevents stale mechanical or serial state from leaking across a disconnect/reconnect boundary. */
	pub fn set_drive_mode(&mut self, mode: DriveMode) {
		if self.drive_mode == mode {
			return;
		}

		self.drive_mode = mode;
		self.iec
			.set_device_connected(mode != DriveMode::Off, self.clock.total_cycles);
		self.last_drive_device_state = 0;
		self.drive_worker.set_connected(mode != DriveMode::Off);
		self.drive_status = self.drive_worker.reset();
	}

	/* The 1541 runs independently behind DriveWorker. Each motherboard cycle publishes the current C64-side IEC levels to the worker, then mirrors back a device state only when the drive has changed an output line. This preserves the cable as an electrical boundary rather than making either computer call into the other directly. */
	#[inline(always)]
	fn drive_cycle(&mut self, cycle: u64) {
		if self.drive_mode == DriveMode::Off {
			return;
		}

		let host_state = self.iec.host_state();
		let device_state = self.drive_worker.tick_host(cycle, host_state);
		if device_state != self.last_drive_device_state {
			self.iec.set_device_state(device_state, cycle);
			self.last_drive_device_state = device_state;
		}
	}

	pub fn get_drive_mode(&self) -> DriveMode {
		self.drive_mode
	}

	/* Mounting media is delegated to the drive computer so its mechanics, write protection and current status remain owned by the 1541 subsystem. The returned status snapshot is cached for UI and telemetry queries. */
	pub fn mount_drive(&mut self, path: &Path) -> bool {
		let (mounted, status) = self.drive_worker.mount(path.to_path_buf());
		self.drive_status = status;
		mounted
	}

	/* Unmounting ejects the medium inside the drive worker without disconnecting the IEC device itself. */
	pub fn unmount_drive(&mut self) -> bool {
		let (unmounted, status) = self.drive_worker.unmount();
		self.drive_status = status;
		unmounted
	}

	/* Application teardown can request one explicit media flush while the motherboard is still alive, making a final persistence failure observable before DriveWorker performs its own destruction-time retry. */
	pub fn flush_drive_media(&mut self) -> bool {
		self.drive_worker.flush_media()
	}

	pub fn drive_busy_led(&self) -> bool {
		self.drive_mode != DriveMode::Off && self.drive_status.busy_led
	}

	pub fn drive_power_led(&self) -> bool {
		self.drive_mode != DriveMode::Off && self.drive_status.power_led
	}

	pub fn drive_current_track(&self) -> Option<u8> {
		(self.drive_mode != DriveMode::Off)
			.then_some(self.drive_status.current_track)
			.flatten()
	}

	pub fn drive_pc(&self) -> Option<u16> {
		(self.drive_mode != DriveMode::Off).then_some(self.drive_status.drive_pc)
	}

	pub fn drive_error_string(&self) -> &str {
		if self.drive_mode == DriveMode::Off {
			""
		} else {
			&self.drive_status.error_string
		}
	}

	pub fn load_cartridge(&mut self, path: &Path) -> Result<()> {
		self.memory.mount_cartridge(path)
	}

	/* PRG injection copies the payload directly into physical RAM. BASIC-format programmes loaded at $0801 also refresh the interpreter pointers that LOAD would normally update, while machine-code programmes leave those workspace values untouched. */
	pub fn inject_prg(&mut self, data: &[u8]) -> Result<()> {
		if data.len() < 3 {
			return Err("PRG file too short (< 3 bytes)".into());
		}
		let load_addr = u16::from_le_bytes([data[0], data[1]]);
		let content = &data[2..];
		let end_exclusive = load_addr as usize + content.len();
		if end_exclusive > 0x1_0000 {
			return Err("PRG exceeds the 64 KiB address space".into());
		}
		let end_addr = end_exclusive as u16;
		/* Host PRGs use the virtual disk-load convention. Set FA before
		 * copying, as SETLFS does before LOAD, so a payload covering zero
		 * page can still replace it. Subsequent loaders use FA ($BA) to
		 * address the same serial device. */
		self.memory.ram.write(0xBA, 8);
		let mut offset = 0usize;
		while offset < content.len() {
			self.memory
				.ram
				.write(load_addr.wrapping_add(offset as u16), content[offset]);
			offset += 1;
		}
		if load_addr == 0x0801 {
			/* LOAD leaves EAL/EAH pointing immediately beyond the loaded BASIC text. */
			self.memory.ram.write(0xAE, end_addr as u8);
			self.memory.ram.write(0xAF, (end_addr >> 8) as u8);
			self.memory.ram.write(0x2B, 0x01);
			self.memory.ram.write(0x2C, 0x08);
			self.memory.ram.write(0x2D, end_addr as u8);
			self.memory.ram.write(0x2E, (end_addr >> 8) as u8);
			self.memory.ram.write(0x2F, end_addr as u8);
			self.memory.ram.write(0x30, (end_addr >> 8) as u8);
			self.memory.ram.write(0x31, end_addr as u8);
			self.memory.ram.write(0x32, (end_addr >> 8) as u8);
		}
		Ok(())
	}

	/* Normal-speed muting discards host samples only, preserving the SID
	 * and converter history. Silent warp controls clocking separately. */
	pub fn set_audio_rendering(&mut self, enabled: bool) {
		self.audio_output_enabled = enabled;
	}

	/* Silent warp deliberately suspends SID work. On returning to clocked
	 * audio, discard converter history from before that accelerated interval. */
	pub fn set_sid_clocking(&mut self, enabled: bool) {
		if enabled && !self.sid_clocking_enabled {
			self.audio_rate_converter.reset();
		}
		self.sid_clocking_enabled = enabled;
	}

	pub fn set_video_composition(&mut self, enabled: bool) {
		self.vic.screen.set_compose_video(enabled);
	}

	pub fn audio_buffer(&self) -> &[f32] {
		&self.audio_buffer_storage
	}

	/* This is the frame-level service entry point. It clears the previous audio batch, advances exactly one PAL frame through tick_cycle(), then snapshots drive and video telemetry after every device has reached the same frame boundary. */
	pub fn tick_frame(&mut self) {
		self.vic.telemetry.begin_frame();
		self.audio_buffer_storage.clear();
		for _ in 0..CYCLES_PER_FRAME {
			self.tick_cycle();
		}
		let ctrl1 = self.vic.regs.ctrl1;
		let sprite_en = self.vic.regs.sprite_en;
		let current_pc = self.cpu.pc;
		let master_cycles = self.cpu_clock_cycles;
		self.refresh_drive_status();
		let dpc = self.drive_pc();
		self.vic
			.telemetry
			.update_report(ctrl1, sprite_en, current_pc, master_cycles, dpc);
	}

	#[inline(always)]
	fn render_sid_cycle(&mut self) {
		if !self.sid_clocking_enabled {
			return;
		}
		let raw_sample = self.memory.tick_sid();
		if let Some(sample) = self.audio_rate_converter.accept_cycle_sample(raw_sample) {
			if self.audio_output_enabled {
				self.audio_buffer_storage.push(soft_clip_audio(sample));
			}
		}
	}

	/* tick_base advances every device that can influence bus ownership or interrupt
	inputs before either the CPU or the REU is allowed to use the current cycle. */
	#[inline(always)]
	fn tick_base<const REU_ENABLED: bool>(&mut self) -> BusAvailability {
		let cycle = self.clock.total_cycles.wrapping_add(1);
		self.clock.total_cycles = cycle;
		self.cpu_clock_cycles = self.cpu_clock_cycles.wrapping_add(1);
		if self.memory.cartridge.tick_if_needed(cycle) {
			self.memory.mark_memory_map_dirty();
		}

		let pins = self.cpu.port.get_pins();

		let (_, iec_clk, iec_data, iec_srq) = self.iec.lines();
		self.memory.cia2.update_iec_inputs(iec_clk, iec_data);
		let tod_pulse = self.clock.advance_tod();
		/* Resolve the preceding CPU write on the shared PB4/LP wire before the next VIC cycle. */
		self.vic.set_light_pen_pin(self.memory.cia1.light_pen_pin_high());
		self.vic.tick_sequencer(&mut self.memory, cycle);
		let availability = BusAvailability {
			ba_high: !self.vic.ba_low,
		};
		let vic_irq = self.vic.is_irq_active();
		self.memory.update_cpu_port_pins(pins);

		let (cia_irq, cia_nmi) = tick_cia::run_cia_cycle(&mut self.memory, tod_pulse, iec_srq);
		self.drive_cycle(cycle);
		/* DMA can read CIA 2 without passing through CpuBus. Keep its IEC
		input boundary just as strict as a processor read. */
		if REU_ENABLED && self.memory.reu.dma.is_some() && self.drive_mode != DriveMode::Off {
			let (previous, current) = self.drive_worker.synchronise_input(cycle);
			let (clk, data) = self.iec.input_lines_for_device(previous);
			self.memory.cia2.update_iec_inputs(clk, data);
			self.iec.set_device_state(current, cycle);
			self.last_drive_device_state = current;
		}
		let irq_active = vic_irq || cia_irq || (REU_ENABLED && self.memory.reu.irq_pending);
		if irq_active && !self.cpu.irq_line {
			self.vic.telemetry.irq_edge_count = self.vic.telemetry.irq_edge_count.wrapping_add(1);
		}
		self.cpu.set_irq_line(irq_active);
		self.cpu
			.set_nmi_line(!(cia_nmi || self.memory.cartridge.nmi_low));
		availability
	}

	/*
	 * An active REU keeps the processor stopped for the complete DMA command.
	 * The REC arbitrates each read or write phase against the VIC-II request.
	 * A paused transfer retains DMA ownership and never lets the processor run
	 * in the gap.
	 */
	/*
	 * Access capture is selected as a const generic so the normal execution path
	 * contains no debugger bookkeeping at all.  The compiler removes both the
	 * conditional and the transaction construction when CAPTURE_ACCESS is false,
	 * which preserves warp throughput while retaining cycle-accurate watchpoints
	 * whenever the interactive debugger owns execution.
	 */
	/*
	 * REU participation is selected before entering the frame loop.  When the
	 * expansion is disabled the REU_ENABLED=false instantiation contains no DMA
	 * arbitration call and no REU IRQ test.  This keeps an optional peripheral out
	 * of the production hot path rather than paying for its absence once per C64
	 * cycle.
	 */
	#[inline(always)]
	fn tick_cycle_with_access_capture<const CAPTURE_ACCESS: bool, const REU_ENABLED: bool>(
		&mut self,
	) {
		let availability = self.tick_base::<REU_ENABLED>();
		let cycle = self.clock.total_cycles;
		let reu_owns_cycle = REU_ENABLED
			&& self
				.memory
				.run_reu_cycle(availability.ba_high, cycle, &mut self.vic);
		if reu_owns_cycle {
			self.cpu.tick(&mut DmaDisconnectedBus, false);
		} else {
			let cpu_ready = if REU_ENABLED { self.memory.reu.cpu_ready_after_dma(availability.ba_high) } else { availability.ba_high };
			let cpu = &mut self.cpu;
			let mut bus = CpuBus::<CAPTURE_ACCESS> {
				memory: &mut self.memory,
				vic: &mut self.vic,
				debug_access: &mut self.debug_last_cpu_access,
				drive_worker: &mut self.drive_worker,
				iec: &self.iec,
				drive_enabled: self.drive_mode != DriveMode::Off,
				last_drive_device_state: &mut self.last_drive_device_state,
			};
			cpu.tick(&mut bus, cpu_ready);
		}
		self.vic.complete_character_access(self.memory.bus_state.latched_value());
		self.render_sid_cycle();
	}

	#[inline(always)]
	pub fn tick_cycle(&mut self) {
		self.tick_cycle_with_access_capture::<false, false>();
	}

	#[inline(always)]
	pub fn tick_cycle_reu(&mut self) {
		self.tick_cycle_with_access_capture::<false, true>();
	}

	#[inline(always)]
	pub fn tick_cycle_debugger(&mut self) {
		self.tick_cycle_with_access_capture::<true, false>();
	}

	#[inline(always)]
	pub fn tick_cycle_debugger_reu(&mut self) {
		self.tick_cycle_with_access_capture::<true, true>();
	}

	#[cold]
	fn tick_cycle_c128_debug_with_access_capture<
		const CAPTURE_ACCESS: bool,
		const REU_ENABLED: bool,
	>(
		&mut self,
	) {
		let availability = self.tick_base::<REU_ENABLED>();
		let allow_2mhz = self.memory.c128_8502_control & 1 != 0 && self.vic.c128_2mhz_allowed();
		if allow_2mhz {
			self.cpu_clock_cycles = self.cpu_clock_cycles.wrapping_add(1);
		}
		let reu_owns_cycle = REU_ENABLED
			&& self.memory.run_reu_cycle(
				availability.ba_high,
				self.clock.total_cycles,
				&mut self.vic,
			);
		if reu_owns_cycle {
			self.cpu.tick(&mut DmaDisconnectedBus, false);
		} else {
			let cpu_ready = if REU_ENABLED { self.memory.reu.cpu_ready_after_dma(availability.ba_high) } else { availability.ba_high };
			let cpu = &mut self.cpu;
			let mut bus = CpuBus::<CAPTURE_ACCESS> {
				memory: &mut self.memory,
				vic: &mut self.vic,
				debug_access: &mut self.debug_last_cpu_access,
				drive_worker: &mut self.drive_worker,
				iec: &self.iec,
				drive_enabled: self.drive_mode != DriveMode::Off,
				last_drive_device_state: &mut self.last_drive_device_state,
			};
			cpu.tick(&mut bus, cpu_ready);
			if allow_2mhz {
				let pins = cpu.port.get_pins();
				bus.memory.update_cpu_port_pins(pins);
				cpu.tick(&mut bus, true);
			}
		}
		self.vic.complete_character_access(self.memory.bus_state.latched_value());
		self.render_sid_cycle();
	}

	#[cold]
	pub fn tick_cycle_c128_debug(&mut self) {
		self.tick_cycle_c128_debug_with_access_capture::<false, false>();
	}

	#[cold]
	pub fn tick_cycle_c128_debug_reu(&mut self) {
		self.tick_cycle_c128_debug_with_access_capture::<false, true>();
	}

	#[cold]
	pub fn tick_cycle_c128_debugger(&mut self) {
		self.tick_cycle_c128_debug_with_access_capture::<true, false>();
	}

	#[cold]
	pub fn tick_cycle_c128_debugger_reu(&mut self) {
		self.tick_cycle_c128_debug_with_access_capture::<true, true>();
	}

	/* Taking, rather than copying indefinitely, gives every bus access one clear
	 * observation point and prevents a stale transaction from retriggering a
	 * watchpoint on a later cycle in which the CPU did not own the bus. */
	pub fn take_debug_bus_access(&mut self) -> Option<DebugBusAccess> {
		self.debug_last_cpu_access.take()
	}

	pub fn set_c128_debug_enabled(&mut self, enabled: bool) {
		self.memory.c128_2mhz_debug_enabled = enabled;
		self.memory.c128_8502_control = 0;
		self.cpu.model = if enabled {
			CpuModel::Mos8502
		} else {
			CpuModel::Mos6510
		};
	}
}
/* The motherboard limiter is a final host-domain safety margin, not a gain stage. Samples below the knee pass unchanged; only exceptional excursions are compressed smoothly towards the signed PCM limits. */
fn soft_clip_audio(sample: i32) -> f32 {
	let sample = i64::from(sample);
	let range = if sample < 0 {
		NEGATIVE_RANGE
	} else {
		POSITIVE_RANGE
	};
	let magnitude = sample.unsigned_abs() as f64;
	let limited = if magnitude < THRESHOLD as f64 {
		magnitude
	} else {
		let threshold_ratio = THRESHOLD as f64 / range;
		let remaining_ratio = 1.0 - threshold_ratio;
		let normalised = (magnitude - THRESHOLD as f64) / range;
		THRESHOLD as f64 + remaining_ratio * (normalised / remaining_ratio).tanh() * range
	};

	let signed = if sample < 0 { -limited } else { limited };
	(signed / range) as f32
}