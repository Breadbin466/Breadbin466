// =======================================================
// src/fdd1541/computer.rs — DriveBus memory, VIA, mechanism and cycle orchestration
// =======================================================

use super::constants::{DOS_ROM, DOS_ROM_SIZE, DRIVE_RAM_SIZE};
use std::path::Path;

use super::disk_drive::DiskMechanism;
use super::iec::DriveIecBus;
use super::via1::Via1;
use super::via2::Via2;
use crate::cpu::bus::SystemBus;

/* DriveBus joins the 6502 address space to the two 6522 VIAs, the DOS ROM and local RAM. It also owns the mechanical subsystem so VIA2 outputs affect the head and spindle in the same drive cycle. */
pub struct DriveBus {
	ram: Box<[u8; DRIVE_RAM_SIZE]>,
	via1: Via1,
	via2: Via2,
	drive: DiskMechanism,
	dos_rom: Box<[u8; DOS_ROM_SIZE]>,
	so_line: bool,
}

impl DriveBus {
	/* DriveBus starts with power-on RAM, stock DOS ROM and reset peripheral state; the owning Fdd1541 performs the first CPU reset once IEC inputs are known. */
	pub fn new() -> Self {
		Self {
			ram: Box::new([0u8; DRIVE_RAM_SIZE]),
			via1: Via1::new(),
			via2: Via2::new(),
			drive: DiskMechanism::new(),
			dos_rom: Box::new(*DOS_ROM),
			so_line: false,
		}
	}

	/* Reset clears the two VIAs and decoder pipeline but deliberately keeps the selected DOS ROM image and mounted medium. */
	pub fn reset(&mut self) {
		self.via1.reset(0);
		self.via2.reset();
		self.drive.reset();
		self.via2.set_head_byte(self.drive.head_byte(), true);
		self.via2.set_byte_ready(false);
		self.via2.set_sync(self.drive.sync());
		self.via2.set_write_protect(self.drive.write_protect());
		self.so_line = false;
	}

	pub fn iec(&self) -> &DriveIecBus {
		self.via1.iec()
	}

	#[inline(always)]
	pub fn set_host_iec_state(&mut self, host_state: u32, cycle: u64) {
		self.via1.set_host_state(host_state, cycle);
	}

	#[inline(always)]
	pub fn set_device_connected(&mut self, connected: bool, cycle: u64) {
		self.via1.set_connected(connected, cycle);
	}

	/* The 1541 ROM window is exactly 16 KiB even though it is mirrored across the upper address space by board decoding. */
	pub fn load_custom_dos_rom(&mut self, path: &Path) -> crate::emulator::Result<()> {
		let data = std::fs::read(path)?;
		if data.len() != DOS_ROM_SIZE {
			return Err(format!("1541 ROM must be exactly {} bytes", DOS_ROM_SIZE).into());
		}
		self.dos_rom.copy_from_slice(&data);
		Ok(())
	}

	pub fn reset_dos_rom(&mut self) {
		self.dos_rom = Box::new(*DOS_ROM);
	}

	pub fn mount(&mut self, path: &Path) -> bool {
		self.drive.mount(path)
	}

	pub fn unmount(&mut self) -> bool {
		self.drive.unmount()
	}

	pub fn power_led(&self) -> bool {
		true
	}

	pub fn busy_led(&self) -> bool {
		self.via2.led_on()
	}

	pub fn is_dirty(&self) -> bool {
		self.drive.is_dirty()
	}

	pub fn current_track(&self) -> Option<u8> {
		self.drive.current_track()
	}

	pub fn iec_lines(&self) -> (bool, bool, bool) {
		self.via1.iec_lines()
	}

	pub fn host_activity(&self) -> u64 {
		self.via1.host_activity()
	}

	pub fn device_activity(&self) -> u64 {
		self.via1.device_activity()
	}

	pub fn host_released(&self) -> bool {
		self.via1.host_released()
	}

	pub fn motor_on(&self) -> bool {
		self.via2.motor_on()
	}

	pub fn disk_present(&self) -> bool {
		self.drive.disk_present()
	}

	pub fn flush_now(&mut self) -> bool {
		self.drive.flush_now()
	}

	pub fn flush_pending(&self) -> bool {
		self.drive.flush_pending()
	}

	pub fn service_flush(&mut self) -> bool {
		self.drive.service_flush()
	}

	#[inline(always)]
	/* Background media flushing is serviced outside the emulated bus cycle so host file I/O cannot alter the timing seen by the drive CPU. */
	pub fn service(&mut self) -> bool {
		let motor = self.via2.motor_on();
		let density = self.via2.density();
		let phase = self.via2.stepper_phase();
		let write_mode = self.via2.head_write_mode();
		let write_byte = self.via2.head_byte_out();

		self.drive
			.step(motor, density, phase, write_mode, write_byte);

		let byte_ready = self.drive.byte_ready();
		let byte_ready_enabled = byte_ready && self.via2.so_enabled();
		self.via2.set_head_byte(self.drive.head_byte(), byte_ready);
		self.via2.set_byte_ready(byte_ready_enabled);
		self.so_line = byte_ready_enabled;
		self.via2.set_sync(self.drive.sync());
		self.via2.set_write_protect(self.drive.write_protect());

		self.via1.tick() | self.via2.tick()
	}

	#[inline(always)]
	/* VIA2 byte-ready drives the 6502 SO input only while the corresponding hardware gate is enabled. */
	pub fn so_line(&self) -> bool {
		self.so_line
	}
}

impl SystemBus for DriveBus {
	#[inline(always)]
	/* Address decoding mirrors the 1541 board: RAM is repeated in the low region, the VIAs occupy their decoded windows, and DOS ROM fills the upper half. */
	fn read(&mut self, addr: u16, _cycle: u64) -> u8 {
		match addr {
			0x0000..=0x17FF => self.ram[(addr & 0x07FF) as usize],
			0x1800..=0x1BFF => self.via1.read(0x1800 | (addr & 0x000F)),
			0x1C00..=0x1FFF => self.via2.read(0x1C00 | (addr & 0x000F)),
			0x8000..=0xFFFF => self.dos_rom[(addr & 0x3FFF) as usize],
			_ => 0,
		}
	}

	#[inline(always)]
	/* Writes reach only RAM or a selected VIA. ROM space absorbs writes without creating a convenience backdoor into the firmware image. */
	fn write(&mut self, addr: u16, value: u8, cycle: u64) {
		match addr {
			0x0000..=0x17FF => self.ram[(addr & 0x07FF) as usize] = value,
			0x1800..=0x1BFF => self.via1.write(0x1800 | (addr & 0x000F), value, cycle),
			0x1C00..=0x1FFF => self.via2.write(0x1C00 | (addr & 0x000F), value),
			_ => {}
		}
	}
}

impl Default for DriveBus {
	fn default() -> Self {
		Self::new()
	}
}