//! Offline stereo WAV renderer for loudness-matched preset comparisons.
#![allow(clippy::disallowed_macros)]

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use oxtt::cli::ParamsArgs;
use oxtt::params::OttParams;
use oxtt::render::{RenderOptions, render};

/// Renders a stereo 32-bit float WAV through oxtt and matches integrated loudness.
#[derive(Parser, Debug)]
#[command(
    version,
    about,
    long_about = None,
    after_help = "Only stereo 32-bit IEEE-float WAV input is supported. Output is always stereo 32-bit IEEE-float WAV.\nNo limiter or clipper is applied; inspect the reported true peak before playback.",
    allow_negative_numbers = true
)]
struct RenderCli {
    /// Source stereo 32-bit float WAV.
    #[arg(long, value_name = "PATH")]
    input: PathBuf,

    /// Destination loudness-matched stereo 32-bit float WAV.
    #[arg(long, value_name = "PATH")]
    output: PathBuf,

    /// Optional destination for the pre-normalization processor output.
    #[arg(long, value_name = "PATH")]
    raw_output: Option<PathBuf>,

    /// Integrated loudness target. Defaults to the source file's measured LUFS.
    #[arg(long, value_name = "LUFS")]
    target_lufs: Option<f64>,

    /// Startup preset and global parameter overrides, identical to the JACK client.
    #[command(flatten)]
    params: ParamsArgs,
}

fn main() -> ExitCode {
    let cli = RenderCli::parse();
    let params = match OttParams::try_from(cli.params) {
        Ok(params) => params,
        Err(error) => {
            eprintln!("oxtt-render: {error}");
            return ExitCode::FAILURE;
        }
    };
    let options = RenderOptions {
        input: cli.input,
        output: cli.output,
        raw_output: cli.raw_output,
        params,
        target_lufs: cli.target_lufs,
    };

    match render(&options) {
        Ok(report) => {
            println!("input_integrated_lufs={:.2}", report.input.integrated_lufs);
            println!("target_lufs={:.2}", report.target_lufs);
            println!("raw_integrated_lufs={:.2}", report.raw.integrated_lufs);
            println!("normalization_gain_db={:.2}", report.normalization_gain_db);
            println!(
                "output_integrated_lufs={:.2}",
                report.output.integrated_lufs
            );
            println!(
                "output_sample_peak_dbfs={:.2}",
                report.output.sample_peak_dbfs
            );
            println!("output_true_peak_dbtp={:.2}", report.output.true_peak_dbtp);
            println!(
                "output_peak_to_loudness_db={:.2}",
                report.output.peak_to_loudness_db
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("oxtt-render: {error}");
            ExitCode::FAILURE
        }
    }
}
