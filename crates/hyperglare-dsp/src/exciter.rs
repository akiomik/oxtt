//! What feeds the resonator bank.
//!
//! A band of tuned band-passes is a filter, not an oscillator. It can only
//! emphasise energy the input already has at the frequencies it is tuned to,
//! so **what arrives at its input decides which of its resonators are audible
//! at all** — and the two halves of the chord need different things.
//!
//! # Two paths, because two kinds of grid point
//!
//! A waveshaper driven by a periodic input produces harmonics of that input's
//! fundamental and nothing between them. That is exactly right for the grid
//! points sitting on the root's own harmonics, and useless for the ones that
//! are not: the third of a chord is not in the root's series, and neither is a
//! detuned octave. Those need something aperiodic.
//!
//! ```text
//! excite(x) = shape(x, drive) + noise_amount · gate(x) · noise()
//! ```
//!
//! - **`shape`** supplies the root-coincident points. More drive, more
//!   partials of the input, so more of them ring.
//! - **`gate · noise`** supplies everything else. It is not a garnish: above
//!   the Q cap's breakpoint, ordinary tuning drift is wider than a resonator's
//!   bandwidth, so the high end of the bank is fed by this path almost alone.
//!
//! # The gate is load-bearing
//!
//! `hyperglare` is the first effect in this family with a signal source inside
//! it. `oxtt` only ever scales what arrived, so silence out of silence is a
//! property of its shape and cannot be got wrong. Here it is a property of
//! this one multiplication, and nothing else holds it up.
//!
//! So the guarantee is stated exactly and tested at the setting that would
//! break it: **once the gate's envelope has decayed past the level `effectkit`
//! calls silence, the output is exactly zero** — not small, zero — with the
//! drive and the noise at maximum.
//!
//! "Past the silence floor" rather than "after the release", because those are
//! not the same duration: a one-pole release of 30 ms reaches 120 dB down
//! after about fourteen of its own time constants, which is a bit under half a
//! second. The flush is what turns the asymptote into an arrival.

use effectkit::decibels::{FLOOR_DB, db_to_amp};

/// Gain at full drive, before the shaper.
///
/// Thirty decibels: enough that a bass fundamental grows a dense series of
/// partials, short of enough to turn every input into the same square wave.
const MAX_DRIVE_DB: f32 = 30.0;

/// How fast the gate opens, in milliseconds.
///
/// Short, because the gate's job is to follow the input's presence rather than
/// its shape, and a slow open would swallow the attack that a percussive
/// source excites the bank with.
const GATE_ATTACK_MS: f32 = 1.0;

/// How fast the gate closes, in milliseconds.
///
/// Long enough that the noise path does not chatter between the grains of a
/// plucked note, short enough that it is shut before a listener would notice
/// the room had gone quiet.
const GATE_RELEASE_MS: f32 = 30.0;

/// Envelope values below this are flushed to zero.
///
/// The gate's guarantee is that silence produces *exactly* zero, and a
/// one-pole envelope only approaches zero. Flushing is what turns the
/// asymptote into an arrival, and it keeps the denormals that would otherwise
/// accumulate here off the audio thread.
///
/// [`FLOOR_DB`] rather than a value near the denormal boundary, because the
/// question this answers is "what counts as silence" and `effectkit` already
/// decided that once. A denormal-scale floor would technically arrive too, but
/// only after some forty-five release constants — nearly a second and a half —
/// which is not what "silent for longer than the release" means to anybody.
/// The noise is multiplied by this, so the level it flushes at is 120 dB down.
fn envelope_floor() -> f32 {
    db_to_amp(FLOOR_DB)
}

/// How the input is turned into something the bank can resonate with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExciterParams {
    /// Waveshaping amount, `0.0` (untouched) to `1.0`.
    pub drive: f32,
    /// How much gated noise to add, `0.0` to `1.0`.
    ///
    /// Not a garnish. See the module documentation: the top of the bank is
    /// fed by this path almost alone.
    pub noise_amount: f32,
}

impl Default for ExciterParams {
    fn default() -> Self {
        Self {
            drive: 0.3,
            noise_amount: 0.25,
        }
    }
}

/// The gate's one-pole coefficients, derived once per sample-rate change.
///
/// Held apart from the state for the same reason [`effectkit::filter::SvfCoeffs`]
/// is: they come from an `exp`, and the per-sample path should not.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExciterCoeffs {
    attack: f32,
    release: f32,
}

impl ExciterCoeffs {
    /// Derives the gate's coefficients for a sample rate.
    #[must_use]
    pub fn new(sample_rate: f32) -> Self {
        Self {
            attack: one_pole(GATE_ATTACK_MS, sample_rate),
            release: one_pole(GATE_RELEASE_MS, sample_rate),
        }
    }
}

/// `exp(-1 / samples)`, floored so a degenerate sample rate cannot divide by
/// zero.
#[inline]
fn one_pole(time_ms: f32, sample_rate: f32) -> f32 {
    let samples = (time_ms * 0.001 * sample_rate).max(1.0);
    (-1.0 / samples).exp()
}

