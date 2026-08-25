//! What layer B needs to know about the surface it is conditioning (see
//! [`crate`] for the layering).
//!
//! [`SixPotBypassConditioner`] is the whole of it: jitter filter, hysteresis
//! deadband, switch debounce, and normalisation onto [`PotTravel`]. What comes
//! out is a [`ConditionedControls`], and what an effect *does* with it is
//! decided on the other side of that seam.
//!
//! [`ConditioningConfig`] holds the four numbers that come out of one control
//! surface's jitter measurement, so that the rule tying them together has an
//! owner.

use nutype::nutype;

use super::raw::{POT_POSITION_MAX, Pots, RawControls};

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

/// A hysteresis deadband width, in [`PotPosition`](crate::PotPosition) steps.
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
/// layer A supplies its own: a [`ControlSource`](crate::ControlSource) through
/// its `CONDITIONING` constant, a layer A that is not a `ControlSource`
/// through a named constant in [`surfaces`](crate::surfaces).
///
/// [`ConditioningConfig::new`] is not a second way in — it is the construction
/// boundary that keeps the four values in range. The named constants are the
/// calibrated values; `new` is what makes an invalid one impossible.
///
/// # `deadband_counts` is in counts, deliberately
///
/// The deadband is applied before normalisation, so counts is the unit it is
/// meaningful in. That makes this type depend on the converter scale
/// [`POT_POSITION_MAX`](crate::POT_POSITION_MAX) fixes, which the seam out of
/// layer B deliberately does not
/// ([`PotTravel`](crate::PotTravel) is a fraction of travel). The asymmetry is
/// intended: a deadband quoted as a fraction would have to be converted back
/// before it could be compared against anything, and the `deadband >= σ` rule
/// would compare a fraction against a count (ADR 0014).
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
    /// seriously, in [`PotPosition`](crate::PotPosition) steps.
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

/// How far along its travel a pot is sitting, in `0.0..=1.0`.
///
/// **The seam out of conditioning.** Counts belong to the converter and stop
/// here: an effect's assignment step should not have to know what an ADC's
/// full scale is to say what its knobs do. A `Pots<f32>` on the other side of
/// this seam would leave "counts or fraction?" to a comment.
///
/// Construction saturates rather than failing. Assignment runs inside Bela's
/// audio callback, where a panic aborts the process and there is nothing to
/// fall back to (docs/contracts.md §6), so
/// [`from_counts`](Self::from_counts) clamps and the invariant that the clamp
/// never actually fires is a test rather than a contract. `try_new` is
/// deliberately not public; `new_const` is for literals, as everywhere else
/// (`src/params/value.rs`).
#[nutype(
    const_fn,
    validate(finite, greater_or_equal = 0.0, less_or_equal = 1.0),
    constructor(visibility = pub(crate)),
    derive(Debug, Clone, Copy, PartialEq, PartialOrd)
)]
pub struct PotTravel(f32);

impl PotTravel {
    /// The bottom of the travel, which is also where a meaningless reading
    /// lands.
    pub const BOTTOM: Self = Self::new_const(0.0);

    /// Returns the wrapped value.
    #[must_use]
    pub const fn get(self) -> f32 {
        self.into_inner()
    }

    /// For literals only. Panics (at compile time, in a `const` context) if `value` is invalid.
    ///
    /// # Panics
    ///
    /// Panics if `value` falls outside `0.0..=1.0`.
    #[must_use]
    #[allow(clippy::panic)] // the only way to fail a const-context literal at compile time.
    pub const fn new_const(value: f32) -> Self {
        match Self::try_new(value) {
            Ok(v) => v,
            Err(_) => panic!("PotTravel literal out of range"),
        }
    }

    /// Converts a position on [`PotPosition`](crate::PotPosition)'s scale into
    /// a fraction of travel, saturating at both ends.
    ///
    /// Takes no full-scale argument. Passing one would add a correspondence
    /// nothing enforces — "any scale, as long as it is the one `PotPosition`
    /// uses" — so the scale stays fixed on the type. A surface with a wider
    /// converter moves `PotPosition`'s ceiling rather than this signature.
    ///
    /// `max` then `min` rather than `clamp`: `clamp` propagates NaN and
    /// asserts its bounds, while `f32::max` returns the non-NaN operand, so a
    /// reading that means nothing saturates to [`BOTTOM`](Self::BOTTOM) — the
    /// same quiet failure a negative reading gets.
    #[must_use]
    // Exactly the two notes clippy attaches to its own suggestion are why the
    // suggestion is refused here: `clamp` panics on non-ordered bounds and
    // returns NaN for a NaN input, and both are the failure this saturating
    // constructor exists to remove.
    #[allow(clippy::manual_clamp)]
    pub fn from_counts(counts: f32) -> Self {
        let travel = (counts / f32::from(POT_POSITION_MAX)).max(0.0).min(1.0);
        // Unreachable: the saturation above lands inside the range this type
        // validates. Falling back rather than unwrapping keeps the panic path
        // off Bela's callback (docs/contracts.md §6).
        Self::try_new(travel).unwrap_or(Self::BOTTOM)
    }
}

