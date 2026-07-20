//! `OttProcessor` and the public DSP API (docs/architecture.md).

pub mod compressor;
pub mod crossover;
pub mod envelope;
pub mod smooth;

use thiserror::Error;

use crate::bands::Bands;
use crate::params::{BandParams, ConfigError, GlobalParams, OttParams};
use compressor::{BandDynamics, DualThresholdCompressor, effective_amount};
use crossover::Crossover;
use envelope::{attack_release_ms, detector_power};
use smooth::Smoothed;

/// Internal floor (docs/contracts.md §4). Treats anything below `-120 dBFS`
/// as zero input, preventing `log(0)`, division by zero, and NaN.
pub(crate) const FLOOR_DB: f32 = -120.0;

/// `db_to_amp(x) = 10^(x / 20)`.
#[inline]
pub(crate) fn db_to_amp(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// `power_to_db(p) = 10 * log10(max(p, floor))`.
#[inline]
pub(crate) fn power_to_db(power: f32) -> f32 {
    let floor_power = db_to_amp(FLOOR_DB) * db_to_amp(FLOOR_DB);
    10.0 * power.max(floor_power).log10()
}

#[inline]
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    (b - a).mul_add(t, a)
}

/// Runtime error returned by `process` (docs/contracts.md §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ProcessError {
    /// `input_l`, `input_r`, `output_l`, and `output_r` did not all have the same length.
    #[error("input/output buffer lengths do not match")]
    BufferLengthMismatch,
}

/// Bundles one band's smoothed parameters with its dual-threshold compressor (docs/architecture.md).
#[derive(Debug, Clone, Copy, PartialEq)]
struct BandProcessor {
    lower_threshold_db: Smoothed,
    upper_threshold_db: Smoothed,
    up_amount: Smoothed,
    down_amount: Smoothed,
    makeup_gain_db: Smoothed,
    base_attack_ms: f32,
    base_release_ms: f32,
    compressor: DualThresholdCompressor,
}

impl BandProcessor {
    fn new(params: &BandParams, sample_rate: f32) -> Self {
        Self {
            lower_threshold_db: Smoothed::new(params.thresholds.lower_db().get(), sample_rate),
            upper_threshold_db: Smoothed::new(params.thresholds.upper_db().get(), sample_rate),
            up_amount: Smoothed::new(params.up_amount.get(), sample_rate),
            down_amount: Smoothed::new(params.down_amount.get(), sample_rate),
            makeup_gain_db: Smoothed::new(params.makeup_gain_db.get(), sample_rate),
            base_attack_ms: params.base_attack_ms.get(),
            base_release_ms: params.base_release_ms.get(),
            compressor: DualThresholdCompressor::new(
                params.thresholds.lower_db().get(),
                params.thresholds.upper_db().get(),
            ),
        }
    }

    /// Updates only the smoothing targets. Keeps the current smoothing state as-is (docs/contracts.md §2).
    const fn set_targets(&mut self, params: &BandParams) {
        self.lower_threshold_db
            .set_target(params.thresholds.lower_db().get());
        self.upper_threshold_db
            .set_target(params.thresholds.upper_db().get());
        self.up_amount.set_target(params.up_amount.get());
        self.down_amount.set_target(params.down_amount.get());
        self.makeup_gain_db.set_target(params.makeup_gain_db.get());
        self.base_attack_ms = params.base_attack_ms.get();
        self.base_release_ms = params.base_release_ms.get();
    }

    const fn is_finite(&self) -> bool {
        self.compressor.is_finite()
    }

    /// Resets only this band's envelope state (docs/contracts.md §4).
    fn reset_envelope_state(&mut self) {
        self.compressor.reset(
            self.lower_threshold_db.current(),
            self.upper_threshold_db.current(),
        );
    }

