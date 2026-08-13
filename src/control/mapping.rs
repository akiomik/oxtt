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

use crate::params::{ControlSnapshot, IoGain, NormalizedF32, OttParams};

use super::raw::{POT_POSITION_MAX, Pots, RawControls};

/// One-pole low-pass coefficient applied to each pot's raw count, per read.
///
/// Idle jitter on the assembled hardware, measured with `pi-tools` over 300
/// readings per position on all six channels, has a standard deviation of
/// 5.30–6.39 counts out of 1023 with the pots at full travel and 2.98–4.19
/// counts at mid travel. The worst case is therefore σ ≈ 6.4, at the end
/// stop rather than at mid scale — the opposite of what a divider's source
/// impedance alone would predict, which is why it was measured rather than
/// assumed.
///
/// At 0.2 the filter attenuates white noise by exactly
/// `sqrt(a / (2 - a))` = 1/3, taking that worst case down to σ ≈ 2.1 counts,
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
/// Eight counts is roughly 3.8 times the σ ≈ 2.1 that survives
/// [`FILTER_COEFFICIENT`], so a motionless pot is quiet rather than provably
/// silent: the residual is noise, and an excursion past 3.8σ still publishes
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
/// On the two gain pots that fraction lands on a dB figure, since
/// [`GAIN_SPAN_DB`] is spread linearly across the same travel: eight counts is
/// `8 / 1023 * 48` ≈ 0.375 dB. That is well under the roughly 1 dB step a
/// listener can pick out on programme material, so the coarsest move the
/// deadband can force is still finer than the ear resolves — and the DSP
/// smooths even that over 20 ms.
///
/// It is hysteresis, not quantization: once a move clears the band, the
/// published value jumps all the way to the filtered value, so repeated small
/// moves in one direction cannot accumulate an offset.
const DEADBAND_COUNTS: f32 = 8.0;

/// The dB value the gain pots produce at their lower stop.
///
/// This is [`IoGain`]'s own lower bound, not a narrowing of it: the pots sweep
/// the whole range the parameter admits (docs/contracts.md §1). Deliberately
/// no cap tighter than the type's — a smaller number would change the knob's
/// dB-per-degree on nothing more than a guess about how far anyone would want
/// to turn it.
const GAIN_MIN_DB: f32 = -24.0;

/// How many dB the gain pots sweep from stop to stop.
///
/// `GAIN_MIN_DB + GAIN_SPAN_DB` is [`IoGain`]'s upper bound, so the map is
/// `dB = raw / POT_POSITION_MAX * GAIN_SPAN_DB + GAIN_MIN_DB`: plain and linear,
/// with count 0 landing on exactly -24 dB, count 1023 on exactly +24 dB, and
/// unity at the mid point of the pot's rotation — which is the position a
/// player can find without looking, and the one a gain knob should mean
/// "unchanged" at.
const GAIN_SPAN_DB: f32 = 48.0;

/// How many consecutive identical readings the bypass switch's position must
/// survive before it is believed.
///
/// Counted in reads rather than milliseconds for the same reason as
/// [`FILTER_COEFFICIENT`]: this layer has no clock. At the
/// [`DEFAULT_POLL_INTERVAL`](crate::control::DEFAULT_POLL_INTERVAL) of 2 ms
/// (500 Hz), fifteen reads means the contact has to hold its new position for
/// 28 ms after the first read that sees it.
///
/// That is three times the eight milliseconds the previous momentary push
/// switch was given, because the part changed. An alternate-action or slide
/// switch is moved by a hand travelling the whole throw rather than by a
/// spring snapping over centre, so the wiper can make and break repeatedly for
/// as long as the movement lasts — tens of milliseconds, not the single digits
/// a snap-action contact bounces for. Twenty-eight milliseconds is chosen to
/// sit above that, not read off a datasheet.
///
/// The cost is latency: a position change is acted on at the fifteenth read,
/// so up to 30 ms (fifteen poll intervals — fourteen of debounce plus up to
/// one interval of sampling delay) passes between the switch reaching its new
/// position and the parameters following. That is still below the ~50 ms at
/// which a foot- or finger-operated switch starts to feel late, and the DSP's
/// 20 ms smoothing dominates what is actually heard anyway
/// (docs/architecture.md).
///
/// This was sized from the switch class rather than an oscilloscope, and has
/// since been confirmed on hardware: a live JACK session exercising the
/// latching switch repeatedly produced exactly one state change per throw
/// with no adjustment needed
/// (`docs/raspberry-pi/control-surface-verification.md`). The failure it
/// guards against is visible in use: too small and a single throw publishes
/// twice.
///
/// Raising the poll rate shortens the latency and weakens the debounce in
/// exact proportion, which is the trade to re-make if the interval changes.
const BYPASS_DEBOUNCE_READS: u8 = 15;

