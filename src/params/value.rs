//! Value objects for individual parameter fields (docs/contracts.md §1).
//!
//! Each type exposes at most two constructors:
//! - `new(...) -> Result<Self, String>`: the only public, runtime-checked way to
//!   build one from an untrusted value (CLI input via `FromStr`, or a library
//!   caller). Only exists where such a construction path actually exists. The
//!   error is a plain `String` rather than `ConfigError` because these constructors
//!   never take a field-name label (see the construction principle below); the
//!   caller that already knows which field it's assigning attaches that context.
//! - `new_const(...) -> Self`: for the fixed preset literals only. Asserts the
//!   same invariants, so an invalid literal fails to compile rather than
//!   slipping past a forgotten runtime check.
//!
//! No constructor here takes an external label/context parameter alongside the
//! value being validated: a value object's constructor accepts only data that is
//! part of its own invariant, never free-text metadata that nothing enforces
//! coherence for.
//!
//! Most of these types share one shape — an inclusive `[min, max]` range check
//! at both `new` and `new_const`, plus `get`, and (for the CLI-configurable
//! ones) `FromStr`/`Display` — so that shape is generated once by the
//! `bounded_f32!` macro below rather than hand-rolled per type. `PositiveF32`
//! has an asymmetric, open-ended bound (`> 0.0`, no upper limit) that doesn't
//! fit the macro's closed-range shape, so it stays hand-written.

use std::fmt::{Display, Formatter, Result as FmtResult};
use std::str::FromStr;

