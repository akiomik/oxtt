//! Second-order filter building blocks (ADR 0001).
//!
//! Two families, for two jobs:
//!
//! - The RBJ cookbook coefficients, a Direct Form I biquad, and the cascaded
//!   pair that makes a 4th-order Linkwitz-Riley section. A crossover: a fixed
//!   Butterworth Q, a cutoff that moves rarely, and both a low-pass and a
//!   high-pass branch.
//! - A topology-preserving (TPT) state-variable filter, [`Svf`], with a
//!   normalised band-pass output. A resonator: a high Q, a centre frequency
//!   that is modulated, and only the band-pass branch.
//!
//! **Direct Form I is the wrong shape for the second job**, which is why the
//! second exists. At a high Q and a low `f/fs` its coefficients are close
//! together and the state is the filter's own output history, so the rounding
//! error is largest exactly where the resonance is sharpest; and interpolating
//! the coefficients while a centre frequency moves is not guaranteed stable.
//! A TPT SVF holds its state as two integrator values, stays stable while its
//! coefficients move, and produces the band-pass branch directly rather than
//! as a difference of others.
//!
//! Effect-independent on purpose. `oxtt-dsp`'s crossover is the only caller of
//! the first family today and is specific to a 3-band OTT; neither family is.

use std::f32::consts::{FRAC_1_SQRT_2, PI};

/// Defensive floor for cutoff values fed into biquad coefficient computation.
/// Stricter than the lowest value docs/oxtt/contracts.md §1 allows (40 Hz); this
/// margin should never be hit in practice since callers only pass already-
/// validated frequencies.
const MIN_CUTOFF_HZ: f32 = 20.0;
/// Upper-bound coefficient for cutoff on the Nyquist side (docs/oxtt/contracts.md §1).
const NYQUIST_RATIO: f32 = 0.45;
/// Q value for each stage of an LR4 with a Butterworth characteristic.
const Q_BUTTERWORTH: f32 = FRAC_1_SQRT_2;

/// The highest centre or cutoff frequency the coefficients are computed for.
///
/// Both families clamp to this rather than to Nyquist, so a caller placing
/// filters itself — a resonator bank choosing where its partials go — can ask
/// where the ceiling is instead of discovering it as a pile-up on the clamp.
#[must_use]
pub fn max_centre_hz(sample_rate: f32) -> f32 {
    (NYQUIST_RATIO * sample_rate).max(MIN_CUTOFF_HZ)
}

fn clamp_cutoff(cutoff_hz: f32, sample_rate: f32) -> f32 {
    let max_hz = max_centre_hz(sample_rate);
    // `f32::clamp` asserts `min <= max`; `max_hz` is runtime-computed, so the
    // optimizer can't prove that bound and treats the assert as reachable
    // (breaks the no-panic proof on `OttProcessor::process`/`reset`, docs/oxtt/contracts.md
    // §6). `max` and `min` chained have the same behavior here (max_hz is
    // always >= MIN_CUTOFF_HZ by construction above) without the assert.
    cutoff_hz.max(MIN_CUTOFF_HZ).min(max_hz)
}

/// Second-order biquad coefficients from the RBJ cookbook formulas (`b0,b1,b2,a1,a2`, normalized by `a0`).
///
/// `cutoff_hz` is clamped into the range the coefficients stay well-behaved
/// over, so no input can produce a non-finite result.
#[must_use]
pub fn biquad_coeffs(cutoff_hz: f32, sample_rate: f32, high_pass: bool) -> [f32; 5] {
    let cutoff_hz = clamp_cutoff(cutoff_hz, sample_rate);
    let omega = 2.0 * PI * cutoff_hz / sample_rate;
    let (sin_w, cos_w) = omega.sin_cos();
    let alpha = sin_w / (2.0 * Q_BUTTERWORTH);

    let (b0, b1, b2) = if high_pass {
        (
            f32::midpoint(1.0, cos_w),
            -(1.0 + cos_w),
            f32::midpoint(1.0, cos_w),
        )
    } else {
        ((1.0 - cos_w) / 2.0, 1.0 - cos_w, (1.0 - cos_w) / 2.0)
    };
    let a0 = 1.0 + alpha;
    let a1 = -2.0 * cos_w;
    let a2 = 1.0 - alpha;

    [b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0]
}