/// What conditioning hands to an effect's assignment step.
///
/// A named type rather than a tuple because it outlives this effect: it is
/// what every `assign` written against this control surface takes, so a tuple
/// here would be a tuple in every one of them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConditionedControls {
    /// Where each pot is sitting, as a fraction of its travel.
    pub pots: Pots<PotTravel>,
    /// The debounced position of the latching bypass switch.
    pub bypass_engaged: bool,
}

/// The debounced bypass switch.
///
/// The panel part is a mechanically *latching* (alternate-action) switch, so
/// its position **is** the bypass state: there is nothing to toggle and no
/// edge to detect, only a level to believe or disbelieve. Everything this type
/// does is therefore debounce — a slide or alternate-action contact makes and
/// breaks intermittently for as long as the hand is moving it, and each of
/// those intermediate makes would otherwise publish (see
/// [`ConditioningConfig::debounce_reads`]).
///
/// That the switch and the software agree by construction is the point of the
/// part change, not a side effect of it. With the old momentary switch the
/// panel could not be reconciled with the software state by looking at it, so
/// the startup state had to be invented — the first reading was declared a
/// baseline and the run came up un-bypassed however the switch sat. A latching
/// switch makes the two the same object, so a switch resting in the bypassed
/// position at startup comes up bypassed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BypassSwitch {
    /// The switch position currently believed: the last one to survive the
    /// surface's [`debounce_reads`](ConditioningConfig::debounce_reads)
    /// consecutive readings. This is the bypass state, not an input to one.
    engaged: bool,
    /// How many consecutive readings have disagreed with `engaged`.
    disagreements: u8,
}

impl BypassSwitch {
    /// Seeds the switch from the very first reading, taking its position at
    /// face value.
    ///
    /// There is no earlier position for the first reading to differ from, so
    /// there is nothing to debounce it against and no reason to distrust it —
    /// the same reasoning as the filter seeding itself from the first reading
    /// instead of fading in from zero. A switch already resting in the
    /// bypassed position therefore comes up bypassed, which is simply what the
    /// panel says.
    const fn seeded(engaged: bool) -> Self {
        Self {
            engaged,
            disagreements: 0,
        }
    }

    /// Feeds one reading in, adopting its position once the debounce believes it.
    const fn update(&mut self, engaged: bool, debounce_reads: u8) {
        if engaged == self.engaged {
            self.disagreements = 0;
            return;
        }

        // `saturating_add` rather than `+`: the counter is reset the moment it
        // reaches a threshold far below `u8::MAX`, so overflow is unreachable,
        // and saturating keeps that a property of the arithmetic rather than a
        // claim about the flow — which is what `update`'s no-panic proof needs.
        self.disagreements = self.disagreements.saturating_add(1);
        if self.disagreements < debounce_reads {
            return;
        }

        self.engaged = engaged;
        self.disagreements = 0;
    }
}

/// The conditioning state, absent until the first
/// [`SixPotBypassConditioner::update`].
#[derive(Debug, Clone, Copy, PartialEq)]
struct Conditioned {
    /// Low-pass filter state, in ADC counts.
    filtered: Pots<f32>,
    /// The deadband reference: the filtered values as of the last time a pot
    /// cleared the surface's
    /// [`deadband_counts`](ConditioningConfig::deadband_counts), in ADC
    /// counts, and always the real pot positions — never the bypassed ones.
    ///
    /// It is also, exactly, what was last handed out: it changes only when a
    /// pot clears the band, which is the only reason a pot ever publishes.
    /// That is what lets the gate below compare against it instead of keeping
    /// a copy of the last output.
    reference: Pots<f32>,
    /// The debounced bypass switch.
    bypass: BypassSwitch,
}

/// Conditions six pots and a latching bypass switch (layer B1).
///
/// Jitter filter, hysteresis deadband, switch debounce, and normalisation into
/// [`PotTravel`]. What the conditioned values *mean* is not decided here — see
/// [`update`](Self::update) for the two things it asks of the assignment that
/// consumes them.
///
/// Everything here is pure: no I/O, no threads, no clock, no allocation and no
/// panic, because a Bela host drives it directly from its real-time `render()`
/// callback with no transport layer in between (ADR 0009). It therefore holds
/// itself to the same prohibitions as the audio callback in
/// docs/contracts.md §6.
///
/// Having no clock is deliberate: the filter is defined per *read*, not per
/// millisecond, so this layer needs to know neither the poll interval nor the
/// sample rate. The caller's poll rate sets the effective time constant, which
/// is what [`ConditioningConfig::nominal_poll_hz`] records.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SixPotBypassConditioner {
    conditioning: ConditioningConfig,
    state: Option<Conditioned>,
}

