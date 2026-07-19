//! Value objects for individual parameter fields (docs/contracts.md §1).
//!
//! Each type exposes at most two constructors:
//! - `new(...) -> Result<Self, ConfigError>`: the only public, runtime-checked way to
//!   build one from an untrusted value (CLI input, or a library caller). Only exists
//!   where such a construction path actually exists.
//! - `new_const(...) -> Self`: `pub(crate)`, for the fixed preset literals only.
//!   Asserts the same invariants, so an invalid literal fails to compile rather than
//!   slipping past a forgotten runtime check.

use super::error::{ConfigError, check_range};

/// A normalized fraction in `0.0..=1.0` (docs/contracts.md §1).
///
/// Shared by the dry/wet mix, the attack/release time multiplier, the
/// upward/downward multipliers, and each band's compression amounts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UnitInterval(f32);

impl UnitInterval {
    /// Validates and wraps `value`.
    ///
    /// # Errors
    ///
    /// Returns `ConfigError` if `value` is not finite or falls outside `0.0..=1.0`.
    pub fn new(field: &'static str, value: f32) -> Result<Self, ConfigError> {
        check_range(field, value, 0.0, 1.0)?;
        Ok(Self(value))
    }

    /// For preset literals only. Panics (at compile time, in a `const` context) if `value` is invalid.
    pub(crate) const fn new_const(value: f32) -> Self {
        assert!(
            value.is_finite() && value >= 0.0 && value <= 1.0,
            "UnitInterval literal out of range"
        );
        Self(value)
    }

    /// Returns the wrapped value.
    #[must_use]
    pub const fn get(self) -> f32 {
        self.0
    }
}

/// A band's downward/upward compression threshold pair, in dB (docs/contracts.md §1).
///
/// Both bounds must lie in `-80.0..=0.0`, and `lower_db` must be less than `upper_db`.
/// Only ever constructed from preset literals (ADR 0006); not CLI-configurable.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThresholdRange {
    lower_db: f32,
    upper_db: f32,
}

impl ThresholdRange {
    /// For preset literals only. Panics (at compile time, in a `const` context) if the pair is invalid.
    pub(crate) const fn new_const(lower_db: f32, upper_db: f32) -> Self {
        assert!(
            lower_db.is_finite() && lower_db >= -80.0 && lower_db <= 0.0,
            "ThresholdRange lower_db literal out of range"
        );
        assert!(
            upper_db.is_finite() && upper_db >= -80.0 && upper_db <= 0.0,
            "ThresholdRange upper_db literal out of range"
        );
        assert!(
            lower_db < upper_db,
            "ThresholdRange lower_db must be less than upper_db"
        );
        Self { lower_db, upper_db }
    }

    /// Returns the downward-compression threshold in dB.
    #[must_use]
    pub const fn lower_db(self) -> f32 {
        self.lower_db
    }

    /// Returns the upward-compression threshold in dB.
    #[must_use]
    pub const fn upper_db(self) -> f32 {
        self.upper_db
    }
}

/// A low/mid and mid/high crossover frequency pair, in Hz (docs/contracts.md §1).
///
/// `low_hz` must lie in `40.0..=2000.0`, `high_hz` must lie in `400.0..=16000.0`,
/// and `high_hz` must be at least one octave above `low_hz`. The Nyquist-relative
/// limit depends on the sample rate, so it's checked separately by `OttParams::validate`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CrossoverPair {
    low_hz: f32,
    high_hz: f32,
}

impl CrossoverPair {
    /// Validates and wraps `low_hz`/`high_hz`.
    ///
    /// # Errors
    ///
    /// Returns `ConfigError` if either frequency is not finite, falls outside its
    /// allowed range, or `high_hz` is less than one octave above `low_hz`.
    pub fn new(low_hz: f32, high_hz: f32) -> Result<Self, ConfigError> {
        check_range("low_crossover_hz", low_hz, 40.0, 2000.0)?;
        check_range("high_crossover_hz", high_hz, 400.0, 16000.0)?;
        if high_hz < 2.0 * low_hz {
            return Err(ConfigError::CrossoverOctave { low_hz, high_hz });
        }
        Ok(Self { low_hz, high_hz })
    }

