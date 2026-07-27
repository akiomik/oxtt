//! Verdict for a Raspberry Pi soak recording (`scripts/pi-soak-test.sh`).
//!
//! This is a faithful port of the Python analyser that used to be inlined in
//! `pi-soak-test.sh`. It inspects the recorded stereo 997 Hz loopback and
//! rejects an incomplete, silent, clipped, or glitchy recording. The point of
//! moving it to Rust is that [`analyze`] is a pure function over frames, so the
//! dropout/clip/gap thresholds — the part whose correctness actually matters —
//! are unit- and property-tested instead of living in a shell heredoc.

use thiserror::Error;

/// Sample rate the soak test runs JACK at, and the rate the recording must use.
pub const SAMPLE_RATE: u32 = 48_000;

/// A frame is audible when either channel's magnitude reaches this. A clean
/// 997 Hz tone only dips below it for a few frames around each zero crossing.
pub const QUIET_THRESHOLD: i32 = 200;

/// Full-scale is 32767; treat anything from here up as clipping, leaving a
/// little headroom so a legitimately loud but unclipped peak isn't flagged.
pub const CLIP_THRESHOLD: i32 = 32_760;

/// Longest quiet run treated as clean rather than a short dropout.
///
/// A clean loopback only dips below [`QUIET_THRESHOLD`] for a handful of frames
/// per zero crossing (up to 17 observed on real hardware). A quiet run longer
/// than this is a real dropout, even though it is far shorter than
/// [`ALLOWED_GAP_FRAMES`] — such dropouts have been seen to repeat every few
/// seconds without ever tripping JACK's xrun log or oxtt's own xrun counter.
pub const GLITCH_GAP_FRAMES: u64 = 40;

/// The tone may start up to this late and end this early (2 s of slack at each
/// end) to absorb JACK/port connection ordering at the start and stop.
pub const ALLOWED_EDGE_FRAMES: u64 = 2 * SAMPLE_RATE as u64;

/// The single longest tolerated quiet gap anywhere in the tone (50 ms).
pub const ALLOWED_GAP_FRAMES: u64 = SAMPLE_RATE as u64 / 20;

/// Per-frame statistics gathered from a recording that passed the length and
/// edge checks. Mirrors the numbers the Python analyser printed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Report {
    /// Total frames in the recording (from the WAV header).
    pub frames: u64,
    /// Index of the first audible frame.
    pub first_audible_frame: u64,
    /// Index of the last audible frame.
    pub last_audible_frame: u64,
    /// Longest quiet run, in frames, between two audible frames.
    pub max_quiet_gap_frames: u64,
    /// Number of quiet runs longer than [`GLITCH_GAP_FRAMES`].
    pub glitch_gap_count: u64,
    /// Number of samples that reached [`CLIP_THRESHOLD`] or above.
    pub clip_sample_count: u64,
}

/// A reason a recording is rejected. The messages match the ones the former
/// Python analyser raised, so `pi-soak-test.sh`'s output is unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SoakError {
    /// Fewer frames than the requested duration (minus the edge slack) allows.
    #[error("recording too short: {frames} frames")]
    TooShort {
        /// Frames actually present.
        frames: u64,
    },
    /// No frame ever reached [`QUIET_THRESHOLD`].
    #[error("recording contains no audible test signal")]
    NoSignal,
    /// The first audible frame is past [`ALLOWED_EDGE_FRAMES`].
    #[error("test signal started too late: {first} frames")]
    StartedTooLate {
        /// Index of the first audible frame.
        first: u64,
    },
    /// The last audible frame is more than [`ALLOWED_EDGE_FRAMES`] before the end.
    #[error("test signal ended too early: last={last} total={total}")]
    EndedTooEarly {
        /// Index of the last audible frame.
        last: u64,
        /// Total frames in the recording.
        total: u64,
    },
    /// A quiet gap exceeded [`ALLOWED_GAP_FRAMES`].
    #[error("unexpected quiet gap: {max_gap} frames")]
    QuietGap {
        /// The longest gap observed.
        max_gap: u64,
    },
    /// One or more quiet runs exceeded [`GLITCH_GAP_FRAMES`].
    #[error(
        "{count} short dropout(s) exceeded {threshold} frames (max={max_gap} frames); \
         JACK and oxtt xrun counters can both read zero while this happens"
    )]
    Dropouts {
        /// Number of glitch-length gaps.
        count: u64,
        /// The [`GLITCH_GAP_FRAMES`] threshold, echoed for the message.
        threshold: u64,
        /// The longest gap observed.
        max_gap: u64,
    },
    /// One or more samples reached [`CLIP_THRESHOLD`].
    #[error("{count} sample(s) reached full-scale ({threshold}/32767)")]
    Clipping {
        /// Number of clipped samples.
        count: u64,
        /// The [`CLIP_THRESHOLD`] threshold, echoed for the message.
        threshold: i32,
    },
}

