//! Layer B of the control surface: raw counts to a complete [`OttParams`]
//! (see [`crate::control`] for the layering).
//!
//! Everything here is pure — no I/O, no threads, no clock — and, more
//! importantly, allocation-free and panic-free, because a Bela port drives
//! this module directly from its real-time `render()` callback with no
//! transport layer in between (ADR 0009). It therefore holds itself to the
//! same prohibitions as the audio callback in docs/contracts.md §6.
//!
//! Having no clock is deliberate: the filter below is defined per *read*, not
//! per millisecond, so this layer needs to know neither the poll interval nor
//! the sample rate. The caller's poll rate sets the effective time constant.

use crate::params::{NormalizedF32, OttParams};

use super::raw::{ADC_MAX_COUNT, Pots, RawControls};

/// One-pole low-pass coefficient applied to each pot's raw count, per read.
///
/// Idle jitter on the assembled hardware, measured with `pi-tools` over 300
/// readings per position on all four channels, has a standard deviation of
/// 5.23–6.48 counts out of 1023 with the pots at full travel and 3.14–4.09
/// counts at mid travel. The worst case is therefore σ ≈ 6.5, at the end
/// stop rather than at mid scale — the opposite of what a divider's source
/// impedance alone would predict, which is why it was measured rather than
/// assumed.
///
/// At 0.2 the filter attenuates white noise by exactly
/// `sqrt(a / (2 - a))` = 1/3, taking that worst case down to σ ≈ 2.2 counts,
/// while reaching 63% of a step in 5 reads and 90% in 11, so a deliberate
/// knob turn still tracks the hand that makes it.
///
/// The value is intentionally not lower. This layer only has to reject
/// jitter: every parameter it publishes is re-smoothed per sample by the DSP
/// with a 20 ms time constant (docs/architecture.md), so zipper noise is
/// already handled downstream and there is nothing to gain from extra lag here.
const FILTER_COEFFICIENT: f32 = 0.2;

/// Hysteresis deadband against the last published value, in ADC counts.
///
/// Eight counts is 3.7 times the σ ≈ 2.2 that survives
/// [`FILTER_COEFFICIENT`], so a motionless pot is quiet rather than provably
/// silent: the residual is noise, and an excursion past 3.7σ still publishes
/// every so often. That costs nothing audible — the value published then
/// differs by under 1% of travel, and the DSP smooths it over 20 ms.
///
/// Since this filter's noise gain is exactly 1/3, keeping three sigma of
/// margin reduces to `DEADBAND_COUNTS >= σ` of the *raw* jitter, which is
/// the form to re-check against if the pots, the wiring, or the ADC change.
///
/// As a fraction of travel the band is 8/1023 ≈ 0.8%, leaving roughly 128
/// distinct positions across a pot's full sweep — finer than a hand can hold,
/// and far finer than the parameters' audible resolution.
///
/// It is hysteresis, not quantization: once a move clears the band, the
/// published value jumps all the way to the filtered value, so repeated small
/// moves in one direction cannot accumulate an offset.
const DEADBAND_COUNTS: f32 = 8.0;

/// How many consecutive identical readings the bypass switch's level must
/// survive before it is believed.
///
/// Counted in reads rather than milliseconds for the same reason as
/// [`FILTER_COEFFICIENT`]: this layer has no clock. At the
/// [`DEFAULT_POLL_INTERVAL`](crate::control::DEFAULT_POLL_INTERVAL) of 2 ms
/// (500 Hz), five reads means the contact has to hold its new level for 8 ms
/// after the first read that sees it — comfortably past the single-digit
/// milliseconds a small momentary switch's contacts bounce for, and past the
/// worst case for the candidate panel part.
///
/// The cost is latency: an edge is acted on at the fifth read, so up to 10 ms
/// (five poll intervals — four of debounce plus up to one interval of
/// sampling delay) passes between the finger landing and `depth` reaching 0.
/// That is an order of magnitude below the ~50 ms at which a foot- or
/// finger-operated switch starts to feel late, and the DSP's 20 ms smoothing
/// dominates what is actually heard anyway (docs/architecture.md).
///
/// Raising the poll rate shortens the latency and weakens the debounce in
/// exact proportion, which is the trade to re-make if the interval changes.
const BYPASS_DEBOUNCE_READS: u8 = 5;