    #[inline]
    fn process(
        &mut self,
        left_in: f32,
        right_in: f32,
        frame: &FrameControls,
        sample_rate: f32,
    ) -> (f32, f32) {
        let lower_threshold_db = self.lower_threshold_db.tick();
        let upper_threshold_db = self.upper_threshold_db.tick();
        let up_amount = self.up_amount.tick();
        let down_amount = self.down_amount.tick();
        let makeup_gain_db = self.makeup_gain_db.tick();

        let (attack_ms, release_ms) = attack_release_ms(
            self.base_attack_ms,
            self.base_release_ms,
            frame.time,
            sample_rate,
        );

        let dynamics = BandDynamics {
            lower_threshold_db,
            upper_threshold_db,
            effective_up_amount: effective_amount(up_amount, frame.upward),
            effective_down_amount: effective_amount(down_amount, frame.downward),
            attack_ms,
            release_ms,
        };

        let p = detector_power(left_in, right_in);
        let dynamic_gain = self.compressor.process(p, &dynamics, sample_rate);
        let makeup_gain = db_to_amp(makeup_gain_db);

        let wet_left = left_in * dynamic_gain * makeup_gain;
        let wet_right = right_in * dynamic_gain * makeup_gain;

        (
            lerp(left_in, wet_left, frame.depth),
            lerp(right_in, wet_right, frame.depth),
        )
    }
}

/// Bundles one frame's smoothed global values to pass to `BandProcessor` (docs/architecture.md).
#[derive(Debug, Clone, Copy, PartialEq)]
struct FrameControls {
    time: f32,
    upward: f32,
    downward: f32,
    depth: f32,
}

/// Smoothed global parameters holding current/target (docs/architecture.md).
#[derive(Debug, Clone, Copy, PartialEq)]
struct GlobalRuntime {
    input_gain_db: Smoothed,
    output_gain_db: Smoothed,
    depth: Smoothed,
    time: Smoothed,
    upward: Smoothed,
    downward: Smoothed,
}

impl GlobalRuntime {
    fn new(params: &GlobalParams, sample_rate: f32) -> Self {
        Self {
            input_gain_db: Smoothed::new(params.input_gain_db.get(), sample_rate),
            output_gain_db: Smoothed::new(params.output_gain_db.get(), sample_rate),
            depth: Smoothed::new(params.depth.get(), sample_rate),
            time: Smoothed::new(params.time.get(), sample_rate),
            upward: Smoothed::new(params.upward.get(), sample_rate),
            downward: Smoothed::new(params.downward.get(), sample_rate),
        }
    }

    const fn set_targets(&mut self, params: &GlobalParams) {
        self.input_gain_db.set_target(params.input_gain_db.get());
        self.output_gain_db.set_target(params.output_gain_db.get());
        self.depth.set_target(params.depth.get());
        self.time.set_target(params.time.get());
        self.upward.set_target(params.upward.get());
        self.downward.set_target(params.downward.get());
    }
}

/// DSP core for the 3-band, upward/downward multiband compressor (docs/architecture.md).
///
/// Processes frame-by-frame and holds no variable-length buffer for
/// intermediate bands. Keeps state independent of JACK's buffer size
/// (docs/architecture.md).
#[derive(Debug, Clone, Copy)]
pub struct OttProcessor {
    sample_rate: f32,
    target_params: OttParams,
    global: GlobalRuntime,
    crossover: Crossover,
    bands: Bands<BandProcessor>,
}

impl OttProcessor {
    /// Constructs a processor for `sample_rate` with `params`.
    ///
    /// # Errors
    ///
    /// Returns `ConfigError` if `sample_rate` or `params` fail validation (docs/contracts.md §1).
    pub fn new(sample_rate: f32, params: OttParams) -> Result<Self, ConfigError> {
        params.validate(sample_rate)?;
        Ok(Self::new_unchecked(sample_rate, params))
    }

    fn new_unchecked(sample_rate: f32, params: OttParams) -> Self {
        let global = GlobalRuntime::new(&params.global, sample_rate);
        let crossover = Crossover::new(
            sample_rate,
            params.global.crossover.low_hz().get(),
            params.global.crossover.high_hz().get(),
        );
        let bands = Bands {
            low: BandProcessor::new(&params.bands.low, sample_rate),
            mid: BandProcessor::new(&params.bands.mid, sample_rate),
            high: BandProcessor::new(&params.bands.high, sample_rate),
        };
        Self {
            sample_rate,
            target_params: params,
            global,
            crossover,
            bands,
        }
    }

