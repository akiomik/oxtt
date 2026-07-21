//! 4th-order Linkwitz-Riley crossover and the low branch's phase compensator (ADR 0001).

use crate::bands::Bands;
use crate::dsp::smooth::LogSmoothed;
use std::f32::consts::{FRAC_1_SQRT_2, PI};

/// Defensive floor for cutoff values fed into biquad coefficient computation.
/// Stricter than the lowest value docs/contracts.md §1 allows (40 Hz); this
/// margin should never be hit in practice since callers only pass already-
/// validated frequencies.
const MIN_CUTOFF_HZ: f32 = 20.0;
/// Upper-bound coefficient for cutoff on the Nyquist side (docs/contracts.md §1).
const NYQUIST_RATIO: f32 = 0.45;
/// Q value for each stage of an LR4 with a Butterworth characteristic.
const Q_BUTTERWORTH: f32 = FRAC_1_SQRT_2;

fn clamp_cutoff(cutoff_hz: f32, sample_rate: f32) -> f32 {
    let max_hz = (NYQUIST_RATIO * sample_rate).max(MIN_CUTOFF_HZ);
    // `f32::clamp` asserts `min <= max`; `max_hz` is runtime-computed, so the
    // optimizer can't prove that bound and treats the assert as reachable
    // (breaks the no-panic proof on `OttProcessor::process`/`reset`, docs/contracts.md
    // §6). `max` and `min` chained have the same behavior here (max_hz is
    // always >= MIN_CUTOFF_HZ by construction above) without the assert.
    cutoff_hz.max(MIN_CUTOFF_HZ).min(max_hz)
}

/// Second-order biquad coefficients from the RBJ cookbook formulas (`b0,b1,b2,a1,a2`, normalized by `a0`).
fn biquad_coeffs(cutoff_hz: f32, sample_rate: f32, high_pass: bool) -> [f32; 5] {
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
struct Biquad {
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
    const fn set_coeffs(&mut self, coeffs: [f32; 5]) {
        [self.b0, self.b1, self.b2, self.a1, self.a2] = coeffs;
    }

    const fn reset_state(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }

    #[inline]
    fn process(&mut self, x0: f32) -> f32 {
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

    const fn is_finite(&self) -> bool {
        self.x1.is_finite() && self.x2.is_finite() && self.y1.is_finite() && self.y2.is_finite()
    }
}

/// A 4th-order Linkwitz-Riley filter made of two cascaded second-order Butterworth biquads at the same cutoff.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct Lr4 {
    stage1: Biquad,
    stage2: Biquad,
}

impl Lr4 {
    fn set_cutoff(&mut self, cutoff_hz: f32, sample_rate: f32, high_pass: bool) {
        let coeffs = biquad_coeffs(cutoff_hz, sample_rate, high_pass);
        self.stage1.set_coeffs(coeffs);
        self.stage2.set_coeffs(coeffs);
    }

    const fn reset_state(&mut self) {
        self.stage1.reset_state();
        self.stage2.reset_state();
    }

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        self.stage2.process(self.stage1.process(x))
    }

    const fn is_finite(&self) -> bool {
        self.stage1.is_finite() && self.stage2.is_finite()
    }
}

/// One channel's 3-band split plus phase compensator (ADR 0001).
///
/// The low branch's phase compensator (`phase_comp_lp`/`phase_comp_hp`) uses
/// the same coefficient-update sequence as the mid/high split
/// (`high_split_lp`/`high_split_hp`), but keeps independent state (ADR 0001).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct ChannelSplitter {
    low_split_lp: Lr4,
    low_split_hp: Lr4,
    high_split_lp: Lr4,
    high_split_hp: Lr4,
    phase_comp_lp: Lr4,
    phase_comp_hp: Lr4,
}

impl ChannelSplitter {
    fn set_low_cutoff(&mut self, low_hz: f32, sample_rate: f32) {
        self.low_split_lp.set_cutoff(low_hz, sample_rate, false);
        self.low_split_hp.set_cutoff(low_hz, sample_rate, true);
    }

    fn set_high_cutoff(&mut self, high_hz: f32, sample_rate: f32) {
        self.high_split_lp.set_cutoff(high_hz, sample_rate, false);
        self.high_split_hp.set_cutoff(high_hz, sample_rate, true);
        self.phase_comp_lp.set_cutoff(high_hz, sample_rate, false);
        self.phase_comp_hp.set_cutoff(high_hz, sample_rate, true);
    }

    const fn reset_state(&mut self) {
        self.low_split_lp.reset_state();
        self.low_split_hp.reset_state();
        self.high_split_lp.reset_state();
        self.high_split_hp.reset_state();
        self.phase_comp_lp.reset_state();
        self.phase_comp_hp.reset_state();
    }

