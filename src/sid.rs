// =======================================================
// src/sid.rs — SID subsystem
// =======================================================

/*
 * SID subsystem.
 *
 * Breadbin466 targets one fixed MOS 6581R4AR analogue profile, based on a
 * datecode 2286 device. The implementation keeps oscillator, envelope,
 * DAC, filter, bus and motherboard-output behaviour as separate layers while
 * exposing the complete chip as the subsystem entry point.
 */

pub mod bus;
pub mod constants;
mod control;
pub mod dac;
pub mod envelope;
pub mod filter;
mod integrator;
mod mixing;
pub mod model;
pub mod oscillator;
pub mod resampler;
mod response;
pub mod sid;
pub mod tables;
mod transients;
pub mod waveforms;

pub use resampler::AudioRateConverter;
/* The public SID surface deliberately exposes only the complete chip and the cycle-to-audio rate converter; oscillator, envelope, DAC and filter details remain internal implementation layers. */
pub use sid::Mos6581;