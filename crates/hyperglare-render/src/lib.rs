//! Offline WAV rendering and EBU R128 measurement.
//!
//! Outside the audio callback and its prohibitions on purpose. It runs the
//! same [`HyperglareProcessor`] the live hosts will, and accepts only stereo
//! 32-bit float WAV so that a deliberately hot render is never quantised on
//! the way out.
//!
//! # Why this matches loudness
//!
//! M0 is a set of comparisons made by ear: which geometry, how much noise,
//! where the post-drive sits. Every one of them changes the level as well as
//! the sound, and a comparison between two loudnesses is decided by the louder
//! one whatever it sounds like.
//!
//! The DSP already removes the confounds it can — the voice count, the
//! geometry's density and the decay all divide out — but only within a
//! setting. Between two renders there is nothing to stop, say, more drive from
//! simply being louder. So the file is matched too, and the gain that did it
//! is reported: a match that needed 9 dB is itself a finding.
//!
//! # The chord is an argument, not a performance
//!
//! MIDI is where a chord comes from eventually. Until then it is fixed for the
//! length of a render, which is enough for M0's first question — whether the
//! thing sounds like anything at all — and not enough for the ones after it,
//! since a chord that never changes cannot show what a chord change sounds
//! like. The note numbers are MIDI's own, so the two agree about what A4 is.

// The real-time set the workspace applies to every crate is aimed at code on
// or below an audio callback (`docs/effectkit/realtime.md`). This runs offline,
// on whole files: it allocates, it opens them, and it formats error messages,
// all of which are exactly what the callback may not do and exactly what an
// offline renderer is for. Allowed here rather than at each site, because the
// reason is the same at every one of them and repeating it would obscure the
// sites where it is not.
#![allow(
    clippy::disallowed_macros,
    clippy::disallowed_methods,
    clippy::disallowed_types
)]

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use ebur128::{EbuR128, Mode};
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use thiserror::Error;

use hyperglare_dsp::note::note_hz;
use hyperglare_dsp::processor::{HyperglareParams, HyperglareProcessor};

/// Stereo, because the processor is.
const CHANNELS: u16 = 2;

/// How many resonators a render can hold.
///
/// Sized for the dearest geometry rather than for the board: a harmonic series
/// needs about 150 partials a voice at the bottom of the band, and an offline
/// comparison should not be the thing that decides against it.
pub const CAPACITY: usize = 1024;

/// The most notes a chord may name.
pub const MAX_NOTES: usize = 8;

/// Multiples of the decay to append when a caller does not say.
///
/// Two takes a tail 120 dB down, which is below anything the render will be
/// listened to at.
const DEFAULT_TAIL_DECAYS: f32 = 2.0;

/// Longest tail that will be appended without being asked for.
///
/// A decay of eight seconds would otherwise turn a three-second source into a
/// nineteen-second file. A caller that wants that says so.
const MAX_DERIVED_TAIL_S: f32 = 6.0;

/// How close to the target an iteration has to land before it stops.
const NORMALIZATION_TOLERANCE_LU: f64 = 0.01;
/// Loudness matching is a fixed point; three passes reach it comfortably.
const MAX_NORMALIZATION_ITERATIONS: usize = 3;

/// Settings for one offline render.
#[derive(Debug, Clone)]
pub struct RenderOptions {
    /// Stereo 32-bit float WAV to process.
    pub input: PathBuf,
    /// Destination for the loudness-matched WAV.
    pub output: PathBuf,
    /// Optional destination for the processor's output before matching.
    pub raw_output: Option<PathBuf>,
    /// The complete processor settings.
    pub params: HyperglareParams,
    /// The chord, as MIDI note numbers. Empty means no resonators sound.
    pub notes: Vec<u8>,
    /// Integrated loudness target. Absent means "match the input".
    pub target_lufs: Option<f64>,
    /// Seconds of silence appended so the resonators can finish.
    ///
    /// Absent derives it from the decay, which is what a caller almost always
    /// wants: twice the decay, capped at six seconds.
    pub tail_seconds: Option<f32>,
}

/// EBU R128 and peak measurements of one stream.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioMetrics {
    /// Integrated loudness in LUFS.
    pub integrated_lufs: f64,
    /// Maximum sample peak in dBFS.
    pub sample_peak_dbfs: f64,
    /// Estimated maximum true peak in dBTP.
    pub true_peak_dbtp: f64,
}

