//! The parameter aggregates passed to `OttProcessor` (docs/contracts.md §1).

use super::error::{ConfigError, validate_sample_rate};
use super::value::{IoGain, MakeupGain, NormalizedF32, PositiveF32, Threshold};
use super::{CrossoverFreqHigh, CrossoverFreqLow};

/// Upper-bound coefficient on the Nyquist side that crossover frequencies must respect (docs/contracts.md §1).
pub const CROSSOVER_NYQUIST_RATIO: f32 = 0.45;

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
    /// Low/mid crossover frequency pair.
    pub low_crossover_hz: CrossoverFreqLow,
    /// Mid/high crossover frequency pair.
    pub high_crossover_hz: CrossoverFreqHigh,
}

/// Per-band parameters (docs/contracts.md §1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BandParams {
    /// Downward compression threshold in dB.
    pub lower_threshold_db: Threshold,
    /// Upward compression threshold in dB.
    pub upper_threshold_db: Threshold,
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
    /// Per-band parameters, indexed by `BAND_LOW`/`BAND_MID`/`BAND_HIGH`.
    pub bands: [BandParams; 3],
}

/// Index of the low band within `OttParams::bands`.
pub const BAND_LOW: usize = 0;
/// Index of the mid band within `OttParams::bands`.
pub const BAND_MID: usize = 1;
/// Index of the high band within `OttParams::bands`.
pub const BAND_HIGH: usize = 2;

/// Names of `OttParams::bands`, indexed the same way as `BAND_LOW`/`BAND_MID`/`BAND_HIGH`.
const BAND_NAMES: [&str; 3] = ["low", "mid", "high"];

impl OttParams {
    /// Validates every invariant that spans more than one field.
    ///
    /// Each individual field is already guaranteed valid by construction:
    /// it's a value object that can only be built through its own validated
    /// constructor (docs/contracts.md §1). What's left is what no single
    /// field's constructor can see on its own: the crossover octave
    /// separation, each band's threshold ordering, and the Nyquist-relative
    /// crossover limit, which additionally needs `sample_rate` and so isn't
    /// known until JACK reports it.
    ///
    /// # Errors
    ///
    /// Returns `ConfigError` if `sample_rate` is invalid, the crossover pair
    /// is less than one octave apart, a crossover frequency exceeds the
    /// Nyquist-relative limit, or a band's threshold pair is inverted.
    pub fn validate(&self, sample_rate: f32) -> Result<(), ConfigError> {
        validate_sample_rate(sample_rate)?;

        let g = &self.global;
        if g.high_crossover_hz.get() < 2.0 * g.low_crossover_hz.get() {
            return Err(ConfigError::CrossoverOctave {
                low_hz: g.low_crossover_hz.get(),
                high_hz: g.high_crossover_hz.get(),
            });
        }

        let nyquist_limit = CROSSOVER_NYQUIST_RATIO * sample_rate;
        if g.low_crossover_hz.get() > nyquist_limit {
            return Err(ConfigError::CrossoverNyquist {
                field: "low_crossover_hz",
                value: g.low_crossover_hz.get(),
                max: nyquist_limit,
            });
        }

        if g.high_crossover_hz.get() > nyquist_limit {
            return Err(ConfigError::CrossoverNyquist {
                field: "high_crossover_hz",
                value: g.high_crossover_hz.get(),
                max: nyquist_limit,
            });
        }

        for (band, name) in self.bands.iter().zip(BAND_NAMES) {
            if band.lower_threshold_db.get() >= band.upper_threshold_db.get() {
                return Err(ConfigError::ThresholdOrder {
                    band: name,
                    lower_db: band.lower_threshold_db.get(),
                    upper_db: band.upper_threshold_db.get(),
                });
            }
        }

        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{BAND_LOW, ConfigError};
    use crate::params::{CrossoverFreqHigh, CrossoverFreqLow, Preset, Threshold};

    #[test]
    fn rejects_crossover_above_nyquist_ratio() {
        let mut params = Preset::SafeStart.params();
        params.global.high_crossover_hz = CrossoverFreqHigh::new_const(8000.0);
        // At 44.1kHz, 0.45*44100 = 19845Hz, so 8kHz is allowed, but confirm it
        // violates the Nyquist constraint near an 8kHz sample-rate boundary.
        assert!(params.validate(16_000.0).is_err());
    }

    #[test]
    fn rejects_crossover_less_than_one_octave_apart() {
        let mut params = Preset::SafeStart.params();
        params.global.low_crossover_hz = CrossoverFreqLow::new_const(1000.0);
        params.global.high_crossover_hz = CrossoverFreqHigh::new_const(1500.0);
        assert!(matches!(
            params.validate(48_000.0),
            Err(ConfigError::CrossoverOctave { .. })
        ));
    }

    #[test]
    fn accepts_crossover_exactly_one_octave_apart() {
        let mut params = Preset::SafeStart.params();
        params.global.low_crossover_hz = CrossoverFreqLow::new_const(1000.0);
        params.global.high_crossover_hz = CrossoverFreqHigh::new_const(2000.0);
        assert!(params.validate(48_000.0).is_ok());
    }

    #[test]
    fn rejects_inverted_thresholds() {
        let mut params = Preset::SafeStart.params();
        params.bands[BAND_LOW].lower_threshold_db = Threshold::new_const(-10.0);
        params.bands[BAND_LOW].upper_threshold_db = Threshold::new_const(-20.0);
        assert!(matches!(
            params.validate(48_000.0),
            Err(ConfigError::ThresholdOrder { band: "low", .. })
        ));
    }
}
