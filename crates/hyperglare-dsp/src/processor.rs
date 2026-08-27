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
//!
//! **What that costs depends on [`HyperglareParams::color`], and at the top of
//! its range it costs the stereo image entirely.** Below one the dry survives
//! and carries the width it arrived with; at one and above the dry is gone, so
//! under `AfterSum` the output is fully mono however wide the input was.
//! Measured on a stereo source: a width of 0.56 in, and 0.00 out.
//!
//! That is a real cost rather than a curiosity, and it is one of the things
//! the comparison has to weigh — `BeforeSplit` keeps the image, and paying for
//! a decorrelator would let `AfterSum` keep it too. Which is worth it is not
//! decidable from here.

use effectkit::decibels::db_to_amp;
use effectkit::filter::{Biquad, biquad_coeffs};

use crate::bank::{BankParams, ResonatorBank};
use crate::exciter::{Exciter, ExciterCoeffs, ExciterParams, shape};
use crate::wet_match::{WetMatch, WetMatchCoeffs};

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
    /// Only reaches the output under [`SearPlacement::BeforeSplit`]. Under
    /// `AfterSum` the wet is mono, so whatever width the output has is the
    /// dry's — and at a `color` of one or more there is no dry.
    pub width: f32,
    /// How much of the wet's level difference from the dry to remove.
    ///
    /// `1.0` makes [`color`](Self::color) a real crossfade; `0.0` leaves the
    /// resonators at whatever level the input happened to excite them to. See
    /// [`crate::wet_match`] for why the default is not zero.
    pub wet_match: f32,
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
            wet_match: 1.0,
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
    wet_match: WetMatch,
    wet_match_coeffs: WetMatchCoeffs,
    params: HyperglareParams,
    /// The chord [`apply_params`](Self::apply_params) was last given.
    ///
    /// Kept so that a sample-rate change can retune from it. Sized to the
    /// bank's capacity because that is already the point past which
    /// [`ResonatorBank::retune`] stops reading, so this remembers everything
    /// a retune could have used and nothing more.
    notes_hz: [f32; N],
    note_count: usize,
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
            wet_match: WetMatch::new(),
            wet_match_coeffs: WetMatchCoeffs::new(sample_rate),
            params,
            notes_hz: [0.0; N],
            note_count: 0,
            sample_rate,
        };
        processor.set_sample_rate(sample_rate);
        processor
    }

    /// Rebuilds everything that depends on the sample rate, and clears state.
    ///
    /// **The bank included.** Its coefficients came from a `tan` of the old
    /// rate, so leaving them detunes the whole chord by the ratio of the two
    /// rates; its filter state is a tail counted in the old rate's samples, so
    /// leaving that rings the old tuning through the change. Both are why the
    /// chord is kept in [`notes_hz`](Self#structfield.notes_hz) — there is
    /// nothing else here to retune from.
    ///
    /// A processor that has never been given a chord retunes to no notes,
    /// which is what it already had.
    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.exciter_coeffs = ExciterCoeffs::new(sample_rate);
        self.wet_match_coeffs = WetMatchCoeffs::new(sample_rate);
        self.wet_match.reset();
        let coeffs = biquad_coeffs(ANTI_ALIAS_RATIO * sample_rate, sample_rate, false);
        for stage in &mut self.anti_alias {
            stage.set_coeffs(coeffs);
            stage.reset_state();
        }
        self.exciter.reset();
        // The tails belong to the old rate; the tuning is rebuilt for the new
        // one. Copied out first because both live behind the same `&mut self`.
        self.bank.reset_state();
        let (notes, count, bank) = (self.notes_hz, self.note_count, self.params.bank);
        self.bank
            .retune(notes.get(..count).unwrap_or(&notes), &bank, sample_rate);
    }

    /// Applies a new setting. Control rate, not sample rate.
    ///
    /// The bank is retuned only when something it depends on has moved, so a
    /// knob that is not a bank knob costs nothing. `color`, the gains, the
    /// sear and the wet match are all in that group; everything inside
    /// [`BankParams`] and the chord itself are not.
    ///
    /// A note that is `NaN` compares unequal to itself, so a chord carrying
    /// one retunes on every call. That is the safe direction to be wrong in,
    /// and it costs nothing a caller passing `NaN` was going to get anyway.
    pub fn apply_params(&mut self, params: &HyperglareParams, notes_hz: &[f32]) {
        let count = notes_hz.len().min(N);
        let known = self.notes_hz.get(..count).unwrap_or(&[]);
        let incoming = notes_hz.get(..count).unwrap_or(&[]);
        let retune =
            params.bank != self.params.bank || count != self.note_count || known != incoming;
        self.params = *params;
        if retune {
            for (slot, hz) in self.notes_hz.iter_mut().zip(notes_hz) {
                *slot = *hz;
            }
            // Past the chord, for the reason `ResonatorBank::retune` clears
            // its own unused slots: `note_count` stops anything from reading
            // here, so a leftover note cannot be heard, and leaving it would
            // make two processors that behave identically compare unequal.
            for slot in self.notes_hz.iter_mut().skip(count) {
                *slot = 0.0;
            }
            self.note_count = count;
            self.bank.retune(notes_hz, &params.bank, self.sample_rate);
        }
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
        self.wet_match.reset();
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

        // Scaled to the dry before the mix, so that `color` crossfades between
        // two things of comparable size rather than between a signal and one
        // twenty decibels under it.
        //
        // One correction for the pair, measured on the mids and applied to
        // both. A matcher per channel would read the dry's own left/right
        // balance and print it onto a wet that has no image of its own, which
        // is a width manufactured from a level rather than one that is there.
        let wet_mid = f32::midpoint(wet_l, wet_r);
        let correction =
            self.wet_match
                .correction(mid, wet_mid, self.params.wet_match, &self.wet_match_coeffs);
        wet_l *= correction;
        wet_r *= correction;

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

    /// A chord that loses notes leaves the bank as if it had always been
    /// small.
    ///
    /// The same principle the rate test rests on, reached by the path a host
    /// actually takes. `retune` stops writing at the chord's length, so a
    /// shrink leaves the slots above it holding the tail of a chord that is no
    /// longer sounding. Nothing reads them — both process paths stop at
    /// `active` — so the two processors below are bit-identical forever, and
    /// an equality that noticed the difference would be reporting history
    /// rather than behaviour.
    ///
    /// **The silence is load-bearing.** Without it the surviving resonators
    /// are still ringing the old chord, which is deliberate and would make
    /// these two differ for a reason that has nothing to do with the point.
    ///
    /// The claim is about the bank rather than the whole processor, because
    /// the processor holds one piece of history that is not inaudible: the
    /// noise generator's position. Two of those are silent together only
    /// while the gate is shut.
    #[test]
    fn a_chord_that_loses_notes_leaves_no_trace_above_it() {
        let params = HyperglareParams {
            bank: BankParams {
                decay_t60_s: 0.05,
                ..BankParams::default()
            },
            ..HyperglareParams::default()
        };
        let big = [55.0f32, 61.7, 65.4, 82.4, 110.0];
        let small = [55.0f32];

        let mut shrunk = Processor::new(params, SR);
        shrunk.apply_params(&params, &big);
        for i in 0..(SR as usize) {
            let x = tone(i, 55.0) * 0.5;
            shrunk.process_frame(x, x);
        }
        shrunk.apply_params(&params, &small);
        // Long enough for the survivors to cross the filter's denormal floor,
        // which is where "decaying" becomes "silent".
        for _ in 0..(2.0 * SR) as usize {
            shrunk.process_frame(0.0, 0.0);
        }

        let mut fresh = Processor::new(params, SR);
        fresh.apply_params(&params, &small);

        for i in 0..(SR as usize) {
            let a = shrunk.process_frame(0.0, 0.0);
            let b = fresh.process_frame(0.0, 0.0);
            assert!(a == b, "frame {i} differs: {a:?} against {b:?}");
        }
        assert!(
            shrunk.bank == fresh.bank,
            "the two sound the same forever and their banks still compare unequal"
        );
        // **Not the whole processor**, and the difference is named rather
        // than assumed. The exciter's noise generator has been advanced by
        // everything the shrunk one played, and two generators at different
        // positions really are different: they are silent together only while
        // the gate is shut, and produce different noise the moment it opens.
        // That is a difference which can be heard, and it is the line the
        // bank's own normalisation draws.
        //
        // Asserted on the exciter and not on the processor, because the
        // processor has more than one thing left in it — the wet matcher's
        // followers approach rest without arriving — and `shrunk != fresh`
        // would go on passing on the strength of the other one long after this
        // paragraph stopped being true.
        assert!(
            shrunk.exciter != fresh.exciter,
            "the noise generator did not advance, so this test says nothing \
             about why two processors differ"
        );
    }

    /// A resonator coming back into use starts from silence, not from the
    /// chord that stopped playing.
    ///
    /// This is the audible half of what `retune` clears above the chord, and
    /// it has exactly one enforcement point. Growing a chord back reaches
    /// slots a previous shrink left behind; if their tails were still in
    /// there, they would ring into an input that is silent.
    #[test]
    fn a_chord_that_grows_back_does_not_resurrect_the_old_tails() {
        let params = HyperglareParams {
            bank: BankParams {
                decay_t60_s: 0.05,
                ..BankParams::default()
            },
            ..HyperglareParams::default()
        };
        let big = [55.0f32, 61.7, 65.4, 82.4, 110.0];
        let small = [55.0f32];

        let mut processor = Processor::new(params, SR);
        processor.apply_params(&params, &big);
        for i in 0..(SR as usize) {
            let x = tone(i, 55.0) * 0.5;
            processor.process_frame(x, x);
        }
        let loud = processor.active();

        processor.apply_params(&params, &small);
        // Silence has to actually arrive before the rest of this means
        // anything: a gate still trickling would ring the resonators that grow
        // back whatever state they are in, which is the other way this test
        // could fail. So the wait is measured rather than assumed.
        let wait = (2.0 * SR) as usize;
        let mut arrived = None;
        for i in 0..wait {
            let (left, right) = processor.process_frame(0.0, 0.0);
            if arrived.is_none() && left == 0.0 && right == 0.0 {
                arrived = Some(i);
            }
        }
        let arrived = arrived.expect("the small chord never reached silence");
        assert!(
            arrived * 5 < wait * 4,
            "silence arrived at frame {arrived} of {wait}, which is close \
             enough to the end that this test is about the wait rather than \
             about the tails"
        );

        // Back to the chord that was ringing, into an input that is not.
        processor.apply_params(&params, &big);
        assert_eq!(processor.active(), loud, "the chord did not grow back");

        for i in 0..(SR as usize) {
            let (left, right) = processor.process_frame(0.0, 0.0);
            assert!(
                left == 0.0 && right == 0.0,
                "frame {i} rang at {left}/{right} into silence. The gate had \
                 arrived at frame {arrived}, so this is a returning resonator \
                 keeping a tail from before the chord shrank"
            );
        }
    }

    /// A rate change rebuilds the bank, and not only the parts around it.
    ///
    /// The strongest statement available: a processor moved to a new rate is
    /// **indistinguishable** from one built at that rate and given the same
    /// chord. Anything left behind at the old rate — a coefficient, a tail, a
    /// forgotten note — makes the two differ.
    ///
    /// **3 kHz is in the list on purpose.** At the ordinary rates the grid
    /// generates the same number of points whatever the rate, because the
    /// band's ceiling is well below every Nyquist involved — so a test that
    /// stopped at 96 kHz would pass without ever exercising a retune that
    /// *shrinks*, which is the case that catches a stale coefficient past the
    /// end of the chord. Raising the band's ceiling would put 96 kHz in the
    /// same class; 3 kHz is here so the property does not quietly stop being
    /// tested when it does.
    #[test]
    fn changing_the_sample_rate_retunes_the_bank() {
        let params = HyperglareParams::default();
        let notes = [55.0f32, 82.4, 110.0];

        for rate in [96_000.0f32, 44_100.0, 3_000.0] {
            let mut moved = Processor::new(params, SR);
            moved.apply_params(&params, &notes);
            moved.set_sample_rate(rate);

            let mut built = Processor::new(params, rate);
            built.apply_params(&params, &notes);

            assert_eq!(
                moved.active(),
                built.active(),
                "at {rate} Hz the moved processor sounds {} resonators, the \
                 freshly built one {}",
                moved.active(),
                built.active()
            );
            assert!(
                moved == built,
                "a processor moved to {rate} Hz still differs from one built there"
            );
        }
    }

    /// The claim `apply_params` makes about cost.
    ///
    /// A knob outside `BankParams` must leave the bank untouched — not
    /// "retuned to the same thing", untouched — because retuning runs a `tan`
    /// and a `powf` per resonator and the doc promises it does not happen.
    #[test]
    fn a_knob_outside_the_bank_does_not_retune_it() {
        let params = HyperglareParams::default();
        let notes = [55.0f32, 82.4, 110.0];
        let mut processor = Processor::new(params, SR);
        processor.apply_params(&params, &notes);
        let tuned = processor.bank;

        let mut quieter = params;
        quieter.color = 0.25;
        quieter.output_gain_db = -6.0;
        processor.apply_params(&quieter, &notes);
        assert!(processor.bank == tuned, "a mix knob retuned the bank");

        // And the guard is not simply "never": the things it does watch still
        // get through, or the chord could never change.
        let mut higher = quieter;
        higher.bank.decay_t60_s = params.bank.decay_t60_s * 4.0;
        processor.apply_params(&higher, &notes);
        assert!(processor.bank != tuned, "a decay change did not retune");

        processor.apply_params(&higher, &notes);
        let settled = processor.bank;
        // Not an octave of the first chord. Under `Geometry::Octaves` a
        // transposition by an octave generates the *same* set of frequencies,
        // so a bank that never noticed the change would pass anyway.
        processor.apply_params(&higher, &[61.7, 92.5, 123.5]);
        assert!(processor.bank != settled, "a chord change did not retune");
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

    /// The two placements are different sounds, which is why the choice is a
    /// parameter rather than a decision.
    #[test]
    fn the_two_sear_placements_are_different_sounds() {
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
        let (al, _) = render(SearPlacement::AfterSum);
        let (bl, _) = render(SearPlacement::BeforeSplit);
        let difference: f32 = al.iter().zip(&bl).map(|(a, b)| (a - b).abs()).sum();
        assert!(
            difference > 1.0,
            "the two placements should not be the same sound: {difference}"
        );
    }

    /// What the mono wet path actually costs, and where.
    ///
    /// Below a colour of one the dry survives and carries the width it arrived
    /// with. At one and above there is no dry, so under `AfterSum` the output
    /// is mono however wide the input was — which on a stereo source is the
    /// whole image. `BeforeSplit` keeps it.
    ///
    /// Driven with a genuinely stereo input, because a mono one cannot tell
    /// the two apart and an earlier version of this test used one.
    #[test]
    fn the_mono_wet_path_costs_the_image_only_when_the_dry_is_gone() {
        let width_of = |placement: SearPlacement, color: f32| {
            let params = HyperglareParams {
                sear_placement: placement,
                color,
                width: 1.0,
                ..HyperglareParams::default()
            };
            let mut processor = Processor::new(params, SR);
            processor.apply_params(&params, &[55.0, 82.4, 110.0]);
            // Different content in each channel: an octave apart.
            let mut l: Vec<f32> = (0..9_600).map(|i| tone(i, 55.0)).collect();
            let mut r: Vec<f32> = (0..9_600).map(|i| tone(i, 110.0)).collect();
            processor.process(&mut l, &mut r);
            let side: f32 = l.iter().zip(&r).map(|(a, b)| (a - b).abs()).sum();
            let mid: f32 = l.iter().map(|a| a.abs()).sum();
            side / mid.max(1e-9)
        };

        // The dry is still there, so the image is.
        assert!(
            width_of(SearPlacement::AfterSum, 0.5) > 0.2,
            "a half-open colour should keep the dry's width"
        );
        // It is not, so it is not.
        assert!(
            width_of(SearPlacement::AfterSum, 1.0) < 1e-6,
            "shaping the sum with no dry left should give a mono output"
        );
        // And the other placement keeps it either way.
        assert!(
            width_of(SearPlacement::BeforeSplit, 1.0) > 0.2,
            "splitting before the shaper should keep an image without the dry"
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
            wet_match: 1.0,
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