/// The debounced bypass switch.
///
/// The panel part is a mechanically *latching* (alternate-action) switch, so
/// its position **is** the bypass state: there is nothing to toggle and no
/// edge to detect, only a level to believe or disbelieve. Everything this type
/// does is therefore debounce — a slide or alternate-action contact makes and
/// breaks intermittently for as long as the hand is moving it, and each of
/// those intermediate makes would otherwise publish (see
/// [`BYPASS_DEBOUNCE_READS`]).
///
/// That the switch and the software agree by construction is the point of the
/// part change, not a side effect of it. With the old momentary switch the
/// panel could not be reconciled with the software state by looking at it, so
/// the startup state had to be invented — the first reading was declared a
/// baseline and the run came up un-bypassed however the switch sat. A latching
/// switch makes the two the same object, so a switch resting in the bypassed
/// position at startup comes up bypassed.
///
/// What an engaged bypass does is an *effect* bypass: the DSP crossfades to
/// the unity sum of the same raw-input crossover bands that feed the latent
/// effect. It is not a raw dry signal, because the raw input and the
/// reconstructed signal do not share a phase response and crossfading between
/// them would comb-filter (ADR 0004, docs/contracts.md §4). The complete pot
/// payload remains live in the latent effect branch, ready for disengage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BypassSwitch {
    /// The switch position currently believed: the last one to survive
    /// [`BYPASS_DEBOUNCE_READS`] consecutive readings. This is the bypass
    /// state, not an input to one.
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
    const fn update(&mut self, engaged: bool) {
        if engaged == self.engaged {
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

        self.engaged = engaged;
        self.disagreements = 0;
    }
}

/// The conditioning and switch state, absent until the first
/// [`ControlMapping::update`].
///
/// `reference` and `published` are deliberately two fields, in two different
/// domains, rather than one. The deadband is hysteresis in the ADC's own
/// units against where the *pots* were last taken seriously, so `reference`
/// has to stay in counts and has to keep tracking the real pots even while
/// bypassed, so disengaging cannot restore stale positions. The publish
/// decision, in contrast, has to be made against what the caller was actually
/// last handed, which is a finished [`ControlSnapshot`]. Pot movements publish
/// their current values even while bypass is engaged.
///
/// Comparing whole snapshots rather than counts also includes switch-only
/// changes. The gate's job is "is this different from what I last handed out",
/// and [`ControlSnapshot`] is `Copy` and `PartialEq`, so asking it directly
/// costs a struct compare and needs no third representation of the same state.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Conditioned {
    /// Low-pass filter state, in ADC counts.
    filtered: Pots<f32>,
    /// The deadband reference: the filtered values as of the last time a pot
    /// cleared [`DEADBAND_COUNTS`], in ADC counts, and always the real pot
    /// positions — never the bypassed ones.
    reference: Pots<f32>,
    /// The most recent published snapshot.
    published: ControlSnapshot,
    /// The debounced bypass switch.
    bypass: BypassSwitch,
}

