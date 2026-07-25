//! Verdict for a Raspberry Pi soak recording (`scripts/pi-soak-test.sh`).
//!
//! Replaces the Python heredoc that used to live in `pi-soak-test.sh`: it reads
//! the stereo 16-bit PCM loopback WAV that `soak_recorder` wrote, runs the pure
//! [`oxtt_jack_tools::analysis`] checks, prints the same statistics on stdout,
//! and exits non-zero with the same diagnostics on stderr.
#![allow(clippy::print_stdout, clippy::print_stderr)] // A CLI tool, not the audio path.

use std::process::ExitCode;

use hound::{SampleFormat, WavReader};
use oxtt_jack_tools::analysis::{SAMPLE_RATE, analyze};

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<ExitCode, String> {
    let mut args = std::env::args_os().skip(1);
    let (Some(path), Some(duration), None) = (args.next(), args.next(), args.next()) else {
        return Err("usage: soak_analyze RECORDING DURATION_SECONDS".to_owned());
    };
    let duration_secs = duration
        .to_str()
        .and_then(|text| text.parse::<u64>().ok())
        .ok_or_else(|| "duration must be a non-negative integer".to_owned())?;

    let mut reader =
        WavReader::open(&path).map_err(|error| format!("failed to open recording: {error}"))?;
    let spec = reader.spec();
    let width_bytes = spec.bits_per_sample / 8;
    if spec.channels != 2
        || spec.sample_format != SampleFormat::Int
        || width_bytes != 2
        || spec.sample_rate != SAMPLE_RATE
    {
        return Err(format!(
            "unexpected WAV format: channels={} width={width_bytes} rate={}",
            spec.channels, spec.sample_rate
        ));
    }
    let total_frames = u64::from(reader.duration());

    let mut pairs = FramePairs {
        samples: reader.samples::<i16>(),
        error: None,
    };
    let outcome = analyze(&mut pairs, total_frames, duration_secs);
    if let Some(error) = pairs.error {
        return Err(format!("failed to read recording: {error}"));
    }

    if let Some(report) = outcome.report {
        println!("frames={}", report.frames);
        println!("first_audible_frame={}", report.first_audible_frame);
        println!("last_audible_frame={}", report.last_audible_frame);
        println!("max_quiet_gap_frames={}", report.max_quiet_gap_frames);
        println!("glitch_gap_count={}", report.glitch_gap_count);
        println!("clip_sample_count={}", report.clip_sample_count);
    }
    outcome
        .verdict
        .map(|()| ExitCode::SUCCESS)
        .map_err(|error| error.to_string())
}

/// Pairs interleaved `i16` samples into `(left, right)` frames, stashing the
/// first read error so the caller can distinguish a truncated file from a
/// genuinely short recording.
struct FramePairs<'a, R>
where
    R: std::io::Read,
{
    samples: hound::WavSamples<'a, R, i16>,
    error: Option<hound::Error>,
}

impl<R> Iterator for FramePairs<'_, R>
where
    R: std::io::Read,
{
    type Item = (i16, i16);

    fn next(&mut self) -> Option<(i16, i16)> {
        let left = self.take()?;
        // A stereo file always has an even sample count, but guard the odd tail
        // rather than pairing it with a fabricated zero.
        let right = self.take()?;
        Some((left, right))
    }
}

impl<R> FramePairs<'_, R>
where
    R: std::io::Read,
{
    fn take(&mut self) -> Option<i16> {
        match self.samples.next()? {
            Ok(sample) => Some(sample),
            Err(error) => {
                self.error = Some(error);
                None
            }
        }
    }
}