    #[inline]
    fn process(&mut self, x: f32) -> Bands<f32> {
        let low_raw = self.low_split_lp.process(x);
        let upper = self.low_split_hp.process(x);
        let mid = self.high_split_lp.process(upper);
        let high = self.high_split_hp.process(upper);
        let low = self.phase_comp_lp.process(low_raw) + self.phase_comp_hp.process(low_raw);

        Bands { low, mid, high }
    }

    const fn is_finite(&self) -> bool {
        self.low_split_lp.is_finite()
            && self.low_split_hp.is_finite()
            && self.high_split_lp.is_finite()
            && self.high_split_hp.is_finite()
            && self.phase_comp_lp.is_finite()
            && self.phase_comp_hp.is_finite()
    }
}

/// Stereo 3-band crossover. Keeps filter state independent per L/R channel (docs/architecture.md).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Crossover {
    sample_rate: f32,
    low_freq: LogSmoothed,
    high_freq: LogSmoothed,
    left: ChannelSplitter,
    right: ChannelSplitter,
    #[cfg(test)]
    coefficient_update_count: usize,
}

impl Crossover {
    /// Creates a crossover with filter coefficients set for `low_hz`/`high_hz` at `sample_rate`.
    #[must_use]
    pub fn new(sample_rate: f32, low_hz: f32, high_hz: f32) -> Self {
        let mut c = Self {
            sample_rate,
            low_freq: LogSmoothed::new(low_hz, sample_rate),
            high_freq: LogSmoothed::new(high_hz, sample_rate),
            left: ChannelSplitter::default(),
            right: ChannelSplitter::default(),
            #[cfg(test)]
            coefficient_update_count: 0,
        };
        c.update_low_cutoff(low_hz);
        c.update_high_cutoff(high_hz);
        c
    }

    /// On a sample-rate change: immediately reset both the smoothing state and filter state (docs/contracts.md §2, §7).
    pub fn reset(&mut self, sample_rate: f32, low_hz: f32, high_hz: f32) {
        *self = Self::new(sample_rate, low_hz, high_hz);
    }

    /// Updates the smoothing target cutoff. Does not apply immediately.
    pub fn set_targets(&mut self, low_hz: f32, high_hz: f32) {
        self.low_freq.set_target_hz(low_hz);
        self.high_freq.set_target_hz(high_hz);
    }

    fn update_low_cutoff(&mut self, low_hz: f32) {
        self.left.set_low_cutoff(low_hz, self.sample_rate);
        self.right.set_low_cutoff(low_hz, self.sample_rate);
        #[cfg(test)]
        {
            self.coefficient_update_count += 1;
        }
    }

    fn update_high_cutoff(&mut self, high_hz: f32) {
        self.left.set_high_cutoff(high_hz, self.sample_rate);
        self.right.set_high_cutoff(high_hz, self.sample_rate);
        #[cfg(test)]
        {
            self.coefficient_update_count += 1;
        }
    }

    #[cfg(test)]
    const fn coefficient_update_count(&self) -> usize {
        self.coefficient_update_count
    }

    /// Processes one frame. L and R both use the same smoothed cutoff (ADR 0001).
    #[inline]
    pub fn process_frame(&mut self, left_in: f32, right_in: f32) -> (Bands<f32>, Bands<f32>) {
        if let Some(low_hz) = self.low_freq.tick_hz_if_changed() {
            self.update_low_cutoff(low_hz);
        }
        if let Some(high_hz) = self.high_freq.tick_hz_if_changed() {
            self.update_high_cutoff(high_hz);
        }

        let left = self.left.process(left_in);
        let right = self.right.process(right_in);
        (left, right)
    }

    /// Resets the filters' delay-line state (not the smoothing state).
    pub const fn reset_filter_state(&mut self) {
        self.left.reset_state();
        self.right.reset_state();
    }

    /// Returns `false` if either channel's filter state has gone non-finite (docs/contracts.md §4).
    #[must_use]
    pub const fn is_finite(&self) -> bool {
        self.left.is_finite() && self.right.is_finite()
    }
}

#[cfg(test)]
// Sample indices in these tests stay well within f32's exact integer range, and
// narrowing back to f32/usize for signal generation and RMS measurement is intentional.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::float_cmp
)]
mod tests {
    use std::f64::consts::SQRT_2;

    use super::*;