/// Turns raw control-surface readings into complete [`OttParams`] (layer B).
///
/// The six pot-driven fields (`global.depth`, `global.time`, `global.upward`,
/// `global.downward`, `global.input_gain_db`, `global.output_gain_db`) are
/// owned by the hardware from the first read onward; the CLI values for those
/// six only describe the state before any reading arrives. Every other field
/// of the base parameters — the crossover pair, all per-band values — is
/// passed through unchanged, since no pot is wired to it.
///
/// The bypass switch is carried as an explicit, debounced level beside the
/// complete pot snapshot. The DSP, rather than this conditioning layer,
/// crossfades its phase-coherent effect and guaranteed-unity bypass branches.
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
    /// deadband reference, then the reference is converted into an
    /// [`OttParams`] and paired with the debounced bypass level. A motionless
    /// pot and an untouched switch therefore yield `None` forever, which is
    /// what keeps the transport layer from being handed a fresh snapshot on
    /// every poll for no reason.
    ///
    /// The very first call seeds the filter from the reading itself and
    /// publishes immediately, rather than starting from zero and fading in —
    /// the same reasoning as `OttProcessor::new` snapping its smoothers to
    /// their targets (docs/contracts.md §2). It seeds the switch the same way,
    /// by believing where it is resting (see [`BypassSwitch::seeded`]), so a
    /// run that starts with the switch in the bypassed position comes up
    /// bypassed.
    ///
    /// [`RawControls::bypass_engaged`] drives [`BypassSwitch`]: once debounced,
    /// the switch's position is the explicit effect-bypass request. The DSP
    /// owns the resulting phase-coherent branch crossfade (ADR 0004,
    /// docs/contracts.md §4, §8).
    // Proves this function can never panic, the same way `OttProcessor::process`
    // does (docs/contracts.md §6), checked by `cargo test --release`. It matters
    // here for the same reason: on Bela this runs inside the real-time callback.
    // The tests below already call it, so no proof-only test is needed.
    #[cfg_attr(all(test, not(debug_assertions)), no_panic::no_panic)]
    pub fn update(&mut self, raw: RawControls) -> Option<ControlSnapshot> {
        let counts = raw.pots.map(|count| f32::from(count.get()));
        // Copied out before `self.state` is borrowed mutably below: `base` is
        // `Copy` and never changes, so the conversion takes it by value rather
        // than re-borrowing `self` in the middle of the match.
        let base = self.base;

        let published = match self.state.as_mut() {
            None => {
                let bypass = BypassSwitch::seeded(raw.bypass_engaged);
                // Unlike the old momentary latch, this is *not* a no-op on a
                // freshly seeded switch: a switch resting in the bypassed
                // position bypasses from the very first snapshot.
                let published = ControlSnapshot {
                    params: params_with_pots(base, counts),
                    bypass_engaged: bypass.engaged,
                };
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

                // The deadband tracks the pots in their own units, so it is
                // compared against `reference` and never against what was
                // published: while bypassed the true pot positions still
                // travel in the snapshot, and the DSP performs the transition.
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

                state.bypass.update(raw.bypass_engaged);

                let next = ControlSnapshot {
                    params: params_with_pots(base, state.reference),
                    bypass_engaged: state.bypass.engaged,
                };
                if next == state.published {
                    return None;
                }
                state.published = next;
                next
            }
        };

        Some(published)
    }
}

/// Overlays the six pot-driven fields onto the base parameters.
///
/// A free function rather than a method so that [`ControlMapping::update`] can
/// call it while holding a mutable borrow of its own conditioning state;
/// `base` is `Copy` and never mutated, so passing it by value costs nothing.
///
/// `counts` is filter state, bounded by the raw counts that produced it, so
/// every fraction below is finite and within `0.0..=1.0`.
fn params_with_pots(base: OttParams, counts: Pots<f32>) -> OttParams {
    let fractions = counts.map(|count| count / f32::from(POT_POSITION_MAX));

    let mut params = base;
    params.global.depth = normalized_or(fractions.depth, base.global.depth);
    params.global.time = normalized_or(fractions.time, base.global.time);
    params.global.upward = normalized_or(fractions.upward, base.global.upward);
    params.global.downward = normalized_or(fractions.downward, base.global.downward);
    params.global.input_gain_db = gain_db_or(fractions.input_gain, base.global.input_gain_db);
    params.global.output_gain_db = gain_db_or(fractions.output_gain, base.global.output_gain_db);
    params
}

/// Turns a pot's fraction of travel into a [`NormalizedF32`].
///
/// The identity map, since a fraction of travel and a `NormalizedF32` are the
/// same `0.0..=1.0` quantity; the function exists to place the fallback below
/// beside its gain counterpart rather than to compute anything.
///
/// `fraction` is within `0.0..=1.0` by construction, so `NormalizedF32`
/// construction cannot fail. The error arm is unreachable; it falls back to
/// the base value rather than unwrapping, because this runs on Bela's
/// real-time callback path, where a panic is prohibited (docs/contracts.md §6).
fn normalized_or(fraction: f32, base: NormalizedF32) -> NormalizedF32 {
    NormalizedF32::try_new(fraction).unwrap_or(base)
}

