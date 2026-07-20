//! Value objects for individual parameter fields (docs/contracts.md §1).
//!
//! Range/finiteness validation is delegated to the `nutype` crate: each type
//! below is a `#[nutype]`-generated newtype with a `validate(...)` clause
//! for its bounds. `nutype` generates the fallible, runtime-checked
//! `try_new(...) -> Result<Self, ...>` (the public entry point for
//! untrusted input — CLI parsing via `derive(FromStr)`, or a library
//! caller), and, via the `const_fn` flag, makes that same constructor
//! callable from a `const` context. Each type adds two hand-written
//! methods on top:
//! - `get(self) -> f32`: renames `nutype`'s `into_inner` to match this
//!   codebase's existing accessor convention (55 call sites in `dsp.rs`).
//! - `new_const(value: f32) -> Self`: wraps `try_new` with a
//!   `match { Ok(v) => v, Err(_) => panic!(...) }`, so preset literals
//!   (`src/params/preset.rs`) keep their existing one-line call-site shape
//!   instead of repeating that `match` at every field; an invalid literal
//!   still fails to compile rather than slipping past a forgotten runtime
//!   check.
//!
//! No constructor here takes an external label/context parameter alongside
//! the value being validated: a value object's constructor accepts only
//! data that is part of its own invariant, never free-text metadata that
//! nothing enforces coherence for. `nutype`'s generated validation error is
//! specific to each type (not a shared, label-carrying `ConfigError`), so
//! this holds for `try_new`/`FromStr` too — the caller assigning the
//! validated value into a named field attaches field context itself.
//!
//! `nutype`'s built-in range validators report a generic, type-specific
//! message (e.g. "`IoGain` is too big. The value must be less than 24.0.")
//! rather than echoing the offending value; that's an intentional trade
//! against hand-writing a `Display` impl per type.
use nutype::nutype;

/// A gain value in dB, range `-24.0..=24.0` (docs/contracts.md §1).
///
/// Used for the pre-split and post-sum gains, which are CLI-configurable.
#[nutype(
    const_fn,
    validate(finite, greater_or_equal = -24.0, less_or_equal = 24.0),
    derive(Debug, Clone, Copy, PartialEq, Display, FromStr)
)]
pub struct IoGain(f32);

impl IoGain {
    /// Returns the wrapped value.
    #[must_use]
    pub const fn get(self) -> f32 {
        self.into_inner()
    }

    /// For preset literals only. Panics (at compile time, in a `const` context) if `value` is invalid.
    ///
    /// # Panics
    ///
    /// Panics if `value` falls outside `-24.0..=24.0`.
    #[must_use]
    #[allow(clippy::panic)] // the only way to fail a const-context literal at compile time.
    pub const fn new_const(value: f32) -> Self {
        match Self::try_new(value) {
            Ok(v) => v,
            Err(_) => panic!("IoGain literal out of range"),
        }
    }
}

/// A positive duration in milliseconds (docs/contracts.md §1).
///
/// Used for each band's base attack/release time at `time = 0.5`. Only ever
/// constructed from preset literals (ADR 0006); not CLI-configurable, so
/// its fallible constructor is crate-private.
#[nutype(
    const_fn,
    validate(finite, greater = 0.0),
    constructor(visibility = pub(crate)),
    derive(Debug, Clone, Copy, PartialEq)
)]
pub struct PositiveF32(f32);

impl PositiveF32 {
    /// Returns the wrapped value.
    #[must_use]
    pub const fn get(self) -> f32 {
        self.into_inner()
    }

    /// For preset literals only. Panics (at compile time, in a `const` context) if `value` is invalid.
    ///
    /// # Panics
    ///
    /// Panics if `value` is not strictly greater than `0.0`.
    #[allow(clippy::panic)] // the only way to fail a const-context literal at compile time.
    pub(crate) const fn new_const(value: f32) -> Self {
        match Self::try_new(value) {
            Ok(v) => v,
            Err(_) => panic!("PositiveF32 literal out of range"),
        }
    }
}

/// A normalized fraction in `0.0..=1.0` (docs/contracts.md §1).
///
/// Shared by the dry/wet mix, the attack/release time multiplier, the
/// upward/downward multipliers, and each band's compression amounts.
#[nutype(
    const_fn,
    validate(finite, greater_or_equal = 0.0, less_or_equal = 1.0),
    derive(Debug, Clone, Copy, PartialEq, Display, FromStr)
)]
pub struct NormalizedF32(f32);

impl NormalizedF32 {
    /// Returns the wrapped value.
    #[must_use]
    pub const fn get(self) -> f32 {
        self.into_inner()
    }

