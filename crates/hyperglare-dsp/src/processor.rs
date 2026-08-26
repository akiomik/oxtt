//! The whole effect: excitation, resonance, post-drive, and the mix.
//!
//! ```text
//! in (L, R)
//!   -> input gain
//!   -> mid = 0.5(L + R)                     the bank runs in mono
//!   -> EXCITE   shape + gated noise         what the resonators ring on
//!   -> ResonatorBank                        the chord
//!   -> SEAR     post-drive                  where in the chain is a choice
//!   -> anti-alias low-pass
//!   -> mix against the dry pair
//!   -> output gain -> soft limit -> finite guard
//! out (L, R)
//! ```
//!
//! # Two non-linearities, in two places, doing two jobs
//!
//! `EXCITE` sits before the bank and decides *which resonators sound*
//! ([`crate::exciter`]). `SEAR` sits after it and makes the resonators that
//! are sounding intermodulate with each other. Folding them into one control
//! would leave a user unable to tell why more distortion sometimes adds
//! partials and sometimes blurs the pitch: those are the two stages, and they
//! are separate knobs because they are separable effects.
//!
//! # Where `SEAR` goes is an open question, so it is a parameter
//!
//! Once the bank's resonators have been summed they cannot be separated again,
//! so a waveshaper placed after the sum intermodulates *every* pair of
//! partials, while one placed after a stereo split only reaches the pairs that
//! landed on the same side. Those are different sounds, and which is wanted is
//! not settled on paper — so [`SearPlacement`] carries the choice the way
//! [`crate::grid::Geometry`] carries the geometry, and the comparison is done
//! by ear.
//!
//! The wet path is mono under [`SearPlacement::AfterSum`]. Widening it needs a
//! decorrelator, which is a delay line and a component of its own; it is left
//! out deliberately, because it would change the width of one arm of a
//! comparison whose subject is the waveshaper's position and not the width.
//! The dry path stays stereo either way, so the output is stereo in both.

use effectkit::decibels::db_to_amp;
use effectkit::filter::{Biquad, biquad_coeffs};

use crate::bank::{BankParams, ResonatorBank};
use crate::exciter::{Exciter, ExciterCoeffs, ExciterParams, shape};

/// Where the anti-alias low-pass sits, as a fraction of the sample rate.
///
/// `SEAR` folds everything above Nyquist back down, and the bank reaches 9 kHz
/// by design, so the folded material is not hypothetical. Whether to spend an
/// oversampler on removing it or to keep the fold as part of the sound is an
/// M0 question; this is the floor either answer builds on.
const ANTI_ALIAS_RATIO: f32 = 0.42;

/// Output level at which the soft limiter starts to bend, in dBFS.
///
/// Close to full scale on purpose. Any function that maps every input into
/// `[-1, 1]` and leaves some region untouched has to bend somewhere below one,
/// so the question is only where — and the answer has to be above where
/// ordinary material sits, or the limiter is a tone control that engages
/// whenever a peak arrives. One decibel of headroom is what gain staging is
/// for.
const LIMIT_THRESHOLD_DB: f32 = -1.0;

/// Where the post-drive waveshaper sits relative to the stereo split.
///
/// Cheapest first. Both are real answers; the comparison is by ear (M0).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearPlacement {
    /// Shape the summed bank, then send one signal to both channels.
    ///
    /// Every pair of resonators intermodulates, which is the behaviour `SEAR`
    /// was described as having. The wet path is mono.
    #[default]
    AfterSum,
    /// Split the bank across the pair first, then shape each side.
    ///
    /// Only resonators that landed on the same side intermodulate, so the
    /// left/right assignment becomes a timbre control as well as a width one —
    /// under the octave geometries each channel then carries partials two
    /// octaves apart rather than one. Costs a second waveshaper.
    BeforeSplit,
}