/// What a render did.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderReport {
    /// The input, which is the default reference.
    pub input: AudioMetrics,
    /// The processor's output, before matching.
    pub raw: AudioMetrics,
    /// What was written.
    pub output: AudioMetrics,
    /// The target that was matched to.
    pub target_lufs: f64,
    /// The gain matching needed, in dB.
    ///
    /// **Worth reading rather than skipping.** A setting that needed a large
    /// correction is a setting that was mostly a level change, and knowing
    /// that is half of what a comparison is for.
    pub normalization_gain_db: f64,
    /// How many resonators the chord and the geometry produced.
    pub active_resonators: usize,
}

/// Everything that can go wrong reading, rendering, measuring or writing.
#[derive(Debug, Error)]
pub enum RenderError {
    /// The input is not stereo.
    #[error("unsupported WAV input {path}: requires exactly 2 channels, found {channels}")]
    InputChannels {
        /// Input path.
        path: PathBuf,
        /// What the file actually has.
        channels: u16,
    },
    /// The input is not 32-bit float.
    #[error("unsupported WAV input {path}: requires 32-bit IEEE float samples")]
    InputFormat {
        /// Input path.
        path: PathBuf,
    },
    /// More notes than a chord may name.
    #[error("too many notes: {count}, at most {MAX_NOTES}")]
    TooManyNotes {
        /// How many were asked for.
        count: usize,
    },
    /// A WAV file could not be read or written.
    #[error("WAV error on {path}: {source}")]
    Wav {
        /// The file involved.
        path: PathBuf,
        /// The underlying failure.
        #[source]
        source: hound::Error,
    },
    /// Loudness measurement failed.
    #[error("loudness measurement failed: {0}")]
    Loudness(#[from] ebur128::Error),
}

/// Renders `options.input` and writes the result, matching loudness.
///
/// # Errors
///
/// Returns [`RenderError`] if the input is not stereo 32-bit float, if the
/// chord names more than [`MAX_NOTES`] notes, or if any file or measurement
/// operation fails.
pub fn render(options: &RenderOptions) -> Result<RenderReport, RenderError> {
    if options.notes.len() > MAX_NOTES {
        return Err(RenderError::TooManyNotes {
            count: options.notes.len(),
        });
    }

    let (spec, input_frames) = read_stereo_f32(&options.input)?;
    // Sample rates are small integers; every rate a WAV can name is exact in
    // `f32`.
    #[allow(clippy::cast_precision_loss)]
    let sample_rate = spec.sample_rate as f32;

    let notes: Vec<f32> = options.notes.iter().copied().map(note_hz).collect();
    let mut processor = HyperglareProcessor::<CAPACITY>::new(options.params, sample_rate);
    processor.apply_params(&options.params, &notes);

    // The input is not the whole render. A resonator asked to ring for a
    // second goes on ringing for a second after its input stops — that is what
    // it is for — and a render that ended with the input would cut the tail
    // off mid-ring, which is both wrong and a click.
    let tail = tail_frames(options, spec.sample_rate);
    let mut left: Vec<f32> = input_frames.iter().map(|(l, _)| *l).collect();
    let mut right: Vec<f32> = input_frames.iter().map(|(_, r)| *r).collect();
    left.resize(left.len().saturating_add(tail), 0.0);
    right.resize(right.len().saturating_add(tail), 0.0);
    processor.process(&mut left, &mut right);

    let input_metrics = measure(&input_frames, spec.sample_rate)?;
    let rendered: Vec<(f32, f32)> = left.iter().copied().zip(right.iter().copied()).collect();
    let raw_metrics = measure(&rendered, spec.sample_rate)?;

    if let Some(path) = &options.raw_output {
        write_stereo_f32(path, spec, &rendered)?;
    }

    let target = options.target_lufs.unwrap_or(input_metrics.integrated_lufs);
    let (matched, gain_db, output_metrics) =
        match_loudness(rendered, spec.sample_rate, target, raw_metrics)?;
    write_stereo_f32(&options.output, spec, &matched)?;

    Ok(RenderReport {
        input: input_metrics,
        raw: raw_metrics,
        output: output_metrics,
        target_lufs: target,
        normalization_gain_db: gain_db,
        active_resonators: processor.active(),
    })
}

/// How many frames of silence to append after the input.
///
/// Derived from the decay unless a caller says otherwise, because the right
/// answer is a property of the setting rather than of the file, and a caller
/// who had to work it out would work it out from the same number.
fn tail_frames(options: &RenderOptions, sample_rate: u32) -> usize {
    let seconds = options.tail_seconds.unwrap_or_else(|| {
        (options.params.bank.decay_t60_s * DEFAULT_TAIL_DECAYS).min(MAX_DERIVED_TAIL_S)
    });
    #[allow(clippy::cast_precision_loss)] // Sample rates are exact in `f32`.
    let frames = seconds.max(0.0) * sample_rate as f32;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Clamped above.
    let frames = frames as usize;
    frames
}

/// Applies gain until the integrated loudness lands on `target`.
///
/// Iterated rather than solved in one step because the measurement is not
/// exactly linear in the gain: the gate that R128 applies can admit or exclude
/// a block as the level moves.
#[allow(clippy::type_complexity)] // The matched audio, the gain it took, and what it measured.
fn match_loudness(
    mut frames: Vec<(f32, f32)>,
    sample_rate: u32,
    target: f64,
    mut metrics: AudioMetrics,
) -> Result<(Vec<(f32, f32)>, f64, AudioMetrics), RenderError> {
    let mut total_db = 0.0;
    for _ in 0..MAX_NORMALIZATION_ITERATIONS {
        let error = target - metrics.integrated_lufs;
        if !error.is_finite() || error.abs() <= NORMALIZATION_TOLERANCE_LU {
            break;
        }
        let gain = db_to_amp(error);
        for (l, r) in &mut frames {
            *l *= gain;
            *r *= gain;
        }
        total_db += error;
        metrics = measure(&frames, sample_rate)?;
    }
    Ok((frames, total_db, metrics))
}

/// Integrated loudness and peaks for a stereo stream.
fn measure(frames: &[(f32, f32)], sample_rate: u32) -> Result<AudioMetrics, RenderError> {
    let mut meter = EbuR128::new(
        u32::from(CHANNELS),
        sample_rate,
        Mode::I | Mode::SAMPLE_PEAK | Mode::TRUE_PEAK,
    )?;
    let interleaved: Vec<f32> = frames.iter().flat_map(|(l, r)| [*l, *r]).collect();
    meter.add_frames_f32(&interleaved)?;

    let mut sample_peak = 0.0f64;
    let mut true_peak = 0.0f64;
    for channel in 0..u32::from(CHANNELS) {
        sample_peak = sample_peak.max(meter.sample_peak(channel)?);
        true_peak = true_peak.max(meter.true_peak(channel)?);
    }
    Ok(AudioMetrics {
        integrated_lufs: meter.loudness_global()?,
        sample_peak_dbfs: amp_to_db(sample_peak),
        true_peak_dbtp: amp_to_db(true_peak),
    })
}

/// Reads a stereo 32-bit float WAV into frames.
fn read_stereo_f32(path: &Path) -> Result<(WavSpec, Vec<(f32, f32)>), RenderError> {
    let file = File::open(path).map_err(|source| RenderError::Wav {
        path: path.to_path_buf(),
        source: hound::Error::IoError(source),
    })?;
    let mut reader = WavReader::new(BufReader::new(file)).map_err(|source| RenderError::Wav {
        path: path.to_path_buf(),
        source,
    })?;
    let spec = reader.spec();
    if spec.channels != CHANNELS {
        return Err(RenderError::InputChannels {
            path: path.to_path_buf(),
            channels: spec.channels,
        });
    }
    if spec.sample_format != SampleFormat::Float || spec.bits_per_sample != 32 {
        return Err(RenderError::InputFormat {
            path: path.to_path_buf(),
        });
    }

    let mut frames = Vec::new();
    let mut samples = reader.samples::<f32>();
    while let Some(left) = samples.next() {
        let left = left.map_err(|source| RenderError::Wav {
            path: path.to_path_buf(),
            source,
        })?;
        // An odd sample count means a truncated final frame; dropping it is
        // more honest than inventing a right channel for it.
        let Some(right) = samples.next() else { break };
        let right = right.map_err(|source| RenderError::Wav {
            path: path.to_path_buf(),
            source,
        })?;
        frames.push((left, right));
    }
    Ok((spec, frames))
}

/// Writes frames as a stereo 32-bit float WAV.
fn write_stereo_f32(path: &Path, spec: WavSpec, frames: &[(f32, f32)]) -> Result<(), RenderError> {
    let spec = WavSpec {
        channels: CHANNELS,
        sample_format: SampleFormat::Float,
        bits_per_sample: 32,
        ..spec
    };
    let mut writer = WavWriter::create(path, spec).map_err(|source| RenderError::Wav {
        path: path.to_path_buf(),
        source,
    })?;
    for (left, right) in frames {
        for sample in [*left, *right] {
            writer
                .write_sample(sample)
                .map_err(|source| RenderError::Wav {
                    path: path.to_path_buf(),
                    source,
                })?;
        }
    }
    writer.finalize().map_err(|source| RenderError::Wav {
        path: path.to_path_buf(),
        source,
    })
}

/// `10^(db/20)`, in `f64` because the measurement is.
///
/// The result is applied to `f32` samples, so the narrowing is the point
/// rather than a loss: a gain the ear can hear is nowhere near `f32`'s
/// precision.
#[allow(clippy::cast_possible_truncation)]
fn db_to_amp(db: f64) -> f32 {
    10f64.powf(db / 20.0) as f32
}

/// `20·log10(amp)`, with a floor so silence reports a number.
fn amp_to_db(amp: f64) -> f64 {
    if amp <= 0.0 {
        f64::NEG_INFINITY
    } else {
        20.0 * amp.log10()
    }
}

#[cfg(test)]
// Test-only arithmetic over sample indices; small counts, positive by
// construction, and a signal generator reads better written out.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::arithmetic_side_effects,
    clippy::suboptimal_flops,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]