/// The debounced bypass switch and the latch it drives.
///
/// The panel part is a *momentary* push switch, so "bypassed" cannot be the
/// switch's position — it is software state that a press toggles. A release
/// edge is deliberately inert: the switch springs back after every press, so
/// acting on both edges would toggle twice per press and leave the latch
/// exactly where it started.
///
/// What the latch does when engaged is an *effect* bypass: `depth = 0`, which
/// keeps the signal on the split-and-reconstruct path and disables only the
/// dynamics. It is not a raw dry signal, because the raw input and the
/// reconstructed signal do not share a phase response and crossfading between
/// them would comb-filter (ADR 0004, docs/contracts.md §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BypassLatch {
    /// The switch level currently believed: the last one to survive
    /// [`BYPASS_DEBOUNCE_READS`] consecutive readings.
    settled: bool,
    /// How many consecutive readings have disagreed with `settled`.
    disagreements: u8,
    /// Whether the effect bypass is engaged.
    engaged: bool,
}

impl BypassLatch {
    /// Seeds the latch from the very first reading, un-bypassed.
    ///
    /// The first reading is a baseline, not an edge. A switch that happens to
    /// be held down as the process starts therefore comes up with the effect
    /// *active* — the same reasoning as the filter seeding itself from the
    /// first reading instead of fading in from zero: there is no earlier state
    /// to have moved away from.
    const fn seeded(pressed: bool) -> Self {
        Self {
            settled: pressed,
            disagreements: 0,
            engaged: false,
        }
    }

    /// Feeds one reading in, toggling the latch on a debounced press edge.
    const fn update(&mut self, pressed: bool) {
        if pressed == self.settled {
            self.disagreements = 0;
            return;
        }

        // `saturating_add` rather than `+`: the counter is reset the moment it
        // reaches a threshold far below `u8::MAX`, so overflow is unreachable,
        // and saturating keeps that a property of the arithmetic rather than a
        // claim about the flow — which is what `update`'s no-panic proof needs.
        self.disagreements = self.disagreements.saturating_add(1);
        if self.disagreements < BYPASS_DEBOUNCE_READS {
            return;
        }

        self.settled = pressed;
        self.disagreements = 0;
        if pressed {
            self.engaged = !self.engaged;
        }
    }

    /// Applies the latch to a conditioned pot set.
    ///
    /// Only `depth` is touched, and only by being forced to 0: the Time,
    /// Upward and Downward pots keep working while bypassed, so releasing the
    /// bypass brings back an effect set up meanwhile rather than a stale one.
    const fn applied_to(self, conditioned: Pots<f32>) -> Pots<f32> {
        if self.engaged {
            Pots {
                depth: 0.0,
                ..conditioned
            }
        } else {
            conditioned
        }
    }
}

/// The conditioning and latch state, absent until the first
/// [`ControlMapping::update`].
///
/// `reference` and `published` are deliberately two fields rather than one.
/// The deadband is hysteresis against where the *pots* were last taken
/// seriously, so it has to keep tracking the real Depth pot even while
/// bypassed — otherwise the band would be compared against a forced zero and
/// un-bypassing would jump. The publish decision, in contrast, has to be made
/// against what the caller was actually last handed, or turning Depth while
/// bypassed would publish a snapshot identical to the previous one on every
/// poll.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Conditioned {
    /// Low-pass filter state, in ADC counts.
    filtered: Pots<f32>,
    /// The deadband reference: the filtered values as of the last time a pot
    /// cleared [`DEADBAND_COUNTS`], in ADC counts, and always the real pot
    /// positions — never the bypassed ones.
    reference: Pots<f32>,
    /// The values behind the most recent published snapshot, in ADC counts,
    /// after [`BypassLatch::applied_to`].
    published: Pots<f32>,
    /// The debounced switch and the bypass latch.
    bypass: BypassLatch,
}