/// Turns an input into something a resonator bank can ring on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Exciter {
    envelope: f32,
    rng: u32,
}

impl Default for Exciter {
    fn default() -> Self {
        Self::new()
    }
}

impl Exciter {
    /// An exciter at rest.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            envelope: 0.0,
            // Any non-zero seed. Fixed rather than random so that two renders
            // of the same settings are the same file and can be compared.
            rng: 0x2545_f491,
        }
    }

    /// Clears the gate and rewinds the noise, so a render is reproducible.
    pub const fn reset(&mut self) {
        self.envelope = 0.0;
        self.rng = 0x2545_f491;
    }

    /// The gate's current opening, `0.0` when the input has been silent.
    #[must_use]
    pub const fn gate(&self) -> f32 {
        self.envelope
    }

    /// One sample in, one sample of excitation out.
    #[inline]
    pub fn process(&mut self, x: f32, params: &ExciterParams, coeffs: &ExciterCoeffs) -> f32 {
        let level = x.abs();
        let coefficient = if level > self.envelope {
            coeffs.attack
        } else {
            coeffs.release
        };
        let envelope = coefficient.mul_add(self.envelope - level, level);
        // Flushed, so silence arrives at zero rather than approaching it.
        self.envelope = if envelope < envelope_floor() {
            0.0
        } else {
            envelope
        };

        let shaped = shape(x, params.drive);
        // The gate is the only thing that stops the noise path from sounding
        // into silence. Multiplying by it is the guarantee.
        let noise = self.next_noise() * params.noise_amount * self.envelope;
        shaped + noise
    }

    /// A uniform sample in `[-1, 1)`, from a xorshift.
    #[inline]
    fn next_noise(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        // The mantissa bits give a float in `[1, 2)` with no division and no
        // integer-to-float rounding; the affine map takes that to `[-1, 1)`.
        f32::from_bits((self.rng >> 9) | 0x3f80_0000).mul_add(2.0, -3.0)
    }
}

/// The waveshaper: a soft clip, crossfaded so that zero drive is a bypass.
///
/// `u / (1 + |u|)`, normalised so that a full-scale input stays full-scale,
/// then blended against the input. The blend is what makes zero drive exactly
/// transparent — without it the shaper's own curve would still be in the path,
/// and "drive at zero" would not mean "no drive".
#[inline]
fn shape(x: f32, drive: f32) -> f32 {
    let drive = drive.clamp(0.0, 1.0);
    if drive == 0.0 {
        return x;
    }
    let gain = db_to_amp(drive * MAX_DRIVE_DB);
    let driven = gain * x;
    // `gain / (1 + gain)` is what a full-scale input becomes, so dividing by
    // it keeps the shaper's output at the scale its input arrived on.
    let normalise = (1.0 + gain) / gain;
    let shaped = driven / (1.0 + driven.abs()) * normalise;
    drive.mul_add(shaped - x, x)
}

#[cfg(test)]
// Test-only arithmetic over sample indices: the counts are small and the
// values positive by construction.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::arithmetic_side_effects,
    clippy::suboptimal_flops,
    clippy::indexing_slicing
)]
mod tests {
    use core::f32::consts::PI;

    use super::*;

    const SR: f32 = 48_000.0;

    fn coeffs() -> ExciterCoeffs {
        ExciterCoeffs::new(SR)
    }

    /// Everything at maximum, which is the only setting at which this can
    /// fail: at `noise_amount = 0` there is no source to leak.
    fn wide_open() -> ExciterParams {
        ExciterParams {
            drive: 1.0,
            noise_amount: 1.0,
        }
    }

    /// **The invariant the gate exists for.** Not "small" — zero.
    ///
    /// `oxtt` gets this from its shape; `hyperglare` gets it from one
    /// multiplication, so it is tested at the setting that would expose the
    /// multiplication being removed.
    #[test]
    fn silence_in_gives_exactly_silence_out_with_everything_at_maximum() {
        let (params, coeffs) = (wide_open(), coeffs());
        let mut exciter = Exciter::new();

        // Something loud first, so the gate has to have actually closed rather
        // than never having opened.
        for i in 0..4_800 {
            exciter.process((i as f32 * 0.01).sin(), &params, &coeffs);
        }
        assert!(exciter.gate() > 0.0, "the gate should be open by now");

        // Long enough for a one-pole release to cross the silence floor from
        // full scale, derived rather than guessed so that this keeps testing
        // the same thing if either constant moves.
        let release_samples = GATE_RELEASE_MS * 0.001 * SR;
        let constants = -(envelope_floor().ln());
        let wait = (release_samples * constants * 1.2) as usize;
        for _ in 0..wait {
            exciter.process(0.0, &params, &coeffs);
        }
        // Exact comparisons on purpose: the claim is zero, not "small". A
        // tolerance here would pass for a gate that had been removed, which is
        // the one failure this test exists to catch.
        #[allow(clippy::float_cmp)]
        {
            assert_eq!(exciter.gate(), 0.0, "the gate never fully closed");
            for i in 0..4_800 {
                let y = exciter.process(0.0, &params, &coeffs);
                assert_eq!(y, 0.0, "sample {i} of silence was {y}, not zero");
            }
        }
    }

