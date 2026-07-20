//! Parameter value objects, validation, and presets.
//!
//! Ranges and invariants follow `docs/contracts.md` §1. Split by responsibility:
//! - [`value`]: per-field value objects (`IoGain`, `NormalizedF32`, `CrossoverFreqLow`, ...).
//! - [`model`]: the `GlobalParams`/`BandParams`/`OttParams` aggregates, and the
//!   cross-field checks (`OttParams::validate`) that no single value object can express.
//! - [`error`]: `ConfigError` and the shared sample-rate check.
//! - [`preset`]: the fixed `Preset` startup configurations (ADR 0006).
//!
//! Command-line argument parsing lives in the top-level [`crate::cli`] module,
//! which converts a parsed `Cli` into an `OttParams`.

mod error;
mod model;
mod preset;
mod value;

pub use error::{ConfigError, MAX_SAMPLE_RATE_HZ, MIN_SAMPLE_RATE_HZ, validate_sample_rate};
pub use model::{
    BAND_HIGH, BAND_LOW, BAND_MID, BandParams, CROSSOVER_NYQUIST_RATIO, GlobalParams, OttParams,
};
pub use preset::Preset;
pub use value::{
    CrossoverFreqHigh, CrossoverFreqLow, IoGain, MakeupGain, NormalizedF32, PositiveF32, Threshold,
};
