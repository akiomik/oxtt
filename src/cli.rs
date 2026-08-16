//! Command-line argument definitions for the `oxtt` binary (docs/contracts.md §1).

#[cfg(feature = "bela-host")]
use core::num::NonZeroU32;

use clap::{Args, Parser};

#[cfg(feature = "bela-host")]
use crate::bela_host::RunOptions;
use crate::params::{
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

/// Command-line arguments for the JACK host.
#[derive(Parser, Debug, Clone)]
#[command(
    version,
    about,
    long_about = None,
    after_help = "NOTE: `default` and `riot` presets are intentionally strong and can exceed 0 dBFS.\nStart with `safe-start` and a low monitor level.",
    allow_negative_numbers = true
)]
pub struct Cli {
    /// Startup preset and global parameter overrides.
    #[command(flatten)]
    pub params: ParamsArgs,

    /// print the JACK xrun count to stderr after a normal exit
    #[arg(long)]
    pub report_xruns_on_exit: bool,

    /// drive depth/time/upward/downward from the hardware control surface
    /// (MCP3008 pots on SPI0/CE0, bypass switch on GPIO17)
    ///
    /// Opt-in rather than on by default even in a `pi-controls` build: the
    /// same binary has to stay runnable on a Pi with no breadboard attached,
    /// which is how the audio verification scripts under `scripts/` run it.
    /// The flag does not exist at all without the feature, so a build that
    /// cannot read the hardware cannot be asked to.
    #[cfg(feature = "pi-controls")]
    #[arg(long)]
    pub controls: bool,
}

/// Command-line arguments for the Bela host.
///
/// A separate parser from [`Cli`] rather than shared flags, because the two
/// hosts differ in more than they share: JACK reports xruns and Bela reports
/// underruns, JACK is told its block size and sample rate by the server while
/// Bela is asked for them, and only Bela has codec levels.
///
/// oxtt does not pass a command line on to `bela::Bela`. Everything the board
/// needs to be told has a flag here, so that there is one `--help` and so that
/// libbela's own options are never exposed — several of them end the process
/// rather than report an error (bela-rs `docs/board-facts.md`), and
/// `--thread-count` would silently break DSP that carries state across frames
/// if `OttApplication::validate_settings` were not there to catch it.
#[cfg(feature = "bela-host")]
#[derive(Parser, Debug, Clone)]
#[command(
    name = "oxtt-bela",
    version,
    about = "A 3-band upward/downward multiband compressor for Bela Gem Stereo",
    long_about = None,
    after_help = "NOTE: `default` and `riot` presets are intentionally strong and can exceed 0 dBFS.\nStart with `safe-start` and a low monitor level.",
    allow_negative_numbers = true
)]
pub struct BelaCli {
    /// Startup preset and global parameter overrides.
    #[command(flatten)]
    pub params: ParamsArgs,

    /// audio frames per block
    #[arg(long, default_value_t = crate::bela_host::PERIOD_SIZE)]
    pub period: NonZeroU32,

    /// audio sample rate to ask the board for
    #[arg(long, value_name = "Hz", default_value_t = crate::bela_host::SAMPLE_RATE_HZ)]
    pub sample_rate: NonZeroU32,

    /// drive depth/time/upward/downward and the two gains from the hardware
    /// control surface (pots on A0-A5, bypass switch on D0)
    ///
    /// Opt-in for the same reason as the Raspberry Pi's flag: the same binary
    /// has to stay runnable on a board with nothing wired to its headers,
    /// which is how the audio verification runs it.
    #[arg(long)]
    pub controls: bool,

    /// measure CPU load this many times per block and report it on exit
    ///
    /// Off by default because libbela refuses CPU monitoring above a period
    /// of `MAX_MONITORED_PERIOD_SIZE`, and failing to start over a diagnostic
    /// nobody asked for would be the wrong trade.
    #[arg(long, value_name = "PER_BLOCK")]
    pub report_cpu: Option<NonZeroU32>,

    /// print the run's underrun and control-surface counts to stderr after a
    /// normal exit
    #[arg(long)]
    pub report_on_exit: bool,

    /// codec analog input gain, applied before the DSP
    ///
    /// Not to be confused with `--input-gain`, which is the per-effect-band
    /// gain inside the DSP. This one is the converter's.
    ///
    /// Set it as high as the source allows without clipping —
    /// `--report-on-exit` says where that is — and check whether the last few
    /// decibels bought anything; with one source they stopped paying at
    /// +6 dB. Below -12 dB the codec stops responding at all.
    #[arg(long, value_name = "dB")]
    pub adc_gain_db: Option<f32>,

    /// codec headphone output level, applied after the DSP
    ///
    /// Not to be confused with `--output-gain`, which is the post-sum gain
    /// inside the DSP. This one is the converter's, and on a Gem Stereo it is
    /// what sets the line output's level — libbela's line out level writes
    /// registers this board does not use. Set it for the level the next device
    /// wants; unlike `--adc-gain-db` it does not buy signal-to-noise.
    #[arg(long, value_name = "dB")]
    pub headphone_level_db: Option<f32>,