/// A Direct Form I second-order biquad. Corresponds to one cascade stage.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Biquad {
    /// Replaces the coefficients, in `biquad_coeffs` order. Leaves state alone.
    pub const fn set_coeffs(&mut self, coeffs: [f32; 5]) {
        [self.b0, self.b1, self.b2, self.a1, self.a2] = coeffs;
    }

    /// Clears the delay line, leaving the coefficients in place.
    pub const fn reset_state(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }

    /// Runs one sample through and advances the state.
    #[inline]
    pub fn process(&mut self, x0: f32) -> f32 {
        let y0 = self.a2.mul_add(
            -self.y2,
            self.a1.mul_add(
                -self.y1,
                self.b2
                    .mul_add(self.x2, self.b0.mul_add(x0, self.b1 * self.x1)),
            ),
        );
        self.x2 = self.x1;
        self.x1 = x0;
        self.y2 = self.y1;
        // Flushed for the same reason [`Svf`]'s integrators are: a filter
        // decaying into silence approaches zero without arriving, and the
        // subnormal arithmetic on the way is neither free nor bounded in time.
        // Only the feedback state needs it — `x1`/`x2` are the caller's own
        // samples and stop being small when the caller does.
        self.y1 = flush(y0);
        y0
    }

    /// Whether the delay line is free of NaN and infinities.
    #[must_use]
    pub const fn is_finite(&self) -> bool {
        self.x1.is_finite() && self.x2.is_finite() && self.y1.is_finite() && self.y2.is_finite()
    }
}

/// Zero for anything smaller than [`DENORMAL_FLOOR`], and unchanged otherwise.
#[inline]
fn flush(x: f32) -> f32 {
    if x.abs() < DENORMAL_FLOOR { 0.0 } else { x }
}

/// A 4th-order Linkwitz-Riley filter made of two cascaded second-order Butterworth biquads at the same cutoff.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Lr4 {
    stage1: Biquad,
    stage2: Biquad,
}

impl Lr4 {
    /// Points both stages at the same cutoff, low-pass or high-pass.
    pub fn set_cutoff(&mut self, cutoff_hz: f32, sample_rate: f32, high_pass: bool) {
        let coeffs = biquad_coeffs(cutoff_hz, sample_rate, high_pass);
        self.stage1.set_coeffs(coeffs);
        self.stage2.set_coeffs(coeffs);
    }

    /// Clears both stages' delay lines, leaving the coefficients in place.
    pub const fn reset_state(&mut self) {
        self.stage1.reset_state();
        self.stage2.reset_state();
    }

    /// Runs one sample through both stages and advances their state.
    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        self.stage2.process(self.stage1.process(x))
    }

    /// Whether both stages' delay lines are free of NaN and infinities.
    #[must_use]
    pub const fn is_finite(&self) -> bool {
        self.stage1.is_finite() && self.stage2.is_finite()
    }
}

/// Coefficients for one [`Svf`], held apart from its state on purpose.
///
/// The separation is what lets a caller recompute coefficients at a control
/// rate — a few hundred hertz — while running the filter at the sample rate.
/// A resonator bank whose centre frequencies all move together pays one
/// [`tan`](f32::tan) per filter per *update*, not per sample; the alternative
/// is two orders of magnitude more transcendental calls for a result that no
/// listener can distinguish.
///
/// `q` is the resonance, not a bandwidth: the band-pass branch's −3 dB width
/// is `centre_hz / q`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SvfCoeffs {
    a1: f32,
    a2: f32,
    a3: f32,
    /// `1/q`, kept because the normalised band-pass output is scaled by it.
    k: f32,
}

/// Lowest Q [`SvfCoeffs::new`] computes for.
///
/// Below `0.5` the analogue prototype is overdamped and has no resonant peak,
/// so a "band-pass" there is not what any caller means by one. Clamping rather
/// than refusing keeps this off the panic path, the same trade the centre
/// frequency's own clamp makes.
pub const MIN_Q: f32 = 0.5;

impl SvfCoeffs {
    /// Derives the coefficients for a centre frequency and a Q.
    ///
    /// `centre_hz` is clamped the same way [`biquad_coeffs`] clamps its
    /// cutoff, so no input can produce a non-finite result, and `q` is floored
    /// at [`MIN_Q`].
    #[must_use]
    pub fn new(centre_hz: f32, sample_rate: f32, q: f32) -> Self {
        let centre_hz = clamp_cutoff(centre_hz, sample_rate);
        // The prewarped integrator gain: the bilinear transform maps the
        // analogue frequency onto the discrete one exactly at this point.
        let g = (PI * centre_hz / sample_rate).tan();
        let k = 1.0 / q.max(MIN_Q);
        let a1 = 1.0 / g.mul_add(g + k, 1.0);
        let a2 = g * a1;
        let a3 = g * a2;
        Self { a1, a2, a3, k }
    }

    /// The Q these coefficients were built for, recovered from `k`.
    #[must_use]
    pub fn q(&self) -> f32 {
        1.0 / self.k
    }
}