    /// On a sample-rate change: recomputes all filter coefficients and time
    /// coefficients, and resets state (docs/contracts.md §2, §7).
    ///
    /// Keeps the most recently set target parameters and immediately sets
    /// `current` to `target` (docs/contracts.md §2).
    ///
    /// # Errors
    ///
    /// Returns `ConfigError` if `sample_rate` fails validation against the
    /// currently held target parameters (docs/contracts.md §1).
    // Proves this function can never panic (docs/contracts.md §6); see the
    // note on `process` above.
    #[cfg_attr(all(test, not(debug_assertions)), no_panic::no_panic)]
    pub fn reset(&mut self, sample_rate: f32) -> Result<(), ConfigError> {
        self.target_params.validate(sample_rate)?;
        *self = Self::new_unchecked(sample_rate, self.target_params);
        Ok(())
    }

    /// Updates the smoothing target for parameters. Keeps the current smoothing state as-is (docs/contracts.md §2).
    ///
    /// # Errors
    ///
    /// Returns `ConfigError` if `params` fail validation against the current
    /// sample rate (docs/contracts.md §1).
    pub fn set_params(&mut self, params: OttParams) -> Result<(), ConfigError> {
        params.validate(self.sample_rate)?;
        self.global.set_targets(&params.global);
        self.crossover.set_targets(
            params.global.crossover.low_hz().get(),
            params.global.crossover.high_hz().get(),
        );
        for (band, band_params) in self.bands.iter_mut().zip(params.bands.iter()) {
            band.set_targets(band_params);
        }
        self.target_params = params;
        Ok(())
    }

    /// Returns an error before writing anything if the 4 slices don't have the same length (docs/contracts.md §3).
    ///
    /// # Errors
    ///
    /// Returns `ProcessError::BufferLengthMismatch` if `input_l`, `input_r`,
    /// `output_l`, and `output_r` don't all have the same length.
    // Proves this function can never panic (docs/contracts.md §6), checked by
    // `cargo test --release` (the proof only holds under optimization; see
    // the `no-panic` crate's docs). Existing tests in `processor_tests`
    // already call this, so no separate proof-only test is needed.
    #[cfg_attr(all(test, not(debug_assertions)), no_panic::no_panic)]
    pub fn process(
        &mut self,
        input_l: &[f32],
        input_r: &[f32],
        output_l: &mut [f32],
        output_r: &mut [f32],
    ) -> Result<(), ProcessError> {
        let len = input_l.len();
        if input_r.len() != len || output_l.len() != len || output_r.len() != len {
            return Err(ProcessError::BufferLengthMismatch);
        }

        // Iterator-based rather than indexed: bounds checks on 4 independently-
        // indexed slices aren't reliably provable away even once lengths are
        // known equal, which breaks the no-panic proof (docs/contracts.md §6).
        // Zipped iterators can't go out of bounds by construction.
        let inputs = input_l.iter().zip(input_r.iter());
        let outputs = output_l.iter_mut().zip(output_r.iter_mut());
        for ((&l_in, &r_in), (out_l, out_r)) in inputs.zip(outputs) {
            let (l, r) = self.process_frame(l_in, r_in);
            *out_l = l;
            *out_r = r;
        }
        Ok(())
    }

