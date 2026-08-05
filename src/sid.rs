// =======================================================
// src/sid.rs — BreadSID subsystem façade
// =======================================================

pub mod constants;
pub mod bus;
pub mod waveforms;
pub mod oscillator;
pub mod envelope;
pub mod dac;
pub mod model;
pub mod tables;
pub mod filter;
pub mod sid;
pub mod resampler;

pub use resampler::AudioRateConverter;
/* The public SID surface deliberately exposes only the complete chip and the cycle-to-audio rate converter; oscillator, envelope, DAC and filter details remain internal implementation layers. */
pub use sid::Mos6581;