/// State below this magnitude is flushed to zero.
///
/// A high-Q resonator decaying into silence is where denormals collect: the
/// integrators approach zero without arriving, and on an out-of-order core the
/// subnormal arithmetic that follows is not free. Flushing keeps that off an
/// audio thread, and it turns the asymptote into an arrival so a caller can
/// say when a tail has actually finished.
///
/// Not at `f32`'s denormal boundary but well above it, because the boundary is
/// not where the trouble starts. A decaying second-order state is a difference
/// of two nearly equal numbers, so below roughly `1e-20` the decrement the
/// coefficients describe stops surviving the rounding and the state *crawls*
/// instead of arriving — measured going from a factor of 0.002 per 50 ms to a
/// factor of 0.87. Flushing above that region is what makes the arrival real.
///
/// Four hundred decibels below full scale, so this is a numerical decision
/// rather than an audibility one: no effect's idea of silence is anywhere near
/// it, which is what keeps the constant appropriate to a shared primitive.
const DENORMAL_FLOOR: f32 = 1e-20;

/// A topology-preserving state-variable filter, band-pass branch.
///
/// The state is the two integrators, not the output history, which is what
/// makes it safe to move [`SvfCoeffs`] underneath a running filter.
///
/// # The output is normalised
///
/// [`process_bandpass`](Self::process_bandpass) returns **unit gain at the
/// centre frequency**, for every Q. The filter's own band-pass branch peaks at
/// `Q`, so at `T60 = 4 s` and 5 kHz — a Q of about 9100 — it would peak near
/// +79 dB, and a bank of these would be dominated by whichever partial happens
/// to sit highest. The `1/Q` is applied here rather than left to the caller so
/// that a spectral envelope applied on top of this means what it says.
///
/// The unnormalised branch is available as
/// [`process_bandpass_raw`](Self::process_bandpass_raw) for a caller that
/// wants the resonant gain itself.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Svf {
    ic1eq: f32,
    ic2eq: f32,
}

impl Svf {
    /// A filter with both integrators at rest.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            ic1eq: 0.0,
            ic2eq: 0.0,
        }
    }

    /// Clears both integrators. Coefficients live in [`SvfCoeffs`] and are
    /// unaffected.
    pub const fn reset_state(&mut self) {
        self.ic1eq = 0.0;
        self.ic2eq = 0.0;
    }

    /// Whether both integrators are free of NaN and infinities.
    #[must_use]
    pub const fn is_finite(&self) -> bool {
        self.ic1eq.is_finite() && self.ic2eq.is_finite()
    }

    /// The band-pass branch before normalisation. Peaks at `Q`.
    #[inline]
    pub fn process_bandpass_raw(&mut self, coeffs: &SvfCoeffs, x: f32) -> f32 {
        let v3 = x - self.ic2eq;
        let v1 = coeffs.a1.mul_add(self.ic1eq, coeffs.a2 * v3);
        let v2 = coeffs
            .a3
            .mul_add(v3, coeffs.a2.mul_add(self.ic1eq, self.ic2eq));
        self.ic1eq = flush(2.0f32.mul_add(v1, -self.ic1eq));
        self.ic2eq = flush(2.0f32.mul_add(v2, -self.ic2eq));
        v1
    }

    /// The band-pass branch at unit gain on the centre frequency, whatever the Q.
    #[inline]
    pub fn process_bandpass(&mut self, coeffs: &SvfCoeffs, x: f32) -> f32 {
        coeffs.k * self.process_bandpass_raw(coeffs, x)
    }
}

#[cfg(test)]
// Test-only arithmetic on sample indices: the counts stay well inside f32's
// exact integer range, the run lengths are positive by construction, and
// readability beats `mul_add` in a signal generator.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::arithmetic_side_effects,
    clippy::suboptimal_flops
)]
mod svf_tests {
    use super::*;

    const SR: f32 = 48_000.0;

    /// Drives a fresh filter with a sine at `hz` and returns the peak of the
    /// last tenth of the run.
    ///
    /// The run length is derived rather than fixed, because a resonator's
    /// approach to steady state is its own decay: the envelope's time constant
    /// is `q / (pi * centre)` seconds, so a Q of 5000 at 1 kHz needs about 1.6
    /// seconds per time constant and a fixed two-second run would measure
    /// `1 - e^-1.26 = 0.72` and call the filter wrong. Eight time constants
    /// puts the remaining error below a thousandth.
    fn steady_peak(hz: f32, coeffs: &SvfCoeffs, normalised: bool) -> f32 {
        let mut f = Svf::new();
        let tau_s = coeffs.q() / (PI * hz);
        let n = (SR * (8.0 * tau_s).max(0.5)) as usize;
        let mut peak = 0.0f32;
        for i in 0..n {
            let x = (2.0 * PI * hz * i as f32 / SR).sin();
            let y = if normalised {
                f.process_bandpass(coeffs, x)
            } else {
                f.process_bandpass_raw(coeffs, x)
            };
            if i > n - n / 10 {
                peak = peak.max(y.abs());
            }
        }
        peak
    }