/// Everything the effect needs to know.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HyperglareParams {
    /// The resonators: where they sit, how long they ring, how they are
    /// balanced.
    pub bank: BankParams,
    /// What feeds them.
    pub exciter: ExciterParams,
    /// Where the post-drive sits.
    pub sear_placement: SearPlacement,
    /// Post-drive amount, `0.0` to `1.0`.
    pub sear: f32,
    /// Stereo spread of the bank, `0.0` to `1.0`.
    ///
    /// Only reaches the output under [`SearPlacement::BeforeSplit`]; see the
    /// module documentation for why the other arm is mono.
    pub width: f32,
    /// Dry/wet, `0.0` to `2.0`.
    ///
    /// Zero is the input untouched, one is the resonators alone, and the range
    /// above one drives the wet path harder while the dry stays out — the
    /// wet-only half of the sweep, where the effect stops being a colour and
    /// becomes the sound.
    pub color: f32,
    /// Input gain, in dB.
    pub input_gain_db: f32,
    /// Output gain, in dB.
    pub output_gain_db: f32,
}

impl Default for HyperglareParams {
    fn default() -> Self {
        Self {
            bank: BankParams::default(),
            exciter: ExciterParams::default(),
            sear_placement: SearPlacement::default(),
            sear: 0.0,
            width: 0.6,
            color: 1.0,
            input_gain_db: 0.0,
            output_gain_db: 0.0,
        }
    }
}

/// The effect, with capacity for `N` resonators.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HyperglareProcessor<const N: usize> {
    bank: ResonatorBank<N>,
    exciter: Exciter,
    exciter_coeffs: ExciterCoeffs,
    anti_alias: [Biquad; 2],
    params: HyperglareParams,
    sample_rate: f32,
}

impl<const N: usize> HyperglareProcessor<N> {
    /// Builds a processor for a sample rate, with no chord sounding.
    #[must_use]
    pub fn new(params: HyperglareParams, sample_rate: f32) -> Self {
        let mut processor = Self {
            bank: ResonatorBank::new(),
            exciter: Exciter::new(),
            exciter_coeffs: ExciterCoeffs::new(sample_rate),
            anti_alias: [Biquad::default(); 2],
            params,
            sample_rate,
        };
        processor.set_sample_rate(sample_rate);
        processor
    }

    /// Rebuilds everything that depends on the sample rate, and clears state.
    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.exciter_coeffs = ExciterCoeffs::new(sample_rate);
        let coeffs = biquad_coeffs(ANTI_ALIAS_RATIO * sample_rate, sample_rate, false);
        for stage in &mut self.anti_alias {
            stage.set_coeffs(coeffs);
            stage.reset_state();
        }
        self.exciter.reset();
    }

    /// Applies a new setting. Control rate, not sample rate.
    ///
    /// The bank is retuned only when something it depends on has moved, so a
    /// knob that is not a bank knob costs nothing.
    pub fn apply_params(&mut self, params: &HyperglareParams, notes_hz: &[f32]) {
        self.params = *params;
        self.bank.retune(notes_hz, &params.bank, self.sample_rate);
    }

    /// How many resonators are sounding.
    #[must_use]
    pub const fn active(&self) -> usize {
        self.bank.active()
    }

    /// Silences the tails and the gate, keeping the tuning.
    pub fn reset_state(&mut self) {
        self.bank.reset_state();
        self.exciter.reset();
        for stage in &mut self.anti_alias {
            stage.reset_state();
        }
    }

    /// One stereo frame in, one stereo frame out.
    #[inline]
    pub fn process_frame(&mut self, left: f32, right: f32) -> (f32, f32) {
        let input = db_to_amp(self.params.input_gain_db);
        let (dry_l, dry_r) = (left * input, right * input);
        let mid = f32::midpoint(dry_l, dry_r);

        let excited = self
            .exciter
            .process(mid, &self.params.exciter, &self.exciter_coeffs);

        let (mut wet_l, mut wet_r) = match self.params.sear_placement {
            SearPlacement::AfterSum => {
                let seared = shape(self.bank.process(excited), self.params.sear);
                (seared, seared)
            }
            SearPlacement::BeforeSplit => {
                let (l, r) = self.bank.process_split(excited, self.params.width);
                (shape(l, self.params.sear), shape(r, self.params.sear))
            }
        };
        // After the waveshaper, always: what it folds down is what this is for.
        let [left_stage, right_stage] = &mut self.anti_alias;
        wet_l = left_stage.process(wet_l);
        wet_r = right_stage.process(wet_r);

        let (dry_gain, wet_gain) = mix_gains(self.params.color);
        let output = db_to_amp(self.params.output_gain_db);
        let out_l = soft_limit(wet_gain.mul_add(wet_l, dry_gain * dry_l) * output);
        let out_r = soft_limit(wet_gain.mul_add(wet_r, dry_gain * dry_r) * output);

        (guard(out_l), guard(out_r))
    }

    /// A block of stereo frames, in place.
    ///
    /// A loop over [`process_frame`](Self::process_frame) and nothing else, so
    /// the two cannot disagree and the output does not depend on how a caller
    /// chunks its buffers.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let (out_l, out_r) = self.process_frame(*l, *r);
            *l = out_l;
            *r = out_r;
        }
    }
}

