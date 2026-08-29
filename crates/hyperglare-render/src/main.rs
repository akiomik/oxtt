//! Offline stereo WAV renderer for the comparisons M0 has to make by ear.
#![allow(clippy::disallowed_macros)]

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use hyperglare_args::ParamsArgs;
use hyperglare_dsp::processor::HyperglareParams;
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
    println!(
        "hyperglare-render: loudness_shortfall_db={:.2}",
        report.loudness_shortfall_db
    );
}