    /// The property a resonator bank depends on: whatever the Q, a sine on the
    /// centre frequency comes out at the amplitude it went in at.
    #[test]
    fn normalised_bandpass_has_unit_gain_at_centre_for_every_q() {
        for q in [0.7_f32, 5.0, 50.0, 500.0, 5000.0] {
            let coeffs = SvfCoeffs::new(1000.0, SR, q);
            let peak = steady_peak(1000.0, &coeffs, true);
            assert!(
                (peak - 1.0).abs() < 0.02,
                "q={q}: peak {peak} should be unity"
            );
        }
    }

    /// The same run unnormalised peaks at Q — which is the reason the
    /// normalisation exists.
    #[test]
    fn raw_bandpass_peaks_at_q() {
        for q in [5.0_f32, 50.0, 500.0] {
            let coeffs = SvfCoeffs::new(1000.0, SR, q);
            let peak = steady_peak(1000.0, &coeffs, false);
            assert!(
                (peak / q - 1.0).abs() < 0.02,
                "q={q}: peak {peak} should be about q"
            );
        }
    }

    /// −3 dB half a bandwidth off centre, which is what makes `q` mean
    /// "centre over width" to a caller choosing one from a decay time.
    #[test]
    fn bandwidth_is_centre_over_q() {
        let (centre, q) = (1000.0_f32, 20.0);
        let coeffs = SvfCoeffs::new(centre, SR, q);
        let half_bw = centre / q / 2.0;
        for edge in [centre - half_bw, centre + half_bw] {
            let peak = steady_peak(edge, &coeffs, true);
            let db = 20.0 * peak.log10();
            assert!(
                (db + 3.0).abs() < 0.5,
                "edge {edge}: {db} dB should be about -3 dB"
            );
        }
    }

    /// A Q of several thousand at a low `f/fs` is the case Direct Form I is
    /// unreliable in, and the case a long decay puts every low partial into.
    #[test]
    fn stays_finite_and_bounded_at_extreme_q() {
        let coeffs = SvfCoeffs::new(50.0, SR, 9100.0);
        let mut f = Svf::new();
        let mut peak = 0.0f32;
        for i in 0..(SR as usize * 4) {
            let x = if i < 480 { 1.0 } else { 0.0 };
            peak = peak.max(f.process_bandpass(&coeffs, x).abs());
        }
        assert!(f.is_finite(), "state went non-finite");
        assert!(peak.is_finite() && peak < 10.0, "peaked at {peak}");
    }

    /// Moving the coefficients under a ringing filter must not blow it up.
    /// This is the whole reason for choosing TPT over Direct Form I.
    #[test]
    fn sweeping_the_centre_frequency_under_a_ringing_filter_stays_bounded() {
        let mut f = Svf::new();
        let mut peak = 0.0f32;
        let n = (SR as usize) * 2;
        for i in 0..n {
            let hz = 60.0 + (i as f32 / n as f32) * 8000.0;
            let coeffs = SvfCoeffs::new(hz, SR, 400.0);
            let x = if i % 4800 < 48 { 1.0 } else { 0.0 };
            peak = peak.max(f.process_bandpass(&coeffs, x).abs());
        }
        assert!(f.is_finite(), "state went non-finite during the sweep");
        assert!(peak < 10.0, "sweep peaked at {peak}");
    }

    #[test]
    fn q_is_floored_rather_than_refused() {
        let clamped = SvfCoeffs::new(1000.0, SR, 0.0);
        assert!((clamped.q() - MIN_Q).abs() < 1e-6);
        assert_eq!(clamped, SvfCoeffs::new(1000.0, SR, MIN_Q));
    }

    #[test]
    fn reset_clears_the_integrators_but_not_the_coefficients() {
        let coeffs = SvfCoeffs::new(1000.0, SR, 50.0);
        let mut f = Svf::new();
        for _ in 0..100 {
            f.process_bandpass(&coeffs, 1.0);
        }
        assert_ne!(f, Svf::new());
        f.reset_state();
        assert_eq!(f, Svf::new());
        assert!((coeffs.q() - 50.0).abs() < 1e-3);
    }
}
