//! Validation error type for cross-field and sample-rate-relative checks.

use thiserror::Error;

use super::value::SampleRateError;

/// Validation error when constructing or updating parameters (docs/contracts.md §1).
#[derive(Debug, Clone, PartialEq, Error)]
pub enum ConfigError {
    /// `high_crossover_hz` was less than one octave above `low_crossover_hz`.
    #[error(
        "high_crossover_hz ({high_hz}) must be at least one octave above low_crossover_hz ({low_hz})"
    )]
    CrossoverOctave {
        /// The offending `low_crossover_hz`.
        low_hz: f32,
        /// The offending `high_crossover_hz`.
        high_hz: f32,
    },
    /// A band's `lower_threshold_db` was not less than its `upper_threshold_db`.
    #[error("lower_threshold_db ({lower_db}) must be less than upper_threshold_db ({upper_db})")]
    ThresholdOrder {
        /// The offending `lower_threshold_db`.
        lower_db: f32,
        /// The offending `upper_threshold_db`.
        upper_db: f32,
    },
    /// A crossover frequency exceeded the Nyquist-relative limit at the current sample rate.
    #[error("{field} ({value}) exceeds {max} at the current sample rate")]
    CrossoverNyquist {
        /// Name of the offending field.
        field: &'static str,
        /// The offending value.
        value: f32,
        /// The limit the value exceeded.
        max: f32,
    },
    /// The sample rate was outside its allowed range or not finite.
    #[error(transparent)]
    SampleRate(#[from] SampleRateError),
}
