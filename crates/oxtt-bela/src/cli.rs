//! Command-line arguments for the `oxtt-bela` binary (docs/contracts.md §1).
//!
//! Lives beside the host rather than in [`crate::cli`] so that the shared
//! argument definitions do not have to know a Bela exists: [`BelaCli`] reads
//! this module's own defaults and builds [`RunOptions`](super::RunOptions),
//! both of which belong to the host.

use core::num::NonZeroU32;

use clap::Parser;

use super::RunOptions;
use oxtt_args::ParamsArgs;

/// Command-line arguments for the Bela host.
///
/// A separate parser from `oxtt`'s own `Cli` rather than shared flags, because the two
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
    #[arg(long, default_value_t = super::PERIOD_SIZE)]
    pub period: NonZeroU32,

    /// audio sample rate to ask the board for
    #[arg(long, value_name = "Hz", default_value_t = super::SAMPLE_RATE_HZ)]
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
