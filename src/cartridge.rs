// =======================================================
// src/cartridge.rs — Cartridge subsystem facade
// =======================================================

/* The cartridge facade keeps image loading, bus-line configuration and mapper-specific behaviour behind one subsystem boundary. The motherboard sees only a Cartridge device; individual cartridge designs remain private implementations of the common mapper contract. */

pub mod bank_storage;
pub mod bus_configuration;
pub mod cartridge_device;
pub mod constants;
pub mod crt_layout;
pub mod crt_loader;
pub mod mapper_action_replay;
pub mod mapper_action_replay_legacy;
pub mod mapper_atomic_power;
pub mod mapper_c64gs;
pub mod mapper_creation;
pub mod mapper_dinamic;
pub mod mapper_easyflash;
pub mod mapper_epyx_fastload;
pub mod mapper_final_cartridge_3;
pub mod mapper_fun_play;
pub mod mapper_gmod2;
pub mod mapper_interface;
pub mod mapper_kcs_power;
pub mod mapper_magic_desk;
pub mod mapper_ocean;
pub mod mapper_pagefox;
pub mod mapper_retro_replay;
pub mod mapper_rgcd;
pub mod mapper_simons_basic;
pub mod mapper_standard;
pub mod mapper_structured_basic;
pub mod mapper_super_games;
pub mod mapper_super_snapshot_5;
pub mod mapper_zaxxon;

pub use cartridge_device::Cartridge;
mod flash_chip;