    /// For preset literals only. Panics (at compile time, in a `const` context) if `value` is invalid.
    ///
    /// # Panics
    ///
    /// Panics if `value` falls outside `0.0..=1.0`.
    #[must_use]
    #[allow(clippy::panic)] // the only way to fail a const-context literal at compile time.
    pub const fn new_const(value: f32) -> Self {
        match Self::try_new(value) {
            Ok(v) => v,
            Err(_) => panic!("NormalizedF32 literal out of range"),
        }
    }
}

/// The low/mid crossover frequency in Hz, range `40.0..=2000.0` (docs/contracts.md §1).
///
/// Combined with `CrossoverFreqHigh` by `OttParams::validate`, which enforces
/// the octave-separation invariant that no single field can express on its own.
#[nutype(
    const_fn,
    validate(finite, greater_or_equal = 40.0, less_or_equal = 2000.0),
    derive(Debug, Clone, Copy, PartialEq, PartialOrd, Display, FromStr)
)]
pub struct CrossoverFreqLow(f32);

impl CrossoverFreqLow {
    /// Returns the wrapped value.
    #[must_use]
    pub const fn get(self) -> f32 {
        self.into_inner()
    }

    /// For preset literals only. Panics (at compile time, in a `const` context) if `value` is invalid.
    ///
    /// # Panics
    ///
    /// Panics if `value` falls outside `40.0..=2000.0`.
    #[must_use]
    #[allow(clippy::panic)] // the only way to fail a const-context literal at compile time.
    pub const fn new_const(value: f32) -> Self {
        match Self::try_new(value) {
            Ok(v) => v,
            Err(_) => panic!("CrossoverFreqLow literal out of range"),
        }
    }
}

/// The mid/high crossover frequency in Hz, range `400.0..=16000.0` (docs/contracts.md §1).
///
/// Combined with `CrossoverFreqLow` by `OttParams::validate`, which enforces
/// the octave-separation invariant that no single field can express on its own.
#[nutype(
    const_fn,
    validate(finite, greater_or_equal = 400.0, less_or_equal = 16000.0),
    derive(Debug, Clone, Copy, PartialEq, PartialOrd, Display, FromStr)
)]
pub struct CrossoverFreqHigh(f32);

impl CrossoverFreqHigh {
    /// Returns the wrapped value.
    #[must_use]
    pub const fn get(self) -> f32 {
        self.into_inner()
    }

    /// For preset literals only. Panics (at compile time, in a `const` context) if `value` is invalid.
    ///
    /// # Panics
    ///
    /// Panics if `value` falls outside `400.0..=16000.0`.
    #[must_use]
    #[allow(clippy::panic)] // the only way to fail a const-context literal at compile time.
    pub const fn new_const(value: f32) -> Self {
        match Self::try_new(value) {
            Ok(v) => v,
            Err(_) => panic!("CrossoverFreqHigh literal out of range"),
        }
    }
}

/// A band's downward/upward compression threshold in dB, range `-80.0..=0.0` (docs/contracts.md §1).
///
/// Used for both `lower_threshold_db` and `upper_threshold_db`. `OttParams::validate`
/// enforces `lower_threshold_db < upper_threshold_db`, since that ordering spans
/// two fields and no single `Threshold` can express it on its own.
#[nutype(
    const_fn,
    validate(finite, greater_or_equal = -80.0, less_or_equal = 0.0),
    derive(Debug, Clone, Copy, PartialEq, PartialOrd)
)]
pub struct Threshold(f32);

impl Threshold {
    /// Returns the wrapped value.
    #[must_use]
    pub const fn get(self) -> f32 {
        self.into_inner()
    }

    /// For preset literals only. Panics (at compile time, in a `const` context) if `value` is invalid.
    ///
    /// # Panics
    ///
    /// Panics if `value` falls outside `-80.0..=0.0`.
    #[must_use]
    #[allow(clippy::panic)] // the only way to fail a const-context literal at compile time.
    pub const fn new_const(value: f32) -> Self {
        match Self::try_new(value) {
            Ok(v) => v,
            Err(_) => panic!("Threshold literal out of range"),
        }
    }
}

/// A band's makeup gain in dB, range `-40.0..=40.0` (docs/contracts.md §1).
///
/// Only ever constructed from preset literals (ADR 0006); not CLI-configurable,
/// so its fallible constructor is crate-private.
#[nutype(
    const_fn,
    validate(finite, greater_or_equal = -40.0, less_or_equal = 40.0),
    constructor(visibility = pub(crate)),
    derive(Debug, Clone, Copy, PartialEq)
)]
pub struct MakeupGain(f32);

