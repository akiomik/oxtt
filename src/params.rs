//! Interprets CLI values, and handles parameter validation, normalization, and presets.
//!
//! Ranges and invariants follow `docs/contracts.md` §1. Split by responsibility:
//! - [`value`]: per-field value objects (`UnitInterval`, `GainDb`, `CrossoverPair`, ...).
//! - [`model`]: the `GlobalParams`/`BandParams`/`OttParams` aggregates.
//! - [`error`]: `ConfigError` and the shared range/finiteness checks.
//! - [`preset`]: the fixed `Preset` startup configurations (ADR 0006).
//! - [`cli`]: command-line argument parsing.

mod cli;
mod error;
mod model;
mod preset;
mod value;

pub use cli::{CliError, CliOutcome, help_text, parse_args, version_text};
pub use error::{ConfigError, MAX_SAMPLE_RATE_HZ, MIN_SAMPLE_RATE_HZ, validate_sample_rate};
pub use model::{
    BAND_HIGH, BAND_LOW, BAND_MID, BandParams, CROSSOVER_NYQUIST_RATIO, GlobalParams, OttParams,
};
pub use preset::Preset;
pub use value::{CrossoverPair, GainDb, MakeupGainDb, PositiveMs, ThresholdRange, UnitInterval};
