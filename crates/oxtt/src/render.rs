//! Offline WAV rendering and EBU R128 measurement.
//!
//! This module deliberately stays outside the JACK host and its real-time
//! callback contract. It uses the same [`OttProcessor`]
//! as the live client, but accepts only stereo 32-bit floating-point WAV so
//! the renderer never clips or quantizes an intentionally hot output.

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use ebur128::{EbuR128, Mode};
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use thiserror::Error;

use crate::dsp::{OttProcessor, ProcessError};
use crate::params::{ConfigError, OttParams};

const CHANNELS: u16 = 2;
const FRAMES_PER_CHUNK: usize = 4_096;
const INTERLEAVED_SAMPLES_PER_CHUNK: usize = 8_192;
const NORMALIZATION_TOLERANCE_LU: f64 = 0.01;
const MAX_NORMALIZATION_ITERATIONS: usize = 3;

/// Immutable settings for one offline render.
#[derive(Debug, Clone)]
pub struct RenderOptions {
    /// Stereo 32-bit float WAV to process.
    pub input: PathBuf,
    /// Destination for the loudness-matched 32-bit float WAV.
    pub output: PathBuf,
    /// Optional destination for the un-normalized processor output.
    pub raw_output: Option<PathBuf>,
    /// Complete processor parameters, usually built from a preset and CLI overrides.
    pub params: OttParams,
    /// Explicit integrated loudness target in LUFS. If absent, the input's
    /// own integrated loudness is the target.
    pub target_lufs: Option<f64>,
}

/// EBU R128 and peak measurements of one audio stream.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioMetrics {
    /// Integrated loudness in LUFS.
    pub integrated_lufs: f64,
    /// Maximum inter-sample-independent peak in dBFS.
    pub sample_peak_dbfs: f64,
    /// Estimated maximum true peak in dBTP.
    pub true_peak_dbtp: f64,
    /// `sample_peak_dbfs - integrated_lufs`, reported as a simple
    /// peak-to-loudness comparison metric rather than a limiter threshold.
    pub peak_to_loudness_db: f64,
}

/// Measurements and gain selected by [`render`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderReport {
    /// The input measurement used as the default reference.
    pub input: AudioMetrics,
    /// The processor output before its final loudness-matching gain.
    pub raw: AudioMetrics,
    /// The written, loudness-matched output.
    pub output: AudioMetrics,
    /// The requested integrated-loudness target in LUFS.
    pub target_lufs: f64,
    /// The gain added after the processor output gain, in dB.
    pub normalization_gain_db: f64,
}