/// Turns raw control-surface readings into complete [`OttParams`] (layer B).
///
/// The four pot-driven fields (`global.depth`, `global.time`, `global.upward`,
/// `global.downward`) are owned by the hardware from the first read onward;
/// the CLI values for those four only describe the state before any reading
/// arrives. Every other field of the base parameters — input/output gain,
/// crossover pair, all per-band values — is passed through unchanged, since
/// no pot is wired to it.
///
/// The bypass switch overrides one of those four: while [`BypassLatch`] is
/// engaged the published `global.depth` is 0 regardless of the Depth pot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ControlMapping {
    base: OttParams,
    state: Option<Conditioned>,
}

impl ControlMapping {
    /// Creates a mapping over the CLI-supplied parameter set.
    #[must_use]
    pub const fn new(base: OttParams) -> Self {
        Self { base, state: None }
    }

    /// Conditions one reading and returns the parameters to publish, if any.
    ///
    /// Returns `Some` only when the snapshot this would publish differs from
    /// the one it published last: `FILTER_COEFFICIENT` first low-passes the
    /// raw counts, then `DEADBAND_COUNTS` compares the result against the
    /// deadband reference, then the bypass latch is applied. A motionless pot
    /// and an untouched switch therefore yield `None` forever, which is what
    /// keeps the transport layer from being handed a fresh snapshot on every
    /// poll for no reason.
    ///
    /// The very first call seeds the filter from the reading itself and
    /// publishes immediately, rather than starting from zero and fading in —
    /// the same reasoning as `OttProcessor::new` snapping its smoothers to
    /// their targets (docs/contracts.md §2). It seeds the switch the same way,
    /// as a baseline rather than an edge (see [`BypassLatch::seeded`]).
    ///
    /// [`RawControls::bypass_pressed`] drives [`BypassLatch`]: a debounced
    /// press toggles an *effect* bypass, which publishes `depth = 0` and
    /// leaves the other three pots alone (ADR 0004, docs/contracts.md §4).
    /// Because the deadband keeps tracking the real Depth pot underneath,
    /// un-bypassing republishes the pot's position *now*, not the position it
    /// held when the bypass was engaged.
    // Proves this function can never panic, the same way `OttProcessor::process`
    // does (docs/contracts.md §6), checked by `cargo test --release`. It matters
    // here for the same reason: on Bela this runs inside the real-time callback.
    // The tests below already call it, so no proof-only test is needed.
    #[cfg_attr(all(test, not(debug_assertions)), no_panic::no_panic)]
    pub fn update(&mut self, raw: RawControls) -> Option<OttParams> {
        let counts = raw.pots.map(|count| f32::from(count.get()));

        let published = match self.state.as_mut() {
            None => {
                let bypass = BypassLatch::seeded(raw.bypass_pressed);
                // `applied_to` is a no-op on a freshly seeded latch, which is
                // exactly the point: it is written here anyway so that
                // `published` means the same thing in both arms.
                let published = bypass.applied_to(counts);
                self.state = Some(Conditioned {
                    filtered: counts,
                    reference: counts,
                    published,
                    bypass,
                });
                published
            }
            Some(state) => {
                // `filtered + a * (raw - filtered)` rather than the equivalent
                // `(1 - a) * filtered + a * raw`: written this way the result
                // stays inside the span of the two inputs even after rounding,
                // which is what keeps the normalized value below in `0.0..=1.0`.
                state.filtered = state.filtered.zip_with(counts, |filtered, raw| {
                    FILTER_COEFFICIENT.mul_add(raw - filtered, filtered)
                });

                // Against `reference`, never against `published`: while
                // bypassed the published depth is a forced zero, and comparing
                // the band against that would make every position of the Depth
                // pot look like a large move away from the bottom of its travel.
                state.reference =
                    state
                        .filtered
                        .zip_with(state.reference, |filtered, reference| {
                            if (filtered - reference).abs() >= DEADBAND_COUNTS {
                                filtered
                            } else {
                                reference
                            }
                        });

                state.bypass.update(raw.bypass_pressed);

                // Against `published`, never against `reference`: a Depth turn
                // made while bypassed moves the reference but not the snapshot,
                // and re-publishing an identical snapshot on every poll is the
                // exact thing the deadband exists to prevent.
                let next = state.bypass.applied_to(state.reference);
                if next == state.published {
                    return None;
                }
                state.published = next;
                next
            }
        };

        Some(self.params_with_pots(published))
    }