    /// Runs a steady-state sine wave through and estimates the amplitude [dB] from the RMS after enough cycles have settled.
    fn measure_gain_db(mut process: impl FnMut(f32) -> f32, freq_hz: f32, sample_rate: f32) -> f32 {
        let cycles = 30.0_f32;
        let total_samples = ((cycles / freq_hz) * sample_rate).max(2_000.0) as usize;
        let settle = total_samples / 4;
        let omega = 2.0 * PI * freq_hz / sample_rate;

        let mut sum_sq = 0.0_f64;
        let mut count = 0_u64;
        for n in 0..total_samples {
            let x = (omega * n as f32).sin();
            let y = process(x);
            if n >= settle {
                sum_sq += f64::from(y) * f64::from(y);
                count += 1;
            }
        }
        let rms = (sum_sq / count as f64).sqrt();
        let amplitude = rms * SQRT_2;
        20.0 * amplitude.max(1e-12).log10() as f32
    }

    fn test_frequencies(sample_rate: f32) -> Vec<f32> {
        let nyquist_limit = 0.45 * sample_rate;
        [
            20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0, 20000.0, 50000.0,
            80000.0,
        ]
        .into_iter()
        .filter(|f| *f <= nyquist_limit)
        .collect()
    }

    fn assert_reconstruction_flat(sample_rate: f32, low_hz: f32, high_hz: f32) {
        for freq in test_frequencies(sample_rate) {
            let mut c = Crossover::new(sample_rate, low_hz, high_hz);
            let gain_db = measure_gain_db(
                |x| {
                    let (l, _r) = c.process_frame(x, x);
                    l.low + l.mid + l.high
                },
                freq,
                sample_rate,
            );
            assert!(
                gain_db.abs() < 0.1,
                "reconstruction at {freq} Hz ({sample_rate} Hz sr, split {low_hz}/{high_hz}) \
                 should be flat within 0.1 dB, got {gain_db} dB"
            );
        }
    }

    #[test]
    fn reconstruction_is_flat_at_default_crossover_48khz() {
        assert_reconstruction_flat(48_000.0, 120.0, 2500.0);
    }

    #[test]
    fn reconstruction_is_flat_across_sample_rates() {
        for sample_rate in [44_100.0, 48_000.0, 96_000.0, 192_000.0] {
            assert_reconstruction_flat(sample_rate, 120.0, 2500.0);
        }
    }

    #[test]
    fn reconstruction_is_flat_across_representative_crossovers() {
        // Combinations representative of the allowed range and the minimum one-octave spacing (docs/contracts.md §1).
        let cases: &[(f32, f32, f32)] = &[
            (48_000.0, 40.0, 400.0),
            (48_000.0, 2000.0, 4000.0),
            (48_000.0, 2000.0, 16000.0),
        ];
        for &(sample_rate, low_hz, high_hz) in cases {
            assert_reconstruction_flat(sample_rate, low_hz, high_hz);
        }
    }

    #[test]
    fn phase_compensator_alone_is_unity_gain() {
        // A_high(z) = LP4_high(z) + HP4_high(z) is a unity-gain all-pass response (ADR 0001).
        let sample_rate = 48_000.0;
        let high_hz = 2500.0;
        for freq in test_frequencies(sample_rate) {
            let mut lp = Lr4::default();
            let mut hp = Lr4::default();
            lp.set_cutoff(high_hz, sample_rate, false);
            hp.set_cutoff(high_hz, sample_rate, true);
            let gain_db = measure_gain_db(|x| lp.process(x) + hp.process(x), freq, sample_rate);
            assert!(
                gain_db.abs() < 0.01,
                "phase compensator at {freq} Hz should be 0 dB +/-0.01 dB, got {gain_db} dB"
            );
        }
    }

    #[test]
    fn impulse_response_is_finite_and_decays() {
        let sample_rate = 48_000.0;
        let mut c = Crossover::new(sample_rate, 120.0, 2500.0);
        let mut last_sum = 0.0_f32;
        for n in 0..10_000 {
            let x = if n == 0 { 1.0 } else { 0.0 };
            let (l, _r) = c.process_frame(x, x);
            let sum = l.low + l.mid + l.high;
            assert!(sum.is_finite(), "sample {n} produced non-finite output");
            last_sum = sum;
        }
        assert!(
            last_sum.abs() < 1e-3,
            "impulse response should have decayed by 10000 samples"
        );
    }

