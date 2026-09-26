// =======================================================
// src/cartridge/mapper_creation.rs — Cartridge mapper factory
// =======================================================

use super::mapper_action_replay::ActionReplayMapper;
use super::mapper_action_replay_legacy::{LegacyActionReplayKind, LegacyActionReplayMapper};
use super::mapper_atomic_power::AtomicPowerMapper;
use super::mapper_c64gs::C64GSMapper;
use super::mapper_dinamic::DinamicMapper;
use super::mapper_easyflash::EasyFlashMapper;
use super::mapper_epyx_fastload::EpyxMapper;
use super::mapper_final_cartridge_3::FinalCartridge3Mapper;
use super::mapper_fun_play::FunPlayMapper;
use super::mapper_gmod2::GMod2Mapper;
use super::mapper_interface::{CartridgeMapper, MapperType};
use super::mapper_kcs_power::KCSMapper;
use super::mapper_magic_desk::MagicDeskMapper;
use super::mapper_ocean::OceanMapper;
use super::mapper_pagefox::PagefoxMapper;
use super::mapper_retro_replay::RetroReplayMapper;
use super::mapper_rgcd::RgcdMapper;
use super::mapper_simons_basic::SimonsBasicMapper;
use super::mapper_standard::StandardMapper;
use super::mapper_structured_basic::StructuredBasicMapper;
use super::mapper_super_games::SuperGamesMapper;
use super::mapper_super_snapshot_5::SuperSnapshot5Mapper;
use super::mapper_zaxxon::ZaxxonMapper;

/* Mapper construction is centralised so CRT identifiers, raw-image fallbacks, mapper state and unsupported-type errors cannot diverge between loader, UI and motherboard paths. */
pub fn create_mapper(kind: MapperType) -> Box<dyn CartridgeMapper> {
	match kind {
		MapperType::Normal => Box::new(StandardMapper::new(kind)),
		MapperType::ActionReplay => Box::new(ActionReplayMapper::new()),
		MapperType::ActionReplay2 => Box::new(LegacyActionReplayMapper::new(
			LegacyActionReplayKind::ActionReplay2,
		)),
		MapperType::ActionReplay3 => Box::new(LegacyActionReplayMapper::new(
			LegacyActionReplayKind::ActionReplay3,
		)),
		MapperType::ActionReplay4 => Box::new(LegacyActionReplayMapper::new(
			LegacyActionReplayKind::ActionReplay4,
		)),
		MapperType::KCS => Box::new(KCSMapper::new()),
		MapperType::FinalCartridge3 => Box::new(FinalCartridge3Mapper::new()),
		MapperType::SimonsBasic => Box::new(SimonsBasicMapper::new()),
		MapperType::Ocean => Box::new(OceanMapper::new()),
		MapperType::FunPlay => Box::new(FunPlayMapper::new()),
		MapperType::SuperGames => Box::new(SuperGamesMapper::new()),
		MapperType::AtomicPower => Box::new(AtomicPowerMapper::new()),
		MapperType::EpyxFastLoad => Box::new(EpyxMapper::new()),
		MapperType::C64GS => Box::new(C64GSMapper::new()),
		MapperType::Dinamic => Box::new(DinamicMapper::new()),
		MapperType::Zaxxon => Box::new(ZaxxonMapper::new()),
		MapperType::MagicDesk => Box::new(MagicDeskMapper::new()),
		MapperType::SuperSnapshot5 => Box::new(SuperSnapshot5Mapper::new()),
		MapperType::StructuredBasic => Box::new(StructuredBasicMapper::new()),
		MapperType::EasyFlash => Box::new(EasyFlashMapper::new(MapperType::EasyFlash)),
		MapperType::EasyFlashXbank => Box::new(EasyFlashMapper::new(MapperType::EasyFlashXbank)),
		MapperType::EasyFlash3 => Box::new(EasyFlashMapper::new(MapperType::EasyFlash3)),
		MapperType::RetroReplay => Box::new(RetroReplayMapper::new()),
		MapperType::RGCD => Box::new(RgcdMapper::new()),
		MapperType::Pagefox => Box::new(PagefoxMapper::new()),
		MapperType::GMod2 => Box::new(GMod2Mapper::new()),
	}
}