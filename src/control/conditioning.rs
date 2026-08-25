//! What layer B needs to know about the surface it is conditioning (see
//! [`crate::control`] for the layering).
//!
//! One type, [`ConditioningConfig`], holding the four numbers that come out
//! of one control surface's jitter measurement. They were separate constants
//! in three different shapes — a trait associated constant, a free constant
//! beside the Bela reading code, and two more inside the mapping layer — and
//! nothing owned the rule that ties them together.

use nutype::nutype;

/// A one-pole low-pass coefficient, in `0.0 < x <= 1.0`.
///
/// Zero is excluded because a filter that never moves is not a filter; one is
/// included because it is the legitimate "no filtering" setting.
///
/// Following the convention in `src/params/value.rs`: `try_new` is the
/// fallible entry point, `new_const` is for literals, `get()` is the accessor.
#[nutype(
    const_fn,
    validate(finite, greater = 0.0, less_or_equal = 1.0),
    derive(Debug, Clone, Copy, PartialEq)
)]
pub struct FilterCoefficient(f32);

impl FilterCoefficient {
    /// Returns the wrapped value.
    #[must_use]
    pub const fn get(self) -> f32 {
        self.into_inner()
    }

    /// For calibrated literals only. Panics (at compile time, in a `const` context) if `value` is invalid.
    ///
    /// # Panics
    ///
    /// Panics if `value` falls outside `0.0 < x <= 1.0`.
    #[must_use]
    #[allow(clippy::panic)] // the only way to fail a const-context literal at compile time.
    pub const fn new_const(value: f32) -> Self {
        match Self::try_new(value) {
            Ok(v) => v,
            Err(_) => panic!("FilterCoefficient literal out of range"),
        }
    }
}

/// A hysteresis deadband width, in [`PotPosition`](super::PotPosition) steps.
///
/// Zero is admitted: a surface quiet enough to need no deadband is a
/// legitimate configuration, and refusing it would be this type inventing a
/// minimum nobody measured.
#[nutype(
    const_fn,
    validate(finite, greater_or_equal = 0.0),
    derive(Debug, Clone, Copy, PartialEq)
)]
pub struct DeadbandCounts(f32);

impl DeadbandCounts {
    /// Returns the wrapped value.
    #[must_use]
    pub const fn get(self) -> f32 {
        self.into_inner()
    }

    /// For calibrated literals only. Panics (at compile time, in a `const` context) if `value` is invalid.
    ///
    /// # Panics
    ///
    /// Panics if `value` is negative or not finite.
    #[must_use]
    #[allow(clippy::panic)] // the only way to fail a const-context literal at compile time.
    pub const fn new_const(value: f32) -> Self {
        match Self::try_new(value) {
            Ok(v) => v,
            Err(_) => panic!("DeadbandCounts literal out of range"),
        }
    }
}

/// A debounce length, counted in reads rather than milliseconds.
///
/// At least one: zero would not shorten the debounce, it would remove it, and
/// a surface that wants no debounce is asking for a different type.
#[nutype(
    const_fn,
    validate(greater_or_equal = 1),
    derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)
)]
pub struct DebounceReads(u8);

impl DebounceReads {
    /// Returns the wrapped value.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.into_inner()
    }

    /// For calibrated literals only. Panics (at compile time, in a `const` context) if `value` is invalid.
    ///
    /// # Panics
    ///
    /// Panics if `value` is zero.
    #[must_use]
    #[allow(clippy::panic)] // the only way to fail a const-context literal at compile time.
    pub const fn new_const(value: u8) -> Self {
        match Self::try_new(value) {
            Ok(v) => v,
            Err(_) => panic!("DebounceReads literal out of range"),
        }
    }
}

/// A polling rate in hertz, strictly positive and finite.
#[nutype(
    const_fn,
    validate(finite, greater = 0.0),
    derive(Debug, Clone, Copy, PartialEq)
)]
pub struct PollHz(f32);

impl PollHz {
    /// Returns the wrapped value.
    #[must_use]
    pub const fn get(self) -> f32 {
        self.into_inner()
    }

    /// For calibrated literals only. Panics (at compile time, in a `const` context) if `value` is invalid.
    ///
    /// # Panics
    ///
    /// Panics if `value` is zero, negative, or not finite.
    #[must_use]
    #[allow(clippy::panic)] // the only way to fail a const-context literal at compile time.
    pub const fn new_const(value: f32) -> Self {
        match Self::try_new(value) {
            Ok(v) => v,
            Err(_) => panic!("PollHz literal out of range"),
        }
    }
}

