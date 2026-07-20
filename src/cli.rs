//! Command-line argument definitions for the `oxtt` binary (docs/contracts.md §1).

use clap::Parser;

use crate::params::{
    CrossoverFreqHigh, CrossoverFreqLow, IoGain, NormalizedF32, OttParams, Preset,
};

/// Command-line arguments for `oxtt`, a 3-band upward/downward multiband
/// compressor for JACK (see `Cargo.toml` description).
#[derive(Parser, Debug, Clone)]
#[command(
    version,
    about,
    long_about = None,
    after_help = "NOTE: `default` preset is intentionally strong and can exceed 0 dBFS.\nStart with `safe-start` and a low monitor level.",
    allow_negative_numbers = true
)]
pub struct Cli {
    /// startup preset
    #[arg(long, value_enum, default_value_t = Preset::default())]
    pub preset: Preset,

    /// pre-split gain, range -24..24
    #[arg(long, value_name = "dB")]
    pub input_gain: Option<IoGain>,

    /// post-sum gain, range -24..24
    #[arg(long, value_name = "dB")]
    pub output_gain: Option<IoGain>,

    /// dry/wet, range 0..1
    #[arg(long)]
    pub depth: Option<NormalizedF32>,

    /// attack/release multiplier, range 0..1
    #[arg(long)]
    pub time: Option<NormalizedF32>,

    /// upward amount multiplier, range 0..1
    #[arg(long)]
    pub upward: Option<NormalizedF32>,

    /// downward amount multiplier, range 0..1
    #[arg(long)]
    pub downward: Option<NormalizedF32>,

    /// low/mid split, range 40..2000
    #[arg(long, value_name = "Hz")]
    pub low_crossover: Option<CrossoverFreqLow>,

    /// mid/high split, range 400..16000
    #[arg(long, value_name = "Hz")]
    pub high_crossover: Option<CrossoverFreqHigh>,
}

// FIXME: This conversion does not call `OttParams::validate`, so cross-field
// invariants (crossover octave separation, threshold ordering) aren't
// enforced here (see `from_cli_does_not_enforce_crossover_octave_separation`
// below). The only current caller reaches `OttParams::validate` indirectly
// through `OttProcessor::new` in `jack_host::run`, after a JACK client has
// already connected and registered ports — so a purely input-level error
// isn't reported until JACK is contacted. Known design gap; revisit whether
// this conversion (or `main.rs`) should validate what it can before that.
impl From<Cli> for OttParams {
    fn from(cli: Cli) -> Self {
        let mut params = cli.preset.params();

        params.global.input_gain_db = cli.input_gain.unwrap_or(params.global.input_gain_db);
        params.global.output_gain_db = cli.output_gain.unwrap_or(params.global.output_gain_db);
        params.global.depth = cli.depth.unwrap_or(params.global.depth);
        params.global.time = cli.time.unwrap_or(params.global.time);
        params.global.upward = cli.upward.unwrap_or(params.global.upward);
        params.global.downward = cli.downward.unwrap_or(params.global.downward);
        params.global.low_crossover_hz =
            cli.low_crossover.unwrap_or(params.global.low_crossover_hz);
        params.global.high_crossover_hz = cli
            .high_crossover
            .unwrap_or(params.global.high_crossover_hz);

        params
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn unset_options_fall_back_to_preset() {
        let cli = Cli::parse_from(["oxtt", "--preset", "safe-start"]);
        let params: OttParams = cli.into();
        assert_eq!(params, Preset::SafeStart.params());
    }

    #[test]
    fn individual_options_override_preset() {
        let cli = Cli::parse_from(["oxtt", "--preset", "default", "--output-gain", "-6"]);
        let params: OttParams = cli.into();
        assert_eq!(params.global.output_gain_db.get(), -6.0);
    }

    #[test]
    fn input_gain_and_output_gain_are_independent() {
        let cli = Cli::parse_from(["oxtt", "--preset", "default", "--input-gain", "3"]);
        let params: OttParams = cli.into();
        assert_eq!(params.global.input_gain_db.get(), 3.0);
        assert_eq!(
            params.global.output_gain_db.get(),
            Preset::Default.params().global.output_gain_db.get()
        );
    }

    #[test]
    fn crossover_options_apply_regardless_of_flag_order() {
        let a = Cli::parse_from(["oxtt", "--low-crossover", "150", "--high-crossover", "3000"]);
        let b = Cli::parse_from(["oxtt", "--high-crossover", "3000", "--low-crossover", "150"]);
        let params_a: OttParams = a.into();
        let params_b: OttParams = b.into();
        assert_eq!(params_a, params_b);
        assert_eq!(params_a.global.low_crossover_hz.get(), 150.0);
        assert_eq!(params_a.global.high_crossover_hz.get(), 3000.0);
    }

    #[test]
    fn rejects_out_of_range_value_at_parse_time() {
        assert!(Cli::try_parse_from(["oxtt", "--depth", "2.0"]).is_err());
        assert!(Cli::try_parse_from(["oxtt", "--input-gain", "100"]).is_err());
        assert!(Cli::try_parse_from(["oxtt", "--low-crossover", "10"]).is_err());
    }

    #[test]
    fn from_cli_does_not_enforce_crossover_octave_separation() {
        // Single-field ranges are checked at parse time, but the octave
        // separation between low/high crossover spans two fields and is
        // enforced later by `OttParams::validate` (docs/contracts.md §1),
        // not by this conversion.
        let cli = Cli::parse_from([
            "oxtt",
            "--low-crossover",
            "1000",
            "--high-crossover",
            "1500",
        ]);
        let params: OttParams = cli.into();
        assert!(params.validate(48_000.0).is_err());
    }
}
