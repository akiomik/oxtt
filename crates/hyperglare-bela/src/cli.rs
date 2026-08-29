//! Command-line arguments for the `hyperglare-bela` binary.
//!
//! Lives beside the host rather than in [`hyperglare_args`] so that the shared
//! argument definitions do not have to know a Bela exists: this module reads
//! the host's own defaults and builds [`RunOptions`], both
//! of which belong to the host.

use core::num::NonZeroU32;

use clap::Parser;

use super::RunOptions;
use hyperglare_args::ParamsArgs;

/// Command-line arguments for the Bela host.
///
/// hyperglare does not pass a command line on to `bela::Bela`. Everything the
/// board needs to be told has a flag here, so that there is one `--help` and
/// so that libbela's own options are never exposed — several of them end the
/// process rather than report an error (bela-rs `docs/board-facts.md`), and
/// `--thread-count` would silently split a resonator's tail across two
/// threads if `HyperglareApplication::validate_settings` were not there.
#[derive(Parser, Debug)]
#[command(
    name = "hyperglare-bela",
    version,
    about = "A resonator-bank colour-bass effect for Bela Gem Stereo",
    long_about = None,
    after_help = "The chord is fixed for the whole run: there is no MIDI path yet, and no \
                  control surface. What this binary is for is whether the DSP runs on the \
                  board and at what cost — read `active_resonators` alongside `cpu_load`, \
                  because the per-sample cost is one filter per resonator that is sounding.",
    allow_negative_numbers = true
)]
pub struct BelaCli {
    /// the chord, as MIDI note numbers
    ///
    /// MIDI's own units, so that this and the MIDI input that eventually
    /// replaces it agree about what A4 is.
    #[arg(long, value_name = "NOTE", value_delimiter = ',', default_values_t = [56u8, 59, 61, 63, 66])]
    pub notes: Vec<u8>,

    /// The processor's settings.
    #[command(flatten)]
    pub params: ParamsArgs,

    /// audio frames per block
    #[arg(long, default_value_t = super::PERIOD_SIZE)]
    pub period: NonZeroU32,

    /// audio sample rate to ask the board for
    #[arg(long, value_name = "Hz", default_value_t = super::SAMPLE_RATE_HZ)]
    pub sample_rate: NonZeroU32,

    /// measure CPU load this many times per block and report it on exit
    ///
    /// Off by default because libbela refuses CPU monitoring above a period
    /// of `MAX_MONITORED_PERIOD_SIZE`, and failing to start over a diagnostic
    /// nobody asked for would be the wrong trade.
    #[arg(long, value_name = "PER_BLOCK")]
    pub report_cpu: Option<NonZeroU32>,

    /// print the run's resonator count, underruns and input peak to stderr
    /// after a normal exit
    #[arg(long)]
    pub report_on_exit: bool,

    /// codec analog input gain, applied before the DSP
    ///
    /// Not to be confused with `--input-gain`, which is inside the DSP. This
    /// one is the converter's, and the board's default of +16 dB clips a
    /// line-level source. Set it as high as the source allows without
    /// clipping — `--report-on-exit` says where that is. Below -12 dB the
    /// codec stops responding at all.
    #[arg(long, value_name = "dB")]
    pub adc_gain_db: Option<f32>,

    /// codec headphone output level, applied after the DSP
    ///
    /// Not to be confused with `--output-gain`, which is inside the DSP. On a
    /// Gem Stereo this is what sets the line output's level.
    #[arg(long, value_name = "dB")]
    pub headphone_level_db: Option<f32>,

    /// light an LED on this digital channel while the input is clipping
    ///
    /// Nothing on this board reports input clipping, so without an indicator
    /// it is only visible after the run, in `--report-on-exit`.
    #[arg(long, value_name = "CHANNEL")]
    pub clip_led: Option<usize>,
}

impl From<&BelaCli> for RunOptions {
    fn from(cli: &BelaCli) -> Self {
        Self {
            period_size: cli.period,
            sample_rate: cli.sample_rate,
            cpu_monitoring: cli.report_cpu,
            adc_gain_db: cli.adc_gain_db,
            headphone_level_db: cli.headphone_level_db,
            clip_led: cli.clip_led,
            report_on_exit: cli.report_on_exit,
        }
    }
}
