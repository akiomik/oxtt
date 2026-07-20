//! oxtt: DSP core for a 3-band upward/downward multiband compressor.
//!
//! Keeps `main.rs` and `jack_host.rs` thin, so `OttProcessor` can be exposed
//! and tested directly here without starting JACK (docs/architecture.md, ADR 0005).

pub mod cli;
pub mod dsp;
pub mod jack_host;
pub mod params;
