//! Startup presets (docs/contracts.md §1, ADR 0006).

use std::str::FromStr;

use super::model::{BandParams, GlobalParams, OttParams};
use super::value::{CrossoverPair, GainDb, MakeupGainDb, PositiveMs, ThresholdRange, UnitInterval};

/// Startup presets (docs/contracts.md §1, ADR 0006).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
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
        thresholds: ThresholdRange::new_const(-35.0, -28.0),
        up_amount: UnitInterval::new_const(0.800),
        down_amount: UnitInterval::new_const(0.900),
        makeup_gain_db: MakeupGainDb::new_const(16.3),
        base_attack_ms: PositiveMs::new_const(2.8),
        base_release_ms: PositiveMs::new_const(40.0),
    };
    const MID_BAND: BandParams = BandParams {
        thresholds: ThresholdRange::new_const(-36.0, -25.0),
        up_amount: UnitInterval::new_const(0.800),
        down_amount: UnitInterval::new_const(0.857),
        makeup_gain_db: MakeupGainDb::new_const(11.7),
        base_attack_ms: PositiveMs::new_const(1.4),
        base_release_ms: PositiveMs::new_const(28.0),
    };
    const HIGH_BAND: BandParams = BandParams {
        thresholds: ThresholdRange::new_const(-35.0, -30.0),
        up_amount: UnitInterval::new_const(0.800),
        down_amount: UnitInterval::new_const(1.000),
        makeup_gain_db: MakeupGainDb::new_const(16.3),
        base_attack_ms: PositiveMs::new_const(0.7),
        base_release_ms: PositiveMs::new_const(15.0),
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
                input_gain_db: GainDb::new_const(0.0),
                output_gain_db: GainDb::new_const(-18.0),
                depth: UnitInterval::new_const(0.5),
                time: UnitInterval::new_const(0.5),
                upward: UnitInterval::new_const(1.0),
                downward: UnitInterval::new_const(1.0),
                crossovers: CrossoverPair::new_const(120.0, 2500.0),
            },
            Self::Default => GlobalParams {
                input_gain_db: GainDb::new_const(0.0),
                output_gain_db: GainDb::new_const(0.0),
                depth: UnitInterval::new_const(1.0),
                time: UnitInterval::new_const(0.5),
                upward: UnitInterval::new_const(1.0),
                downward: UnitInterval::new_const(1.0),
                crossovers: CrossoverPair::new_const(120.0, 2500.0),
            },
        };
        OttParams { global, bands }
    }

    /// Returns the preset's `--preset` CLI value.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SafeStart => "safe-start",
            Self::Default => "default",
        }
    }
}

impl FromStr for Preset {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "safe-start" => Ok(Self::SafeStart),
            "default" => Ok(Self::Default),
            _ => Err(()),
        }
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