/// How one control surface is conditioned: everything layer B has to be told
/// about the hardware it is reading.
///
/// # One type, because it is one measurement
///
/// All four values come out of the same session on the same breadboard: the
/// idle jitter of a motionless pot, the switch part's make/break behaviour,
/// and the rate the readings arrive at. Held separately, the rule that ties
/// them together belongs to nobody:
///
/// > **`deadband_counts` must be at least the standard deviation of the
/// > *raw* idle jitter.** The filter's noise gain at `filter_coefficient` is
/// > `sqrt(a / (2 - a))`, which is exactly 1/3 at 0.2, so three sigma of
/// > margin on the filtered signal reduces to one sigma of the raw. That is
/// > the form to re-check against when the pots, the wiring or the converter
/// > change (`docs/raspberry-pi/control-surface-verification.md`,
/// > `docs/bela/control-surface-verification.md`, ADR 0012).
///
/// # There is no `Default`
///
/// ADR 0012 decided the deadband is the surface's, not this layer's. A
/// `Default::default()` would be a way back to a figure nobody measured. Each
/// layer A supplies its own: a [`ControlSource`](super::ControlSource) through
/// its `CONDITIONING` constant, a layer A that is not a `ControlSource`
/// through a named constant in [`surfaces`](super::surfaces).
///
/// [`ConditioningConfig::new`] is not a second way in — it is the construction
/// boundary that keeps the four values in range. The named constants are the
/// calibrated values; `new` is what makes an invalid one impossible.
///
/// # `deadband_counts` is in counts, deliberately
///
/// The deadband is applied before normalisation, so counts is the unit it is
/// meaningful in. That makes this type depend on the converter scale
/// [`POT_POSITION_MAX`](super::POT_POSITION_MAX) fixes, which the seam out of
/// layer B deliberately does not
/// ([`PotTravel`](super::PotTravel) is a fraction of travel). The asymmetry is
/// intended: a deadband quoted as a fraction would have to be converted back
/// before it could be compared against anything.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConditioningConfig {
    filter_coefficient: FilterCoefficient,
    deadband_counts: DeadbandCounts,
    debounce_reads: DebounceReads,
    nominal_poll_hz: PollHz,
}

impl ConditioningConfig {
    /// Builds a configuration from four already-validated values.
    #[must_use]
    pub const fn new(
        filter_coefficient: FilterCoefficient,
        deadband_counts: DeadbandCounts,
        debounce_reads: DebounceReads,
        nominal_poll_hz: PollHz,
    ) -> Self {
        Self {
            filter_coefficient,
            deadband_counts,
            debounce_reads,
            nominal_poll_hz,
        }
    }

    /// The one-pole coefficient applied to each pot's raw count, per read.
    ///
    /// Per *read* rather than per millisecond: layer B has no clock, so the
    /// caller's poll rate is what turns this into a time constant.
    #[must_use]
    pub const fn filter_coefficient(self) -> f32 {
        self.filter_coefficient.get()
    }

    /// The hysteresis band a filtered reading must clear to be taken
    /// seriously, in [`PotPosition`](super::PotPosition) steps.
    ///
    /// Hysteresis, not quantisation: once a move clears the band the value
    /// jumps all the way, so repeated small moves in one direction cannot
    /// accumulate an offset.
    #[must_use]
    pub const fn deadband_counts(self) -> f32 {
        self.deadband_counts.get()
    }

    /// How many consecutive disagreeing reads the bypass switch's position
    /// must survive before it is believed.
    #[must_use]
    pub const fn debounce_reads(self) -> u8 {
        self.debounce_reads.get()
    }

    /// The rate this surface is *aimed* at being read at.
    ///
    /// **Nominal, not effective.** A host that reads on a divisor of its own
    /// block rate lands near this rather than on it — a Bela at 48 kHz with a
    /// period of 64 achieves 375 Hz against a nominal 500
    /// (`PollDecimator::effective_hz`). Converting
    /// [`debounce_reads`](Self::debounce_reads) into milliseconds follows the
    /// effective rate, not this one.
    ///
    /// It belongs here rather than beside the reading code because it only
    /// means anything alongside the other three: fifteen reads is 28 ms of
    /// hold at 500 Hz and 37 ms at 375, and the filter's time constant scales
    /// the same way.
    #[must_use]
    pub const fn nominal_poll_hz(self) -> f32 {
        self.nominal_poll_hz.get()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn a_filter_coefficient_spans_the_open_zero_to_one_range() {
        assert!(FilterCoefficient::try_new(0.0).is_err());
        assert!(FilterCoefficient::try_new(-0.1).is_err());
        assert!(FilterCoefficient::try_new(f32::NAN).is_err());
        assert!(FilterCoefficient::try_new(1.000_001).is_err());
        assert_eq!(FilterCoefficient::try_new(1.0).unwrap().get(), 1.0);
        assert_eq!(FilterCoefficient::try_new(0.2).unwrap().get(), 0.2);
    }

    #[test]
    fn a_deadband_admits_zero_but_not_a_negative_or_a_nan() {
        assert_eq!(DeadbandCounts::try_new(0.0).unwrap().get(), 0.0);
        assert!(DeadbandCounts::try_new(-1.0).is_err());
        assert!(DeadbandCounts::try_new(f32::NAN).is_err());
        assert!(DeadbandCounts::try_new(f32::INFINITY).is_err());
    }

    #[test]
    fn a_debounce_of_zero_is_not_a_short_debounce_but_none() {
        assert!(DebounceReads::try_new(0).is_err());
        assert_eq!(DebounceReads::try_new(1).unwrap().get(), 1);
    }

    #[test]
    fn a_poll_rate_must_be_positive_and_finite() {
        assert!(PollHz::try_new(0.0).is_err());
        assert!(PollHz::try_new(-500.0).is_err());
        assert!(PollHz::try_new(f32::INFINITY).is_err());
        assert!(PollHz::try_new(f32::NAN).is_err());
        assert_eq!(PollHz::try_new(500.0).unwrap().get(), 500.0);
    }

    #[test]
    fn the_getters_return_what_was_put_in() {
        let config = ConditioningConfig::new(
            FilterCoefficient::new_const(0.2),
            DeadbandCounts::new_const(3.0),
            DebounceReads::new_const(15),
            PollHz::new_const(500.0),
        );
        assert_eq!(config.filter_coefficient(), 0.2);
        assert_eq!(config.deadband_counts(), 3.0);
        assert_eq!(config.debounce_reads(), 15);
        assert_eq!(config.nominal_poll_hz(), 500.0);
    }
}