/// The result of analysing a recording.
///
/// `report` is `Some` once the recording passed the length and edge checks, so
/// its statistics are meaningful even when `verdict` is an `Err` from the
/// gap/glitch/clip thresholds — the caller can print the numbers before
/// reporting the failure, exactly as the Python analyser did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outcome {
    /// Statistics, present once length/edge checks passed.
    pub report: Option<Report>,
    /// `Ok(())` if the recording passed every check.
    pub verdict: Result<(), SoakError>,
}

/// Analyse a recording given its frames, declared length, and requested duration.
///
/// `total_frames` is the frame count from the WAV header (used for the
/// too-short and ended-too-early checks); `frames` yields the actual
/// `(left, right)` samples in order. Both come from the same file for a
/// complete recording.
#[must_use]
pub fn analyze<I>(frames: I, total_frames: u64, duration_secs: u64) -> Outcome
where
    I: IntoIterator<Item = (i16, i16)>,
{
    let minimum = duration_secs
        .saturating_mul(u64::from(SAMPLE_RATE))
        .saturating_sub(ALLOWED_EDGE_FRAMES);
    if total_frames < minimum {
        return reject(SoakError::TooShort {
            frames: total_frames,
        });
    }

    let scan = Scan::run(frames);

    let Some((first, last)) = scan.audible else {
        return reject(SoakError::NoSignal);
    };
    if first > ALLOWED_EDGE_FRAMES {
        return reject(SoakError::StartedTooLate { first });
    }
    if last < total_frames.saturating_sub(ALLOWED_EDGE_FRAMES) {
        return reject(SoakError::EndedTooEarly {
            last,
            total: total_frames,
        });
    }

    let report = Report {
        frames: total_frames,
        first_audible_frame: first,
        last_audible_frame: last,
        max_quiet_gap_frames: scan.max_gap,
        glitch_gap_count: scan.glitch_count,
        clip_sample_count: scan.clip_count,
    };
    let verdict = if scan.max_gap > ALLOWED_GAP_FRAMES {
        Err(SoakError::QuietGap {
            max_gap: scan.max_gap,
        })
    } else if scan.glitch_count > 0 {
        Err(SoakError::Dropouts {
            count: scan.glitch_count,
            threshold: GLITCH_GAP_FRAMES,
            max_gap: scan.max_gap,
        })
    } else if scan.clip_count > 0 {
        Err(SoakError::Clipping {
            count: scan.clip_count,
            threshold: CLIP_THRESHOLD,
        })
    } else {
        Ok(())
    };
    Outcome {
        report: Some(report),
        verdict,
    }
}

const fn reject(error: SoakError) -> Outcome {
    Outcome {
        report: None,
        verdict: Err(error),
    }
}