impl SixPotBypassConditioner {
    /// Creates a conditioner for a surface with the given measured behaviour.
    #[must_use]
    pub const fn new(conditioning: ConditioningConfig) -> Self {
        Self {
            conditioning,
            state: None,
        }
    }

    /// Conditions one reading and returns the controls to publish, if any.
    ///
    /// Returns `Some` only when something moved: the filter first low-passes
    /// the raw counts, then the deadband compares the result against the
    /// reference, then the switch is debounced. A motionless pot and an
    /// untouched switch therefore yield `None` forever, which is what keeps a
    /// transport layer from being handed a fresh update on every poll for no
    /// reason.
    ///
    /// The very first call seeds the filter from the reading itself and
    /// publishes immediately, rather than starting from zero and fading in —
    /// the same reasoning as `OttProcessor::new` snapping its smoothers to
    /// their targets (docs/contracts.md §2). It seeds the switch the same way,
    /// by believing where it is resting (see [`BypassSwitch::seeded`]), so a
    /// run that starts with the switch in the bypassed position comes up
    /// bypassed.
    ///
    /// # What this asks of the assignment that consumes the result
    ///
    /// 1. **It must be a pure function of `(base, conditioned)`.** The gate
    ///    above decides "nothing moved" by looking at its own state, not at
    ///    the assigned parameters. That is sound only if equal inputs assign
    ///    equal outputs; an assignment that carried state of its own could be
    ///    gated out of a change it wanted to make.
    /// 2. **It must not panic and must not allocate** (docs/contracts.md §6).
    ///    On a Bela the assignment runs inside the audio callback, on the far
    ///    side of this seam. Nothing here can enforce that, so each effect's
    ///    assignment carries its own no-panic proof.
    ///
    /// The gate can err in one direction only. Because the reference changes
    /// only when a pot clears the band, and the switch only when the debounce
    /// believes a new position, a "nothing moved" answer is never wrong —
    /// there is no state a pure assignment could have turned into a different
    /// output. The other direction, publishing when the assigned parameters
    /// would have come out identical, is possible and harmless: applying an
    /// update is idempotent.
    // Proves this function can never panic, the same way
    // `OttProcessor::process` does (docs/contracts.md §6), checked by
    // `cargo test --release`. It matters here for the same reason: on Bela
    // this runs inside the real-time callback.
    #[cfg_attr(all(test, not(debug_assertions)), no_panic::no_panic)]
    pub fn update(&mut self, raw: RawControls) -> Option<ConditionedControls> {
        let counts = raw.pots.map(|count| f32::from(count.get()));
        // Copied out before `self.state` is borrowed mutably below: it is
        // `Copy` and never changes, so reading it by value avoids re-borrowing
        // `self` in the middle of the match.
        let conditioning = self.conditioning;

        let (reference, bypass_engaged) = match self.state.as_mut() {
            None => {
                let bypass = BypassSwitch::seeded(raw.bypass_engaged);
                self.state = Some(Conditioned {
                    filtered: counts,
                    reference: counts,
                    bypass,
                });
                // Unlike the old momentary latch, this is *not* a no-op on a
                // freshly seeded switch: a switch resting in the bypassed
                // position bypasses from the very first publish.
                (counts, bypass.engaged)
            }
            Some(state) => {
                let previous_reference = state.reference;
                let previous_bypass = state.bypass.engaged;

                // `filtered + a * (raw - filtered)` rather than the equivalent
                // `(1 - a) * filtered + a * raw`: written this way the result
                // stays inside the span of the two inputs even after rounding,
                // which is what keeps `PotTravel::from_counts` below off its
                // clamp.
                let filter_coefficient = conditioning.filter_coefficient();
                state.filtered = state.filtered.zip_with(counts, |filtered, raw| {
                    filter_coefficient.mul_add(raw - filtered, filtered)
                });

                // The deadband tracks the pots in their own units, and the
                // reference keeps following the real pots even while bypassed,
                // so disengaging cannot restore a stale position.
                let deadband_counts = conditioning.deadband_counts();
                state.reference =
                    state
                        .filtered
                        .zip_with(state.reference, |filtered, reference| {
                            if (filtered - reference).abs() >= deadband_counts {
                                filtered
                            } else {
                                reference
                            }
                        });

                state
                    .bypass
                    .update(raw.bypass_engaged, conditioning.debounce_reads());

                if state.reference == previous_reference && state.bypass.engaged == previous_bypass
                {
                    return None;
                }
                (state.reference, state.bypass.engaged)
            }
        };

        Some(ConditionedControls {
            pots: reference.map(PotTravel::from_counts),
            bypass_engaged,
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod tests {
    use super::*;
    use crate::raw::PotPosition;
    use crate::surfaces::GEM;

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

    /// Every pot at the same position, switch resting disengaged.
    fn uniform(counts: u16) -> RawControls {
        let at = PotPosition::new_const(0);
        let at = PotPosition::try_new(counts).unwrap_or(at);
        RawControls {
            pots: Pots {
                adc0: at,
                adc1: at,
                adc2: at,
                adc3: at,
                adc4: at,
                adc5: at,
            },
            bypass_engaged: false,
        }
    }

    /// The Bela Gem's, which is the narrower of the two measured surfaces.
    fn conditioner() -> SixPotBypassConditioner {
        SixPotBypassConditioner::new(GEM)
    }

    #[test]
    fn the_bottom_and_the_top_of_the_scale_are_the_ends_of_the_travel() {
        assert_eq!(PotTravel::from_counts(0.0).get(), 0.0);
        assert_eq!(
            PotTravel::from_counts(f32::from(POT_POSITION_MAX)).get(),
            1.0
        );
    }

    /// The saturation is a construction contract, not a claim that anything
    /// reaches it: `from_counts` runs inside Bela's audio callback, where
    /// there is nothing to fall back to and a panic ends the process.
    #[test]
    fn counts_outside_the_scale_saturate_rather_than_failing() {
        assert_eq!(PotTravel::from_counts(-1.0).get(), 0.0);
        assert_eq!(PotTravel::from_counts(f32::NAN).get(), 0.0);
        assert_eq!(PotTravel::from_counts(f32::NEG_INFINITY).get(), 0.0);
        assert_eq!(PotTravel::from_counts(2_000.0).get(), 1.0);
        assert_eq!(PotTravel::from_counts(f32::INFINITY).get(), 1.0);
    }

    /// The other half of that: nothing a conditioner actually produces gets
    /// anywhere near the clamp, because the filter stays inside the span of
    /// its inputs and its inputs are `PotPosition`s.
    #[test]
    fn conditioned_travel_never_reaches_the_saturation() {
        let mut conditioner = conditioner();
        // Full scale down to zero and back, which is the widest excursion the
        // filter can be asked to follow.
        for counts in (0..=POT_POSITION_MAX)
            .step_by(7)
            .chain((0..=POT_POSITION_MAX).step_by(11).rev())
        {
            if let Some(controls) = conditioner.update(uniform(counts)) {
                let travel = controls.pots.adc0.get();
                assert!(
                    (0.0..=1.0).contains(&travel),
                    "conditioned travel {travel} left the unit interval at {counts} counts"
                );
            }
        }
    }

    #[test]
    fn the_first_reading_publishes_and_a_motionless_surface_then_stays_quiet() {
        let mut conditioner = conditioner();
        let first = conditioner.update(uniform(400));
        assert!(first.is_some(), "the first reading must publish");
        assert_eq!(first.map(|c| c.pots.adc0.get()), Some(400.0 / 1023.0));

        for _ in 0..100 {
            assert!(
                conditioner.update(uniform(400)).is_none(),
                "a motionless surface must publish only once"
            );
        }
    }

    /// A run that starts with the switch already thrown comes up bypassed:
    /// the part latches, so its position is the state rather than a stimulus.
    #[test]
    fn a_switch_resting_bypassed_is_believed_immediately() {
        let mut conditioner = conditioner();
        let raw = RawControls {
            bypass_engaged: true,
            ..uniform(0)
        };
        assert_eq!(
            conditioner.update(raw).map(|c| c.bypass_engaged),
            Some(true)
        );
    }

    /// `update` runs inside Bela's audio callback, so it must be provably
    /// panic-free on its own — not merely as part of whatever an effect's
    /// assignment does with the result (docs/contracts.md §6).
    ///
    /// A proof-only test: `#[no_panic]` is checked at link time under
    /// `cargo test --release`, so what this body does matters far less than
    /// that it reaches the function at all. It drives the two arms `update`
    /// has — the seeding call and the steady state — and feeds both ends of
    /// the scale through the second.
    #[test]
    fn conditioning_one_reading_cannot_panic() {
        #[cfg_attr(all(test, not(debug_assertions)), no_panic::no_panic)]
        fn condition(conditioner: &mut SixPotBypassConditioner, raw: RawControls) -> bool {
            conditioner.update(raw).is_some()
        }

        let mut conditioner = conditioner();
        let published = [0, POT_POSITION_MAX, 512, 0]
            .into_iter()
            .filter(|&counts| condition(&mut conditioner, uniform(counts)))
            .count();
        assert!(published > 0, "the proof must have reached the function");
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