    /// For preset literals only. Panics (at compile time, in a `const` context) if the pair is invalid.
    pub(crate) const fn new_const(low_hz: f32, high_hz: f32) -> Self {
        assert!(
            low_hz.is_finite() && low_hz >= 40.0 && low_hz <= 2000.0,
            "CrossoverPair low_hz literal out of range"
        );
        assert!(
            high_hz.is_finite() && high_hz >= 400.0 && high_hz <= 16000.0,
            "CrossoverPair high_hz literal out of range"
        );
        assert!(
            high_hz >= 2.0 * low_hz,
            "CrossoverPair literal violates octave separation"
        );
        Self { low_hz, high_hz }
    }

    /// Returns the low/mid crossover frequency in Hz.
    #[must_use]
    pub const fn low_hz(self) -> f32 {
        self.low_hz
    }

    /// Returns the mid/high crossover frequency in Hz.
    #[must_use]
    pub const fn high_hz(self) -> f32 {
        self.high_hz
    }
}

/// A gain value in dB, range `-24.0..=24.0` (docs/contracts.md §1).
///
/// Used for the pre-split and post-sum gains, which are CLI-configurable.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GainDb(f32);

impl GainDb {
    /// Validates and wraps `value`.
    ///
    /// # Errors
    ///
    /// Returns `ConfigError` if `value` is not finite or falls outside `-24.0..=24.0`.
    pub fn new(field: &'static str, value: f32) -> Result<Self, ConfigError> {
        check_range(field, value, -24.0, 24.0)?;
        Ok(Self(value))
    }

    /// For preset literals only. Panics (at compile time, in a `const` context) if `value` is invalid.
    pub(crate) const fn new_const(value: f32) -> Self {
        assert!(
            value.is_finite() && value >= -24.0 && value <= 24.0,
            "GainDb literal out of range"
        );
        Self(value)
    }

    /// Returns the wrapped value.
    #[must_use]
    pub const fn get(self) -> f32 {
        self.0
    }
}

/// A band's makeup gain in dB, range `-40.0..=40.0` (docs/contracts.md §1).
///
/// Only ever constructed from preset literals (ADR 0006); not CLI-configurable.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MakeupGainDb(f32);

impl MakeupGainDb {
    /// For preset literals only. Panics (at compile time, in a `const` context) if `value` is invalid.
    pub(crate) const fn new_const(value: f32) -> Self {
        assert!(
            value.is_finite() && value >= -40.0 && value <= 40.0,
            "MakeupGainDb literal out of range"
        );
        Self(value)
    }

    /// Returns the wrapped value.
    #[must_use]
    pub const fn get(self) -> f32 {
        self.0
    }
}

/// A positive duration in milliseconds (docs/contracts.md §1).
///
/// Used for each band's base attack/release time at `time = 0.5`. Only ever
/// constructed from preset literals (ADR 0006); not CLI-configurable.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PositiveMs(f32);

impl PositiveMs {
    /// For preset literals only. Panics (at compile time, in a `const` context) if `value` is invalid.
    pub(crate) const fn new_const(value: f32) -> Self {
        assert!(
            value.is_finite() && value > 0.0,
            "PositiveMs literal must be positive"
        );
        Self(value)
    }

    /// Returns the wrapped value.
    #[must_use]
    pub const fn get(self) -> f32 {
        self.0
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn gain_db_rejects_nan_and_infinite() {
        assert!(matches!(
            GainDb::new("input_gain_db", f32::NAN),
            Err(ConfigError::NotFinite { .. })
        ));
        assert!(matches!(
            GainDb::new("output_gain_db", f32::INFINITY),
            Err(ConfigError::NotFinite { .. })
        ));
    }

    #[test]
    fn gain_db_rejects_out_of_range() {
        assert!(matches!(
            GainDb::new("input_gain_db", 100.0),
            Err(ConfigError::OutOfRange { .. })
        ));
    }

    #[test]
    fn unit_interval_rejects_out_of_range() {
        assert!(matches!(
            UnitInterval::new("depth", 1.5),
            Err(ConfigError::OutOfRange { .. })
        ));
    }

    #[test]
    #[should_panic(expected = "ThresholdRange lower_db must be less than upper_db")]
    fn threshold_range_new_const_rejects_inverted_thresholds() {
        ThresholdRange::new_const(-10.0, -20.0);
    }

    #[test]
    fn crossover_pair_rejects_less_than_one_octave_apart() {
        assert!(matches!(
            CrossoverPair::new(1000.0, 1500.0),
            Err(ConfigError::CrossoverOctave { .. })
        ));
    }
}
