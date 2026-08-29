//! The command-line arguments `hyperglare`'s binaries share.
//!
//! One definition for the offline renderer and the Bela host, for the reason
//! `oxtt-args` exists: the two binaries differ in where the audio comes from
//! and agree on everything about the effect, and a flag that means one thing
//! on one of them and another elsewhere is a bug nobody sees until a render
//! and a board disagree.
//!
//! What is *not* here is anything about a run: file paths, loudness targets,
//! block sizes and codec levels belong to whichever host has them.

use clap::ValueEnum;

use hyperglare_dsp::bank::{BankParams, q_max_for_breakpoint};
use hyperglare_dsp::exciter::ExciterParams;
use hyperglare_dsp::grid::{DEFAULT_HIGH_HZ, DEFAULT_LOW_HZ, Geometry, Grid};
use hyperglare_dsp::note::note_hz;
use hyperglare_dsp::processor::{HyperglareParams, SearPlacement};

/// The processor's settings, one flag each.
#[derive(clap::Args, Debug)]
pub struct ParamsArgs {
    /// How a note is replicated into resonator frequencies.
    #[arg(long, value_enum, default_value_t = GeometryArg::Octaves)]
    pub geometry: GeometryArg,

    /// Cents of detune per octave. Zero sits the grid on the input's own
    /// harmonics, which is where it does the least.
    #[arg(long, value_name = "CENTS", default_value_t = 0.0)]
    pub stretch: f32,

    /// Bottom of the band resonators are placed in.
    #[arg(long, value_name = "HZ", default_value_t = DEFAULT_LOW_HZ)]
    pub low_hz: f32,

    /// Top of the band resonators are placed in.
    #[arg(long, value_name = "HZ", default_value_t = DEFAULT_HIGH_HZ)]
    pub high_hz: f32,

    /// Decay to 60 dB, in seconds, below the breakpoint.
    ///
    /// The default is the knee: colour stops growing there and only the
    /// reverberation keeps going. Longer is a real setting, and a listener
    /// called 0.6 "flashy, and it matches the original concept".
    #[arg(long, value_name = "SECONDS", default_value_t = 0.25)]
    pub decay: f32,

    /// Where the bank stops sustaining and starts ringing.
    ///
    /// The Q cap is derived from this and the decay, because a frequency is a
    /// thing an ear can be pointed at and a Q is not.
    #[arg(long, value_name = "HZ", default_value_t = 1_100.0)]
    pub breakpoint_hz: f32,

    /// How much of the bandwidth difference between resonators to compensate.
    /// `0.5` is exact for broadband excitation, `0` for tonal.
    #[arg(long, value_name = "EXPONENT", default_value_t = 0.25)]
    pub compensation: f32,

    /// Spectral tilt, -1 (dark) to 1 (bright).
    #[arg(long, default_value_t = 0.5)]
    pub tilt: f32,

    /// Cents of detune spread across the voices.
    #[arg(long, value_name = "CENTS", default_value_t = 0.0)]
    pub drift: f32,

    /// Cents between the members of a pair, under `octave-pairs`.
    ///
    /// Wide spreads turn each grid point into a cluster, which is what fills
    /// the high band; narrow ones beat against each other instead.
    #[arg(long, value_name = "CENTS", default_value_t = 7.0)]
    pub pair_spread: f32,

    /// Voice count the level is normalised for. Not the number of notes.
    #[arg(long, default_value_t = 4)]
    pub voices: usize,

    /// Waveshaping into the bank: what makes the root's own partials ring.
    #[arg(long, default_value_t = 0.3)]
    pub drive: f32,

    /// Gated noise into the bank: what makes everything else ring.
    #[arg(long, default_value_t = 0.5)]
    pub noise: f32,

    /// Post-drive amount.
    #[arg(long, default_value_t = 0.0)]
    pub sear: f32,

    /// Where the post-drive sits relative to the stereo split.
    #[arg(long, value_enum, default_value_t = PlacementArg::AfterSum)]
    pub sear_placement: PlacementArg,

    /// Stereo spread. Only reaches the output under `before-split`.
    #[arg(long, default_value_t = 0.6)]
    pub width: f32,

    /// Dry/wet. Zero is the input, one is the resonators, two drives them.
    #[arg(long, default_value_t = 1.0)]
    pub color: f32,

    /// Input gain, in dB.
    #[arg(long, value_name = "DB", default_value_t = 0.0)]
    pub input_gain: f32,

    /// Output gain, in dB.
    #[arg(long, value_name = "DB", default_value_t = 0.0)]
    pub output_gain: f32,
}

/// [`Geometry`], as a command-line value.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum GeometryArg {
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
pub enum PlacementArg {
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
            color: args.color,
            input_gain_db: args.input_gain,
            output_gain_db: args.output_gain,
        }
    }
}

/// The chord, in hertz, from the note numbers a command line carries.
///
/// Here rather than in each binary so that a render and a board run cannot
/// disagree about what A4 is: [`note_hz`] is the one place that knows, and
/// this is the one path from a command line to it.
#[must_use]
pub fn chord_hz(notes: &[u8]) -> Vec<f32> {
    notes.iter().copied().map(note_hz).collect()
}
