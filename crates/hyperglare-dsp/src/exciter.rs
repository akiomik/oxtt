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

use crate::bands::{BANDS, Split, SplitCoeffs};

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
            // The top of the bank is fed by this path almost alone, so a
            // quarter is a quarter of the glare. M0 listened at 0.5 to 0.9.
            noise_amount: 0.5,
        }
    }
}

/// Everything the exciter's per-sample path needs that costs a transcendental.
///
/// Held apart from the state for the same reason [`effectkit::filter::SvfCoeffs`]
/// is, and holding the settings for the same reason [`ResonatorBank`] keeps no
/// [`BankParams`]: **the per-sample path sees derived values and nothing
/// else**, so it cannot be handed a coefficient and a setting that disagree
/// about the same knob.
///
/// Derived from the sample rate *and* the settings, so a caller rebuilds this
/// when either moves. Both are control-rate events.
///
/// [`ResonatorBank`]: crate::bank::ResonatorBank
/// [`BankParams`]: crate::bank::BankParams
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExciterCoeffs {
    attack: f32,
    release: f32,
    drive: Shaper,
    noise_amount: f32,
    split: SplitCoeffs,
}

impl ExciterCoeffs {
    /// Derives the exciter's coefficients for a sample rate and a setting.
    #[must_use]
    pub fn new(sample_rate: f32, params: &ExciterParams) -> Self {
        Self {
            attack: one_pole(GATE_ATTACK_MS, sample_rate),
            release: one_pole(GATE_RELEASE_MS, sample_rate),
            drive: Shaper::new(params.drive),
            noise_amount: params.noise_amount.clamp(0.0, 1.0),
            split: SplitCoeffs::new(sample_rate),
        }
    }
}

/// The waveshaper's constants for one drive setting.
///
/// [`shape`](Self::shape) needs a `10^(x/20)` and the two quantities derived
/// from it. With the drive held still that is the same number every sample,
/// and a `powf` there costs two orders of magnitude more than the arithmetic
/// around it. Deriving it once is the trade the filters already make.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shaper {
    amount: f32,
    gain: f32,
    /// `(1 + gain) / gain`: what it takes to put a full-scale input back at
    /// full scale after the clip.
    normalise: f32,
}

impl Default for Shaper {
    fn default() -> Self {
        Self::new(0.0)
    }
}

impl Shaper {
    /// Derives the constants for a drive amount, `0.0` (bypass) to `1.0`.
    #[must_use]
    pub fn new(amount: f32) -> Self {
        let amount = amount.clamp(0.0, 1.0);
        let gain = db_to_amp(amount * MAX_DRIVE_DB);
        Self {
            amount,
            gain,
            normalise: (1.0 + gain) / gain,
        }
    }

    /// A soft clip, crossfaded so that zero drive is a bypass.
    ///
    /// `u / (1 + |u|)`, normalised so that a full-scale input stays full
    /// scale, then blended against the input. The blend is what makes zero
    /// drive exactly transparent — without it the shaper's own curve would
    /// still be in the path, and "drive at zero" would not mean "no drive".
    #[inline]
    #[must_use]
    pub fn shape(&self, x: f32) -> f32 {
        if self.amount == 0.0 {
            return x;
        }
        let driven = self.gain * x;
        let shaped = driven / (1.0 + driven.abs()) * self.normalise;
        self.amount.mul_add(shaped - x, x)
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
    split: Split,
    envelope: [f32; BANDS],
    rng: u32,
}

impl Default for Exciter {
    fn default() -> Self {
        Self::new()
    }
}

impl Exciter {
    /// An exciter at rest.
    ///
    /// Not `const`: the split holds biquads, whose `Default` is derived and so
    /// cannot run in a `const` context. Nothing needs one, and a constructor
    /// that runs is cheaper than a shared crate growing an API for this.
    #[must_use]
    pub fn new() -> Self {
        Self {
            split: Split::new(),
            envelope: [0.0; BANDS],
            // Any non-zero seed. Fixed rather than random so that two renders
            // of the same settings are the same file and can be compared.
            rng: 0x2545_f491,
        }
    }