mod tests {
    use std::env::temp_dir;

    use core::f32::consts::PI;

    use hyperglare_dsp::bank::BankParams;
    use hyperglare_dsp::grid::{Geometry, Grid};

    use super::*;

    const SR: u32 = 48_000;

    /// A distorted bass with a little noise on it: what colour bass is applied
    /// to, and what the bank needs in order to have anything to resonate on.
    fn source(path: &Path, seconds: f32) {
        let n = (SR as f32 * seconds) as usize;
        let mut rng = 0x2545_f491u32;
        let frames: Vec<(f32, f32)> = (0..n)
            .map(|i| {
                let t = i as f32 / SR as f32;
                rng ^= rng << 13;
                rng ^= rng >> 17;
                rng ^= rng << 5;
                let noise = f32::from_bits((rng >> 9) | 0x3f80_0000).mul_add(2.0, -3.0);
                let x = (3.0 * (2.0 * PI * 55.0 * t).sin()).tanh() * 0.4;
                let x = 0.02f32.mul_add(noise, x);
                (x, x * 0.9)
            })
            .collect();
        let spec = WavSpec {
            channels: CHANNELS,
            sample_rate: SR,
            bits_per_sample: 32,
            sample_format: SampleFormat::Float,
        };
        write_stereo_f32(path, spec, &frames).unwrap();
    }