impl MakeupGain {
    /// Returns the wrapped value.
    #[must_use]
    pub const fn get(self) -> f32 {
        self.into_inner()
    }

    /// For preset literals only. Panics (at compile time, in a `const` context) if `value` is invalid.
    ///
    /// # Panics
    ///
    /// Panics if `value` falls outside `-40.0..=40.0`.
    #[allow(clippy::panic)] // the only way to fail a const-context literal at compile time.
    pub(crate) const fn new_const(value: f32) -> Self {
        match Self::try_new(value) {
            Ok(v) => v,
            Err(_) => panic!("MakeupGain literal out of range"),
        }
    }
}

/// Lower bound of the allowed sample rate range (docs/contracts.md §1).
const MIN_SAMPLE_RATE_HZ: f32 = 8_000.0;
/// Upper bound of the allowed sample rate range (docs/contracts.md §1).
const MAX_SAMPLE_RATE_HZ: f32 = 384_000.0;

/// A sample rate in Hz, range `8_000.0..=384_000.0` (docs/contracts.md §1).
///
/// Not CLI-configurable: JACK assigns this at connection time and can
/// change it mid-session, so the only caller is `OttParams::validate`.
#[nutype(
    const_fn,
    validate(finite, greater_or_equal = MIN_SAMPLE_RATE_HZ, less_or_equal = MAX_SAMPLE_RATE_HZ),
    constructor(visibility = pub(crate)),
    derive(Debug, Clone, Copy, PartialEq)
)]
pub(crate) struct SampleRate(f32);

impl SampleRate {
    /// Returns the wrapped value.
    #[must_use]
    pub(crate) const fn get(self) -> f32 {
        self.into_inner()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn io_gain_rejects_nan_and_infinite() {
        assert!(IoGain::try_new(f32::NAN).is_err());
        assert!(IoGain::try_new(f32::INFINITY).is_err());
        assert!(IoGain::try_new(f32::NEG_INFINITY).is_err());
    }

    #[test]
    fn io_gain_rejects_out_of_range() {
        assert!(IoGain::try_new(24.1).is_err());
        assert!(IoGain::try_new(-24.1).is_err());
        assert!(IoGain::try_new(24.0).is_ok());
        assert!(IoGain::try_new(-24.0).is_ok());
    }

    #[test]
    fn io_gain_error_message_is_descriptive() {
        let err = IoGain::try_new(100.0).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("IoGain"), "error message was: {message}");
    }

    #[test]
    fn normalized_f32_rejects_nan_and_out_of_range() {
        assert!(NormalizedF32::try_new(f32::NAN).is_err());
        assert!(NormalizedF32::try_new(1.5).is_err());
        assert!(NormalizedF32::try_new(-0.1).is_err());
        assert!(NormalizedF32::try_new(0.0).is_ok());
        assert!(NormalizedF32::try_new(1.0).is_ok());
    }

    #[test]
    fn crossover_freq_low_rejects_out_of_range() {
        assert!(CrossoverFreqLow::try_new(39.9).is_err());
        assert!(CrossoverFreqLow::try_new(2000.1).is_err());
        assert!(CrossoverFreqLow::try_new(40.0).is_ok());
        assert!(CrossoverFreqLow::try_new(2000.0).is_ok());
    }

    #[test]
    fn crossover_freq_high_rejects_out_of_range() {
        assert!(CrossoverFreqHigh::try_new(399.9).is_err());
        assert!(CrossoverFreqHigh::try_new(16000.1).is_err());
        assert!(CrossoverFreqHigh::try_new(400.0).is_ok());
        assert!(CrossoverFreqHigh::try_new(16000.0).is_ok());
    }

    #[test]
    fn threshold_rejects_out_of_range() {
        assert!(Threshold::try_new(-80.1).is_err());
        assert!(Threshold::try_new(0.1).is_err());
        assert!(Threshold::try_new(-80.0).is_ok());
        assert!(Threshold::try_new(0.0).is_ok());
    }

    #[test]
    fn sample_rate_rejects_out_of_range_and_non_finite() {
        assert!(SampleRate::try_new(1_000.0).is_err());
        assert!(SampleRate::try_new(500_000.0).is_err());
        assert!(SampleRate::try_new(f32::NAN).is_err());
        assert!(SampleRate::try_new(48_000.0).is_ok());
    }

    #[test]
    fn from_str_parses_valid_values_and_rejects_invalid_ones() {
        assert_eq!("6.0".parse::<IoGain>().unwrap().get(), 6.0);
        assert!("not-a-number".parse::<IoGain>().is_err());
        assert!("100".parse::<IoGain>().is_err());
    }
}
