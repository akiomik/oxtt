//! Startup presets (docs/contracts.md §1, ADR 0006).

use clap::ValueEnum;

use crate::bands::Bands;

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
    /// Aggressive, fully wet compression voicing that can exceed 0 dBFS.
    Riot,
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
    const RIOT_LOW_BAND: BandParams = BandParams {
        thresholds: ThresholdRange::new_const(
            Threshold::new_const(-32.0),
            Threshold::new_const(-29.0),
        ),
        up_amount: NormalizedF32::new_const(1.0),
        down_amount: NormalizedF32::new_const(1.0),
        makeup_gain_db: MakeupGain::new_const(16.0),
        base_attack_ms: PositiveF32::new_const(5.0),
        base_release_ms: PositiveF32::new_const(100.0),
    };
    const RIOT_MID_BAND: BandParams = BandParams {
        thresholds: ThresholdRange::new_const(
            Threshold::new_const(-35.0),
            Threshold::new_const(-31.0),
        ),
        up_amount: NormalizedF32::new_const(1.0),
        down_amount: NormalizedF32::new_const(1.0),
        makeup_gain_db: MakeupGain::new_const(18.0),
        base_attack_ms: PositiveF32::new_const(2.0),
        base_release_ms: PositiveF32::new_const(60.0),
    };
    const RIOT_HIGH_BAND: BandParams = BandParams {
        thresholds: ThresholdRange::new_const(
            Threshold::new_const(-38.0),
            Threshold::new_const(-34.0),
        ),
        up_amount: NormalizedF32::new_const(1.0),
        down_amount: NormalizedF32::new_const(1.0),
        makeup_gain_db: MakeupGain::new_const(20.0),
        base_attack_ms: PositiveF32::new_const(0.8),
        base_release_ms: PositiveF32::new_const(30.0),
    };

    const fn bands() -> Bands<BandParams> {
        Bands {
            low: Self::LOW_BAND,
            mid: Self::MID_BAND,
            high: Self::HIGH_BAND,
        }
    }

    const fn riot_bands() -> Bands<BandParams> {
        Bands {
            low: Self::RIOT_LOW_BAND,
            mid: Self::RIOT_MID_BAND,
            high: Self::RIOT_HIGH_BAND,
        }
    }

    /// Returns the complete parameters for this preset.
    #[must_use]
    pub const fn params(self) -> OttParams {
        let (global, bands) = match self {
            Self::SafeStart => (
                GlobalParams {
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
                Self::bands(),
            ),
            Self::Default => (
                GlobalParams {
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
                Self::bands(),
            ),
            Self::Riot => (
                GlobalParams {
                    input_gain_db: IoGain::new_const(6.0),
                    output_gain_db: IoGain::new_const(-6.0),
                    depth: NormalizedF32::new_const(1.0),
                    time: NormalizedF32::new_const(0.30),
                    upward: NormalizedF32::new_const(1.0),
                    downward: NormalizedF32::new_const(1.0),
                    crossover: CrossoverSplit::new_const(
                        CrossoverFreqLow::new_const(180.0),
                        CrossoverFreqHigh::new_const(1800.0),
                    ),
                },
                Self::riot_bands(),
            ),
        };
        OttParams { global, bands }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::bands::Bands;

    #[test]
    fn all_preset_params_are_valid() {
        Preset::SafeStart.params().validate(48_000.0).unwrap();
        Preset::Default.params().validate(48_000.0).unwrap();
        Preset::Riot.params().validate(48_000.0).unwrap();
    }

    #[test]
    fn presets_share_band_values() {
        assert_eq!(
            Preset::SafeStart.params().bands,
            Preset::Default.params().bands
        );
    }

    #[test]
    fn riot_has_the_v0_voicing() {
        assert_eq!(
            Preset::Riot.params(),
            OttParams {
                global: GlobalParams {
                    input_gain_db: IoGain::new_const(6.0),
                    output_gain_db: IoGain::new_const(-6.0),
                    depth: NormalizedF32::new_const(1.0),
                    time: NormalizedF32::new_const(0.30),
                    upward: NormalizedF32::new_const(1.0),
                    downward: NormalizedF32::new_const(1.0),
                    crossover: CrossoverSplit::new_const(
                        CrossoverFreqLow::new_const(180.0),
                        CrossoverFreqHigh::new_const(1800.0),
                    ),
                },
                bands: Bands {
                    low: BandParams {
                        thresholds: ThresholdRange::new_const(
                            Threshold::new_const(-32.0),
                            Threshold::new_const(-29.0),
                        ),
                        up_amount: NormalizedF32::new_const(1.0),
                        down_amount: NormalizedF32::new_const(1.0),
                        makeup_gain_db: MakeupGain::new_const(16.0),
                        base_attack_ms: PositiveF32::new_const(5.0),
                        base_release_ms: PositiveF32::new_const(100.0),
                    },
                    mid: BandParams {
                        thresholds: ThresholdRange::new_const(
                            Threshold::new_const(-35.0),
                            Threshold::new_const(-31.0),
                        ),
                        up_amount: NormalizedF32::new_const(1.0),
                        down_amount: NormalizedF32::new_const(1.0),
                        makeup_gain_db: MakeupGain::new_const(18.0),
                        base_attack_ms: PositiveF32::new_const(2.0),
                        base_release_ms: PositiveF32::new_const(60.0),
                    },
                    high: BandParams {
                        thresholds: ThresholdRange::new_const(
                            Threshold::new_const(-38.0),
                            Threshold::new_const(-34.0),
                        ),
                        up_amount: NormalizedF32::new_const(1.0),
                        down_amount: NormalizedF32::new_const(1.0),
                        makeup_gain_db: MakeupGain::new_const(20.0),
                        base_attack_ms: PositiveF32::new_const(0.8),
                        base_release_ms: PositiveF32::new_const(30.0),
                    },
                },
            }
        );
    }
}