/// Errors raised while reading, processing, measuring, or writing a WAV file.
#[derive(Debug, Error)]
pub enum RenderError {
    /// The input does not have the processor's stereo layout.
    #[error("unsupported WAV input {path}: requires exactly 2 channels, found {channels}")]
    InputChannels {
        /// Input path.
        path: PathBuf,
        /// Channel count found in the file.
        channels: u16,
    },
    /// The input is not IEEE 754 32-bit float WAV.
    #[error(
        "unsupported WAV input {path}: requires 32-bit IEEE float samples, found {sample_format:?} {bits_per_sample}-bit"
    )]
    InputSampleFormat {
        /// Input path.
        path: PathBuf,
        /// Storage format found in the file.
        sample_format: SampleFormat,
        /// Bits per sample found in the file.
        bits_per_sample: u16,
    },
    /// An explicit target was not a finite LUFS value.
    #[error("target LUFS must be finite, found {target_lufs}")]
    InvalidTarget {
        /// Invalid target value.
        target_lufs: f64,
    },
    /// An output would overwrite the input or another requested output.
    #[error("input, output, and raw-output paths must all be different")]
    OverlappingPaths,
    /// An incomplete interleaved frame was found in a WAV data chunk.
    #[error("WAV input {path} ended in the middle of a stereo frame")]
    IncompleteFrame {
        /// Input path.
        path: PathBuf,
    },
    /// EBU R128 could not determine integrated loudness, such as for silence
    /// or an input too short to form a measurement block.
    #[error("could not measure integrated loudness for {path}: {source}")]
    Loudness {
        /// File being measured.
        path: PathBuf,
        /// Analyzer failure.
        #[source]
        source: ebur128::Error,
    },
    /// A pass did not converge to the requested integrated loudness.
    #[error(
        "could not converge to target loudness {target_lufs:.2} LUFS; measured {measured_lufs:.2} LUFS"
    )]
    LoudnessMismatch {
        /// Requested target in LUFS.
        target_lufs: f64,
        /// Final measured value in LUFS.
        measured_lufs: f64,
    },
    /// Parameter validation rejected the input sample rate.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// The processor rejected a buffer before writing it.
    #[error(transparent)]
    Process(#[from] ProcessError),
    /// WAV parsing or writing failed.
    #[error(transparent)]
    Wav(#[from] hound::Error),
    /// EBU R128 analyzer setup or peak calculation failed.
    #[error(transparent)]
    Analyzer(#[from] ebur128::Error),
}

/// Renders `options.input` through `options.params` and writes a
/// loudness-matched 32-bit float WAV.
///
/// The final gain is measured iteratively before any final output is opened,
/// then the written file is measured during its own rendering pass. No limiter,
/// clipper, resampling, or sample-format conversion is applied.
///
/// # Errors
///
/// Returns [`RenderError`] for unsupported input WAV properties, invalid
/// parameters, unmeasurable loudness, I/O failures, or a failed match.
pub fn render(options: &RenderOptions) -> Result<RenderReport, RenderError> {
    validate_paths(options)?;

    let input = measure_input(&options.input)?;
    let target_lufs = options.target_lufs.unwrap_or(input.integrated_lufs);
    if !target_lufs.is_finite() {
        return Err(RenderError::InvalidTarget { target_lufs });
    }

    let raw = process_pass(
        &options.input,
        options.params,
        0.0,
        options.raw_output.as_deref(),
    )?;
    let mut normalization_gain_db = target_lufs - raw.integrated_lufs;

    for _ in 0..MAX_NORMALIZATION_ITERATIONS {
        let measured = process_pass(&options.input, options.params, normalization_gain_db, None)?;
        let error_lu = target_lufs - measured.integrated_lufs;
        if error_lu.abs() <= NORMALIZATION_TOLERANCE_LU {
            break;
        }
        normalization_gain_db += error_lu;
    }

    let output = process_pass(
        &options.input,
        options.params,
        normalization_gain_db,
        Some(&options.output),
    )?;
    if (target_lufs - output.integrated_lufs).abs() > NORMALIZATION_TOLERANCE_LU {
        return Err(RenderError::LoudnessMismatch {
            target_lufs,
            measured_lufs: output.integrated_lufs,
        });
    }

    Ok(RenderReport {
        input,
        raw,
        output,
        target_lufs,
        normalization_gain_db,
    })
}

fn validate_paths(options: &RenderOptions) -> Result<(), RenderError> {
    let raw_overlaps = options
        .raw_output
        .as_ref()
        .is_some_and(|raw_output| raw_output == &options.input || raw_output == &options.output);
    if options.input == options.output || raw_overlaps {
        return Err(RenderError::OverlappingPaths);
    }
    Ok(())
}

fn measure_input(path: &Path) -> Result<AudioMetrics, RenderError> {
    let mut reader = open_input(path)?;
    let mut analyzer = new_analyzer(reader.spec().sample_rate)?;
    let mut interleaved = Vec::with_capacity(INTERLEAVED_SAMPLES_PER_CHUNK);
    let mut samples = reader.samples::<f32>();

    loop {
        interleaved.clear();
        for _ in 0..FRAMES_PER_CHUNK {
            let Some(left) = samples.next().transpose()? else {
                break;
            };
            let right =
                samples
                    .next()
                    .transpose()?
                    .ok_or_else(|| RenderError::IncompleteFrame {
                        path: path.to_path_buf(),
                    })?;
            interleaved.extend([left, right]);
        }
        if interleaved.is_empty() {
            break;
        }
        analyzer.add_frames_f32(&interleaved)?;
    }

    metrics(&analyzer, path)
}

fn process_pass(
    input: &Path,
    params: OttParams,
    normalization_gain_db: f64,
    output: Option<&Path>,
) -> Result<AudioMetrics, RenderError> {
    let mut reader = open_input(input)?;
    let spec = reader.spec();
    // The input is validated by `OttProcessor` immediately below; its maximum
    // supported sample rate is 384 kHz, well inside f32's exact-integer range.
    #[allow(clippy::cast_precision_loss)]
    let sample_rate = spec.sample_rate as f32;
    let mut processor = OttProcessor::new(sample_rate, params)?;
    let mut analyzer = new_analyzer(spec.sample_rate)?;
    let mut writer = output
        .map(|path| WavWriter::create(path, float_stereo_spec(spec.sample_rate)))
        .transpose()?;
    let normalization_gain = db_to_amplitude(normalization_gain_db);
    let mut input_left = Vec::with_capacity(FRAMES_PER_CHUNK);
    let mut input_right = Vec::with_capacity(FRAMES_PER_CHUNK);
    let mut output_left = Vec::with_capacity(FRAMES_PER_CHUNK);
    let mut output_right = Vec::with_capacity(FRAMES_PER_CHUNK);
    let mut interleaved = Vec::with_capacity(INTERLEAVED_SAMPLES_PER_CHUNK);
    let mut samples = reader.samples::<f32>();

    loop {
        input_left.clear();
        input_right.clear();
        for _ in 0..FRAMES_PER_CHUNK {
            let Some(left) = samples.next().transpose()? else {
                break;
            };
            let right =
                samples
                    .next()
                    .transpose()?
                    .ok_or_else(|| RenderError::IncompleteFrame {
                        path: input.to_path_buf(),
                    })?;
            input_left.push(left);
            input_right.push(right);
        }
        if input_left.is_empty() {
            break;
        }

        output_left.resize(input_left.len(), 0.0);
        output_right.resize(input_right.len(), 0.0);
        processor.process(
            &input_left,
            &input_right,
            &mut output_left,
            &mut output_right,
        )?;

        interleaved.clear();
        for (&left, &right) in output_left.iter().zip(&output_right) {
            interleaved.extend([left * normalization_gain, right * normalization_gain]);
        }
        analyzer.add_frames_f32(&interleaved)?;
        if let Some(writer) = writer.as_mut() {
            for sample in &interleaved {
                writer.write_sample(*sample)?;
            }
        }
    }

    if let Some(writer) = writer {
        writer.finalize()?;
    }
    metrics(&analyzer, output.unwrap_or(input))
}

fn open_input(path: &Path) -> Result<WavReader<BufReader<File>>, RenderError> {
    let reader = WavReader::open(path)?;
    let spec = reader.spec();
    if spec.channels != CHANNELS {
        return Err(RenderError::InputChannels {
            path: path.to_path_buf(),
            channels: spec.channels,
        });
    }
    if spec.sample_format != SampleFormat::Float || spec.bits_per_sample != 32 {
        return Err(RenderError::InputSampleFormat {
            path: path.to_path_buf(),
            sample_format: spec.sample_format,
            bits_per_sample: spec.bits_per_sample,
        });
    }
    Ok(reader)
}

fn new_analyzer(sample_rate: u32) -> Result<EbuR128, RenderError> {
    Ok(EbuR128::new(
        u32::from(CHANNELS),
        sample_rate,
        Mode::I | Mode::SAMPLE_PEAK | Mode::TRUE_PEAK,
    )?)
}

fn metrics(analyzer: &EbuR128, path: &Path) -> Result<AudioMetrics, RenderError> {
    let integrated_lufs = analyzer
        .loudness_global()
        .map_err(|source| RenderError::Loudness {
            path: path.to_path_buf(),
            source,
        })?;
    let sample_peak = analyzer.sample_peak(0)?.max(analyzer.sample_peak(1)?);
    let true_peak = analyzer.true_peak(0)?.max(analyzer.true_peak(1)?);
    let sample_peak_dbfs = amplitude_to_db(sample_peak);
    Ok(AudioMetrics {
        integrated_lufs,
        sample_peak_dbfs,
        true_peak_dbtp: amplitude_to_db(true_peak),
        peak_to_loudness_db: sample_peak_dbfs - integrated_lufs,
    })
}

const fn float_stereo_spec(sample_rate: u32) -> WavSpec {
    WavSpec {
        channels: CHANNELS,
        sample_rate,
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    }
}

#[allow(clippy::cast_possible_truncation)] // WAV samples and `OttProcessor` are f32.
fn db_to_amplitude(db: f64) -> f32 {
    10_f64.powf(db / 20.0) as f32
}

fn amplitude_to_db(amplitude: f64) -> f64 {
    if amplitude == 0.0 {
        f64::NEG_INFINITY
    } else {
        20.0 * amplitude.log10()
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::cast_precision_loss,
    clippy::disallowed_macros,
    clippy::disallowed_methods
)]
mod tests {
    use std::env;
    use std::f32::consts::TAU;
    use std::fs;
    use std::path::PathBuf;
    use std::process;
    use std::thread;

    use hound::WavWriter;

    use super::*;
    use crate::params::Preset;

    fn path(name: &str) -> PathBuf {
        env::temp_dir().join(format!(
            "oxtt-render-{name}-{}-{}.wav",
            process::id(),
            thread::current().name().unwrap_or("test")
        ))
    }

    fn write_float_input(path: &Path) {
        let mut writer = WavWriter::create(path, float_stereo_spec(48_000)).unwrap();
        for frame in 0..(48_000 * 2) {
            let phase = (frame as f32 / 48_000.0) * TAU * 997.0;
            let sample = phase.sin() * 0.1;
            writer.write_sample(sample).unwrap();
            writer.write_sample(sample).unwrap();
        }
        writer.finalize().unwrap();
    }

    #[test]
    fn renders_stereo_float_wav_to_the_requested_loudness() {
        let input = path("input");
        let output = path("output");
        let raw_output = path("raw");
        write_float_input(&input);

        let report = render(&RenderOptions {
            input: input.clone(),
            output: output.clone(),
            raw_output: Some(raw_output.clone()),
            params: Preset::Riot.params(),
            target_lufs: Some(-18.0),
        })
        .unwrap();

        assert!((report.output.integrated_lufs - (-18.0)).abs() <= NORMALIZATION_TOLERANCE_LU);
        assert!(report.normalization_gain_db.is_finite());
        for output in [&output, &raw_output] {
            let reader = WavReader::open(output).unwrap();
            assert_eq!(reader.spec(), float_stereo_spec(48_000));
            assert_eq!(reader.duration(), 96_000);
        }

        fs::remove_file(input).unwrap();
        fs::remove_file(output).unwrap();
        fs::remove_file(raw_output).unwrap();
    }

    #[test]
    fn rejects_integer_wav_before_processing() {
        let input = path("integer");
        let output = path("integer-output");
        let mut writer = WavWriter::create(
            &input,
            WavSpec {
                channels: CHANNELS,
                sample_rate: 48_000,
                bits_per_sample: 16,
                sample_format: SampleFormat::Int,
            },
        )
        .unwrap();
        writer.write_sample(0_i16).unwrap();
        writer.write_sample(0_i16).unwrap();
        writer.finalize().unwrap();

        let error = render(&RenderOptions {
            input: input.clone(),
            output,
            raw_output: None,
            params: Preset::Riot.params(),
            target_lufs: Some(-18.0),
        })
        .unwrap_err();
        assert!(matches!(error, RenderError::InputSampleFormat { .. }));

        fs::remove_file(input).unwrap();
    }
}
