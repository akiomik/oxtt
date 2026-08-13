//! oxtt: DSP core for a 3-band upward/downward multiband compressor.
//!
//! Keeps the host adapters thin, so `OttProcessor` can be exposed and tested
//! directly here without starting an audio system at all
//! (docs/architecture.md, ADR 0007).
//!
//! Two hosts, one per feature: `jack-host` (on by default) runs the DSP under
//! a JACK server, and `bela-host` runs it under Bela's `render` callback on a
//! Bela Gem Stereo (ADR 0011). Everything below the host adapters — the DSP,
//! the parameters, and the control surface's mapping layer — is shared and
//! builds with neither feature enabled.

pub mod bands;
#[cfg(feature = "bela-host")]
pub mod bela_host;
pub mod cli;
pub mod control;
pub mod dsp;
#[cfg(feature = "jack-host")]
pub mod jack_host;
pub mod params;
pub mod render;
