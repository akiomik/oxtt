//! Offline stereo WAV renderer for the comparisons M0 has to make by ear.
#![allow(clippy::disallowed_macros)]

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, ValueEnum};
use hyperglare_dsp::bank::{BankParams, q_max_for_breakpoint};
use hyperglare_dsp::exciter::ExciterParams;
use hyperglare_dsp::grid::{DEFAULT_HIGH_HZ, DEFAULT_LOW_HZ, Geometry, Grid};
use hyperglare_dsp::processor::{HyperglareParams, SearPlacement};
use hyperglare_render::{RenderOptions, RenderReport, render};

/// Renders a stereo 32-bit float WAV through hyperglare and matches loudness.
#[derive(Parser, Debug)]
#[command(
    version,
    about,
    long_about = None,
    after_help = "Only stereo 32-bit IEEE-float WAV input is supported; output is the same.\n\n\
                  The chord is fixed for the whole render. That is enough to hear whether the \
                  effect sounds like anything, and not enough to hear what a chord change sounds \
                  like — which needs the MIDI path.\n\n\
                  Renders are loudness-matched to the input by default. Read the reported \
                  normalisation gain: a setting that needed a large correction was mostly a level \
                  change.",
    allow_negative_numbers = true
)]
struct RenderCli {
    /// Source stereo 32-bit float WAV.
    #[arg(long, value_name = "PATH")]
    input: PathBuf,

    /// Destination loudness-matched stereo 32-bit float WAV.
    #[arg(long, value_name = "PATH")]
    output: PathBuf,

    /// Optional destination for the output before loudness matching.
    #[arg(long, value_name = "PATH")]
    raw_output: Option<PathBuf>,

    /// Integrated loudness target. Defaults to the input's own.
    #[arg(long, value_name = "LUFS")]
    target_lufs: Option<f64>,

    /// Seconds of silence appended so the resonators can finish ringing.
    /// Defaults to twice the decay, capped at six seconds.
    #[arg(long, value_name = "SECONDS")]
    tail: Option<f32>,

    /// The chord, as MIDI note numbers. MIDI's own units, so the eventual
    /// MIDI input and this argument agree about what A4 is.
    #[arg(long, value_name = "NOTE", value_delimiter = ',', default_values_t = [33u8, 40, 45])]
    notes: Vec<u8>,

    #[command(flatten)]
    params: ParamsArgs,
}

/// The processor's settings, one flag each.
#[derive(clap::Args, Debug)]
struct ParamsArgs {
    /// How a note is replicated into resonator frequencies.
    #[arg(long, value_enum, default_value_t = GeometryArg::Octaves)]
    geometry: GeometryArg,

    /// Cents of detune per octave. Zero sits the grid on the input's own
    /// harmonics, which is where it does the least.
    #[arg(long, value_name = "CENTS", default_value_t = 0.0)]
    stretch: f32,

    /// Bottom of the band resonators are placed in.
    #[arg(long, value_name = "HZ", default_value_t = DEFAULT_LOW_HZ)]
    low_hz: f32,

    /// Top of the band resonators are placed in.
    #[arg(long, value_name = "HZ", default_value_t = DEFAULT_HIGH_HZ)]
    high_hz: f32,

    /// Decay to 60 dB, in seconds, below the breakpoint.
    #[arg(long, value_name = "SECONDS", default_value_t = 0.6)]
    decay: f32,

    /// Where the bank stops sustaining and starts ringing.
    ///
    /// The Q cap is derived from this and the decay, because a frequency is a
    /// thing an ear can be pointed at and a Q is not.
    #[arg(long, value_name = "HZ", default_value_t = 1_100.0)]
    breakpoint_hz: f32,

    /// How much of the bandwidth difference between resonators to compensate.
    /// `0.5` is exact for broadband excitation, `0` for tonal.
    #[arg(long, value_name = "EXPONENT", default_value_t = 0.25)]
    compensation: f32,

    /// Spectral tilt, -1 (dark) to 1 (bright).
    #[arg(long, default_value_t = 0.5)]
    tilt: f32,

    /// Cents of detune spread across the voices.
    #[arg(long, value_name = "CENTS", default_value_t = 0.0)]
    drift: f32,

    /// Cents between the members of a pair, under `octave-pairs`.
    ///
    /// Wide spreads turn each grid point into a cluster, which is what fills
    /// the high band; narrow ones beat against each other instead.
    #[arg(long, value_name = "CENTS", default_value_t = 7.0)]
    pair_spread: f32,

    /// Voice count the level is normalised for. Not the number of notes.
    #[arg(long, default_value_t = 4)]
    voices: usize,

    /// Waveshaping into the bank: what makes the root's own partials ring.
    #[arg(long, default_value_t = 0.3)]
    drive: f32,

