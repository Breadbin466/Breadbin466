// =======================================================
// src/cartridge/constants.rs — Cartridge subsystem constants
// =======================================================

/* The CRT signature identifies container images before mapper-specific parsing begins. */
pub const CRT_MAGIC: &[u8; 16] = b"C64 CARTRIDGE   ";

/* Mapper storage uses the 8 KiB width of the ROML and ROMH expansion-port windows and bounds hostile or corrupt bank indices. */
pub const STORAGE_BANK_SIZE: usize = 8192;
pub const STORAGE_MAX_ALLOWED_BANKS: usize = 128;

/* Retro Replay persistence stores the complete battery-backed RAM image with a small format marker. */
pub const RETRO_REPLAY_RAM_SIZE: usize = 0x8000;
pub const RETRO_REPLAY_RAM_BANK_SIZE: usize = 0x2000;
pub const RETRO_REPLAY_NVRAM_MAGIC: &[u8; 4] = b"RRAM";

/* Action Replay II charge thresholds model the delayed analogue enable and disable points used by its legacy mapper. */
pub const ACTION_REPLAY_2_ENABLE_THRESHOLD: u16 = 65;
pub const ACTION_REPLAY_2_DISABLE_THRESHOLD: u16 = 162;