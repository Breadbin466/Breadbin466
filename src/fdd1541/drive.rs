// =======================================================
// src/fdd1541/drive.rs — Fdd1541 CPU, memory, VIA and mechanism integration
// =======================================================

use super::constants::DRIVE_CPU_FREQ_HZ;
use std::path::Path;

use super::computer::DriveBus;
use super::iec::DriveIecBus;
use crate::cpu::bus::SystemBus;
use crate::cpu::{Cpu, CpuModel};

/* The drive is modelled as an independent computer. Host cycles are converted to 1541 clock opportunities, while all communication with the C64 crosses only the resolved IEC line state. */
pub struct Fdd1541 {
	cpu: Cpu,
	bus: DriveBus,
	cycle: u64,
	irq_line: bool,
	clock_acc: u32,
	host_clock_hz: u32,
	last_host_state: u32,
	connected: bool,
}

impl Fdd1541 {
	/* Construction fixes the drive to an NMOS 6502 and a local clock domain while leaving the IEC lines released until the host publishes their state. */
	pub fn new(host_clock_hz: u32) -> Self {
		Self {
			cpu: Cpu::new(CpuModel::Mos6502),
			bus: DriveBus::new(),
			cycle: 0,
			irq_line: false,
			clock_acc: 0,
			host_clock_hz,
			last_host_state: 0,
			connected: true,
		}
	}

	/* A drive reset restarts the local computer without inventing a cable transition on the host side. */
	pub fn reset(&mut self) {
		self.reset_with_iec_state(self.last_host_state, self.connected);
	}

	/* A reset preserves the externally visible IEC environment supplied by the host so the restarted drive observes the same cable state as the real hardware. */
	pub fn reset_with_iec_state(&mut self, host_state: u32, connected: bool) {
		self.last_host_state = host_state;
		self.connected = connected;
		self.bus.reset();
		self.bus.set_device_connected(connected, self.cycle);
		self.bus.set_host_iec_state(host_state, self.cycle);
		self.cpu.reset(&mut self.bus);
		self.irq_line = false;
	}

	pub fn iec(&self) -> &DriveIecBus {
		self.bus.iec()
	}

	#[inline(always)]
	pub fn device_iec_state(&self) -> u32 {
		self.bus.iec().device_state()
	}

	/* Replacing DOS ROM changes only the firmware image; CPU and mechanism state remain untouched until an explicit reset. */
	pub fn load_custom_dos_rom(&mut self, path: &Path) -> crate::emulator::Result<()> {
		self.bus.load_custom_dos_rom(path)
	}

	pub fn reset_dos_rom(&mut self) {
		self.bus.reset_dos_rom();
	}

	/* Mounting delegates media interpretation to the mechanism and preserves the current head and spindle state. */
	pub fn mount(&mut self, path: &Path) -> bool {
		self.bus.mount(path)
	}

	pub fn unmount(&mut self) -> bool {
		self.bus.unmount()
	}

	pub fn flush_now(&mut self) -> bool {
		self.bus.flush_now()
	}

	#[inline(always)]
	/* One native drive cycle clocks the 6502, both VIAs and the disk mechanism against the same local timebase before publishing the resulting IEC outputs. */
	fn step_once(&mut self) {
		self.cycle = self.cycle.wrapping_add(1);
		if self.bus.flush_pending() {
			self.bus.service_flush();
		}

		self.cpu.set_irq_line(self.irq_line);
		self.cpu.set_so_line(self.bus.so_line());
		self.cpu.tick(&mut self.bus, true);

		self.irq_line = self.bus.service();
	}

	#[inline(always)]
	/* Fractional clock accumulation converts the host clock into the 1541 clock without periodically dropping or duplicating drive cycles. */
	fn step_host_cycle(&mut self) {
		self.clock_acc += DRIVE_CPU_FREQ_HZ - self.host_clock_hz;
		self.step_once();
		if self.clock_acc >= self.host_clock_hz {
			self.clock_acc -= self.host_clock_hz;
			self.step_once();
		}
	}

	#[inline(always)]
	/* The returned packed state is the only drive-to-host electrical result of this host cycle; internal drive state remains private. */
	pub fn run_stable_host_cycle(&mut self) -> u32 {
		self.step_host_cycle();
		self.bus.iec().device_state()
	}

	#[inline(always)]
	/* A committed host cycle applies changed IEC inputs before the local 1541 clock advances, making the new line levels visible at the correct scheduling boundary. */
	pub fn run_committed_host_cycle(&mut self, host_state: u32, connected: bool) -> u32 {
		let cycle = self.cycle;
		if connected != self.connected {
			self.bus.set_device_connected(connected, cycle);
			self.connected = connected;
		}
		if host_state != self.last_host_state {
			self.bus.set_host_iec_state(host_state, cycle);
			self.last_host_state = host_state;
		}
		self.run_stable_host_cycle()
	}

	pub fn peek_ram(&mut self, addr: u16) -> u8 {
		self.bus.read(addr & 0x07FF, self.cycle)
	}

	pub fn drive_pc(&self) -> u16 {
		self.cpu.pc
	}

	pub fn power_led(&self) -> bool {
		self.bus.power_led()
	}

	pub fn busy_led(&self) -> bool {
		self.bus.busy_led()
	}

	pub fn is_dirty(&self) -> bool {
		self.bus.is_dirty()
	}

	pub fn current_track(&self) -> Option<u8> {
		self.bus.current_track()
	}

	/* The DOS error channel text is read from the drive RAM workspace used by the resident firmware, so the result reflects the actual DOS state rather than a host-side status translation. */
	pub fn get_error_string(&mut self) -> String {
		let start_addr = 0x02D5u16;

		let end_low_byte = self.peek_ram(0x0249);

		let mut length = (end_low_byte as usize).wrapping_sub(0xD5) & 0xFF;

		if length > 40 {
			length = 0;
		}

		let mut error_msg = String::with_capacity(length);
		for i in 0..length {
			let byte = self.peek_ram(start_addr + i as u16);

			if byte == 0x0D || byte == 0x00 {
				break;
			}
			if byte >= 32 && byte <= 126 {
				error_msg.push(byte as char);
			}
		}

		error_msg
	}
}