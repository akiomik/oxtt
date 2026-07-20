//! The parameter aggregates passed to `OttProcessor` (docs/contracts.md §1).

use crate::bands::Bands;

use super::error::ConfigError;
use super::value::{IoGain, MakeupGain, NormalizedF32, PositiveF32, SampleRate, Threshold};
use super::{CrossoverFreqHigh, CrossoverFreqLow};

/// Upper-bound coefficient on the Nyquist side that crossover frequencies must respect (docs/contracts.md §1).
pub const CROSSOVER_NYQUIST_RATIO: f32 = 0.45;

/// A validated low/high crossover pair, at least one octave apart (docs/contracts.md §1).
///
/// `low_hz`/`high_hz` are always consumed together (`Crossover::new`,
/// `Crossover::set_targets`), so the octave-separation invariant that spans
/// them lives here rather than in `GlobalParams`: once a `CrossoverSplit`
/// exists, it is guaranteed valid, regardless of which other fields
/// `GlobalParams` happens to hold.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CrossoverSplit {
    low_hz: CrossoverFreqLow,
    high_hz: CrossoverFreqHigh,
}

impl CrossoverSplit {
    /// Builds a `CrossoverSplit` from CLI-sourced or otherwise untrusted values.
    ///
    /// # Errors
    ///
    /// Returns `ConfigError::CrossoverOctave` if `high_hz` is less than one
    /// octave above `low_hz`.
    pub const fn try_new(
        low_hz: CrossoverFreqLow,
        high_hz: CrossoverFreqHigh,
    ) -> Result<Self, ConfigError> {
        if high_hz.get() < 2.0 * low_hz.get() {
            return Err(ConfigError::CrossoverOctave {
                low_hz: low_hz.get(),
                high_hz: high_hz.get(),
            });
        }
        Ok(Self { low_hz, high_hz })
    }

    /// For preset literals only. Panics (at compile time, in a `const` context) if `low_hz`/`high_hz` aren't at least one octave apart.
    ///
    /// # Panics
    ///
    /// Panics if `high_hz` is less than one octave above `low_hz`.
    #[must_use]
    #[allow(clippy::panic)] // the only way to fail a const-context literal at compile time.
    pub const fn new_const(low_hz: CrossoverFreqLow, high_hz: CrossoverFreqHigh) -> Self {
        match Self::try_new(low_hz, high_hz) {
            Ok(v) => v,
            Err(_) => {
                panic!("CrossoverSplit literal: high_hz must be at least one octave above low_hz")
            }
        }
    }

    /// Returns the low/mid crossover frequency.
    #[must_use]
    pub const fn low_hz(self) -> CrossoverFreqLow {
        self.low_hz
    }

    /// Returns the mid/high crossover frequency.
    #[must_use]
    pub const fn high_hz(self) -> CrossoverFreqHigh {
        self.high_hz
    }
}

/// A validated ascending threshold pair for one band (docs/contracts.md §1).
///
/// `lower_db`/`upper_db` are always consumed together
/// (`DualThresholdCompressor::new`, `BandEnvelope::new`), so the ordering
/// invariant that spans them lives here rather than in `BandParams`.
///
/// Not CLI-configurable (ADR 0006), so its fallible constructor is
/// crate-private, matching `PositiveF32`/`MakeupGain`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThresholdRange {
    lower_db: Threshold,
    upper_db: Threshold,
}

impl ThresholdRange {
    /// # Errors
    ///
    /// Returns `ConfigError::ThresholdOrder` if `lower_db` is not less than `upper_db`.
    pub(crate) const fn try_new(
        lower_db: Threshold,
        upper_db: Threshold,
    ) -> Result<Self, ConfigError> {
        if lower_db.get() >= upper_db.get() {
            return Err(ConfigError::ThresholdOrder {
                lower_db: lower_db.get(),
                upper_db: upper_db.get(),
            });
        }
        Ok(Self { lower_db, upper_db })
    }

    /// For preset literals only. Panics (at compile time, in a `const` context) if `lower_db` isn't less than `upper_db`.
    ///
    /// # Panics
    ///
    /// Panics if `lower_db` is not less than `upper_db`.
    #[allow(clippy::panic)] // the only way to fail a const-context literal at compile time.
    pub(crate) const fn new_const(lower_db: Threshold, upper_db: Threshold) -> Self {
        match Self::try_new(lower_db, upper_db) {
            Ok(v) => v,
            Err(_) => panic!("ThresholdRange literal: lower_db must be less than upper_db"),
        }
    }

    /// Returns the downward compression threshold.
    #[must_use]
    pub const fn lower_db(self) -> Threshold {
        self.lower_db
    }

    /// Returns the upward compression threshold.
    #[must_use]
    pub const fn upper_db(self) -> Threshold {
        self.upper_db
    }
}