    /// Gated noise into the bank: what makes everything else ring.
    #[arg(long, default_value_t = 0.5)]
    noise: f32,

    /// How much of the wet's level difference from the dry to remove, so that
    /// `--color` is a real crossfade. Zero leaves the resonators raw.
    #[arg(long, value_name = "AMOUNT", default_value_t = 1.0)]
    wet_match: f32,

    /// Post-drive amount.
    #[arg(long, default_value_t = 0.0)]
    sear: f32,

    /// Where the post-drive sits relative to the stereo split.
    #[arg(long, value_enum, default_value_t = PlacementArg::AfterSum)]
    sear_placement: PlacementArg,

    /// Stereo spread. Only reaches the output under `before-split`.
    #[arg(long, default_value_t = 0.6)]
    width: f32,

    /// Dry/wet. Zero is the input, one is the resonators, two drives them.
    #[arg(long, default_value_t = 1.0)]
    color: f32,

    /// Input gain, in dB.
    #[arg(long, value_name = "DB", default_value_t = 0.0)]
    input_gain: f32,

    /// Output gain, in dB.
    #[arg(long, value_name = "DB", default_value_t = 0.0)]
    output_gain: f32,
}

/// [`Geometry`], as a command-line value.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum GeometryArg {
    /// Every note of the chord, in every octave.
    Octaves,
    /// The same, each octave doubled into a pair a few cents apart.
    OctavePairs,
    /// One bell's overtones per note. About twenty times the resonators.
    Harmonics,
}

impl From<GeometryArg> for Geometry {
    fn from(value: GeometryArg) -> Self {
        match value {
            GeometryArg::Octaves => Self::Octaves,
            GeometryArg::OctavePairs => Self::OctavePairs,
            GeometryArg::Harmonics => Self::Harmonics,
        }
    }
}

/// [`SearPlacement`], as a command-line value.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum PlacementArg {
    /// Shape the summed bank. Every pair of partials intermodulates; mono wet.
    AfterSum,
    /// Split across the pair first, then shape each side.
    BeforeSplit,
}

impl From<PlacementArg> for SearPlacement {
    fn from(value: PlacementArg) -> Self {
        match value {
            PlacementArg::AfterSum => Self::AfterSum,
            PlacementArg::BeforeSplit => Self::BeforeSplit,
        }
    }
}

impl From<&ParamsArgs> for HyperglareParams {
    fn from(args: &ParamsArgs) -> Self {
        Self {
            bank: BankParams {
                grid: Grid {
                    geometry: args.geometry.into(),
                    detune_cents_per_octave: args.stretch,
                    pair_spread_cents: args.pair_spread,
                    low_hz: args.low_hz,
                    high_hz: args.high_hz,
                },
                decay_t60_s: args.decay,
                q_max: q_max_for_breakpoint(args.breakpoint_hz, args.decay),
                compensation_exponent: args.compensation,
                tilt: args.tilt,
                drift_cents: args.drift,
                voices: args.voices,
            },
            exciter: ExciterParams {
                drive: args.drive,
                noise_amount: args.noise,
            },
            sear_placement: args.sear_placement.into(),
            sear: args.sear,
            width: args.width,
            wet_match: args.wet_match,
            color: args.color,
            input_gain_db: args.input_gain,
            output_gain_db: args.output_gain,
        }
    }
}

fn main() -> ExitCode {
    let cli = RenderCli::parse();
    let options = RenderOptions {
        input: cli.input,
        output: cli.output,
        raw_output: cli.raw_output,
        params: HyperglareParams::from(&cli.params),
        notes: cli.notes,
        target_lufs: cli.target_lufs,
        tail_seconds: cli.tail,
    };

    match render(&options) {
        Ok(report) => {
            print_report(&report);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("hyperglare-render: {error}");
            ExitCode::FAILURE
        }
    }
}

/// One `name=value` line each, the way the Bela host reports.
fn print_report(report: &RenderReport) {
    println!(
        "hyperglare-render: active_resonators={}",
        report.active_resonators
    );
    println!(
        "hyperglare-render: input_lufs={:.2} input_true_peak_dbtp={:.2}",
        report.input.integrated_lufs, report.input.true_peak_dbtp
    );
    println!(
        "hyperglare-render: raw_lufs={:.2} raw_true_peak_dbtp={:.2}",
        report.raw.integrated_lufs, report.raw.true_peak_dbtp
    );
    println!(
        "hyperglare-render: output_lufs={:.2} output_true_peak_dbtp={:.2}",
        report.output.integrated_lufs, report.output.true_peak_dbtp
    );
    println!("hyperglare-render: target_lufs={:.2}", report.target_lufs);
    println!(
        "hyperglare-render: normalization_gain_db={:.2}",
        report.normalization_gain_db
    );
}