    /// Overlays the four pot-driven fields onto the base parameters.
    fn params_with_pots(&self, published: Pots<f32>) -> OttParams {
        let global = self.base.global;
        let base_pots = Pots {
            depth: global.depth,
            time: global.time,
            upward: global.upward,
            downward: global.downward,
        };

        let normalized = published.zip_with(base_pots, |counts, base| {
            // `counts` is a filtered value bounded by the raw counts that
            // produced it, so `counts / ADC_MAX_COUNT` is finite and within
            // `0.0..=1.0` and `NormalizedF32` construction cannot fail. The
            // error arm is unreachable; it falls back to the base value rather
            // than unwrapping, because this runs on Bela's real-time callback
            // path, where a panic is prohibited (docs/contracts.md §6).
            NormalizedF32::try_new(counts / f32::from(ADC_MAX_COUNT)).unwrap_or(base)
        });

        let mut params = self.base;
        params.global.depth = normalized.depth;
        params.global.time = normalized.time;
        params.global.upward = normalized.upward;
        params.global.downward = normalized.downward;
        params
    }
}

#[cfg(test)]
// These tests compare exact conditioned values against hand-computed ones, so
// float equality is the intent rather than an accident. The indexing is a
// fixed-length jitter pattern walked modulo its own length, and
// `disallowed_macros` covers `prop_assert!`, which expands to `format!`.
#[allow(
    clippy::float_cmp,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::disallowed_macros
)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::control::AdcCount;
    use crate::params::Preset;

    const PROPERTY_CASES: u32 = 128;
    const SAMPLE_RATE: f32 = 48_000.0;
    /// Long enough for the filter to converge on a held position and for the
    /// deadband to have taken it, at `FILTER_COEFFICIENT`'s 11-read 90% point.
    const HELD_READS: usize = 100;

    fn count(raw: u16) -> AdcCount {
        AdcCount::try_new(raw).unwrap()
    }

    fn reading(depth: u16, time: u16, upward: u16, downward: u16) -> RawControls {
        RawControls {
            pots: Pots {
                depth: count(depth),
                time: count(time),
                upward: count(upward),
                downward: count(downward),
            },
            bypass_pressed: false,
        }
    }

    fn uniform(raw: u16) -> RawControls {
        reading(raw, raw, raw, raw)
    }

    fn normalized(raw: u16) -> f32 {
        f32::from(raw) / f32::from(ADC_MAX_COUNT)
    }

    /// The same reading with the switch held down or released.
    fn switched(raw: RawControls, bypass_pressed: bool) -> RawControls {
        RawControls {
            bypass_pressed,
            ..raw
        }
    }

    /// Feeds one reading in `reads` times, returning the last snapshot published.
    fn feed(mapping: &mut ControlMapping, raw: RawControls, reads: usize) -> Option<OttParams> {
        let mut last = None;
        for _ in 0..reads {
            if let Some(params) = mapping.update(raw) {
                last = Some(params);
            }
        }
        last
    }

    /// Feeds a switch level in exactly [`BYPASS_DEBOUNCE_READS`] times, which
    /// is the shortest run the debounce believes.
    fn settle(mapping: &mut ControlMapping, raw: RawControls) -> Option<OttParams> {
        feed(mapping, raw, usize::from(BYPASS_DEBOUNCE_READS))
    }

    #[test]
    fn first_update_publishes_the_reading_without_fading_in() {
        let mut mapping = ControlMapping::new(Preset::SafeStart.params());
        let params = mapping
            .update(reading(0, 512, 1023, 250))
            .expect("the first reading must publish");

        assert_eq!(params.global.depth.get(), normalized(0));
        assert_eq!(params.global.time.get(), normalized(512));
        assert_eq!(params.global.upward.get(), normalized(1023));
        assert_eq!(params.global.downward.get(), normalized(250));
    }

    #[test]
    fn repeated_identical_readings_publish_only_once() {
        let mut mapping = ControlMapping::new(Preset::SafeStart.params());
        assert!(
            mapping.update(uniform(500)).is_some(),
            "the first reading must publish"
        );
        for _ in 0..100 {
            assert!(
                mapping.update(uniform(500)).is_none(),
                "an unchanged reading must not publish"
            );
        }
    }

    #[test]
    fn idle_jitter_within_the_deadband_never_publishes() {
        let mut mapping = ControlMapping::new(Preset::SafeStart.params());
        assert!(
            mapping.update(uniform(500)).is_some(),
            "the first reading must publish"
        );

        // Deliberately harsher than the hardware: the measured idle spread is
        // 32 counts peak-to-peak over 300 readings (σ ≈ 6.5, see
        // `FILTER_COEFFICIENT`), so ±20 exercises excursions beyond anything
        // a motionless pot was seen to produce.
        let jitter = [20_i32, -20, 15, -15, 10, -10, 18, -12, -20, 20];
        for step in 0..200 {
            let offset = jitter[step % jitter.len()];
            let raw = u16::try_from(500 + offset).unwrap();
            assert!(
                mapping.update(uniform(raw)).is_none(),
                "idle jitter of {offset} counts must not publish"
            );
        }
    }

    #[test]
    fn a_deliberate_turn_publishes_and_moves_toward_the_new_position() {
        let mut mapping = ControlMapping::new(Preset::SafeStart.params());
        let start = mapping
            .update(uniform(200))
            .expect("the first reading must publish");

        // Hold the pot at its new position and poll; the filter needs a few
        // reads to clear the deadband, and must then approach 800 monotonically.
        let mut last = start.global.depth.get();
        let mut publishes = 0;
        for _ in 0..100 {
            if let Some(params) = mapping.update(uniform(800)) {
                let depth = params.global.depth.get();
                assert!(
                    depth > last,
                    "depth {depth} must increase toward the new position from {last}"
                );
                assert!(
                    depth <= normalized(800),
                    "depth {depth} must not overshoot the raw position"
                );
                last = depth;
                publishes += 1;
            }
        }

        assert!(publishes > 0, "a full-scale turn must publish");
        assert!(
            last > normalized(790),
            "depth {last} must converge on the new position"
        );
    }

    #[test]
    fn a_turn_downward_publishes_a_lower_value() {
        let mut mapping = ControlMapping::new(Preset::SafeStart.params());
        mapping
            .update(uniform(800))
            .expect("the first reading must publish");

        let mut published = None;
        for _ in 0..100 {
            if let Some(params) = mapping.update(uniform(200)) {
                published = Some(params.global.depth.get());
            }
        }

        let depth = published.expect("a full-scale turn must publish");
        assert!(
            depth < normalized(210),
            "depth {depth} must converge downward on the new position"
        );
    }

    #[test]
    fn only_the_four_pot_fields_are_replaced() {
        let base = Preset::Default.params();
        let mut mapping = ControlMapping::new(base);
        let params = mapping
            .update(reading(10, 20, 30, 40))
            .expect("the first reading must publish");

        assert_eq!(params.global.input_gain_db, base.global.input_gain_db);
        assert_eq!(params.global.output_gain_db, base.global.output_gain_db);
        assert_eq!(params.global.crossover, base.global.crossover);
        assert_eq!(params.bands, base.bands);

        assert_ne!(params.global.depth, base.global.depth);
        assert_ne!(params.global.time, base.global.time);
        assert_ne!(params.global.upward, base.global.upward);
        assert_ne!(params.global.downward, base.global.downward);
    }

    /// The descendant of `the_bypass_switch_is_not_wired_up_yet`, which pinned
    /// a single pressed reading publishing nothing back when the switch was
    /// ignored outright. The observation still holds, for a different reason:
    /// one reading is below [`BYPASS_DEBOUNCE_READS`], so it is bounce until
    /// proven otherwise.
    #[test]
    fn a_single_pressed_reading_is_not_enough_to_toggle() {
        let mut mapping = ControlMapping::new(Preset::SafeStart.params());
        mapping
            .update(switched(uniform(500), false))
            .expect("the first reading must publish");

        assert!(
            mapping.update(switched(uniform(500), true)).is_none(),
            "an undebounced press must not toggle the bypass"
        );
    }

    #[test]
    fn a_debounced_press_bypasses_by_zeroing_depth() {
        let mut mapping = ControlMapping::new(Preset::SafeStart.params());
        let idle = reading(600, 700, 800, 900);
        let before = mapping
            .update(idle)
            .expect("the first reading must publish");

        // The edge is acted on at exactly the debounce count, no earlier.
        for read in 1..BYPASS_DEBOUNCE_READS {
            assert!(
                mapping.update(switched(idle, true)).is_none(),
                "press reading {read} of {BYPASS_DEBOUNCE_READS} must not toggle yet"
            );
        }
        let params = mapping
            .update(switched(idle, true))
            .expect("a debounced press must publish the bypass");

        assert_eq!(
            params.global.depth.get(),
            0.0,
            "an engaged bypass must publish depth 0"
        );
        assert_eq!(
            params.global.time.get(),
            before.global.time.get(),
            "the bypass must not disturb the Time pot"
        );
        assert_eq!(
            params.global.upward.get(),
            before.global.upward.get(),
            "the bypass must not disturb the Upward pot"
        );
        assert_eq!(
            params.global.downward.get(),
            before.global.downward.get(),
            "the bypass must not disturb the Downward pot"
        );
    }

    #[test]
    fn un_bypassing_restores_the_depth_pots_current_position() {
        let mut mapping = ControlMapping::new(Preset::SafeStart.params());
        mapping
            .update(reading(600, 500, 500, 500))
            .expect("the first reading must publish");
        settle(&mut mapping, switched(reading(600, 500, 500, 500), true))
            .expect("a debounced press must publish the bypass");

        // Turn Depth right down while the effect is bypassed. Nothing is
        // published — the snapshot is unchanged — but the deadband reference
        // underneath must follow the pot.
        let moved = reading(200, 500, 500, 500);
        assert!(
            feed(&mut mapping, switched(moved, true), HELD_READS).is_none(),
            "a Depth turn made while bypassed must not publish"
        );

        // A momentary switch is released before it can be pressed again.
        assert!(
            settle(&mut mapping, switched(moved, false)).is_none(),
            "releasing the switch must not publish"
        );
        let params = settle(&mut mapping, switched(moved, true))
            .expect("a second debounced press must un-bypass and republish");

        let depth = params.global.depth.get();
        assert!(
            depth > normalized(190) && depth < normalized(210),
            "un-bypassing must restore the pot's current position, got {depth}"
        );
    }

    #[test]
    fn contact_bounce_toggles_the_latch_exactly_once() {
        let mut mapping = ControlMapping::new(Preset::SafeStart.params());
        let idle = uniform(600);
        mapping
            .update(idle)
            .expect("the first reading must publish");

        // A bouncing contact: no run of identical readings reaches
        // `BYPASS_DEBOUNCE_READS` until the contact settles closed, and the
        // release bounces the same way afterwards.
        let bounce = [true, false, true, false, true, true, false, true];
        let levels = bounce
            .iter()
            .copied()
            .chain([true; BYPASS_DEBOUNCE_READS as usize])
            .chain(bounce.iter().map(|pressed| !pressed))
            .chain([false; BYPASS_DEBOUNCE_READS as usize]);

        let mut publishes = 0_u32;
        let mut last = None;
        for pressed in levels {
            if let Some(params) = mapping.update(switched(idle, pressed)) {
                publishes += 1;
                last = Some(params);
            }
        }

        assert_eq!(
            publishes, 1,
            "one bouncing press and release must toggle the latch exactly once"
        );
        let params = last.expect("the settled press must have published");
        assert_eq!(
            params.global.depth.get(),
            0.0,
            "the single toggle must have engaged the bypass"
        );
    }

    #[test]
    fn a_release_edge_alone_never_toggles() {
        let mut mapping = ControlMapping::new(Preset::SafeStart.params());
        let idle = uniform(600);
        mapping
            .update(switched(idle, true))
            .expect("the first reading must publish");

        // The switch is let go, well past the debounce count. A release is not
        // an event, so nothing is published — had it toggled, the latch would
        // have engaged and published depth 0.
        assert!(
            feed(&mut mapping, switched(idle, false), HELD_READS).is_none(),
            "a release edge must not toggle the bypass"
        );

        // And the latch is genuinely still un-bypassed, not merely quiet.
        let params = settle(&mut mapping, switched(idle, true))
            .expect("a press after the release must publish");
        assert_eq!(
            params.global.depth.get(),
            0.0,
            "the first press after a release must engage the bypass"
        );
    }

    #[test]
    fn a_switch_held_down_at_startup_does_not_come_up_bypassed() {
        let mut mapping = ControlMapping::new(Preset::SafeStart.params());
        let idle = reading(600, 500, 500, 500);
        let params = mapping
            .update(switched(idle, true))
            .expect("the first reading must publish");

        assert_eq!(
            params.global.depth.get(),
            normalized(600),
            "a switch already down at startup must be a baseline, not an edge"
        );
        assert!(
            feed(&mut mapping, switched(idle, true), HELD_READS).is_none(),
            "holding the baseline level must never toggle"
        );
    }

    #[test]
    fn turning_depth_while_bypassed_publishes_nothing() {
        let mut mapping = ControlMapping::new(Preset::SafeStart.params());
        mapping
            .update(reading(600, 500, 500, 500))
            .expect("the first reading must publish");
        settle(&mut mapping, switched(reading(600, 500, 500, 500), true))
            .expect("a debounced press must publish the bypass");

        // Far past the deadband, and past the filter's settling time. The
        // snapshot would be identical to the last one every time, so the
        // transport layer must not see any of it.
        assert!(
            feed(
                &mut mapping,
                switched(reading(0, 500, 500, 500), true),
                HELD_READS
            )
            .is_none(),
            "a Depth turn made while bypassed must not publish"
        );
    }

    #[test]
    fn turning_time_while_bypassed_still_publishes_with_depth_zero() {
        let mut mapping = ControlMapping::new(Preset::SafeStart.params());
        mapping
            .update(reading(600, 200, 500, 500))
            .expect("the first reading must publish");
        settle(&mut mapping, switched(reading(600, 200, 500, 500), true))
            .expect("a debounced press must publish the bypass");

        let params = feed(
            &mut mapping,
            switched(reading(600, 900, 500, 500), true),
            HELD_READS,
        )
        .expect("a Time turn made while bypassed must still publish");

        assert!(
            params.global.time.get() > normalized(890),
            "the Time pot must keep working while bypassed, got {}",
            params.global.time.get()
        );
        assert_eq!(
            params.global.depth.get(),
            0.0,
            "a publish made while bypassed must still carry depth 0"
        );
    }

    fn arbitrary_count() -> impl Strategy<Value = AdcCount> {
        (0..=ADC_MAX_COUNT).prop_map(count)
    }

    fn arbitrary_reading() -> impl Strategy<Value = RawControls> {
        (
            arbitrary_count(),
            arbitrary_count(),
            arbitrary_count(),
            arbitrary_count(),
            any::<bool>(),
        )
            .prop_map(
                |(depth, time, upward, downward, bypass_pressed)| RawControls {
                    pots: Pots {
                        depth,
                        time,
                        upward,
                        downward,
                    },
                    bypass_pressed,
                },
            )
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(PROPERTY_CASES))]

        #[test]
        fn every_published_snapshot_is_valid(
            readings in prop::collection::vec(arbitrary_reading(), 1..64),
        ) {
            let mut mapping = ControlMapping::new(Preset::SafeStart.params());
            for raw in readings {
                if let Some(params) = mapping.update(raw) {
                    prop_assert!(params.validate(SAMPLE_RATE).is_ok());
                }
            }
        }

        /// Stated as a property because the interesting part is that it holds
        /// for *every* way the pots can move, not for one chosen sweep: while
        /// the switch stays down after a debounced press, no pot position and
        /// no amount of jitter can put a nonzero depth back on the output.
        #[test]
        fn nothing_leaks_through_an_engaged_bypass(
            start in arbitrary_reading(),
            moves in prop::collection::vec(arbitrary_reading(), 1..64),
        ) {
            let mut mapping = ControlMapping::new(Preset::SafeStart.params());
            mapping.update(switched(start, false));
            // Whether the press publishes is not the property: a Depth pot
            // already sitting at 0 makes engaging the bypass a no-op on the
            // snapshot. What follows must hold either way.
            settle(&mut mapping, switched(start, true));

            for raw in moves {
                if let Some(params) = mapping.update(switched(raw, true)) {
                    prop_assert_eq!(params.global.depth.get(), 0.0);
                }
            }
        }
    }
}