/// Dry and wet gains for a `color` in `0.0..=2.0`.
///
/// Below one it is a crossfade. Above one the dry is already gone, so the
/// range drives the wet instead — the two halves are different operations, and
/// the only thing they share is that turning the knob up gets more effect.
#[inline]
fn mix_gains(color: f32) -> (f32, f32) {
    let color = color.clamp(0.0, 2.0);
    if color <= 1.0 {
        (1.0 - color, color)
    } else {
        (0.0, color)
    }
}

/// A soft knee above [`LIMIT_THRESHOLD_DB`], bounding the output to full
/// scale.
///
/// The bank runs resonators with a Q in the hundreds and the two waveshapers
/// can both be at maximum, so "finite" is not a strong enough promise on its
/// own.
///
/// Everything below the threshold passes bit-exactly, so the effect is
/// transparent at zero colour for material that is gain-staged; a peak above
/// the threshold is bent even then, which is the price of the output being
/// bounded at all.
#[inline]
fn soft_limit(x: f32) -> f32 {
    let threshold = db_to_amp(LIMIT_THRESHOLD_DB);
    let magnitude = x.abs();
    if magnitude <= threshold {
        return x;
    }
    let over = magnitude - threshold;
    let headroom = 1.0 - threshold;
    let limited = headroom.mul_add(over / (over + headroom), threshold);
    limited.copysign(x)
}

/// Non-finite samples become silence rather than escaping into a host.
#[inline]
const fn guard(x: f32) -> f32 {
    if x.is_finite() { x } else { 0.0 }
}

