//! Second-order filter building blocks (ADR 0001).
//!
//! The RBJ cookbook coefficients, a Direct Form I biquad, and the cascaded
//! pair that makes a 4th-order Linkwitz-Riley section.
//!
//! Effect-independent on purpose. `oxtt-dsp`'s crossover is the only caller
//! today and is specific to a 3-band OTT; these are not.

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

fn clamp_cutoff(cutoff_hz: f32, sample_rate: f32) -> f32 {
    let max_hz = (NYQUIST_RATIO * sample_rate).max(MIN_CUTOFF_HZ);
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
        self.y1 = y0;
        y0
    }

    /// Whether the delay line is free of NaN and infinities.
    #[must_use]
    pub const fn is_finite(&self) -> bool {
        self.x1.is_finite() && self.x2.is_finite() && self.y1.is_finite() && self.y2.is_finite()
    }
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
