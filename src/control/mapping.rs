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

/// The conditioning state, absent until the first [`ControlMapping::update`].
#[derive(Debug, Clone, Copy, PartialEq)]
struct Conditioned {
    /// Low-pass filter state, in ADC counts.
    filtered: Pots<f32>,
    /// The filtered values as of the most recent publish, in ADC counts.
    published: Pots<f32>,
}

/// Turns raw control-surface readings into complete [`OttParams`] (layer B).
///
/// The four pot-driven fields (`global.depth`, `global.time`, `global.upward`,
/// `global.downward`) are owned by the hardware from the first read onward;
/// the CLI values for those four only describe the state before any reading
/// arrives. Every other field of the base parameters — input/output gain,
/// crossover pair, all per-band values — is passed through unchanged, since
/// no pot is wired to it.
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
    /// Returns `Some` only when the conditioned value actually moved:
    /// `FILTER_COEFFICIENT` first low-passes the raw counts, then
    /// `DEADBAND_COUNTS` compares the result against the last published
    /// value. A motionless pot therefore yields `None` forever, which is what
    /// keeps the transport layer from being handed a fresh snapshot on every
    /// poll for no reason.
    ///
    /// The very first call seeds the filter from the reading itself and
    /// publishes immediately, rather than starting from zero and fading in —
    /// the same reasoning as `OttProcessor::new` snapping its smoothers to
    /// their targets (docs/contracts.md §2).
    ///
    /// [`RawControls::bypass_pressed`] is deliberately ignored for now.
    /// Debouncing it, latching the state in software, and applying the latch
    /// as an effect bypass (`depth = 0`) is a follow-up change; accepting the
    /// field here means the reading layer and its wiring are already final
    /// when that lands.
    // Proves this function can never panic, the same way `OttProcessor::process`
    // does (docs/contracts.md §6), checked by `cargo test --release`. It matters
    // here for the same reason: on Bela this runs inside the real-time callback.
    // The tests below already call it, so no proof-only test is needed.
    #[cfg_attr(all(test, not(debug_assertions)), no_panic::no_panic)]
    pub fn update(&mut self, raw: RawControls) -> Option<OttParams> {
        let counts = raw.pots.map(|count| f32::from(count.get()));

        let published = match self.state.as_mut() {
            None => {
                self.state = Some(Conditioned {
                    filtered: counts,
                    published: counts,
                });
                counts
            }
            Some(state) => {
                // `filtered + a * (raw - filtered)` rather than the equivalent
                // `(1 - a) * filtered + a * raw`: written this way the result
                // stays inside the span of the two inputs even after rounding,
                // which is what keeps the normalized value below in `0.0..=1.0`.
                state.filtered = state.filtered.zip_with(counts, |filtered, raw| {
                    FILTER_COEFFICIENT.mul_add(raw - filtered, filtered)
                });

                let next = state
                    .filtered
                    .zip_with(state.published, |filtered, published| {
                        if (filtered - published).abs() >= DEADBAND_COUNTS {
                            filtered
                        } else {
                            published
                        }
                    });
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

    #[test]
    fn the_bypass_switch_is_not_wired_up_yet() {
        let mut mapping = ControlMapping::new(Preset::SafeStart.params());
        mapping
            .update(RawControls {
                bypass_pressed: false,
                ..uniform(500)
            })
            .expect("the first reading must publish");

        // Pressing the switch is a follow-up change; today it must not
        // publish anything by itself.
        assert!(
            mapping
                .update(RawControls {
                    bypass_pressed: true,
                    ..uniform(500)
                })
                .is_none(),
            "the bypass switch must not affect this layer yet"
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
    }
}
