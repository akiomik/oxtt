//! Startup presets (docs/contracts.md §1, ADR 0006).

use clap::ValueEnum;

use super::model::{BandParams, CrossoverSplit, GlobalParams, OttParams, ThresholdRange};
use super::value::{
    CrossoverFreqHigh, CrossoverFreqLow, IoGain, MakeupGain, NormalizedF32, PositiveF32, Threshold,
};

/// Startup presets (docs/contracts.md §1, ADR 0006).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum Preset {
    /// Conservative output level, suitable for a first listen (docs/contracts.md §1).
    #[default]
    SafeStart,
    /// Intentionally strong preset that can exceed 0 dBFS (ADR 0006).
    Default,
}

impl Preset {
    // Band values are fixed as a compatibility target for the `Default` preset, per ADR 0006.
    const LOW_BAND: BandParams = BandParams {
        thresholds: ThresholdRange::new_const(
            Threshold::new_const(-35.0),
            Threshold::new_const(-28.0),
        ),
        up_amount: NormalizedF32::new_const(0.800),
        down_amount: NormalizedF32::new_const(0.900),
        makeup_gain_db: MakeupGain::new_const(16.3),
        base_attack_ms: PositiveF32::new_const(2.8),
        base_release_ms: PositiveF32::new_const(40.0),
    };
    const MID_BAND: BandParams = BandParams {
        thresholds: ThresholdRange::new_const(
            Threshold::new_const(-36.0),
            Threshold::new_const(-25.0),
        ),
        up_amount: NormalizedF32::new_const(0.800),
        down_amount: NormalizedF32::new_const(0.857),
        makeup_gain_db: MakeupGain::new_const(11.7),
        base_attack_ms: PositiveF32::new_const(1.4),
        base_release_ms: PositiveF32::new_const(28.0),
    };
    const HIGH_BAND: BandParams = BandParams {
        thresholds: ThresholdRange::new_const(
            Threshold::new_const(-35.0),
            Threshold::new_const(-30.0),
        ),
        up_amount: NormalizedF32::new_const(0.800),
        down_amount: NormalizedF32::new_const(1.000),
        makeup_gain_db: MakeupGain::new_const(16.3),
        base_attack_ms: PositiveF32::new_const(0.7),
        base_release_ms: PositiveF32::new_const(15.0),
    };

    const fn bands() -> [BandParams; 3] {
        [Self::LOW_BAND, Self::MID_BAND, Self::HIGH_BAND]
    }

    /// Returns the complete parameters for this preset.
    #[must_use]
    pub const fn params(self) -> OttParams {
        let bands = Self::bands();
        let global = match self {
            Self::SafeStart => GlobalParams {
                input_gain_db: IoGain::new_const(0.0),
                output_gain_db: IoGain::new_const(-18.0),
                depth: NormalizedF32::new_const(0.5),
                time: NormalizedF32::new_const(0.5),
                upward: NormalizedF32::new_const(1.0),
                downward: NormalizedF32::new_const(1.0),
                crossover: CrossoverSplit::new_const(
                    CrossoverFreqLow::new_const(120.0),
                    CrossoverFreqHigh::new_const(2500.0),
                ),
            },
            Self::Default => GlobalParams {
                input_gain_db: IoGain::new_const(0.0),
                output_gain_db: IoGain::new_const(0.0),
                depth: NormalizedF32::new_const(1.0),
                time: NormalizedF32::new_const(0.5),
                upward: NormalizedF32::new_const(1.0),
                downward: NormalizedF32::new_const(1.0),
                crossover: CrossoverSplit::new_const(
                    CrossoverFreqLow::new_const(120.0),
                    CrossoverFreqHigh::new_const(2500.0),
                ),
            },
        };
        OttParams { global, bands }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn safe_start_and_default_params_are_valid() {
        Preset::SafeStart.params().validate(48_000.0).unwrap();
        Preset::Default.params().validate(48_000.0).unwrap();
    }

    #[test]
    fn presets_share_band_values() {
        assert_eq!(
            Preset::SafeStart.params().bands,
            Preset::Default.params().bands
        );
    }
}
