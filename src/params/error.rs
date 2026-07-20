//! Validation error type and the shared range/finiteness checks value objects build on.

use thiserror::Error;

/// Lower bound of the allowed sample rate range (docs/contracts.md §1).
pub const MIN_SAMPLE_RATE_HZ: f32 = 8_000.0;
/// Upper bound of the allowed sample rate range (docs/contracts.md §1).
pub const MAX_SAMPLE_RATE_HZ: f32 = 384_000.0;

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
    #[error(
        "{band} band: lower_threshold_db ({lower_db}) must be less than upper_threshold_db ({upper_db})"
    )]
    ThresholdOrder {
        /// Name of the offending band (`"low"`, `"mid"`, or `"high"`).
        band: &'static str,
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
    /// The sample rate was outside `MIN_SAMPLE_RATE_HZ..=MAX_SAMPLE_RATE_HZ` or not finite.
    #[error(
        "sample_rate must be finite and in [{MIN_SAMPLE_RATE_HZ}, {MAX_SAMPLE_RATE_HZ}], got {value}"
    )]
    SampleRate {
        /// The offending sample rate.
        value: f32,
    },
}

/// Validates the sample rate alone (docs/contracts.md §1).
///
/// # Errors
///
/// Returns `ConfigError::SampleRate` if `sample_rate` is not finite or falls
/// outside `MIN_SAMPLE_RATE_HZ..=MAX_SAMPLE_RATE_HZ`.
pub fn validate_sample_rate(sample_rate: f32) -> Result<(), ConfigError> {
    if sample_rate.is_finite() && (MIN_SAMPLE_RATE_HZ..=MAX_SAMPLE_RATE_HZ).contains(&sample_rate) {
        Ok(())
    } else {
        Err(ConfigError::SampleRate { value: sample_rate })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_sample_rate_out_of_range() {
        assert!(validate_sample_rate(1_000.0).is_err());
        assert!(validate_sample_rate(500_000.0).is_err());
        assert!(validate_sample_rate(f32::NAN).is_err());
        assert!(validate_sample_rate(48_000.0).is_ok());
    }
}
