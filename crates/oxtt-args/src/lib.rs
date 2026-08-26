//! The command-line arguments every oxtt binary shares.
//!
//! [`ParamsArgs`] is the startup preset and the global parameter overrides.
//! All three binaries — the JACK host, the Bela host and the offline renderer
//! — flatten it into their own parser, which is the whole reason it is a crate
//! rather than a module of one of them.
//!
//! Each binary's own flags stay with that binary: they differ by more than
//! they share (one reports xruns and the other underruns, only one has codec
//! levels, only one writes a file), so a shared parser would be a union of
//! things that do not belong together.

use clap::Args;

use oxtt_dsp::params::{
    ConfigError, CrossoverFreqHigh, CrossoverFreqLow, CrossoverSplit, IoGain, NormalizedF32,
    OttParams, Preset,
};

/// Command-line arguments for `oxtt`, a 3-band upward/downward multiband
/// compressor for JACK (see `Cargo.toml` description).
#[derive(Args, Debug, Clone)]
pub struct ParamsArgs {
    /// startup preset
    #[arg(long, value_enum, default_value_t = Preset::default())]
    pub preset: Preset,

    /// per-effect-band input gain, range -24..24
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

/// Crossover octave separation is checked here, immediately after parsing
/// and before JACK is ever contacted: `CrossoverSplit::try_new` cannot
/// produce an invalid pair, so this is the earliest point the invariant can
/// be enforced. The Nyquist-relative crossover limit is not checked here —
/// it additionally needs the sample rate, which isn't known until JACK
/// reports it, so `OttParams::validate` is reached later, indirectly,
/// through `OttProcessor::new` in `jack_host::run`.
impl TryFrom<ParamsArgs> for OttParams {
    type Error = ConfigError;

    fn try_from(args: ParamsArgs) -> Result<Self, ConfigError> {
        let mut params = args.preset.params();

        params.global.input_gain_db = args.input_gain.unwrap_or(params.global.input_gain_db);
        params.global.output_gain_db = args.output_gain.unwrap_or(params.global.output_gain_db);
        params.global.depth = args.depth.unwrap_or(params.global.depth);
        params.global.time = args.time.unwrap_or(params.global.time);
        params.global.upward = args.upward.unwrap_or(params.global.upward);
        params.global.downward = args.downward.unwrap_or(params.global.downward);

        let low_crossover_hz = args
            .low_crossover
            .unwrap_or_else(|| params.global.crossover.low_hz());
        let high_crossover_hz = args
            .high_crossover
            .unwrap_or_else(|| params.global.crossover.high_hz());
        params.global.crossover = CrossoverSplit::try_new(low_crossover_hz, high_crossover_hz)?;

        Ok(params)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod tests {
    use clap::Parser;

    use super::*;

    /// The shared arguments flattened into a parser of their own, which is how
    /// all three binaries use them.
    #[derive(Parser, Debug, Clone)]
    #[command(name = "oxtt", allow_negative_numbers = true)]
    struct TestCli {
        #[command(flatten)]
        params: ParamsArgs,
    }

    fn parse(args: &[&str]) -> OttParams {
        OttParams::try_from(TestCli::parse_from(args).params).unwrap()
    }

    #[test]
    fn unset_options_fall_back_to_preset() {
        assert_eq!(
            parse(&["oxtt", "--preset", "safe-start"]),
            Preset::SafeStart.params()
        );
    }

    #[test]
    fn individual_options_override_preset() {
        let params = parse(&["oxtt", "--preset", "default", "--output-gain", "-6"]);
        assert_eq!(params.global.output_gain_db.get(), -6.0);
    }

    #[test]
    fn riot_selects_its_complete_v0_parameters() {
        assert_eq!(parse(&["oxtt", "--preset", "riot"]), Preset::Riot.params());
    }

    #[test]
    fn input_gain_and_output_gain_are_independent() {
        let params = parse(&["oxtt", "--preset", "default", "--input-gain", "3"]);
        assert_eq!(params.global.input_gain_db.get(), 3.0);
        assert_eq!(
            params.global.output_gain_db.get(),
            Preset::Default.params().global.output_gain_db.get()
        );
    }

    #[test]
    fn crossover_options_apply_regardless_of_flag_order() {
        let a = parse(&["oxtt", "--low-crossover", "150", "--high-crossover", "3000"]);
        let b = parse(&["oxtt", "--high-crossover", "3000", "--low-crossover", "150"]);
        assert_eq!(a, b);
        assert_eq!(a.global.crossover.low_hz().get(), 150.0);
        assert_eq!(a.global.crossover.high_hz().get(), 3000.0);
    }

    #[test]
    fn rejects_out_of_range_value_at_parse_time() {
        assert!(TestCli::try_parse_from(["oxtt", "--depth", "2.0"]).is_err());
        assert!(TestCli::try_parse_from(["oxtt", "--input-gain", "100"]).is_err());
        assert!(TestCli::try_parse_from(["oxtt", "--low-crossover", "10"]).is_err());
    }

    #[test]
    fn crossover_octave_separation_is_enforced_before_any_host_exists() {
        // Single-field ranges are checked at parse time; the octave
        // separation between low/high crossover spans two fields but no
        // longer needs the sample rate, so it is enforced right here too,
        // before a binary has touched an audio system (docs/oxtt/contracts.md §1).
        let args = TestCli::parse_from([
            "oxtt",
            "--low-crossover",
            "1000",
            "--high-crossover",
            "1500",
        ])
        .params;
        assert!(matches!(
            OttParams::try_from(args),
            Err(ConfigError::CrossoverOctave { .. })
        ));
    }
}