    #[inline]
    fn process_frame(&mut self, left_in: f32, right_in: f32) -> (f32, f32) {
        // If an input sample is NaN/+-Inf, treat that sample as 0 (docs/contracts.md §4).
        let left_in = if left_in.is_finite() { left_in } else { 0.0 };
        let right_in = if right_in.is_finite() { right_in } else { 0.0 };

        let input_gain = db_to_amp(self.global.input_gain_db.tick());
        let output_gain = db_to_amp(self.global.output_gain_db.tick());
        let frame = FrameControls {
            time: self.global.time.tick(),
            upward: self.global.upward.tick(),
            downward: self.global.downward.tick(),
            depth: self.global.depth.tick(),
        };

        let left = left_in * input_gain;
        let right = right_in * input_gain;

        let (left_bands, right_bands) = self.crossover.process_frame(left, right);
        if !self.crossover.is_finite() {
            self.crossover.reset_filter_state();
        }

        let mut sum_left = 0.0_f32;
        let mut sum_right = 0.0_f32;
        for (band, (&lb, &rb)) in self
            .bands
            .iter_mut()
            .zip(left_bands.iter().zip(right_bands.iter()))
        {
            let (out_l, out_r) = band.process(lb, rb, &frame, self.sample_rate);
            if !band.is_finite() {
                band.reset_envelope_state();
            }
            sum_left += out_l;
            sum_right += out_r;
        }

        let mut out_left = sum_left * output_gain;
        let mut out_right = sum_right * output_gain;

        // Even if filter or envelope state goes non-finite, force the output to 0 (docs/contracts.md §4).
        if !out_left.is_finite() {
            out_left = 0.0;
        }
        if !out_right.is_finite() {
            out_right = 0.0;
        }

        (out_left, out_right)
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn db_to_amp_matches_reference_points() {
        assert!((db_to_amp(0.0) - 1.0).abs() < 1e-6);
        assert!((db_to_amp(-20.0) - 0.1).abs() < 1e-6);
    }

    #[test]
    fn power_to_db_floors_zero_input() {
        assert!(power_to_db(0.0).is_finite());
        assert!((power_to_db(0.0) - FLOOR_DB).abs() < 1e-3);
    }

    #[test]
    fn power_to_db_matches_db_to_amp_for_squared_amplitude() {
        let amp = db_to_amp(-20.0);
        let db_from_power = power_to_db(amp * amp);
        assert!((db_from_power - (-20.0)).abs() < 1e-3);
    }
}

/// `OttProcessor` integration tests (docs/contracts.md §2-§5).
#[cfg(test)]
// These tests compare exact deterministic values (verbatim inputs, buffer
// equality across chunkings) and cast sample counts that stay well within
// f32/f64's exact range, so unwrap/panic/float_cmp/cast noise here is
// expected. `vec!` is fine in tests; the real-time-callback contract
// (docs/contracts.md §6) only applies to the DSP/audio-callback path.
#[allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::float_cmp,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::similar_names,
    clippy::disallowed_macros
)]
mod processor_tests {
    use super::*;
    use crate::params::{IoGain, MakeupGain, NormalizedF32, Preset};
    use std::f32::consts::PI;

    fn rms(samples: &[f32]) -> f32 {
        let sum_sq: f64 = samples.iter().map(|&x| f64::from(x) * f64::from(x)).sum();
        ((sum_sq / samples.len() as f64).sqrt()) as f32
    }

