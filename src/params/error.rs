//! Validation error type and the shared range/finiteness checks value objects build on.

use thiserror::Error;

/// Lower bound of the allowed sample rate range (docs/contracts.md §1).
pub const MIN_SAMPLE_RATE_HZ: f32 = 8_000.0;
/// Upper bound of the allowed sample rate range (docs/contracts.md §1).
pub const MAX_SAMPLE_RATE_HZ: f32 = 384_000.0;

/// Validation error when constructing or updating parameters (docs/contracts.md §1).
#[derive(Debug, Clone, PartialEq, Error)]
pub enum ConfigError {
    /// A field's value was NaN or infinite.
    #[error("{field} must be finite, got {value}")]
    NotFinite {
        /// Name of the offending field.
        field: &'static str,
        /// The offending value.
        value: f32,
    },
    /// A field's value fell outside its allowed range.
    #[error("{field} must be in [{min}, {max}], got {value}")]
    OutOfRange {
        /// Name of the offending field.
        field: &'static str,
        /// Lower bound of the allowed range, inclusive.
        min: f32,
        /// Upper bound of the allowed range, inclusive.
        max: f32,
        /// The offending value.
        value: f32,
    },
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

pub(super) const fn check_finite(field: &'static str, value: f32) -> Result<(), ConfigError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(ConfigError::NotFinite { field, value })
    }
}

pub(super) fn check_range(
    field: &'static str,
    value: f32,
    min: f32,
    max: f32,
) -> Result<(), ConfigError> {
    check_finite(field, value)?;
    if value < min || value > max {
        Err(ConfigError::OutOfRange {
            field,
            min,
            max,
            value,
        })
    } else {
        Ok(())
    }
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
