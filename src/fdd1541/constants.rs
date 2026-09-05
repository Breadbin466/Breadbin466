// =======================================================
// src/fdd1541/constants.rs — 1541 hardware and execution constants
// =======================================================

/* The drive CPU runs at 1 MHz while the internal mechanism timeline uses sixteen master ticks per CPU cycle. */
pub(crate) const DRIVE_CPU_FREQ_HZ: u32 = 1_000_000;
pub(crate) const GCR_ENCODE: [u8; 16] = [
	0x0A, 0x0B, 0x12, 0x13, 0x0E, 0x0F, 0x16, 0x17, 0x09, 0x19, 0x1A, 0x1B, 0x0D, 0x1D, 0x1E, 0x15,
];
/* The event ring is a power of two so monotonically increasing counters can select slots with a mask while still detecting fullness from their distance. */
pub(crate) const DRIVE_RING_CAPACITY: usize = 65_536;
pub(crate) const DRIVE_RING_MASK: u64 = DRIVE_RING_CAPACITY as u64 - 1;
pub(crate) const DRIVE_TIGHT_WINDOW: u32 = 2_000;
pub(crate) const DRIVE_SKEW_CHECK_MASK: u64 = 7;
pub(crate) const DRIVE_MAX_SKEW: u64 = 56;
pub(crate) const DRIVE_BATCH_LIMIT: u64 = 4_096;
/* The seven low IFR bits retain the MOS 6522 interrupt-source layout; bit 7 is the derived IRQ summary. */
pub(crate) const VIA_IFR_CA2: u8 = 0x01;
pub(crate) const VIA_IFR_CA1: u8 = 0x02;
pub(crate) const VIA_IFR_SR: u8 = 0x04;
pub(crate) const VIA_IFR_CB2: u8 = 0x08;
pub(crate) const VIA_IFR_CB1: u8 = 0x10;
pub(crate) const VIA_IFR_T2: u8 = 0x20;
pub(crate) const VIA_IFR_T1: u8 = 0x40;
pub(crate) const VIA_IFR_IRQ: u8 = 0x80;
pub(crate) const DRIVE_MASTER_PER_CPU: u32 = 16;
pub(crate) const DRIVE_RESET_HALF_TRACK: u8 = 38;
/* A nominal 300 RPM revolution lasts 200 ms, represented here in the drive master-clock domain. Density zones then choose how many master ticks form each recorded bit cell. */
pub(crate) const DRIVE_MASTER_CYCLES_PER_ROTATION: u64 = 3_200_000;
pub(crate) const G64_CELL_CYCLES: [u64; 4] = [64, 60, 56, 52];
pub(crate) const NOMINAL_TRACK_BYTES: [usize; 4] = [6_250, 6_666, 7_142, 7_692];
/* Media changes pass through ejecting, absent and inserting intervals so firmware sees write-protect and read-channel transitions over time rather than an instantaneous image swap. */
pub(crate) const DISK_EJECTING_CYCLES: u32 = 400_000;
pub(crate) const DISK_ABSENT_CYCLES: u32 = 200_000;
pub(crate) const DISK_INSERTING_CYCLES: u32 = 400_000;
pub(crate) const DISK_CHANGE_CYCLES: u32 =
	DISK_EJECTING_CYCLES + DISK_ABSENT_CYCLES + DISK_INSERTING_CYCLES;