/// Accumulator for a single pass over the frames.
struct Scan {
    /// `(first, last)` audible frame indices, tracked together so "first is set
    /// iff last is set" holds by construction (no unreachable `unwrap`).
    audible: Option<(u64, u64)>,
    max_gap: u64,
    glitch_count: u64,
    clip_count: u64,
}

impl Scan {
    fn run<I>(frames: I) -> Self
    where
        I: IntoIterator<Item = (i16, i16)>,
    {
        let mut audible: Option<(u64, u64)> = None;
        let mut gap: u64 = 0;
        let mut max_gap: u64 = 0;
        let mut glitch_count: u64 = 0;
        let mut clip_count: u64 = 0;

        for (index, (left, right)) in (0_u64..).zip(frames) {
            let peak = i32::from(left).abs().max(i32::from(right).abs());
            if peak >= CLIP_THRESHOLD {
                clip_count = clip_count.saturating_add(1);
            }
            if peak >= QUIET_THRESHOLD {
                if let Some((first, _)) = audible {
                    max_gap = max_gap.max(gap);
                    if gap > GLITCH_GAP_FRAMES {
                        glitch_count = glitch_count.saturating_add(1);
                    }
                    audible = Some((first, index));
                } else {
                    audible = Some((index, index));
                }
                gap = 0;
            } else if audible.is_some() {
                gap = gap.saturating_add(1);
            }
        }

        Self {
            audible,
            max_gap,
            glitch_count,
            clip_count,
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::{ProptestConfig, any, prop_assert_eq, proptest};

    use super::{
        ALLOWED_EDGE_FRAMES, ALLOWED_GAP_FRAMES, CLIP_THRESHOLD, GLITCH_GAP_FRAMES, Outcome,
        QUIET_THRESHOLD, Report, SAMPLE_RATE, SoakError, analyze,
    };

    /// One audible frame at a mid-scale amplitude the tone would actually reach.
    const TONE: (i16, i16) = (8_000, 8_000);
    const QUIET: (i16, i16) = (0, 0);

    /// Analyse a slice, passing its own length as the declared frame count.
    fn analyze_all(frames: &[(i16, i16)], duration_secs: u64) -> Outcome {
        analyze(frames.iter().copied(), frames.len() as u64, duration_secs)
    }

    fn tone(len: usize) -> Vec<(i16, i16)> {
        vec![TONE; len]
    }

    #[test]
    fn clean_tone_passes() {
        let frames = tone(150_000);
        let outcome = analyze_all(&frames, 3);
        assert_eq!(outcome.verdict, Ok(()), "clean tone should pass");
        let report = outcome.report.expect("report present on success");
        assert_eq!(
            report,
            Report {
                frames: 150_000,
                first_audible_frame: 0,
                last_audible_frame: 149_999,
                max_quiet_gap_frames: 0,
                glitch_gap_count: 0,
                clip_sample_count: 0,
            }
        );
    }

    #[test]
    fn silence_is_no_signal() {
        let frames = vec![QUIET; 150_000];
        let outcome = analyze_all(&frames, 3);
        assert_eq!(outcome.verdict, Err(SoakError::NoSignal));
        assert!(outcome.report.is_none(), "no stats without a signal");
    }

    #[test]
    fn too_short_is_rejected_before_scanning() {
        // Requested 10 s, but only ~1 s of frames: below 10*48000 - edge slack.
        let frames = tone(48_000);
        let outcome = analyze_all(&frames, 10);
        assert_eq!(outcome.verdict, Err(SoakError::TooShort { frames: 48_000 }));
    }

    #[test]
    fn late_start_is_rejected() {
        let lead = usize::try_from(ALLOWED_EDGE_FRAMES).unwrap() + 1;
        let mut frames = vec![QUIET; lead];
        frames.extend(tone(150_000));
        let outcome = analyze_all(&frames, 3);
        assert_eq!(
            outcome.verdict,
            Err(SoakError::StartedTooLate {
                first: ALLOWED_EDGE_FRAMES + 1,
            })
        );
    }

    #[test]
    fn early_end_is_rejected() {
        let trailing = usize::try_from(ALLOWED_EDGE_FRAMES).unwrap() + 1;
        let mut frames = tone(150_000);
        let total = frames.len() as u64;
        frames.extend(vec![QUIET; trailing]);
        let outcome = analyze_all(&frames, 3);
        assert_eq!(
            outcome.verdict,
            Err(SoakError::EndedTooEarly {
                last: total - 1,
                total: total + trailing as u64,
            })
        );
    }

    #[test]
    fn short_dropout_is_a_glitch() {
        // A quiet run longer than GLITCH_GAP_FRAMES but shorter than the
        // allowed max gap must still be flagged.
        let gap = usize::try_from(GLITCH_GAP_FRAMES).unwrap() + 5;
        assert!((gap as u64) <= ALLOWED_GAP_FRAMES);
        let mut frames = tone(60_000);
        frames.extend(vec![QUIET; gap]);
        frames.extend(tone(60_000));
        let outcome = analyze_all(&frames, 2);
        assert_eq!(
            outcome.verdict,
            Err(SoakError::Dropouts {
                count: 1,
                threshold: GLITCH_GAP_FRAMES,
                max_gap: gap as u64,
            })
        );
        // Stats are still reported alongside the failure.
        assert!(outcome.report.is_some());
    }

    #[test]
    fn long_gap_outranks_glitch() {
        let gap = usize::try_from(ALLOWED_GAP_FRAMES).unwrap() + 1;
        let mut frames = tone(60_000);
        frames.extend(vec![QUIET; gap]);
        frames.extend(tone(60_000));
        let outcome = analyze_all(&frames, 2);
        assert_eq!(
            outcome.verdict,
            Err(SoakError::QuietGap {
                max_gap: gap as u64,
            })
        );
    }

    #[test]
    fn clipping_is_rejected() {
        let mut frames = tone(150_000);
        frames[1_000] = (i16::try_from(CLIP_THRESHOLD).unwrap(), 0);
        let outcome = analyze_all(&frames, 3);
        assert_eq!(
            outcome.verdict,
            Err(SoakError::Clipping {
                count: 1,
                threshold: CLIP_THRESHOLD,
            })
        );
    }

    #[test]
    fn trailing_quiet_within_slack_still_passes() {
        // A quiet tail shorter than the edge slack is fine and, crucially, is
        // not counted as a gap (gaps are only measured between audible frames).
        let mut frames = tone(150_000);
        frames.extend(vec![QUIET; 1_000]);
        let outcome = analyze_all(&frames, 3);
        assert_eq!(outcome.verdict, Ok(()));
        assert_eq!(
            outcome.report.unwrap().max_quiet_gap_frames,
            0,
            "trailing quiet is not a gap"
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(32))]

        /// A constant, audible, unclipped tone of any length always passes:
        /// first frame at 0, last at the end, no gaps, no clipping.
        #[test]
        fn constant_audible_tone_always_passes(
            len in 50_000_usize..=60_000,
            amp in QUIET_THRESHOLD..CLIP_THRESHOLD,
            _seed in any::<u8>(),
        ) {
            let value = i16::try_from(amp).unwrap();
            let frames = vec![(value, value); len];
            // duration 1 s keeps the too-short/edge checks trivially satisfied.
            let outcome = analyze(frames.iter().copied(), frames.len() as u64, 1);
            prop_assert_eq!(outcome.verdict, Ok(()));
        }
    }

    #[test]
    fn constants_match_the_python_analyser() {
        assert_eq!(SAMPLE_RATE, 48_000);
        assert_eq!(ALLOWED_EDGE_FRAMES, 96_000);
        assert_eq!(ALLOWED_GAP_FRAMES, 2_400);
        assert_eq!(GLITCH_GAP_FRAMES, 40);
        assert_eq!(CLIP_THRESHOLD, 32_760);
    }
}