/// Turns a gain pot's fraction of travel into an [`IoGain`], linearly across
/// the whole of that type's range.
///
/// `mul_add` rather than `fraction * GAIN_SPAN_DB + GAIN_MIN_DB` for the same
/// reason the filter uses it: one rounding instead of two, which is what keeps
/// the two stops landing on exactly -24 dB and exactly +24 dB rather than a
/// few ulps outside them.
///
/// `fraction` is within `0.0..=1.0` by construction, so the result is within
/// `[GAIN_MIN_DB, GAIN_MIN_DB + GAIN_SPAN_DB]`, which is `IoGain`'s range
/// exactly, and construction cannot fail. The error arm is unreachable for the
/// same reason as [`normalized_or`]'s, and is handled the same way rather than
/// unwrapped (docs/contracts.md §6).
fn gain_db_or(fraction: f32, base: IoGain) -> IoGain {
    IoGain::try_new(fraction.mul_add(GAIN_SPAN_DB, GAIN_MIN_DB)).unwrap_or(base)
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
    use crate::control::PotPosition;
    use crate::params::Preset;

    const PROPERTY_CASES: u32 = 128;
    const SAMPLE_RATE: f32 = 48_000.0;
    /// Long enough for the filter to converge on a held position and for the
    /// deadband to have taken it, at `FILTER_COEFFICIENT`'s 11-read 90% point.
    const HELD_READS: usize = 100;
    /// The closest a 10-bit count gets to the centre of a pot's rotation, which
    /// is 511.5. Where the gain pots sit for the tests that are not about them.
    const GAIN_CENTRE_COUNT: u16 = 512;

    fn count(raw: u16) -> PotPosition {
        PotPosition::try_new(raw).unwrap()
    }

    /// A reading of the four effect pots, with both gain pots parked at the
    /// centre of their travel — near enough unity that a test which is not
    /// about the gains does not have to think about them.
    fn reading(depth: u16, time: u16, upward: u16, downward: u16) -> RawControls {
        RawControls {
            pots: Pots {
                depth: count(depth),
                time: count(time),
                upward: count(upward),
                downward: count(downward),
                input_gain: count(GAIN_CENTRE_COUNT),
                output_gain: count(GAIN_CENTRE_COUNT),
            },
            bypass_engaged: false,
        }
    }

    /// The same reading with the two gain pots moved somewhere specific.
    fn with_gains(raw: RawControls, input_gain: u16, output_gain: u16) -> RawControls {
        RawControls {
            pots: Pots {
                input_gain: count(input_gain),
                output_gain: count(output_gain),
                ..raw.pots
            },
            ..raw
        }
    }

    /// Every pot, gains included, at the same position.
    fn uniform(raw: u16) -> RawControls {
        with_gains(reading(raw, raw, raw, raw), raw, raw)
    }

    fn normalized(raw: u16) -> f32 {
        f32::from(raw) / f32::from(POT_POSITION_MAX)
    }

    /// The dB a gain pot at `raw` maps to, spelled out independently of the
    /// code under test rather than reusing its constants.
    // `mul_add` is what the implementation uses and what clippy wants here.
    // Writing the multiply and the add separately is the point: an oracle that
    // reproduces the implementation's instruction sequence would agree with a
    // wrong implementation just as readily, so this one is the arithmetic as a
    // reader would write it and the tests allow for the extra rounding.
    #[allow(clippy::suboptimal_flops)]
    fn gain_db(raw: u16) -> f32 {
        f32::from(raw) / 1023.0 * 48.0 - 24.0
    }

    /// The same reading with the bypass switch resting in the given position.
    fn switched(raw: RawControls, bypass_engaged: bool) -> RawControls {
        RawControls {
            bypass_engaged,
            ..raw
        }
    }

    /// Feeds one reading in `reads` times, returning the last snapshot published.
    fn feed(
        mapping: &mut ControlMapping,
        raw: RawControls,
        reads: usize,
    ) -> Option<ControlSnapshot> {
        let mut last = None;
        for _ in 0..reads {
            if let Some(params) = mapping.update(raw) {
                last = Some(params);
            }
        }
        last
    }

    /// Feeds a switch position in exactly [`BYPASS_DEBOUNCE_READS`] times,
    /// which is the shortest run the debounce believes.
    fn settle(mapping: &mut ControlMapping, raw: RawControls) -> Option<ControlSnapshot> {
        feed(mapping, raw, usize::from(BYPASS_DEBOUNCE_READS))
    }

    #[test]
    fn first_update_publishes_the_reading_without_fading_in() {
        let mut mapping = ControlMapping::new(Preset::SafeStart.params());
        let params = mapping
            .update(reading(0, 512, 1023, 250))
            .expect("the first reading must publish");

        assert_eq!(params.params.global.depth.get(), normalized(0));
        assert_eq!(params.params.global.time.get(), normalized(512));
        assert_eq!(params.params.global.upward.get(), normalized(1023));
        assert_eq!(params.params.global.downward.get(), normalized(250));
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
        // 32 counts peak-to-peak over 300 readings (σ ≈ 6.4, see
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
        let mut last = start.params.global.depth.get();
        let mut publishes = 0;
        for _ in 0..100 {
            if let Some(params) = mapping.update(uniform(800)) {
                let depth = params.params.global.depth.get();
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
                published = Some(params.params.global.depth.get());
            }
        }

        let depth = published.expect("a full-scale turn must publish");
        assert!(
            depth < normalized(210),
            "depth {depth} must converge downward on the new position"
        );
    }

    #[test]
    fn only_the_six_pot_fields_are_replaced() {
        let base = Preset::Default.params();
        let mut mapping = ControlMapping::new(base);
        let params = mapping
            .update(with_gains(reading(10, 20, 30, 40), 50, 60))
            .expect("the first reading must publish");

        assert_eq!(params.params.global.crossover, base.global.crossover);
        assert_eq!(params.params.bands, base.bands);

        assert_ne!(params.params.global.depth, base.global.depth);
        assert_ne!(params.params.global.time, base.global.time);
        assert_ne!(params.params.global.upward, base.global.upward);
        assert_ne!(params.params.global.downward, base.global.downward);
        assert_ne!(
            params.params.global.input_gain_db,
            base.global.input_gain_db
        );
        assert_ne!(
            params.params.global.output_gain_db,
            base.global.output_gain_db
        );
    }

    /// The dB pair a fresh mapping publishes for gain pots resting at
    /// `input_gain`/`output_gain`.
    ///
    /// The *first* publish is the exact reading — the filter seeds from it and
    /// the deadband has nothing to lag behind yet — so these tests read the
    /// mapping itself rather than the mapping plus a settling error.
    fn first_published_gains(input_gain: u16, output_gain: u16) -> (f32, f32) {
        let mut mapping = ControlMapping::new(Preset::SafeStart.params());
        let params = mapping
            .update(with_gains(
                reading(500, 500, 500, 500),
                input_gain,
                output_gain,
            ))
            .expect("the first reading must publish");
        (
            params.params.global.input_gain_db.get(),
            params.params.global.output_gain_db.get(),
        )
    }

    /// Both stops land exactly on `IoGain`'s own bounds, so the pots sweep the
    /// parameter's whole range and nothing narrower.
    #[test]
    fn the_gain_pots_reach_both_ends_of_the_io_gain_range_exactly() {
        assert_eq!(
            first_published_gains(0, 0),
            (-24.0, -24.0),
            "a gain pot at its lower stop must be exactly -24 dB"
        );
        assert_eq!(
            first_published_gains(POT_POSITION_MAX, POT_POSITION_MAX),
            (24.0, 24.0),
            "a gain pot at its upper stop must be exactly +24 dB"
        );
    }

    /// Unity at the centre of the rotation is the point of the linear map: it
    /// is the position a hand finds without looking, so it has to be the one
    /// that means "unchanged". A 10-bit pot cannot sit exactly on 511.5, so the
    /// claim is about the two counts either side of it.
    #[test]
    fn the_rotation_centre_is_unity_gain_within_a_fraction_of_a_db() {
        for raw in [511_u16, 512] {
            let (input_db, output_db) = first_published_gains(raw, raw);
            assert!(
                input_db.abs() < 0.05,
                "count {raw} is the rotation centre, but input gain is {input_db} dB"
            );
            assert!(
                output_db.abs() < 0.05,
                "count {raw} is the rotation centre, but output gain is {output_db} dB"
            );
        }
    }

    /// The map is linear, so a fixed step in counts is a fixed step in dB
    /// wherever on the sweep it is taken — the property a piecewise map or a
    /// snap-to-unity dead zone would break, and the reason this one has
    /// neither.
    #[test]
    fn equal_steps_in_counts_are_equal_steps_in_db() {
        let step = |from: u16, to: u16| {
            first_published_gains(to, to).0 - first_published_gains(from, from).0
        };

        let bottom = step(0, 200);
        let middle = step(400, 600);
        let top = step(823, POT_POSITION_MAX);
        assert!(
            (bottom - middle).abs() < 1e-4 && (middle - top).abs() < 1e-4,
            "200 counts must be the same number of dB anywhere on the sweep, got {bottom}, {middle}, {top}"
        );
    }

    /// Both gain pots read from their own channel and write their own field.
    ///
    /// The tolerance is the deadband: `DEADBAND_COUNTS` is hysteresis, so a
    /// settled value can sit up to eight counts (≈ 0.375 dB) short of the pot.
    #[test]
    fn turning_a_gain_pot_publishes_and_moves_only_its_own_field() {
        let mut mapping = ControlMapping::new(Preset::SafeStart.params());
        let idle = reading(500, 500, 500, 500);
        mapping
            .update(with_gains(idle, 100, 100))
            .expect("the first reading must publish");

        let turned_input = feed(&mut mapping, with_gains(idle, 900, 100), HELD_READS)
            .expect("an Input Gain turn must publish");
        assert!(
            (turned_input.params.global.input_gain_db.get() - gain_db(900)).abs() < 0.4,
            "the Input Gain pot must reach {} dB, got {}",
            gain_db(900),
            turned_input.params.global.input_gain_db.get()
        );
        assert!(
            (turned_input.params.global.output_gain_db.get() - gain_db(100)).abs() < 0.4,
            "turning Input Gain must leave Output Gain where it was, got {}",
            turned_input.params.global.output_gain_db.get()
        );

        let turned_output = feed(&mut mapping, with_gains(idle, 900, 900), HELD_READS)
            .expect("an Output Gain turn must publish");
        assert!(
            (turned_output.params.global.output_gain_db.get() - gain_db(900)).abs() < 0.4,
            "the Output Gain pot must reach {} dB, got {}",
            gain_db(900),
            turned_output.params.global.output_gain_db.get()
        );
        assert!(
            (turned_output.params.global.input_gain_db.get() - gain_db(900)).abs() < 0.4,
            "turning Output Gain must leave Input Gain where it was, got {}",
            turned_output.params.global.input_gain_db.get()
        );
    }

    /// Asserts that bypass is transported as an explicit level rather than
    /// encoded by a parameter triple.
    fn assert_bypassed(snapshot: &ControlSnapshot, what: &str) {
        assert!(
            snapshot.bypass_engaged,
            "{what}: bypass level must be engaged"
        );
    }

    /// The descendant of `the_bypass_switch_is_not_wired_up_yet`, which pinned
    /// a single engaged reading publishing nothing back when the switch was
    /// ignored outright. The observation still holds, for a different reason:
    /// one reading is below [`BYPASS_DEBOUNCE_READS`], so it is bounce until
    /// proven otherwise.
    #[test]
    fn a_single_engaged_reading_is_not_enough_to_believe() {
        let mut mapping = ControlMapping::new(Preset::SafeStart.params());
        mapping
            .update(switched(uniform(500), false))
            .expect("the first reading must publish");

        assert!(
            mapping.update(switched(uniform(500), true)).is_none(),
            "an undebounced position change must not engage the bypass"
        );
    }

    #[test]
    fn a_debounced_move_to_the_bypassed_position_publishes_an_explicit_level() {
        let mut mapping = ControlMapping::new(Preset::SafeStart.params());
        let idle = with_gains(reading(600, 700, 800, 900), 200, 300);
        let before = mapping
            .update(idle)
            .expect("the first reading must publish");

        // The change is acted on at exactly the debounce count, no earlier.
        for read in 1..BYPASS_DEBOUNCE_READS {
            assert!(
                mapping.update(switched(idle, true)).is_none(),
                "engaged reading {read} of {BYPASS_DEBOUNCE_READS} must not be believed yet"
            );
        }
        let params = mapping
            .update(switched(idle, true))
            .expect("a debounced move must publish the bypass");

        assert_bypassed(&params, "a debounced move to the bypassed position");
        assert_eq!(
            params.params.global.time.get(),
            before.params.global.time.get(),
            "the bypass must not disturb the Time pot"
        );
        assert_eq!(
            params.params.global.upward.get(),
            before.params.global.upward.get(),
            "the bypass must not disturb the Upward pot"
        );
        assert_eq!(
            params.params.global.downward.get(),
            before.params.global.downward.get(),
            "the bypass must not disturb the Downward pot"
        );
    }

    #[test]
    fn disengaging_restores_all_three_bypass_controlled_pots_current_positions() {
        let mut mapping = ControlMapping::new(Preset::SafeStart.params());
        let start = with_gains(reading(600, 500, 500, 500), 600, 600);
        mapping
            .update(start)
            .expect("the first reading must publish");
        settle(&mut mapping, switched(start, true))
            .expect("a debounced move must publish the bypass");

        // Turn all three bypass-controlled pots while bypassed. Their current
        // positions must still reach the DSP so a later disengage uses the
        // newest targets rather than stale latched values.
        let moved = with_gains(reading(200, 500, 500, 500), 100, 900);
        assert!(
            feed(&mut mapping, switched(moved, true), HELD_READS).is_some(),
            "turning bypass-controlled pots must publish their latest targets"
        );

        // A latching switch is simply moved back; there is no release to wait
        // for and no second throw to make.
        let params = settle(&mut mapping, switched(moved, false))
            .expect("moving the switch back must disengage and republish");

        let depth = params.params.global.depth.get();
        assert!(
            depth > normalized(190) && depth < normalized(210),
            "disengaging must restore the Depth pot's current position, got {depth}"
        );
        let input_db = params.params.global.input_gain_db.get();
        assert!(
            (input_db - gain_db(100)).abs() < 0.4,
            "disengaging must restore the Input Gain pot's current position, got {input_db}"
        );
        let output_db = params.params.global.output_gain_db.get();
        assert!(
            (output_db - gain_db(900)).abs() < 0.4,
            "disengaging must restore the Output Gain pot's current position, got {output_db}"
        );
    }

    #[test]
    fn contact_bounce_settles_to_exactly_one_state_change() {
        let mut mapping = ControlMapping::new(Preset::SafeStart.params());
        let idle = uniform(600);
        mapping
            .update(idle)
            .expect("the first reading must publish");

        // A hand moving an alternate-action switch: the wiper makes and breaks
        // repeatedly while it travels, and no run of identical readings reaches
        // `BYPASS_DEBOUNCE_READS` until it comes to rest in the new position.
        let bounce = [true, false, true, false, true, true, false, true];
        let levels = bounce
            .iter()
            .copied()
            .chain([true; BYPASS_DEBOUNCE_READS as usize]);

        let mut publishes = 0_u32;
        let mut last = None;
        for engaged in levels {
            if let Some(params) = mapping.update(switched(idle, engaged)) {
                publishes += 1;
                last = Some(params);
            }
        }

        assert_eq!(
            publishes, 1,
            "one bouncing throw must change the published state exactly once"
        );
        let params = last.expect("the settled position must have published");
        assert_bypassed(&params, "a bouncing throw into the bypassed position");
    }

    /// The descendant of `a_switch_held_down_at_startup_does_not_come_up_bypassed`,
    /// with the expected behaviour deliberately inverted.
    ///
    /// The old switch was momentary, so bypass was software state a press
    /// toggled: the panel could not say what that state was, and the first
    /// reading had to be treated as a baseline rather than an edge — a switch
    /// found down at startup meant nothing, and the run came up un-bypassed.
    /// The panel part is now mechanically latching, so its position *is* the
    /// state. There is no longer anything to invent: a switch resting in the
    /// bypassed position at startup is the panel saying "bypassed", and the run
    /// must come up that way.
    #[test]
    fn a_switch_resting_bypassed_at_startup_comes_up_bypassed() {
        let mut mapping = ControlMapping::new(Preset::SafeStart.params());
        let idle = with_gains(reading(600, 500, 500, 500), 900, 900);
        let params = mapping
            .update(switched(idle, true))
            .expect("the first reading must publish");

        assert_bypassed(&params, "a switch resting bypassed at the first update");
        assert!(
            feed(&mut mapping, switched(idle, true), HELD_READS).is_none(),
            "holding the same position must never publish again"
        );

        // And it is genuinely bypassed rather than coincidentally quiet:
        // moving the switch out of that position brings the pots back.
        let params = settle(&mut mapping, switched(idle, false))
            .expect("moving the switch out of bypass must publish");
        assert_eq!(
            params.params.global.depth.get(),
            normalized(600),
            "disengaging must hand back the Depth pot's position"
        );
        assert!(
            (params.params.global.input_gain_db.get() - gain_db(900)).abs() < 0.4,
            "disengaging must hand back the Input Gain pot's position, got {}",
            params.params.global.input_gain_db.get()
        );
    }

    #[test]
    fn turning_bypass_controlled_pots_still_publishes_latest_targets() {
        let mut mapping = ControlMapping::new(Preset::SafeStart.params());
        let start = with_gains(reading(600, 500, 500, 500), 500, 500);
        mapping
            .update(start)
            .expect("the first reading must publish");
        settle(&mut mapping, switched(start, true))
            .expect("a debounced move must publish the bypass");

        // Each bypass-controlled pot is carried despite the engaged level.
        for (what, moved) in [
            ("Depth", with_gains(reading(0, 500, 500, 500), 500, 500)),
            (
                "Input Gain",
                with_gains(reading(0, 500, 500, 500), 1023, 500),
            ),
            (
                "Output Gain",
                with_gains(reading(0, 500, 500, 500), 1023, 0),
            ),
        ] {
            assert!(
                feed(&mut mapping, switched(moved, true), HELD_READS).is_some(),
                "a {what} turn made while bypassed must publish its latest target"
            );
        }
    }

    #[test]
    fn turning_non_bypass_controls_while_bypassed_keeps_the_explicit_level() {
        let mut mapping = ControlMapping::new(Preset::SafeStart.params());
        let start = with_gains(reading(600, 200, 500, 500), 900, 100);
        mapping
            .update(start)
            .expect("the first reading must publish");
        settle(&mut mapping, switched(start, true))
            .expect("a debounced move must publish the bypass");

        for (what, moved) in [
            ("Time", reading(600, 900, 500, 500)),
            ("Upward", reading(600, 900, 50, 500)),
            ("Downward", reading(600, 900, 50, 1000)),
        ] {
            let moved = with_gains(moved, 900, 100);
            let published = feed(&mut mapping, switched(moved, true), HELD_READS);
            assert!(
                published.is_some(),
                "a {what} turn made while bypassed must still publish"
            );
            if let Some(params) = published {
                assert_bypassed(&params, what);
            }
        }

        // The last publish carries every one of those three moves, so the
        // effect set up while bypassed is what disengaging brings back.
        let final_state = feed(
            &mut mapping,
            switched(with_gains(reading(600, 900, 50, 1000), 900, 100), false),
            HELD_READS,
        )
        .expect("moving the switch back must disengage and republish");
        assert!(
            final_state.params.global.time.get() > normalized(890),
            "the Time pot must keep working while bypassed, got {}",
            final_state.params.global.time.get()
        );
        assert!(
            final_state.params.global.upward.get() < normalized(60),
            "the Upward pot must keep working while bypassed, got {}",
            final_state.params.global.upward.get()
        );
        assert!(
            final_state.params.global.downward.get() > normalized(990),
            "the Downward pot must keep working while bypassed, got {}",
            final_state.params.global.downward.get()
        );
    }

    fn arbitrary_count() -> impl Strategy<Value = PotPosition> {
        (0..=POT_POSITION_MAX).prop_map(count)
    }

    fn arbitrary_reading() -> impl Strategy<Value = RawControls> {
        (
            arbitrary_count(),
            arbitrary_count(),
            arbitrary_count(),
            arbitrary_count(),
            arbitrary_count(),
            arbitrary_count(),
            any::<bool>(),
        )
            .prop_map(
                |(depth, time, upward, downward, input_gain, output_gain, bypass_engaged)| {
                    RawControls {
                        pots: Pots {
                            depth,
                            time,
                            upward,
                            downward,
                            input_gain,
                            output_gain,
                        },
                        bypass_engaged,
                    }
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
                    prop_assert!(params.params.validate(SAMPLE_RATE).is_ok());
                }
            }
        }

        /// Every update while the switch rests bypassed must retain the
        /// explicit level even as its complete parameter payload changes.
        #[test]
        fn engaged_bypass_is_never_inferred_from_or_lost_by_parameter_values(
            start in arbitrary_reading(),
            moves in prop::collection::vec(arbitrary_reading(), 1..64),
        ) {
            let mut mapping = ControlMapping::new(Preset::SafeStart.params());
            mapping.update(switched(start, false));
            // Whether engaging the bypass publishes is not the property: pots
            // already sitting at the values the bypass forces make it a no-op
            // on the snapshot. What follows must hold either way.
            settle(&mut mapping, switched(start, true));

            for raw in moves {
                if let Some(params) = mapping.update(switched(raw, true)) {
                    prop_assert!(params.bypass_engaged);
                }
            }
        }

        /// `gain_db_or`'s error arm really is unreachable across the whole
        /// 10-bit range, which is a claim about every count rather than about
        /// the handful an example test can name.
        ///
        /// The arm falls back to the base value rather than unwrapping
        /// (docs/contracts.md §6), so taking it would be *silent*: the pot
        /// would simply stop working and the preset's gain would stand. That
        /// is what this checks for — `SafeStart`'s -18 dB output gain and 0 dB
        /// input gain are whole dB away from anything the map produces, so a
        /// fallback cannot hide inside the tolerance.
        #[test]
        fn the_gain_map_never_falls_back_to_the_base_value(
            input_raw in 0..=POT_POSITION_MAX,
            output_raw in 0..=POT_POSITION_MAX,
        ) {
            let mut mapping = ControlMapping::new(Preset::SafeStart.params());
            let params = mapping
                .update(with_gains(reading(500, 500, 500, 500), input_raw, output_raw))
                .expect("the first reading must publish");

            // Not exact equality: `gain_db` rounds twice where `gain_db_or`
            // fuses into one `mul_add`, so the two agree to a few ulps rather
            // than bit for bit.
            prop_assert!((params.params.global.input_gain_db.get() - gain_db(input_raw)).abs() < 1e-5);
            prop_assert!((params.params.global.output_gain_db.get() - gain_db(output_raw)).abs() < 1e-5);
        }
    }
}