    /// Clears the gates and the split and rewinds the noise, so a render is
    /// reproducible.
    pub fn reset(&mut self) {
        self.split.reset_state();
        self.envelope = [0.0; BANDS];
        self.rng = 0x2545_f491;
    }

    /// One band's gate opening, `0.0` when that band has been silent.
    ///
    /// Out of range returns `0.0`: a band that does not exist has not heard
    /// anything.
    #[must_use]
    pub fn gate(&self, band: usize) -> f32 {
        self.envelope.get(band).copied().unwrap_or(0.0)
    }

    /// Whether any band's gate is open at all.
    ///
    /// What a caller asking "is this exciter silent" wants, and what the
    /// silence guarantee is stated over. The per-band opening is for a caller
    /// asking which part of the source is doing the work.
    #[must_use]
    pub fn any_open(&self) -> bool {
        self.envelope.iter().any(|e| *e > 0.0)
    }

    /// One sample in, one sample of excitation per band out.
    ///
    /// Both paths are taken from the band rather than from the input, so a
    /// band the source is silent in produces exactly zero and the resonators
    /// drawing on it stay silent — which is the whole of ADR 0016.
    #[inline]
    pub fn process(&mut self, x: f32, coeffs: &ExciterCoeffs) -> [f32; BANDS] {
        let split = self.split.process(x, &coeffs.split);
        // **One draw for the frame, shared across the bands.** The bands
        // partition the spectrum, so two resonators reading the same draw are
        // at different frequencies and extract uncorrelated parts of it; a
        // draw per band would cost four more xorshifts to make no audible
        // difference. What must not be shared is the gate, and that is not.
        let noise = self.next_noise() * coeffs.noise_amount;

        let mut out = [0.0; BANDS];
        for ((slot, band), envelope) in out.iter_mut().zip(split).zip(self.envelope.iter_mut()) {
            let level = band.abs();
            let coefficient = if level > *envelope {
                coeffs.attack
            } else {
                coeffs.release
            };
            let next = coefficient.mul_add(*envelope - level, level);
            // Flushed, so silence arrives at zero rather than approaching it.
            *envelope = if next < envelope_floor() { 0.0 } else { next };
            // The gate is the only thing that stops the noise path from
            // sounding into silence. Multiplying by it is the guarantee, and
            // it is now a guarantee per band.
            *slot = noise.mul_add(*envelope, coeffs.drive.shape(band));
        }
        out
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

    fn coeffs(params: ExciterParams) -> ExciterCoeffs {
        ExciterCoeffs::new(SR, &params)
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
        let params = wide_open();
        let coeffs = coeffs(params);
        let mut exciter = Exciter::new();

        // Something loud first, so the gate has to have actually closed rather
        // than never having opened.
        for i in 0..4_800 {
            exciter.process((i as f32 * 0.01).sin(), &coeffs);
        }
        assert!(exciter.any_open(), "no band's gate opened");

        // Long enough for a one-pole release to cross the silence floor from
        // full scale, derived rather than guessed so that this keeps testing
        // the same thing if either constant moves.
        let release_samples = GATE_RELEASE_MS * 0.001 * SR;
        let constants = -(envelope_floor().ln());
        let wait = (release_samples * constants * 1.2) as usize;
        for _ in 0..wait {
            exciter.process(0.0, &coeffs);
        }
        // Exact comparisons on purpose: the claim is zero, not "small". A
        // tolerance here would pass for a gate that had been removed, which is
        // the one failure this test exists to catch.
        #[allow(clippy::float_cmp)]
        {
            assert!(!exciter.any_open(), "a gate never fully closed");
            for band in 0..BANDS {
                assert_eq!(exciter.gate(band), 0.0, "band {band} never closed");
            }
            for i in 0..4_800 {
                for (band, y) in exciter.process(0.0, &coeffs).into_iter().enumerate() {
                    assert_eq!(y, 0.0, "sample {i} of band {band} was {y}, not zero");
                }
            }
        }
    }

    /// The other half: with the input present, the noise path is actually
    /// contributing. A gate stuck shut would satisfy the test above.
    #[test]
    fn the_noise_path_contributes_while_the_input_is_present() {
        let quiet = ExciterParams {
            drive: 0.0,
            noise_amount: 0.0,
        };
        let noisy = ExciterParams {
            drive: 0.0,
            noise_amount: 1.0,
        };

        // The two differ only in the noise, so their difference is it. The
        // bands do not sum to the input any more, so this compares the
        // exciter against itself rather than against `x`.
        let run = |params: ExciterParams| {
            let coeffs = coeffs(params);
            let mut exciter = Exciter::new();
            let mut out = Vec::new();
            for i in 0..4_800 {
                let x = (2.0 * PI * 110.0 * i as f32 / SR).sin();
                out.push(exciter.process(x, &coeffs));
            }
            out
        };
        let added = |params: ExciterParams| {
            run(quiet)
                .into_iter()
                .zip(run(params))
                .flat_map(|(a, b)| a.into_iter().zip(b))
                .map(|(a, b)| f64::from(b - a) * f64::from(b - a))
                .sum::<f64>()
        };

        // Exact: asking for no noise must add none, not merely little.
        #[allow(clippy::float_cmp)]
        {
            assert_eq!(added(quiet), 0.0, "no noise asked for, none added");
        }
        assert!(
            added(noisy) > 1.0,
            "the noise path should be audible while the input is present"
        );
    }

    /// Zero drive is a bypass, not "the shaper's curve at low gain". Without
    /// the crossfade the soft clip would still be in the path.
    #[test]
    fn zero_drive_is_transparent() {
        let params = ExciterParams {
            drive: 0.0,
            noise_amount: 0.0,
        };
        let coeffs = coeffs(params);
        let mut exciter = Exciter::new();
        // The bands no longer sum to the input, so "transparent" is stated
        // against the split rather than against the sample: with no drive and
        // no noise, what comes out of a band is exactly what the split put in
        // it, with the shaper's curve nowhere in the path.
        let split_coeffs = SplitCoeffs::new(SR);
        let mut split = Split::new();
        #[allow(clippy::float_cmp)]
        for x in [0.0f32, 0.1, -0.25, 0.5, -1.0, 1.0, 0.3, -0.7] {
            let gated = exciter.process(x, &coeffs);
            let bare = split.process(x, &split_coeffs);
            for (band, (y, expected)) in gated.into_iter().zip(bare).enumerate() {
                assert_eq!(y, expected, "band {band} of {x} came back as {y}");
            }
        }
    }

    /// Drive adds partials that were not in the input: this is what feeds the
    /// grid points sitting on the root's own harmonics.
    #[test]
    fn drive_adds_harmonics_of_the_input() {
        // A pure sine has one partial. Count how much energy leaves the
        // fundamental once the shaper is working.
        let third_harmonic = |drive: f32| {
            let params = ExciterParams {
                drive,
                noise_amount: 0.0,
            };
            let coeffs = coeffs(params);
            let mut exciter = Exciter::new();
            let (freq, samples) = (220.0f32, 4_800usize);
            let (mut re, mut im) = (0.0f64, 0.0f64);
            for i in 0..samples {
                let t = i as f32 / SR;
                // Summed across the bands: the shaper puts the third
                // harmonic two bands above the fundamental, so a single band
                // would measure the split instead of the drive.
                let y: f32 = exciter
                    .process((2.0 * PI * freq * t).sin(), &coeffs)
                    .iter()
                    .sum();
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
            let shaper = Shaper::new(drive);
            let y = shaper.shape(1.0);
            assert!(
                (y - 1.0).abs() < 1e-5,
                "drive {drive}: full scale became {y}"
            );
            assert!((shaper.shape(-1.0) + 1.0).abs() < 1e-5);
        }
    }

    /// Reset rewinds the noise, so two renders of one setting are one file.
    #[test]
    fn reset_makes_a_render_reproducible() {
        let params = wide_open();
        let coeffs = coeffs(params);
        let run = |exciter: &mut Exciter| {
            let mut out = Vec::new();
            for i in 0..512 {
                out.push(exciter.process((i as f32 * 0.01).sin(), &coeffs));
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