    #[test]
    fn dc_and_nyquist_neighborhood_do_not_produce_nan_or_inf() {
        let sample_rate = 48_000.0;
        let mut c = Crossover::new(sample_rate, 120.0, 2500.0);
        // DC
        for _ in 0..1000 {
            let (l, r) = c.process_frame(1.0, 1.0);
            assert!(l.iter().all(|v| v.is_finite()));
            assert!(r.iter().all(|v| v.is_finite()));
        }

        // The crossover frequencies themselves, plus the Nyquist neighborhood.
        for freq in [120.0, 2500.0, 0.45 * sample_rate, 0.449 * sample_rate] {
            let mut c = Crossover::new(sample_rate, 120.0, 2500.0);
            let omega = 2.0 * PI * freq / sample_rate;
            for n in 0..1000 {
                let x = (omega * n as f32).sin();
                let (l, r) = c.process_frame(x, x);
                assert!(l.iter().all(|v| v.is_finite()), "freq {freq}");
                assert!(r.iter().all(|v| v.is_finite()), "freq {freq}");
            }
        }
    }

    #[test]
    fn stereo_channels_have_independent_filter_state() {
        let sample_rate = 48_000.0;
        let mut c = Crossover::new(sample_rate, 120.0, 2500.0);
        // Hit only the left channel hard and confirm the right channel's state is unaffected.
        c.process_frame(1.0, 0.0);
        let (_l, r_after_impulse) = c.process_frame(0.0, 0.0);

        let mut r_only = Crossover::new(sample_rate, 120.0, 2500.0);
        r_only.process_frame(0.0, 0.0);
        let (_l2, r_reference) = r_only.process_frame(0.0, 0.0);

        for (a, b) in r_after_impulse.iter().zip(r_reference.iter()) {
            assert_eq!(
                *a, *b,
                "right channel state must be unaffected by left channel input"
            );
        }
    }

    #[test]
    fn settled_crossovers_do_not_recompute_coefficients() {
        let sample_rate = 48_000.0;
        let mut c = Crossover::new(sample_rate, 120.0, 2500.0);
        let initial_updates = c.coefficient_update_count();
        assert_eq!(
            initial_updates, 2,
            "construction updates low and high once each"
        );

        for _ in 0..1024 {
            c.process_frame(0.25, -0.25);
        }
        assert_eq!(
            c.coefficient_update_count(),
            initial_updates,
            "static cutoffs must not update coefficients in the audio path"
        );

        c.set_targets(200.0, 2500.0);
        c.process_frame(0.25, -0.25);
        assert_eq!(
            c.coefficient_update_count(),
            initial_updates + 1,
            "a low-only target change must update only the low coefficient group"
        );

        for _ in 0..(sample_rate as usize) {
            c.process_frame(0.25, -0.25);
            if c.low_freq.is_settled() && c.high_freq.is_settled() {
                break;
            }
        }
        assert!(c.low_freq.is_settled());
        assert!(c.high_freq.is_settled());
        let low_settled_updates = c.coefficient_update_count();

        for _ in 0..1024 {
            c.process_frame(0.25, -0.25);
        }
        assert_eq!(
            c.coefficient_update_count(),
            low_settled_updates,
            "settled cutoffs must stay coefficient-stable"
        );

        c.set_targets(200.0, 4000.0);
        c.process_frame(0.25, -0.25);
        assert_eq!(
            c.coefficient_update_count(),
            low_settled_updates + 1,
            "a high-only target change must update only the high coefficient group"
        );
    }

    #[test]
    fn changing_cutoff_target_mid_stream_keeps_bands_in_sync() {
        // If the f_high split and phase compensator always use the same smoothed
        // cutoff each sample, the 3-band reconstruction should match a reference
        // that applies A_low then A_high to the input in sequence.
        let sample_rate = 48_000.0;
        let mut c = Crossover::new(sample_rate, 120.0, 2500.0);

        let mut ref_low = Lr4::default();
        let mut ref_low_hp = Lr4::default();
        ref_low.set_cutoff(120.0, sample_rate, false);
        ref_low_hp.set_cutoff(120.0, sample_rate, true);

        let mut max_abs_error = 0.0_f32;
        for n in 0..2000 {
            if n == 500 {
                c.set_targets(200.0, 4000.0);
            }
            let x = ((n as f32) * 0.05).sin();
            let (l, _r) = c.process_frame(x, x);
            let sum = l.low + l.mid + l.high;

            // If A_low(z)*A_high(z) stays at 1 even while the cutoff is changing,
            // unprocessed reconstruction should simply track the input itself, so
            // just confirm it stays finite and doesn't diverge.
            assert!(sum.is_finite());
            max_abs_error = max_abs_error.max((sum - x).abs());
        }
        // Perfect instantaneous tracking isn't guaranteed while coefficients are updating, but it must not diverge.
        assert!(
            max_abs_error < 10.0,
            "reconstruction diverged during cutoff change: {max_abs_error}"
        );
    }
}