    /// The other half: with the input present, the noise path is actually
    /// contributing. A gate stuck shut would satisfy the test above.
    #[test]
    fn the_noise_path_contributes_while_the_input_is_present() {
        let coeffs = coeffs();
        let quiet = ExciterParams {
            drive: 0.0,
            noise_amount: 0.0,
        };
        let noisy = ExciterParams {
            drive: 0.0,
            noise_amount: 1.0,
        };

        let energy = |params: &ExciterParams| {
            let mut exciter = Exciter::new();
            let mut sum = 0.0f64;
            for i in 0..4_800 {
                // A pure tone: with no drive and no noise, the output is the
                // input, so any excess is the noise path.
                let x = (2.0 * PI * 110.0 * i as f32 / SR).sin();
                let y = exciter.process(x, params, &coeffs);
                sum += f64::from(y - x) * f64::from(y - x);
            }
            sum
        };

        // Exact: asking for no noise must add none, not merely little.
        #[allow(clippy::float_cmp)]
        {
            assert_eq!(energy(&quiet), 0.0, "no noise asked for, none added");
        }
        assert!(
            energy(&noisy) > 1.0,
            "the noise path should be audible while the input is present"
        );
    }

    /// Zero drive is a bypass, not "the shaper's curve at low gain". Without
    /// the crossfade the soft clip would still be in the path.
    #[test]
    fn zero_drive_is_transparent() {
        let coeffs = coeffs();
        let params = ExciterParams {
            drive: 0.0,
            noise_amount: 0.0,
        };
        let mut exciter = Exciter::new();
        // Exact: "transparent" means the sample comes back unchanged, not
        // approximately unchanged.
        #[allow(clippy::float_cmp)]
        for x in [0.0f32, 0.1, -0.25, 0.5, -1.0, 1.0] {
            let y = exciter.process(x, &params, &coeffs);
            assert_eq!(y, x, "{x} came back as {y}");
        }
    }

    /// Drive adds partials that were not in the input: this is what feeds the
    /// grid points sitting on the root's own harmonics.
    #[test]
    fn drive_adds_harmonics_of_the_input() {
        let coeffs = coeffs();
        // A pure sine has one partial. Count how much energy leaves the
        // fundamental once the shaper is working.
        let third_harmonic = |drive: f32| {
            let params = ExciterParams {
                drive,
                noise_amount: 0.0,
            };
            let mut exciter = Exciter::new();
            let (freq, samples) = (220.0f32, 4_800usize);
            let (mut re, mut im) = (0.0f64, 0.0f64);
            for i in 0..samples {
                let t = i as f32 / SR;
                let y = exciter.process((2.0 * PI * freq * t).sin(), &params, &coeffs);
                let probe = 2.0 * PI * (3.0 * freq) * t;
                re += f64::from(y) * f64::from(probe.cos());
                im += f64::from(y) * f64::from(probe.sin());
            }
            re.hypot(im) / samples as f64
        };

        let clean = third_harmonic(0.0);
        let driven = third_harmonic(1.0);
        assert!(
            clean < 1e-3,
            "a sine through a bypass should stay a sine: {clean}"
        );
        assert!(
            driven > 20.0 * clean.max(1e-6),
            "drive should produce a third harmonic: {clean} -> {driven}"
        );
    }

    /// A full-scale input stays full-scale however hard it is driven, so the
    /// drive knob is not also a volume knob.
    #[test]
    fn the_shaper_holds_its_scale_across_the_drive_range() {
        for drive in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
            let y = shape(1.0, drive);
            assert!(
                (y - 1.0).abs() < 1e-5,
                "drive {drive}: full scale became {y}"
            );
            assert!((shape(-1.0, drive) + 1.0).abs() < 1e-5);
        }
    }

    /// Reset rewinds the noise, so two renders of one setting are one file.
    #[test]
    fn reset_makes_a_render_reproducible() {
        let (params, coeffs) = (wide_open(), coeffs());
        let run = |exciter: &mut Exciter| {
            let mut out = Vec::new();
            for i in 0..512 {
                out.push(exciter.process((i as f32 * 0.01).sin(), &params, &coeffs));
            }
            out
        };
        let mut exciter = Exciter::new();
        let first = run(&mut exciter);
        exciter.reset();
        assert_eq!(first, run(&mut exciter));
    }

    /// The noise is noise: uniform-ish, centred, and inside the scale the
    /// bank's normalisation assumes.
    #[test]
    fn the_noise_is_centred_and_bounded() {
        let mut exciter = Exciter::new();
        let mut sum = 0.0f64;
        let mut peak = 0.0f32;
        for _ in 0..100_000 {
            let n = exciter.next_noise();
            sum += f64::from(n);
            peak = peak.max(n.abs());
        }
        let mean = sum / 100_000.0;
        assert!(mean.abs() < 0.02, "noise should be centred, mean {mean}");
        assert!(
            (0.9..=1.0).contains(&peak),
            "noise should fill [-1, 1): {peak}"
        );
    }
}