/// Global parameters shared across all bands (docs/contracts.md §1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlobalParams {
    /// Pre-split gain in dB.
    pub input_gain_db: IoGain,
    /// Post-sum gain in dB.
    pub output_gain_db: IoGain,
    /// Dry/wet mix.
    pub depth: NormalizedF32,
    /// Attack/release time multiplier.
    pub time: NormalizedF32,
    /// Upward compression amount multiplier.
    pub upward: NormalizedF32,
    /// Downward compression amount multiplier.
    pub downward: NormalizedF32,
    /// Low/mid and mid/high crossover frequency pair.
    pub crossover: CrossoverSplit,
}

/// Per-band parameters (docs/contracts.md §1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BandParams {
    /// Downward/upward compression threshold pair in dB.
    pub thresholds: ThresholdRange,
    /// Upward compression amount.
    pub up_amount: NormalizedF32,
    /// Downward compression amount.
    pub down_amount: NormalizedF32,
    /// Makeup gain in dB.
    pub makeup_gain_db: MakeupGain,
    /// Attack time in ms at `time = 0.5`.
    pub base_attack_ms: PositiveF32,
    /// Release time in ms at `time = 0.5`.
    pub base_release_ms: PositiveF32,
}

/// All parameters used to construct or update an `OttProcessor`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OttParams {
    /// Parameters shared across all bands.
    pub global: GlobalParams,
    /// Per-band parameters.
    pub bands: Bands<BandParams>,
}

impl OttParams {
    /// Validates the one invariant that no value object can express on its own:
    /// the Nyquist-relative crossover limit, which needs `sample_rate` and so
    /// isn't known until JACK reports it.
    ///
    /// Every other cross-field invariant (crossover octave separation, each
    /// band's threshold ordering) is already guaranteed by construction: a
    /// `CrossoverSplit`/`ThresholdRange` cannot exist in an invalid state, so
    /// there is nothing left to check for them here (docs/contracts.md §1).
    ///
    /// # Errors
    ///
    /// Returns `ConfigError` if `sample_rate` is invalid or a crossover
    /// frequency exceeds the Nyquist-relative limit.
    pub fn validate(&self, sample_rate: f32) -> Result<(), ConfigError> {
        let sample_rate = SampleRate::try_new(sample_rate)?;

        let g = &self.global;
        let nyquist_limit = CROSSOVER_NYQUIST_RATIO * sample_rate.get();
        if g.crossover.low_hz().get() > nyquist_limit {
            return Err(ConfigError::CrossoverNyquist {
                field: "low_crossover_hz",
                value: g.crossover.low_hz().get(),
                max: nyquist_limit,
            });
        }

        if g.crossover.high_hz().get() > nyquist_limit {
            return Err(ConfigError::CrossoverNyquist {
                field: "high_crossover_hz",
                value: g.crossover.high_hz().get(),
                max: nyquist_limit,
            });
        }

        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod tests {
    use super::{ConfigError, CrossoverSplit, ThresholdRange};
    use crate::params::{CrossoverFreqHigh, CrossoverFreqLow, Preset, Threshold};

    #[test]
    fn crossover_split_rejects_less_than_one_octave_apart() {
        assert!(matches!(
            CrossoverSplit::try_new(
                CrossoverFreqLow::new_const(1000.0),
                CrossoverFreqHigh::new_const(1500.0),
            ),
            Err(ConfigError::CrossoverOctave { .. })
        ));
    }

    #[test]
    fn crossover_split_accepts_exactly_one_octave_apart() {
        assert!(
            CrossoverSplit::try_new(
                CrossoverFreqLow::new_const(1000.0),
                CrossoverFreqHigh::new_const(2000.0),
            )
            .is_ok()
        );
    }

    #[test]
    fn threshold_range_rejects_inverted_thresholds() {
        assert!(matches!(
            ThresholdRange::try_new(Threshold::new_const(-10.0), Threshold::new_const(-20.0)),
            Err(ConfigError::ThresholdOrder { .. })
        ));
    }

    #[test]
    fn threshold_range_accepts_ascending_thresholds() {
        assert!(
            ThresholdRange::try_new(Threshold::new_const(-20.0), Threshold::new_const(-10.0))
                .is_ok()
        );
    }

    #[test]
    fn rejects_crossover_above_nyquist_ratio() {
        let mut params = Preset::SafeStart.params();
        params.global.crossover = CrossoverSplit::new_const(
            params.global.crossover.low_hz(),
            CrossoverFreqHigh::new_const(8000.0),
        );
        // At 44.1kHz, 0.45*44100 = 19845Hz, so 8kHz is allowed, but confirm it
        // violates the Nyquist constraint near an 8kHz sample-rate boundary.
        assert!(params.validate(16_000.0).is_err());
    }

    #[test]
    fn rejects_invalid_sample_rate() {
        let params = Preset::SafeStart.params();
        assert!(matches!(
            params.validate(f32::NAN),
            Err(ConfigError::SampleRate(_))
        ));
    }
}