    fn scratch(name: &str) -> PathBuf {
        let mut path = temp_dir();
        path.push(format!("hyperglare-render-test-{name}.wav"));
        path
    }

    fn options(input: &Path, output: &Path, geometry: Geometry) -> RenderOptions {
        RenderOptions {
            input: input.to_path_buf(),
            output: output.to_path_buf(),
            raw_output: None,
            params: HyperglareParams {
                bank: BankParams {
                    grid: Grid {
                        geometry,
                        ..Grid::default()
                    },
                    ..BankParams::default()
                },
                color: 1.0,
                ..HyperglareParams::default()
            },
            // A1, E2, A2.
            notes: vec![33, 40, 45],
            target_lufs: None,
            tail_seconds: Some(0.0),
        }
    }

    /// The whole point of the crate: a file goes in, a different file comes
    /// out, and it is the same length and shape.
    #[test]
    fn a_render_changes_the_audio_and_keeps_the_format() {
        let input = scratch("in-basic");
        let output = scratch("out-basic");
        source(&input, 1.0);

        let report = render(&options(&input, &output, Geometry::Octaves)).unwrap();
        assert!(report.active_resonators > 0, "nothing was tuned");

        let (spec, dry) = read_stereo_f32(&input).unwrap();
        let (out_spec, wet) = read_stereo_f32(&output).unwrap();
        assert_eq!(spec.sample_rate, out_spec.sample_rate);
        assert_eq!(wet.len(), dry.len());

        let difference: f32 = dry
            .iter()
            .zip(&wet)
            .map(|((a, _), (b, _))| (a - b).abs())
            .sum();
        assert!(difference > 1.0, "the render did nothing: {difference}");
    }