#[cfg(test)]
// Test-only arithmetic over sample indices and decibels: small counts,
// positive by construction, and a signal generator reads better written out.
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
    use crate::grid::{Geometry, Grid};

    const SR: f32 = 48_000.0;
    type Processor = HyperglareProcessor<256>;

    fn tone(i: usize, hz: f32) -> f32 {
        (2.0 * PI * hz * i as f32 / SR).sin()
    }

    /// Runs a bass note through and returns the stereo RMS pair.
    fn run(processor: &mut Processor, seconds: f32, level: f32) -> (f32, f32) {
        let samples = (SR * seconds) as usize;
        let mut sum = (0.0f64, 0.0f64);
        let mut counted = 0usize;
        for i in 0..samples {
            let x = tone(i, 55.0) * level;
            let (left, right) = processor.process_frame(x, x);
            if i > samples / 2 {
                sum.0 += f64::from(left) * f64::from(left);
                sum.1 += f64::from(right) * f64::from(right);
                counted += 1;
            }
        }
        let n = counted as f64;
        ((sum.0 / n).sqrt() as f32, (sum.1 / n).sqrt() as f32)
    }

    /// The invariant `docs/hyperglare/contracts.md` §4 warns is no longer
    /// structural, at the whole-processor level and at the settings that would
    /// expose it.
    ///
    /// **How long silence takes to arrive is a setting, not a constant.** The
    /// gate closes in tens of milliseconds, but the resonators go on ringing
    /// for as long as they were told to — that is what they are for — and only
    /// reach exactly zero when their state crosses the filter's denormal
    /// floor, which is about ten decay times away. This runs at a short decay
    /// so the test is a test rather than a minute of audio; the arrival scales
    /// with `decay_t60_s` and does not otherwise change.
    #[test]
    fn silence_in_gives_exactly_silence_out_once_the_tails_have_arrived() {
        let decay = 0.05;
        let params = HyperglareParams {
            exciter: ExciterParams {
                drive: 1.0,
                noise_amount: 1.0,
            },
            sear: 1.0,
            color: 2.0,
            output_gain_db: 24.0,
            bank: BankParams {
                decay_t60_s: decay,
                ..BankParams::default()
            },
            ..HyperglareParams::default()
        };
        for placement in [SearPlacement::AfterSum, SearPlacement::BeforeSplit] {
            let params = HyperglareParams {
                sear_placement: placement,
                ..params
            };
            let mut processor = Processor::new(params, SR);
            processor.apply_params(&params, &[55.0, 82.4, 110.0]);
            // Excite it first, so the gate has to close rather than never open.
            for i in 0..(SR as usize / 2) {
                processor.process_frame(tone(i, 55.0), tone(i, 55.0));
            }
            // Two stages in series, not one. The gate goes on feeding the
            // bank while it closes — about fourteen of its own 30 ms
            // constants, so a bit under half a second — and only then does the
            // tail start decaying toward the filter's floor, which is another
            // seven decay times. Doubled for margin.
            let gate_arrival = SR * 0.45;
            let tail_arrival = SR * decay * 8.0;
            let wait = ((gate_arrival + tail_arrival) * 2.0) as usize;
            for _ in 0..wait {
                processor.process_frame(0.0, 0.0);
            }
            #[allow(clippy::float_cmp)]
            for i in 0..4_800 {
                let (l, r) = processor.process_frame(0.0, 0.0);
                assert_eq!(l, 0.0, "{placement:?}: left sample {i} was {l}");
                assert_eq!(r, 0.0, "{placement:?}: right sample {i} was {r}");
            }
        }
    }

    /// A long decay is still monotonic toward zero — it just takes as long as
    /// it was asked to. The claim above is about arrival, not about level.
    #[test]
    fn a_long_decay_keeps_falling_rather_than_settling() {
        let params = HyperglareParams {
            color: 1.0,
            bank: BankParams {
                decay_t60_s: 4.0,
                ..BankParams::default()
            },
            ..HyperglareParams::default()
        };
        let mut processor = Processor::new(params, SR);
        processor.apply_params(&params, &[55.0]);
        for i in 0..(SR as usize / 2) {
            processor.process_frame(tone(i, 55.0), tone(i, 55.0));
        }
        let mut peaks = Vec::new();
        for _ in 0..6 {
            let mut peak = 0.0f32;
            for _ in 0..(SR as usize) {
                let (l, _) = processor.process_frame(0.0, 0.0);
                peak = peak.max(l.abs());
            }
            peaks.push(peak);
        }
        for pair in peaks.windows(2) {
            assert!(
                pair[1] < pair[0] * 0.6,
                "each second of silence should fall well below the last: {peaks:?}"
            );
        }
    }

    /// Zero colour is the input, bit for bit — below the limiter's threshold.
    ///
    /// The qualification is not a hedge. Bounding the output at full scale
    /// means bending somewhere below it, so "transparent" can only ever mean
    /// "transparent for material that is gain-staged". A peak above the
    /// threshold is changed by the limiter and by nothing else, which the
    /// second half checks.
    #[test]
    fn zero_colour_passes_the_input_through_below_the_limiter() {
        let params = HyperglareParams {
            color: 0.0,
            ..HyperglareParams::default()
        };
        let mut processor = Processor::new(params, SR);
        processor.apply_params(&params, &[55.0]);
        let threshold = db_to_amp(LIMIT_THRESHOLD_DB);

        #[allow(clippy::float_cmp)]
        for i in 0..2_400 {
            let (x, y) = (tone(i, 55.0) * 0.5, tone(i, 220.0) * 0.5);
            assert!(x.abs() < threshold && y.abs() < threshold);
            assert_eq!(processor.process_frame(x, y), (x, y));
        }

        // Above it, the only thing between input and output is the limiter.
        #[allow(clippy::float_cmp)]
        for x in [0.95f32, -0.95, 1.0, -1.0] {
            assert!(x.abs() > threshold);
            assert_eq!(
                processor.process_frame(x, x),
                (soft_limit(x), soft_limit(x))
            );
        }
    }

    /// A block is a loop over frames, so a caller cannot change the output by
    /// changing how it chunks its buffers (`contracts.md` §3).
    #[test]
    fn output_does_not_depend_on_block_size() {
        let params = HyperglareParams {
            sear: 0.5,
            ..HyperglareParams::default()
        };
        let render = |chunk: usize| {
            let mut processor = Processor::new(params, SR);
            processor.apply_params(&params, &[55.0, 82.4]);
            let mut l: Vec<f32> = (0..4_800).map(|i| tone(i, 55.0)).collect();
            let mut r = l.clone();
            for (a, b) in l.chunks_mut(chunk).zip(r.chunks_mut(chunk)) {
                processor.process(a, b);
            }
            (l, r)
        };
        assert_eq!(render(1), render(4_800));
        assert_eq!(render(64), render(4_800));
        assert_eq!(render(37), render(4_800));
    }

    /// The two placements are different sounds — which is why the choice is a
    /// parameter — and both are stereo at the output.
    #[test]
    fn the_two_sear_placements_differ_and_both_produce_stereo() {
        let base = HyperglareParams {
            sear: 0.8,
            color: 1.0,
            width: 1.0,
            ..HyperglareParams::default()
        };
        let render = |placement: SearPlacement| {
            let params = HyperglareParams {
                sear_placement: placement,
                ..base
            };
            let mut processor = Processor::new(params, SR);
            processor.apply_params(&params, &[55.0, 82.4, 110.0]);
            let mut l: Vec<f32> = (0..9_600).map(|i| tone(i, 55.0)).collect();
            let mut r = l.clone();
            processor.process(&mut l, &mut r);
            (l, r)
        };
        let (al, ar) = render(SearPlacement::AfterSum);
        let (bl, br) = render(SearPlacement::BeforeSplit);

        // Shaping the sum leaves nothing to separate, so its wet path is mono;
        // the dry is identical in both channels here, so the output is too.
        #[allow(clippy::float_cmp)]
        {
            assert_eq!(al, ar, "shaping the sum should give a mono wet path");
        }
        let spread: f32 = bl.iter().zip(&br).map(|(a, b)| (a - b).abs()).sum();
        assert!(spread > 1.0, "splitting first should give width: {spread}");

        let difference: f32 = al.iter().zip(&bl).map(|(a, b)| (a - b).abs()).sum();
        assert!(
            difference > 1.0,
            "the two placements should not be the same sound: {difference}"
        );
    }

    /// Nothing the processor can be asked for escapes into a host as a
    /// non-finite sample or as an unbounded one.
    #[test]
    fn output_stays_finite_and_bounded_under_every_extreme() {
        let params = HyperglareParams {
            bank: BankParams {
                decay_t60_s: 8.0,
                q_max: 20_000.0,
                tilt: 1.0,
                drift_cents: 40.0,
                grid: Grid {
                    geometry: Geometry::OctavePairs,
                    detune_cents_per_octave: 100.0,
                    ..Grid::default()
                },
                ..BankParams::default()
            },
            exciter: ExciterParams {
                drive: 1.0,
                noise_amount: 1.0,
            },
            sear: 1.0,
            width: 1.0,
            color: 2.0,
            input_gain_db: 24.0,
            output_gain_db: 24.0,
            sear_placement: SearPlacement::BeforeSplit,
        };
        let mut processor = Processor::new(params, SR);
        processor.apply_params(&params, &[27.5, 41.2, 55.0, 82.4, 110.0, 164.8]);
        let mut peak = 0.0f32;
        for i in 0..(SR as usize * 2) {
            let x = if i % 240 < 16 { 1.0 } else { tone(i, 55.0) };
            let (l, r) = processor.process_frame(x, -x);
            assert!(l.is_finite() && r.is_finite(), "sample {i}: {l}, {r}");
            peak = peak.max(l.abs()).max(r.abs());
        }
        assert!(peak <= 1.0, "the limiter let {peak} through");
    }

    /// A non-finite input does not escape, and does not leave the processor
    /// permanently poisoned once real audio returns.
    #[test]
    fn a_non_finite_input_is_contained() {
        let params = HyperglareParams::default();
        let mut processor = Processor::new(params, SR);
        processor.apply_params(&params, &[55.0]);
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let (l, r) = processor.process_frame(bad, bad);
            assert!(l.is_finite() && r.is_finite(), "{bad} escaped as {l}, {r}");
        }
        processor.reset_state();
        let (l, r) = processor.process_frame(0.5, 0.5);
        assert!(
            l.is_finite() && r.is_finite(),
            "reset did not recover: {l}, {r}"
        );
    }

    /// The limiter is out of the path of an ordinary setting.
    #[test]
    fn the_limiter_is_transparent_below_its_threshold() {
        let threshold = db_to_amp(LIMIT_THRESHOLD_DB);
        #[allow(clippy::float_cmp)]
        for x in [0.0f32, 0.1, -0.25, 0.5] {
            assert!(x.abs() < threshold);
            assert_eq!(soft_limit(x), x);
        }
        assert!(soft_limit(4.0) < 1.0);
        assert!(soft_limit(-4.0) > -1.0);
        assert!(soft_limit(1e9).is_finite());
    }

    /// Colour is a crossfade below one and drives the wet above it.
    #[test]
    fn colour_crossfades_then_drives() {
        #[allow(clippy::float_cmp)]
        {
            assert_eq!(mix_gains(0.0), (1.0, 0.0));
            assert_eq!(mix_gains(1.0), (0.0, 1.0));
            assert_eq!(mix_gains(2.0), (0.0, 2.0));
            assert_eq!(mix_gains(9.0), (0.0, 2.0));
            assert_eq!(mix_gains(-1.0), (1.0, 0.0));
        }
        let (dry, wet) = mix_gains(0.25);
        assert!(
            (dry + wet - 1.0).abs() < 1e-6,
            "below one it is a crossfade"
        );
    }

    /// More colour is more effect, which is the one thing the two halves of
    /// the range share.
    #[test]
    fn more_colour_is_more_effect() {
        let level = |color: f32| {
            let params = HyperglareParams {
                color,
                ..HyperglareParams::default()
            };
            let mut processor = Processor::new(params, SR);
            processor.apply_params(&params, &[55.0, 82.4, 110.0]);
            let (l, _) = run(&mut processor, 3.0, 0.5);
            l
        };
        let dry_only = level(0.0);
        let mixed = level(1.0);
        let driven = level(2.0);
        assert!(driven > mixed, "{mixed} -> {driven}");
        let _ = dry_only;
    }
}
