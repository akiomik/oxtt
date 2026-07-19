//! The parameter aggregates passed to `OttProcessor` (docs/contracts.md §1).

use super::error::{ConfigError, validate_sample_rate};
use super::value::{CrossoverPair, GainDb, MakeupGainDb, PositiveMs, ThresholdRange, UnitInterval};

/// Upper-bound coefficient on the Nyquist side that crossover frequencies must respect (docs/contracts.md §1).
pub const CROSSOVER_NYQUIST_RATIO: f32 = 0.45;

/// Global parameters shared across all bands (docs/contracts.md §1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlobalParams {
    /// Pre-split gain in dB.
    pub input_gain_db: GainDb,
    /// Post-sum gain in dB.
    pub output_gain_db: GainDb,
    /// Dry/wet mix.
    pub depth: UnitInterval,
    /// Attack/release time multiplier.
    pub time: UnitInterval,
    /// Upward compression amount multiplier.
    pub upward: UnitInterval,
    /// Downward compression amount multiplier.
    pub downward: UnitInterval,
    /// Low/mid and mid/high crossover frequency pair.
    pub crossovers: CrossoverPair,
}

/// Per-band parameters (docs/contracts.md §1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BandParams {
    /// Downward/upward compression threshold pair in dB.
    pub thresholds: ThresholdRange,
    /// Upward compression amount.
    pub up_amount: UnitInterval,
    /// Downward compression amount.
    pub down_amount: UnitInterval,
    /// Makeup gain in dB.
    pub makeup_gain_db: MakeupGainDb,
    /// Attack time in ms at `time = 0.5`.
    pub base_attack_ms: PositiveMs,
    /// Release time in ms at `time = 0.5`.
    pub base_release_ms: PositiveMs,
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

impl OttParams {
    /// Validates the sample-rate-dependent Nyquist constraint.
    ///
    /// Every other invariant is guaranteed by construction: each field is a
    /// value object that can only be built through its own validated
    /// constructor (docs/contracts.md §1). This is the one check that needs
    /// `sample_rate`, which isn't known until JACK reports it.
    ///
    /// # Errors
    ///
    /// Returns `ConfigError` if `sample_rate` is invalid, or a crossover
    /// frequency exceeds the Nyquist-relative limit.
    pub fn validate(&self, sample_rate: f32) -> Result<(), ConfigError> {
        validate_sample_rate(sample_rate)?;

        let nyquist_limit = CROSSOVER_NYQUIST_RATIO * sample_rate;
        let g = &self.global;
        if g.crossovers.low_hz() > nyquist_limit {
            return Err(ConfigError::CrossoverNyquist {
                field: "low_crossover_hz",
                value: g.crossovers.low_hz(),
                max: nyquist_limit,
            });
        }
        if g.crossovers.high_hz() > nyquist_limit {
            return Err(ConfigError::CrossoverNyquist {
                field: "high_crossover_hz",
                value: g.crossovers.high_hz(),
                max: nyquist_limit,
            });
        }

        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::params::Preset;

    #[test]
    fn rejects_crossover_above_nyquist_ratio() {
        let mut params = Preset::SafeStart.params();
        params.global.crossovers = CrossoverPair::new(params.global.crossovers.low_hz(), 8000.0)
            .unwrap();
        // At 44.1kHz, 0.45*44100 = 19845Hz, so 8kHz is allowed, but confirm it
        // violates the Nyquist constraint near an 8kHz sample-rate boundary.
        assert!(params.validate(16_000.0).is_err());
    }
}