    /// A resonator rings after its input stops, so a render is longer than
    /// the file it came from. Without the tail the last note is cut off
    /// mid-ring, which is both wrong and a click.
    #[test]
    fn the_tail_rings_out_past_the_end_of_the_input() {
        let input = scratch("in-tail");
        let output = scratch("out-tail");
        source(&input, 0.5);

        let mut opts = options(&input, &output, Geometry::Octaves);
        opts.params.bank.decay_t60_s = 0.4;
        opts.tail_seconds = None; // derived from the decay
        render(&opts).unwrap();

        let (_, dry) = read_stereo_f32(&input).unwrap();
        let (_, wet) = read_stereo_f32(&output).unwrap();
        assert!(
            wet.len() > dry.len(),
            "the render should outlast its input: {} vs {}",
            wet.len(),
            dry.len()
        );

        // And it should end quietly, rather than being cut while still loud.
        let tail_peak = |frames: &[(f32, f32)], from: usize| {
            frames[from..]
                .iter()
                .map(|(l, r)| l.abs().max(r.abs()))
                .fold(0.0f32, f32::max)
        };
        let start = dry.len();
        let last = wet.len() - wet.len() / 20;
        let early = tail_peak(&wet, start);
        let end = tail_peak(&wet, last);
        assert!(
            end < early * 0.2,
            "the tail should have decayed by the end: {early} -> {end}"
        );
    }

    /// Matching is what makes an A/B a comparison of sound rather than of
    /// loudness, so it has to actually land on the target.
    #[test]
    fn the_output_is_matched_to_the_target() {
        let input = scratch("in-loud");
        let output = scratch("out-loud");
        source(&input, 2.0);

        let mut opts = options(&input, &output, Geometry::Octaves);
        opts.params.output_gain_db = -20.0;
        let report = render(&opts).unwrap();

        assert!(
            (report.output.integrated_lufs - report.target_lufs).abs() < 0.5,
            "output {} should have landed on {}",
            report.output.integrated_lufs,
            report.target_lufs
        );
        // The gain it took is the diagnostic: 20 dB of output trim has to come
        // back as roughly 20 dB of correction.
        assert!(
            report.normalization_gain_db > 10.0,
            "a 20 dB trim should need a large correction, got {}",
            report.normalization_gain_db
        );
    }

    /// The comparison M0 exists to make has to be possible: the geometries
    /// must produce audibly different files at the same loudness.
    #[test]
    fn the_geometries_render_differently_at_the_same_loudness() {
        let input = scratch("in-geom");
        source(&input, 1.0);

        let mut rendered = Vec::new();
        for (name, geometry) in [
            ("octaves", Geometry::Octaves),
            ("pairs", Geometry::OctavePairs),
            ("harmonics", Geometry::Harmonics),
        ] {
            let output = scratch(&format!("out-geom-{name}"));
            let report = render(&options(&input, &output, geometry)).unwrap();
            let (_, frames) = read_stereo_f32(&output).unwrap();
            rendered.push((name, report, frames));
        }

        // Densities really do differ by the order of magnitude that makes the
        // comparison worth level-matching.
        assert!(rendered[2].1.active_resonators > 10 * rendered[0].1.active_resonators);

        for (name, report, _) in &rendered {
            assert!(
                (report.output.integrated_lufs - report.target_lufs).abs() < 0.5,
                "{name} did not land on the target"
            );
        }

        let energy = |a: &[(f32, f32)], b: &[(f32, f32)]| -> f32 {
            a.iter().zip(b).map(|((x, _), (y, _))| (x - y).abs()).sum()
        };
        assert!(
            energy(&rendered[0].2, &rendered[1].2) > 1.0,
            "octaves and pairs rendered the same"
        );
        assert!(
            energy(&rendered[0].2, &rendered[2].2) > 1.0,
            "octaves and harmonics rendered the same"
        );
    }

    /// A chord that names nothing produces no resonators, and the dry path is
    /// all that is left. Guards the "empty means silent" edge rather than
    /// letting it become a panic later.
    #[test]
    fn an_empty_chord_renders_the_dry_path_alone() {
        let input = scratch("in-empty");
        let output = scratch("out-empty");
        source(&input, 0.5);

        let mut opts = options(&input, &output, Geometry::Octaves);
        opts.notes.clear();
        opts.params.color = 0.0;
        let report = render(&opts).unwrap();
        assert_eq!(report.active_resonators, 0);
    }

    /// The chord is checked rather than trusted, because it comes from a
    /// command line.
    #[test]
    fn too_many_notes_is_refused() {
        let input = scratch("in-many");
        let output = scratch("out-many");
        source(&input, 0.1);
        let mut opts = options(&input, &output, Geometry::Octaves);
        opts.notes = (0..=MAX_NOTES as u8).collect();
        assert!(matches!(
            render(&opts),
            Err(RenderError::TooManyNotes { .. })
        ));
    }
}