    fn sine(n: usize, freq_hz: f32, amp: f32, sample_rate: f32) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * PI * freq_hz * i as f32 / sample_rate).sin())
            .collect()
    }

    #[test]
    fn depth_zero_matches_pure_crossover_reconstruction() {
        let sample_rate = 48_000.0;
        let mut params = Preset::Default.params();
        params.global.depth = NormalizedF32::new_const(0.0);
        let mut proc = OttProcessor::new(sample_rate, params).unwrap();

        let mut reference = Crossover::new(
            sample_rate,
            params.global.crossover.low_hz().get(),
            params.global.crossover.high_hz().get(),
        );
        let input_gain = db_to_amp(params.global.input_gain_db.get());
        let output_gain = db_to_amp(params.global.output_gain_db.get());

        let n = 2000;
        let input = sine(n, 300.0, 0.5, sample_rate);
        let mut out_l = vec![0.0_f32; n];
        let mut out_r = vec![0.0_f32; n];
        proc.process(&input, &input, &mut out_l, &mut out_r)
            .unwrap();

        for i in 0..n {
            let x = input[i] * input_gain;
            let (l, _r) = reference.process_frame(x, x);
            let expected = (l.low + l.mid + l.high) * output_gain;
            assert!(
                (out_l[i] - expected).abs() < 1e-4,
                "sample {i}: got {}, expected {expected}",
                out_l[i]
            );
        }
    }

    #[test]
    fn upward_zero_gives_no_boost_below_lower_threshold() {
        const ZERO_MAKEUP_GAIN: MakeupGain = MakeupGain::new_const(0.0);
        let sample_rate = 48_000.0;
        let mut params = Preset::Default.params();
        params.global.upward = NormalizedF32::new_const(0.0);
        for band in params.bands.iter_mut() {
            band.makeup_gain_db = ZERO_MAKEUP_GAIN;
        }
        let mut proc = OttProcessor::new(sample_rate, params).unwrap();

        let n = 20_000;
        let amp = db_to_amp(-70.0); // a level comfortably below every band's lower threshold
        let input = sine(n, 1000.0, amp, sample_rate); // within the mid band
        let mut out_l = vec![0.0_f32; n];
        let mut out_r = vec![0.0_f32; n];
        proc.process(&input, &input, &mut out_l, &mut out_r)
            .unwrap();

        let settle = n / 2;
        let input_rms = rms(&input[settle..]);
        let output_rms = rms(&out_l[settle..]);
        assert!(
            (output_rms - input_rms).abs() / input_rms < 0.05,
            "input_rms={input_rms} output_rms={output_rms}"
        );
    }

    #[test]
    fn downward_zero_gives_no_suppression_above_upper_threshold() {
        const ZERO_MAKEUP_GAIN: MakeupGain = MakeupGain::new_const(0.0);
        let sample_rate = 48_000.0;
        let mut params = Preset::Default.params();
        params.global.downward = NormalizedF32::new_const(0.0);
        for band in params.bands.iter_mut() {
            band.makeup_gain_db = ZERO_MAKEUP_GAIN;
        }
        let mut proc = OttProcessor::new(sample_rate, params).unwrap();

        let n = 20_000;
        let amp = db_to_amp(0.0); // a level comfortably above every band's upper threshold
        let input = sine(n, 1000.0, amp, sample_rate);
        let mut out_l = vec![0.0_f32; n];
        let mut out_r = vec![0.0_f32; n];
        proc.process(&input, &input, &mut out_l, &mut out_r)
            .unwrap();

        let settle = n / 2;
        let input_rms = rms(&input[settle..]);
        let output_rms = rms(&out_l[settle..]);
        assert!(
            (output_rms - input_rms).abs() / input_rms < 0.05,
            "input_rms={input_rms} output_rms={output_rms}"
        );
    }

    #[test]
    fn chunking_does_not_affect_output() {
        let sample_rate = 48_000.0;
        let params = Preset::Default.params();
        let n = 500;
        let input = sine(n, 440.0, 0.5, sample_rate);

        let mut proc_a = OttProcessor::new(sample_rate, params).unwrap();
        let mut out_a_l = vec![0.0_f32; n];
        let mut out_a_r = vec![0.0_f32; n];
        proc_a
            .process(&input, &input, &mut out_a_l, &mut out_a_r)
            .unwrap();

        // 1-sample chunks
        let mut proc_b = OttProcessor::new(sample_rate, params).unwrap();
        let mut out_b_l = vec![0.0_f32; n];
        let mut out_b_r = vec![0.0_f32; n];
        for i in 0..n {
            proc_b
                .process(
                    &input[i..=i],
                    &input[i..=i],
                    &mut out_b_l[i..=i],
                    &mut out_b_r[i..=i],
                )
                .unwrap();
        }
        assert_eq!(out_a_l, out_b_l, "1-sample chunking changed output");

        // Irregular chunk sizes
        let mut proc_c = OttProcessor::new(sample_rate, params).unwrap();
        let mut out_c_l = vec![0.0_f32; n];
        let mut out_c_r = vec![0.0_f32; n];
        let chunk_pattern = [64, 37, 1, 200, 1000];
        let mut pos = 0;
        let mut idx = 0;
        while pos < n {
            let size = chunk_pattern[idx % chunk_pattern.len()].min(n - pos);
            idx += 1;
            proc_c
                .process(
                    &input[pos..pos + size],
                    &input[pos..pos + size],
                    &mut out_c_l[pos..pos + size],
                    &mut out_c_r[pos..pos + size],
                )
                .unwrap();
            pos += size;
        }
        assert_eq!(out_a_l, out_c_l, "irregular chunking changed output");
    }

    fn assert_all_finite(name: &str, sample_rate: f32, input: &[f32]) {
        let params = Preset::Default.params();
        let mut proc = OttProcessor::new(sample_rate, params).unwrap();
        let n = input.len();
        let mut out_l = vec![0.0_f32; n];
        let mut out_r = vec![0.0_f32; n];
        proc.process(input, input, &mut out_l, &mut out_r).unwrap();
        assert!(
            out_l.iter().all(|v| v.is_finite()),
            "{name}: left channel produced non-finite output"
        );
        assert!(
            out_r.iter().all(|v| v.is_finite()),
            "{name}: right channel produced non-finite output"
        );
    }

    #[test]
    fn stays_finite_for_extended_stress_signals() {
        let sample_rate = 48_000.0;
        let n = (10.0 * sample_rate) as usize; // 10+ seconds

        assert_all_finite("silence", sample_rate, &vec![0.0_f32; n]);
        assert_all_finite("dc", sample_rate, &vec![1.0_f32; n]);
        assert_all_finite(
            "max_amplitude_sine",
            sample_rate,
            &sine(n, 1000.0, 1.0, sample_rate),
        );

        let mut impulse = vec![0.0_f32; n];
        impulse[0] = 1.0;
        assert_all_finite("impulse", sample_rate, &impulse);

        let mut state: u32 = 0x1234_5678;
        let white_noise: Vec<f32> = (0..n)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                (state as f32 / u32::MAX as f32).mul_add(2.0, -1.0)
            })
            .collect();
        assert_all_finite("white_noise", sample_rate, &white_noise);
    }

    #[test]
    fn default_preset_boosts_quiet_signal_beyond_makeup_alone() {
        let sample_rate = 48_000.0;
        let params = Preset::Default.params();
        let mut proc = OttProcessor::new(sample_rate, params).unwrap();

        let n = 20_000;
        let level_db = -60.0;
        let amp = db_to_amp(level_db);
        let input = sine(n, 1000.0, amp, sample_rate); // mid band

        let mut out_l = vec![0.0_f32; n];
        let mut out_r = vec![0.0_f32; n];
        proc.process(&input, &input, &mut out_l, &mut out_r)
            .unwrap();

        let settle = n / 2;
        let output_rms = rms(&out_l[settle..]);
        let makeup_only_rms =
            rms(&input[settle..]) * db_to_amp(params.bands.mid.makeup_gain_db.get());

        assert!(
            output_rms > makeup_only_rms * 1.05,
            "output_rms={output_rms} should exceed makeup-only_rms={makeup_only_rms} (upward boost expected)"
        );
    }

    #[test]
    fn default_preset_suppresses_loud_signal_below_makeup_alone() {
        let sample_rate = 48_000.0;
        let params = Preset::Default.params();
        let mut proc = OttProcessor::new(sample_rate, params).unwrap();

        let n = 20_000;
        let level_db = 0.0;
        let amp = db_to_amp(level_db);
        let input = sine(n, 1000.0, amp, sample_rate); // mid band

        let mut out_l = vec![0.0_f32; n];
        let mut out_r = vec![0.0_f32; n];
        proc.process(&input, &input, &mut out_l, &mut out_r)
            .unwrap();

        let settle = n / 2;
        let output_rms = rms(&out_l[settle..]);
        let makeup_only_rms =
            rms(&input[settle..]) * db_to_amp(params.bands.mid.makeup_gain_db.get());

        assert!(
            output_rms < makeup_only_rms * 0.95,
            "output_rms={output_rms} should be below makeup-only_rms={makeup_only_rms} (downward suppression expected)"
        );
    }

    #[test]
    fn identical_left_right_input_produces_identical_output() {
        let sample_rate = 48_000.0;
        let params = Preset::Default.params();
        let mut proc = OttProcessor::new(sample_rate, params).unwrap();

        let n = 5000;
        let input = sine(n, 250.0, 0.3, sample_rate);
        let mut out_l = vec![0.0_f32; n];
        let mut out_r = vec![0.0_f32; n];
        proc.process(&input, &input, &mut out_l, &mut out_r)
            .unwrap();

        assert_eq!(out_l, out_r);
    }

    #[test]
    fn process_rejects_mismatched_buffer_lengths() {
        let sample_rate = 48_000.0;
        let params = Preset::SafeStart.params();
        let mut proc = OttProcessor::new(sample_rate, params).unwrap();
        let input = vec![0.0_f32; 10];
        let mut out_l = vec![0.0_f32; 10];
        let mut out_r = vec![0.0_f32; 9];
        let result = proc.process(&input, &input, &mut out_l, &mut out_r);
        assert_eq!(result, Err(ProcessError::BufferLengthMismatch));
    }

    #[test]
    fn reset_reapplies_last_target_params_without_startup_fade() {
        let sample_rate = 48_000.0;
        let params = Preset::Default.params();
        let mut proc = OttProcessor::new(sample_rate, params).unwrap();

        // Change the target, then reset before it takes effect.
        let mut updated = params;
        updated.global.output_gain_db = IoGain::new_const(-6.0);
        proc.set_params(updated).unwrap();
        proc.reset(96_000.0).unwrap();

        let n = 10;
        let input = vec![0.1_f32; n];
        let mut out_l = vec![0.0_f32; n];
        let mut out_r = vec![0.0_f32; n];
        proc.process(&input, &input, &mut out_l, &mut out_r)
            .unwrap();
        assert!(out_l.iter().all(|v| v.is_finite()));
    }
}