    /// light an LED on this digital channel while the input is clipping
    ///
    /// Nothing on this board reports input clipping, so without an indicator
    /// it is only visible after the run, in `--report-on-exit`. `D0` is
    /// refused: the bypass switch is wired there whether or not `--controls`
    /// asked for it. See `docs/bela/control-surface-setup.md` for the wiring.
    #[arg(long, value_name = "CHANNEL")]
    pub clip_led: Option<usize>,
}

#[cfg(feature = "bela-host")]
impl From<&BelaCli> for RunOptions {
    fn from(cli: &BelaCli) -> Self {
        Self {
            period_size: cli.period,
            sample_rate: cli.sample_rate,
            controls: cli.controls,
            cpu_monitoring: cli.report_cpu,
            adc_gain_db: cli.adc_gain_db,
            headphone_level_db: cli.headphone_level_db,
            clip_led: cli.clip_led,
            report_on_exit: cli.report_on_exit,
        }
    }
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

impl TryFrom<Cli> for OttParams {
    type Error = ConfigError;

    fn try_from(cli: Cli) -> Result<Self, ConfigError> {
        cli.params.try_into()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn unset_options_fall_back_to_preset() {
        let cli = Cli::parse_from(["oxtt", "--preset", "safe-start"]);
        let params = OttParams::try_from(cli).unwrap();
        assert_eq!(params, Preset::SafeStart.params());
    }

    #[test]
    fn individual_options_override_preset() {
        let cli = Cli::parse_from(["oxtt", "--preset", "default", "--output-gain", "-6"]);
        let params = OttParams::try_from(cli).unwrap();
        assert_eq!(params.global.output_gain_db.get(), -6.0);
    }

    #[test]
    fn riot_selects_its_complete_v0_parameters() {
        let cli = Cli::parse_from(["oxtt", "--preset", "riot"]);
        let params = OttParams::try_from(cli).unwrap();
        assert_eq!(params, Preset::Riot.params());
    }

    #[test]
    fn input_gain_and_output_gain_are_independent() {
        let cli = Cli::parse_from(["oxtt", "--preset", "default", "--input-gain", "3"]);
        let params = OttParams::try_from(cli).unwrap();
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
        let params_a = OttParams::try_from(a).unwrap();
        let params_b = OttParams::try_from(b).unwrap();
        assert_eq!(params_a, params_b);
        assert_eq!(params_a.global.crossover.low_hz().get(), 150.0);
        assert_eq!(params_a.global.crossover.high_hz().get(), 3000.0);
    }

    #[test]
    fn rejects_out_of_range_value_at_parse_time() {
        assert!(Cli::try_parse_from(["oxtt", "--depth", "2.0"]).is_err());
        assert!(Cli::try_parse_from(["oxtt", "--input-gain", "100"]).is_err());
        assert!(Cli::try_parse_from(["oxtt", "--low-crossover", "10"]).is_err());
    }

    #[test]
    fn xrun_report_is_opt_in() {
        assert!(!Cli::parse_from(["oxtt"]).report_xruns_on_exit);
        assert!(Cli::parse_from(["oxtt", "--report-xruns-on-exit"]).report_xruns_on_exit);
    }

    /// The control surface must stay off unless it is asked for, so that a
    /// `pi-controls` build with no breadboard attached still starts (see the
    /// flag's own documentation).
    #[cfg(feature = "pi-controls")]
    #[test]
    fn the_control_surface_is_opt_in() {
        assert!(!Cli::parse_from(["oxtt"]).controls);
        assert!(Cli::parse_from(["oxtt", "--controls"]).controls);
    }

    /// Without the feature the flag is not merely off, it does not exist —
    /// asking for it is an argument error rather than a silent no-op.
    #[cfg(not(feature = "pi-controls"))]
    #[test]
    fn there_is_no_control_surface_flag_without_the_feature() {
        assert!(
            Cli::try_parse_from(["oxtt", "--controls"]).is_err(),
            "--controls must not parse in a build that cannot read the hardware"
        );
    }

    #[test]
    fn try_from_cli_enforces_crossover_octave_separation_before_jack_is_contacted() {
        // Single-field ranges are checked at parse time; the octave
        // separation between low/high crossover spans two fields but no
        // longer needs the sample rate, so it's enforced right here too,
        // before `main` ever touches JACK (docs/contracts.md §1).
        let cli = Cli::parse_from([
            "oxtt",
            "--low-crossover",
            "1000",
            "--high-crossover",
            "1500",
        ]);
        assert!(matches!(
            OttParams::try_from(cli),
            Err(ConfigError::CrossoverOctave { .. })
        ));
    }
}