/// Generates a bounded-`f32` newtype: a private inner `f32`, `new`/`new_const`
/// range checks against `[$min, $max]` sharing the same bounds (so they can't
/// drift out of sync), and `get`. Add `, str` to also generate `FromStr` +
/// `Display` for types that are CLI-configurable, and/or `, derive($extra)`
/// to add an extra derive (e.g. `PartialOrd`, for types compared across
/// fields by `OttParams::validate`). Doc comments attached to the invocation
/// become the struct's doc comment, same as a hand-written type.
///
/// Each combination of trailing `derive(...)`/`str` markers is its own arm
/// (rather than two stacked `$(...)?` groups in one arm) because
/// `macro_rules!` can't unambiguously decide, from one token of lookahead,
/// whether an optional group that starts with a comma is present or absent
/// when two such groups are adjacent.
macro_rules! bounded_f32 {
    (
        $(#[$doc:meta])*
        $name:ident, $min:expr, $max:expr, $err:literal, str
    ) => {
        bounded_f32!(@struct $(#[$doc])* $name, $min, $max, $err,);
        bounded_f32!(@str_impls $name);
    };

    (
        $(#[$doc:meta])*
        $name:ident, $min:expr, $max:expr, $err:literal, derive($($extra:ident),+ $(,)?), str
    ) => {
        bounded_f32!(@struct $(#[$doc])* $name, $min, $max, $err, $($extra),+);
        bounded_f32!(@str_impls $name);
    };

    (
        $(#[$doc:meta])*
        $name:ident, $min:expr, $max:expr, $err:literal, derive($($extra:ident),+ $(,)?)
    ) => {
        bounded_f32!(@struct $(#[$doc])* $name, $min, $max, $err, $($extra),+);
    };

    (
        @struct
        $(#[$doc:meta])*
        $name:ident, $min:expr, $max:expr, $err:literal, $($extra:ident),*
    ) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq $(, $extra)*)]
        pub struct $name(f32);

        impl $name {
            /// Validates and wraps `value`.
            ///
            /// # Errors
            ///
            /// Returns `String` if `value` falls outside this type's valid range.
            pub fn new(value: f32) -> Result<Self, String> {
                if !($min..=$max).contains(&value) {
                    return Err(format!("{}, got {value}", $err));
                }

                Ok(Self(value))
            }

            /// For preset literals only. Panics (at compile time, in a `const` context) if `value` is invalid.
            ///
            /// # Panics
            ///
            /// Panics if `value` falls outside this type's valid range.
            #[must_use]
            pub const fn new_const(value: f32) -> Self {
                assert!(value >= $min && value <= $max, $err);
                Self(value)
            }

            /// Returns the wrapped value.
            #[must_use]
            pub const fn get(self) -> f32 {
                self.0
            }
        }
    };

    (@str_impls $name:ident) => {
        impl FromStr for $name {
            type Err = String;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                f32::from_str(s)
                    .map_err(|e| e.to_string())
                    .and_then(Self::new)
            }
        }

        impl Display for $name {
            fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
                self.0.fmt(f)
            }
        }
    };
}

bounded_f32!(
    /// A gain value in dB, range `-24.0..=24.0` (docs/contracts.md §1).
    ///
    /// Used for the pre-split and post-sum gains, which are CLI-configurable.
    IoGain, -24.0, 24.0, "gain must be in [-24, 24]", str
);

/// A positive duration in milliseconds (docs/contracts.md §1).
///
/// Used for each band's base attack/release time at `time = 0.5`. Only ever
/// constructed from preset literals (ADR 0006); not CLI-configurable. Its
/// bound (`> 0.0`, no upper limit) doesn't fit `bounded_f32!`'s closed-range
/// shape, so it's hand-written rather than macro-generated.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PositiveF32(f32);

impl PositiveF32 {
    /// For preset literals only. Panics (at compile time, in a `const` context) if `value` is invalid.
    ///
    /// # Panics
    ///
    /// Panics if `value` is not strictly greater than `0.0`.
    pub(crate) const fn new_const(value: f32) -> Self {
        assert!(value > 0.0, "PositiveF32 literal out of range");
        Self(value)
    }

    /// Returns the wrapped value.
    #[must_use]
    pub const fn get(self) -> f32 {
        self.0
    }
}

bounded_f32!(
    /// A normalized fraction in `0.0..=1.0` (docs/contracts.md §1).
    ///
    /// Shared by the dry/wet mix, the attack/release time multiplier, the
    /// upward/downward multipliers, and each band's compression amounts.
    NormalizedF32, 0.0, 1.0, "value must be in [0, 1]", str
);

bounded_f32!(
    /// The low/mid crossover frequency in Hz, range `40.0..=2000.0` (docs/contracts.md §1).
    ///
    /// Combined with `CrossoverFreqHigh` by `OttParams::validate`, which enforces
    /// the octave-separation invariant that no single field can express on its own.
    CrossoverFreqLow, 40.0, 2000.0, "value must be in [40, 2000]", derive(PartialOrd), str
);

bounded_f32!(
    /// The mid/high crossover frequency in Hz, range `400.0..=16000.0` (docs/contracts.md §1).
    ///
    /// Combined with `CrossoverFreqLow` by `OttParams::validate`, which enforces
    /// the octave-separation invariant that no single field can express on its own.
    CrossoverFreqHigh, 400.0, 16000.0, "value must be in [400, 16000]", derive(PartialOrd), str
);

bounded_f32!(
    /// A band's downward/upward compression threshold in dB, range `-80.0..=0.0` (docs/contracts.md §1).
    ///
    /// Used for both `lower_threshold_db` and `upper_threshold_db`. `OttParams::validate`
    /// enforces `lower_threshold_db < upper_threshold_db`, since that ordering spans
    /// two fields and no single `Threshold` can express it on its own.
    Threshold, -80.0, 0.0, "threshold must be in [-80, 0]", derive(PartialOrd)
);

/// A band's makeup gain in dB, range `-40.0..=40.0` (docs/contracts.md §1).
///
/// Only ever constructed from preset literals (ADR 0006); not CLI-configurable.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MakeupGain(f32);

impl MakeupGain {
    /// For preset literals only. Panics (at compile time, in a `const` context) if `value` is invalid.
    pub(crate) const fn new_const(value: f32) -> Self {
        assert!(
            value >= -40.0 && value <= 40.0,
            "MakeupGain literal out of range"
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
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn io_gain_rejects_nan_and_infinite() {
        assert!(IoGain::new(f32::NAN).is_err());
        assert!(IoGain::new(f32::INFINITY).is_err());
        assert!(IoGain::new(f32::NEG_INFINITY).is_err());
    }

    #[test]
    fn io_gain_rejects_out_of_range() {
        assert!(IoGain::new(24.1).is_err());
        assert!(IoGain::new(-24.1).is_err());
        assert!(IoGain::new(24.0).is_ok());
        assert!(IoGain::new(-24.0).is_ok());
    }

    #[test]
    fn io_gain_error_message_includes_the_offending_value() {
        let err = IoGain::new(100.0).unwrap_err();
        assert!(err.contains("100"), "error message was: {err}");
    }

    #[test]
    fn normalized_f32_rejects_nan_and_out_of_range() {
        assert!(NormalizedF32::new(f32::NAN).is_err());
        assert!(NormalizedF32::new(1.5).is_err());
        assert!(NormalizedF32::new(-0.1).is_err());
        assert!(NormalizedF32::new(0.0).is_ok());
        assert!(NormalizedF32::new(1.0).is_ok());
    }

    #[test]
    fn crossover_freq_low_rejects_out_of_range() {
        assert!(CrossoverFreqLow::new(39.9).is_err());
        assert!(CrossoverFreqLow::new(2000.1).is_err());
        assert!(CrossoverFreqLow::new(40.0).is_ok());
        assert!(CrossoverFreqLow::new(2000.0).is_ok());
    }

    #[test]
    fn crossover_freq_high_rejects_out_of_range() {
        assert!(CrossoverFreqHigh::new(399.9).is_err());
        assert!(CrossoverFreqHigh::new(16000.1).is_err());
        assert!(CrossoverFreqHigh::new(400.0).is_ok());
        assert!(CrossoverFreqHigh::new(16000.0).is_ok());
    }

    #[test]
    fn threshold_rejects_out_of_range() {
        assert!(Threshold::new(-80.1).is_err());
        assert!(Threshold::new(0.1).is_err());
        assert!(Threshold::new(-80.0).is_ok());
        assert!(Threshold::new(0.0).is_ok());
    }

    #[test]
    fn from_str_parses_valid_values_and_rejects_invalid_ones() {
        assert_eq!("6.0".parse::<IoGain>().unwrap().get(), 6.0);
        assert!("not-a-number".parse::<IoGain>().is_err());
        assert!("100".parse::<IoGain>().is_err());
    }
}
