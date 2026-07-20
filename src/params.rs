//! Parameter value objects, validation, and presets.
//!
//! Ranges and invariants follow `docs/contracts.md` §1. Split by responsibility:
//! - [`value`]: per-field value objects (`IoGain`, `NormalizedF32`, `CrossoverFreqLow`, ...).
//! - [`model`]: the `GlobalParams`/`BandParams`/`OttParams` aggregates, plus
//!   `CrossoverSplit`/`ThresholdRange` — small composite value objects that
//!   each own exactly the cross-field invariant their two constituent fields
//!   share (octave separation, threshold ordering), so a constructed
//!   `OttParams` already satisfies them. The only check left for
//!   `OttParams::validate` is the Nyquist-relative crossover limit, since
//!   that additionally needs the sample rate.
//! - [`error`][]: `ConfigError`.
//! - [`preset`]: the fixed `Preset` startup configurations (ADR 0006).
//!
//! Command-line argument parsing lives in the top-level [`crate::cli`] module,
//! which converts a parsed `Cli` into an `OttParams` via `TryFrom`, so the
//! octave-separation invariant is enforced immediately after parsing, before
//! JACK is ever contacted.

mod error;
mod model;
mod preset;
mod value;

pub use error::ConfigError;
pub use model::{
    BandParams, CROSSOVER_NYQUIST_RATIO, CrossoverSplit, GlobalParams, OttParams, ThresholdRange,
};
pub use preset::Preset;
pub use value::{
    CrossoverFreqHigh, CrossoverFreqLow, IoGain, MakeupGain, NormalizedF32, PositiveF32, Threshold,
};