/* The read amplifier model schedules bounded pseudo-random transitions after trustworthy flux disappears; these ranges are expressed in the master-clock domain. */
pub(crate) const POST_FLUX_SETTLING_MIN: u32 = 18 * DRIVE_MASTER_PER_CPU;
pub(crate) const POST_FLUX_SETTLING_SPAN: u32 = 2 * DRIVE_MASTER_PER_CPU;
pub(crate) const SPURIOUS_FLUX_INTERVAL_MIN: u32 = 2 * DRIVE_MASTER_PER_CPU;
pub(crate) const SPURIOUS_FLUX_INTERVAL_SPAN: u32 = 23 * DRIVE_MASTER_PER_CPU;
pub(crate) const NOISE_PRNG_MASK: u32 = 0x7FFF_FFFF;
/* The following groups describe the on-disk containers accepted by the drive. They are storage formats, not alternate drive models. */
pub(crate) const G64_SIGNATURE: &[u8; 8] = b"GCR-1541";
pub(crate) const G64_MAX_HALF_TRACKS: usize = 84;
pub(crate) const G64_HEADER_LEN: usize = 12;
pub(crate) const G64_MAX_TRACK_SIZE: usize = 7_928;
pub(crate) const D64_IMAGE_SIZE: usize = 174_848;
pub(crate) const D64_BAM_OFFSET: usize = 91_392;
pub(crate) const D64_DISK_NAME: &[u8] = b"BREADBIN466";
pub(crate) const D64_DISK_ID: &[u8; 2] = b"BB";
pub(crate) const NIB_SIGNATURE: &[u8; 13] = b"MNIB-1541-RAW";
pub(crate) const NIB_HEADER_LENGTH: usize = 0x100;
pub(crate) const NIB_TRACK_LENGTH: usize = 0x2000;
pub(crate) const NIB_HALF_TRACK_COUNT: usize = 84;
pub(crate) const NIB_TRACK_BYTES_MIN: [usize; 4] = [6_183, 6_598, 7_073, 7_616];
pub(crate) const NIB_TRACK_BYTES_MAX: [usize; 4] = [6_311, 6_726, 7_201, 7_824];
pub(crate) const NIB_MAX_DECOMPRESSED_LENGTH: usize =
	NIB_HEADER_LENGTH + NIB_TRACK_LENGTH * NIB_HALF_TRACK_COUNT;
/* Each packed IEC state bit records whether one participant actively pulls a line or exposes a derived handshake condition. Physical line levels are resolved separately as wired-AND signals. */
pub(crate) const IEC_HOST_ATN: u32 = 1 << 0;
pub(crate) const IEC_HOST_CLK: u32 = 1 << 1;
pub(crate) const IEC_HOST_DATA: u32 = 1 << 2;
pub(crate) const IEC_HOST_SRQ: u32 = 1 << 3;
pub(crate) const IEC_DEVICE_ATN: u32 = 1 << 4;
pub(crate) const IEC_DEVICE_CLK: u32 = 1 << 5;
pub(crate) const IEC_DEVICE_DATA: u32 = 1 << 6;
pub(crate) const IEC_DEVICE_SRQ: u32 = 1 << 7;
pub(crate) const IEC_DEVICE_ATNA: u32 = 1 << 8;
pub(crate) const IEC_DEVICE_ATN_ACK: u32 = 1 << 9;
pub(crate) const IEC_DEVICE_CONNECTED: u32 = 1 << 10;
pub(crate) const IEC_HOST_PULLS: u32 = IEC_HOST_ATN | IEC_HOST_CLK | IEC_HOST_DATA;
pub(crate) const IEC_DEVICE_PULLS: u32 = IEC_DEVICE_ATN | IEC_DEVICE_CLK | IEC_DEVICE_DATA;
pub(crate) const IEC_DEVICE_STATE: u32 =
	IEC_DEVICE_PULLS | IEC_DEVICE_SRQ | IEC_DEVICE_ATNA | IEC_DEVICE_ATN_ACK;
pub(crate) const IEC_INITIAL_STATE: u32 = IEC_DEVICE_CONNECTED;
/* The 16 KiB DOS ROM is mapped into the upper half of the 6502 address space; the lower decoded region contains mirrored RAM and the two VIA devices. */
pub(crate) const DOS_ROM: &[u8; 16_384] =
	include_bytes!("../../roms/dos1541-325302-01+901229-05.bin");
pub(crate) const DRIVE_RAM_SIZE: usize = 0x0800;
pub(crate) const DOS_ROM_SIZE: usize = 16_384;