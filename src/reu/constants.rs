// =======================================================
// src/reu/constants.rs — MOS 8726 register constants
// =======================================================

/*
 * MOS 8726 register indices and bit definitions.
 *
 * The register file is kept as a compact byte array because software can read
 * back the live DMA counters while a command is active.  Named indices prevent
 * the controller logic from depending on unexplained numeric offsets and keep
 * the visible register layout traceable to the hardware interface.
 */

pub(crate) const REGISTER_COUNT: usize = 11;

pub(crate) const STATUS: usize = 0;
pub(crate) const COMMAND: usize = 1;
pub(crate) const C64_ADDRESS_LOW: usize = 2;
pub(crate) const C64_ADDRESS_HIGH: usize = 3;
pub(crate) const REU_ADDRESS_LOW: usize = 4;
pub(crate) const REU_ADDRESS_HIGH: usize = 5;
pub(crate) const REU_ADDRESS_BANK: usize = 6;
pub(crate) const TRANSFER_LENGTH_LOW: usize = 7;
pub(crate) const TRANSFER_LENGTH_HIGH: usize = 8;
pub(crate) const INTERRUPT_MASK: usize = 9;
pub(crate) const ADDRESS_CONTROL: usize = 10;

pub(crate) const STATUS_VERSION: u8 = 0x10;
pub(crate) const STATUS_VERIFY_ERROR: u8 = 0x20;
pub(crate) const STATUS_END_OF_BLOCK: u8 = 0x40;
pub(crate) const STATUS_IRQ_PENDING: u8 = 0x80;
pub(crate) const STATUS_EVENT_MASK: u8 =
	STATUS_VERIFY_ERROR | STATUS_END_OF_BLOCK | STATUS_IRQ_PENDING;

pub(crate) const COMMAND_TRANSFER_TYPE_MASK: u8 = 0x03;
pub(crate) const COMMAND_FF00_DISABLE: u8 = 0x10;
pub(crate) const COMMAND_AUTOLOAD: u8 = 0x20;
pub(crate) const COMMAND_EXECUTE: u8 = 0x80;
pub(crate) const COMMAND_WRITABLE_MASK: u8 = 0xB3;
pub(crate) const COMMAND_COMPLETION_MASK: u8 = 0x33;

pub(crate) const INTERRUPT_VERIFY_ENABLE: u8 = 0x20;
pub(crate) const INTERRUPT_END_OF_BLOCK_ENABLE: u8 = 0x40;
pub(crate) const INTERRUPT_GLOBAL_ENABLE: u8 = 0x80;
pub(crate) const INTERRUPT_UNUSED_READ_HIGH: u8 = 0x1F;

pub(crate) const ADDRESS_CONTROL_FIX_REU: u8 = 0x40;
pub(crate) const ADDRESS_CONTROL_FIX_C64: u8 = 0x80;
pub(crate) const ADDRESS_CONTROL_UNUSED_READ_HIGH: u8 = 0x3F;

pub(crate) const UNUSED_REGISTER_VALUE: u8 = 0xFF;
pub(crate) const FULL_64K_TRANSFER_LENGTH: usize = 65_536;