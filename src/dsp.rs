//! `OttProcessor` and the public DSP API (docs/architecture.md).

pub mod compressor;
pub mod crossover;
pub mod envelope;
pub mod smooth;

use thiserror::Error;

use crate::bands::Bands;
use crate::params::{BandParams, ConfigError, ControlSnapshot, GlobalParams, OttParams};
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

/// Maximum remaining value before a bypass stage snaps its smoother exactly to
/// its target. The depth threshold is -60 dB in the wet/dry weight and the
/// gain threshold is one hundredth dB, both below audibility but finite.
const BYPASS_DEPTH_SETTLE: f32 = 0.001;
const BYPASS_GAIN_SETTLE_DB: f32 = 0.01;

/// Sample-driven state for the coordinated effect-bypass transition.
///
/// Each stage owns exactly one moving group: depth first when engaging, gains
/// first when disengaging. A requested reversal is observed at every sample,
/// but finishes the current safe stage to the zero-depth waypoint before
/// taking the latest requested direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BypassStage {
    Active,
    EngagingDepth,
    EngagingGains,
    Bypassed,
    DisengagingGains,
    DisengagingDepth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BypassRuntime {
    requested: bool,
    stage: BypassStage,
}

impl BypassRuntime {
    const fn active() -> Self {
        Self {
            requested: false,
            stage: BypassStage::Active,
        }
    }

    const fn reset(&mut self, global: &mut GlobalRuntime, params: &GlobalParams) {
        if self.requested {
            global.snap_bypass();
            self.stage = BypassStage::Bypassed;
        } else {
            global.snap_effect(params);
            self.stage = BypassStage::Active;
        }
    }

    const fn set_requested(
        &mut self,
        requested: bool,
        global: &mut GlobalRuntime,
        params: &GlobalParams,
    ) {
        self.requested = requested;
        match self.stage {
            BypassStage::Active if requested => {
                global.hold_gains();
                global.force_depth_zero();
                self.stage = BypassStage::EngagingDepth;
            }
            BypassStage::Bypassed if !requested => {
                global.force_depth_zero();
                global.set_gain_targets(params);
                self.stage = BypassStage::DisengagingGains;
            }
            BypassStage::EngagingDepth => global.force_depth_zero(),
            BypassStage::EngagingGains if !requested => {
                global.set_gain_targets(params);
                self.stage = BypassStage::DisengagingGains;
            }
            BypassStage::EngagingGains | BypassStage::Bypassed => global.force_bypass(),
            BypassStage::DisengagingGains => {
                if requested {
                    global.force_unity_gains();
                    self.stage = BypassStage::EngagingGains;
                } else {
                    global.force_depth_zero();
                    global.set_gain_targets(params);
                }
            }
            BypassStage::DisengagingDepth => {
                if requested {
                    global.hold_gains();
                    global.force_depth_zero();
                    self.stage = BypassStage::EngagingDepth;
                } else {
                    global.set_depth_target(params);
                }
            }
            BypassStage::Active => global.set_effect_targets(params),
        }
    }

    fn advance(&mut self, global: &mut GlobalRuntime, params: &GlobalParams) {
        match self.stage {
            BypassStage::EngagingDepth if global.depth_near_zero() => {
                global.snap_depth_zero();
                self.stage = if self.requested {
                    global.force_unity_gains();
                    BypassStage::EngagingGains
                } else {
                    global.set_gain_targets(params);
                    BypassStage::DisengagingGains
                };
            }
            BypassStage::EngagingGains if global.gains_near_unity() => {
                global.snap_unity_gains();
                self.stage = if self.requested {
                    BypassStage::Bypassed
                } else {
                    global.set_gain_targets(params);
                    BypassStage::DisengagingGains
                };
            }
            BypassStage::Bypassed if !self.requested => {
                global.set_gain_targets(params);
                self.stage = BypassStage::DisengagingGains;
            }
            BypassStage::DisengagingGains if global.gains_near_targets() => {
                global.snap_gains_to_targets();
                self.stage = if self.requested {
                    global.force_depth_zero();
                    BypassStage::EngagingDepth
                } else {
                    global.set_depth_target(params);
                    BypassStage::DisengagingDepth
                };
            }
            BypassStage::DisengagingDepth if global.depth_near_target() => {
                global.snap_depth_to_target();
                self.stage = if self.requested {
                    global.hold_gains();
                    global.force_depth_zero();
                    BypassStage::EngagingDepth
                } else {
                    // A gain-pot update received during this depth-only stage
                    // is intentionally deferred. Depth is now stationary, so
                    // the active state can begin its ordinary gain smoothing.
                    global.set_gain_targets(params);
                    BypassStage::Active
                };
            }
            _ => {}
        }
    }
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

    const fn set_non_bypass_targets(&mut self, params: &GlobalParams) {
        self.time.set_target(params.time.get());
        self.upward.set_target(params.upward.get());
        self.downward.set_target(params.downward.get());
    }

    const fn set_effect_targets(&mut self, params: &GlobalParams) {
        self.input_gain_db.set_target(params.input_gain_db.get());
        self.output_gain_db.set_target(params.output_gain_db.get());
        self.depth.set_target(params.depth.get());
    }

    const fn set_gain_targets(&mut self, params: &GlobalParams) {
        self.input_gain_db.set_target(params.input_gain_db.get());
        self.output_gain_db.set_target(params.output_gain_db.get());
    }

    const fn set_depth_target(&mut self, params: &GlobalParams) {
        self.depth.set_target(params.depth.get());
    }

    const fn hold_gains(&mut self) {
        self.input_gain_db.set_target(self.input_gain_db.current());
        self.output_gain_db
            .set_target(self.output_gain_db.current());
    }

    const fn force_depth_zero(&mut self) {
        self.depth.set_target(0.0);
    }

    const fn force_unity_gains(&mut self) {
        self.input_gain_db.set_target(0.0);
        self.output_gain_db.set_target(0.0);
    }

    const fn force_bypass(&mut self) {
        self.force_depth_zero();
        self.force_unity_gains();
    }

    const fn snap_depth_zero(&mut self) {
        self.depth.snap(0.0);
    }

    const fn snap_unity_gains(&mut self) {
        self.input_gain_db.snap(0.0);
        self.output_gain_db.snap(0.0);
    }

    const fn snap_gains_to_targets(&mut self) {
        self.input_gain_db.snap(self.input_gain_db.target());
        self.output_gain_db.snap(self.output_gain_db.target());
    }

    const fn snap_depth_to_target(&mut self) {
        self.depth.snap(self.depth.target());
    }

    const fn snap_bypass(&mut self) {
        self.depth.snap(0.0);
        self.input_gain_db.snap(0.0);
        self.output_gain_db.snap(0.0);
    }

    const fn snap_effect(&mut self, params: &GlobalParams) {
        self.input_gain_db.snap(params.input_gain_db.get());
        self.output_gain_db.snap(params.output_gain_db.get());
        self.depth.snap(params.depth.get());
        self.time.snap(params.time.get());
        self.upward.snap(params.upward.get());
        self.downward.snap(params.downward.get());
    }

    fn depth_near_zero(&self) -> bool {
        self.depth.current().abs() <= BYPASS_DEPTH_SETTLE
    }

    fn depth_near_target(&self) -> bool {
        (self.depth.current() - self.depth.target()).abs() <= BYPASS_DEPTH_SETTLE
    }

    fn gains_near_unity(&self) -> bool {
        self.input_gain_db.current().abs() <= BYPASS_GAIN_SETTLE_DB
            && self.output_gain_db.current().abs() <= BYPASS_GAIN_SETTLE_DB
    }

    fn gains_near_targets(&self) -> bool {
        (self.input_gain_db.current() - self.input_gain_db.target()).abs() <= BYPASS_GAIN_SETTLE_DB
            && (self.output_gain_db.current() - self.output_gain_db.target()).abs()
                <= BYPASS_GAIN_SETTLE_DB
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
    bypass: BypassRuntime,
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
            bypass: BypassRuntime::active(),
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
        let bypass_requested = self.bypass.requested;
        *self = Self::new_unchecked(sample_rate, self.target_params);
        self.bypass.requested = bypass_requested;
        self.bypass
            .reset(&mut self.global, &self.target_params.global);
        Ok(())
    }

    /// Updates the smoothing target for parameters. Keeps the current smoothing state as-is (docs/contracts.md §2).
    ///
    /// # Errors
    ///
    /// Returns `ConfigError` if `params` fail validation against the current
    /// sample rate (docs/contracts.md §1).
    // Proves this function can never panic (docs/contracts.md §6); see the
    // note on `process` below. It is held to the callback contract because the
    // control surface applies its snapshots from inside the audio callback
    // (`AudioProcessHandler::process`), not from the control thread.
    #[cfg_attr(all(test, not(debug_assertions)), no_panic::no_panic)]
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
        self.bypass.requested = false;
        self.bypass.stage = BypassStage::Active;
        Ok(())
    }

    /// Applies a control-surface snapshot with an explicit bypass level.
    ///
    /// Unlike [`Self::set_params`], this coordinates depth and the two gains
    /// through the allocation-free, sample-driven bypass state machine. All
    /// other parameters retain ordinary target-only update semantics.
    ///
    /// # Errors
    ///
    /// Returns `ConfigError` if the snapshot parameters are invalid for this
    /// processor's sample rate; on error no state changes.
    #[cfg_attr(all(test, not(debug_assertions)), no_panic::no_panic)]
    pub fn set_control_snapshot(&mut self, snapshot: ControlSnapshot) -> Result<(), ConfigError> {
        snapshot.params.validate(self.sample_rate)?;
        self.global.set_non_bypass_targets(&snapshot.params.global);
        self.crossover.set_targets(
            snapshot.params.global.crossover.low_hz().get(),
            snapshot.params.global.crossover.high_hz().get(),
        );
        for (band, band_params) in self.bands.iter_mut().zip(snapshot.params.bands.iter()) {
            band.set_targets(band_params);
        }
        self.target_params = snapshot.params;
        self.bypass.set_requested(
            snapshot.bypass_engaged,
            &mut self.global,
            &snapshot.params.global,
        );
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

        self.bypass
            .advance(&mut self.global, &self.target_params.global);

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
// f32/f64's exact range, so unwrap/cast noise here is expected.
// `vec!` is fine in tests; the real-time-callback contract
// (docs/contracts.md §6) only applies to the DSP/audio-callback path.
#[allow(
    clippy::unwrap_used,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::similar_names,
    clippy::disallowed_macros,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp
)]
mod processor_tests {
    use super::*;
    use crate::params::{
        CrossoverFreqHigh, CrossoverFreqLow, CrossoverSplit, IoGain, MakeupGain, NormalizedF32,
        Preset,
    };
    use proptest::prelude::*;
    use std::f32::consts::PI;

    const PROPERTY_CASES: u32 = 128;
    const BYPASS_RMS_WINDOW: usize = 480; // 10 ms at the required 48 kHz.
    /// A 1 ms hop prevents a transient from hiding between disjoint 10 ms windows.
    const BYPASS_RMS_HOP: usize = 48;
    /// The test permits at most 0.1 dB above the louder warmed endpoint.
    /// This is below a meaningful level-meter excursion while leaving room for
    /// the crossover's normal reconstruction ripple.
    const BYPASS_TRANSITION_TOLERANCE_DB: f32 = 0.1;

    fn arbitrary_samples() -> impl Strategy<Value = Vec<f32>> {
        prop::collection::vec(any::<u32>().prop_map(f32::from_bits), 0..=256)
    }

    fn arbitrary_stereo_samples() -> impl Strategy<Value = (Vec<f32>, Vec<f32>)> {
        prop::collection::vec((any::<u32>(), any::<u32>()), 0..=256).prop_map(|frames| {
            let (left_bits, right_bits): (Vec<_>, Vec<_>) = frames.into_iter().unzip();
            (
                left_bits.into_iter().map(f32::from_bits).collect(),
                right_bits.into_iter().map(f32::from_bits).collect(),
            )
        })
    }

    fn finite_samples() -> impl Strategy<Value = Vec<f32>> {
        prop::collection::vec(any::<i16>().prop_map(f32::from), 0..=256)
    }

    fn rms(samples: &[f32]) -> f32 {
        let sum_sq: f64 = samples.iter().map(|&x| f64::from(x) * f64::from(x)).sum();
        ((sum_sq / samples.len() as f64).sqrt()) as f32
    }

    fn sine(n: usize, freq_hz: f32, amp: f32, sample_rate: f32) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * PI * freq_hz * i as f32 / sample_rate).sin())
            .collect()
    }

    fn render_sine(
        processor: &mut OttProcessor,
        start: usize,
        frames: usize,
        sample_rate: f32,
    ) -> Vec<f32> {
        let input: Vec<f32> = (start..start + frames)
            .map(|i| 0.05 * (2.0 * PI * 1_000.0 * i as f32 / sample_rate).sin())
            .collect();
        let mut output = vec![0.0; frames];
        let mut right = vec![0.0; frames];
        processor
            .process(&input, &input, &mut output, &mut right)
            .unwrap();
        output
    }

    fn bypass_probe_params() -> OttParams {
        let mut params = Preset::Default.params();
        params.global.depth = NormalizedF32::new_const(1.0);
        params.global.input_gain_db = IoGain::new_const(0.0);
        params.global.output_gain_db = IoGain::new_const(-12.0);
        params
    }

    fn rms_peak_windows(samples: &[f32]) -> f32 {
        samples
            .windows(BYPASS_RMS_WINDOW)
            .step_by(BYPASS_RMS_HOP)
            .map(rms)
            .fold(0.0, f32::max)
    }

    fn warmed_rms(params: OttParams, snapshot: ControlSnapshot) -> f32 {
        let sample_rate = 48_000.0;
        let mut processor = OttProcessor::new(sample_rate, params).unwrap();
        processor.set_control_snapshot(snapshot).unwrap();
        let warm_frames = (sample_rate as usize) * 2;
        let _ = render_sine(&mut processor, 0, warm_frames, sample_rate);
        rms(&render_sine(
            &mut processor,
            warm_frames,
            BYPASS_RMS_WINDOW,
            sample_rate,
        ))
    }

    #[test]
    fn coordinated_bypass_transitions_stay_within_warmed_endpoint_levels() {
        let sample_rate = 48_000.0;
        let effect = bypass_probe_params();
        let bypass = ControlSnapshot {
            params: effect,
            bypass_engaged: true,
        };
        let active = ControlSnapshot {
            params: effect,
            bypass_engaged: false,
        };
        let endpoint_limit = warmed_rms(effect, bypass).max(warmed_rms(effect, active))
            * db_to_amp(BYPASS_TRANSITION_TOLERANCE_DB);

        for (name, initial, next) in [
            ("effect to bypass", active, bypass),
            ("bypass to effect", bypass, active),
        ] {
            let mut processor = OttProcessor::new(sample_rate, effect).unwrap();
            processor.set_control_snapshot(initial).unwrap();
            let warm_frames = (sample_rate as usize) * 2;
            let _ = render_sine(&mut processor, 0, warm_frames, sample_rate);
            processor.set_control_snapshot(next).unwrap();
            let transition = render_sine(
                &mut processor,
                warm_frames,
                sample_rate as usize,
                sample_rate,
            );
            let peak = rms_peak_windows(&transition);
            assert!(
                peak <= endpoint_limit,
                "{name}: 10 ms RMS peak {peak} exceeds endpoint limit {endpoint_limit}"
            );
        }
    }

    #[test]
    fn bypass_transition_with_non_unity_input_gain_stays_within_endpoint_levels() {
        let sample_rate = 48_000.0;
        let mut effect = bypass_probe_params();
        effect.global.input_gain_db = IoGain::new_const(6.0);
        effect.global.output_gain_db = IoGain::new_const(-18.0);
        let active = ControlSnapshot {
            params: effect,
            bypass_engaged: false,
        };
        let bypass = ControlSnapshot {
            params: effect,
            bypass_engaged: true,
        };
        let endpoint_limit = warmed_rms(effect, bypass).max(warmed_rms(effect, active))
            * db_to_amp(BYPASS_TRANSITION_TOLERANCE_DB);
        let mut processor = OttProcessor::new(sample_rate, effect).unwrap();
        processor.set_control_snapshot(active).unwrap();
        let warm_frames = (sample_rate as usize) * 2;
        let _ = render_sine(&mut processor, 0, warm_frames, sample_rate);
        processor.set_control_snapshot(bypass).unwrap();
        let peak = rms_peak_windows(&render_sine(
            &mut processor,
            warm_frames,
            sample_rate as usize,
            sample_rate,
        ));
        assert!(
            peak <= endpoint_limit,
            "non-unity input: 10 ms RMS peak {peak} exceeds endpoint limit {endpoint_limit}"
        );
    }

    #[test]
    fn bypass_reversal_uses_the_latest_snapshot_after_the_safe_waypoint() {
        let sample_rate = 48_000.0;
        let initial = bypass_probe_params();
        let mut latest = initial;
        latest.global.depth = NormalizedF32::new_const(0.25);
        latest.global.output_gain_db = IoGain::new_const(-18.0);
        let mut processor = OttProcessor::new(sample_rate, initial).unwrap();
        processor
            .set_control_snapshot(ControlSnapshot {
                params: initial,
                bypass_engaged: true,
            })
            .unwrap();
        let _ = render_sine(&mut processor, 0, 240, sample_rate);
        processor
            .set_control_snapshot(ControlSnapshot {
                params: latest,
                bypass_engaged: false,
            })
            .unwrap();
        let output = render_sine(&mut processor, 240, sample_rate as usize * 2, sample_rate);
        assert!(output.iter().all(|sample| sample.is_finite()));
        assert_eq!(processor.bypass.stage, BypassStage::Active);
        assert_eq!(processor.target_params, latest);
        assert_eq!(processor.global.depth.target(), latest.global.depth.get());
        assert_eq!(
            processor.global.output_gain_db.target(),
            latest.global.output_gain_db.get()
        );
    }

    #[test]
    fn engaging_depth_reversal_routes_directly_to_latest_active_gains() {
        let sample_rate = 48_000.0;
        let initial = bypass_probe_params();
        let mut latest = initial;
        latest.global.output_gain_db = IoGain::new_const(-18.0);
        let mut processor = OttProcessor::new(sample_rate, initial).unwrap();
        processor
            .set_control_snapshot(ControlSnapshot {
                params: initial,
                bypass_engaged: true,
            })
            .unwrap();
        let _ = render_sine(&mut processor, 0, 240, sample_rate);
        assert_eq!(processor.bypass.stage, BypassStage::EngagingDepth);
        processor
            .set_control_snapshot(ControlSnapshot {
                params: latest,
                bypass_engaged: false,
            })
            .unwrap();
        let _ = render_sine(&mut processor, 240, 8_000, sample_rate);
        assert_eq!(processor.bypass.stage, BypassStage::DisengagingGains);
        assert_eq!(
            processor.global.output_gain_db.target(),
            latest.global.output_gain_db.get()
        );
    }

    #[test]
    fn zero_depth_gain_stages_reverse_without_visiting_the_opposite_endpoint() {
        let sample_rate = 48_000.0;
        let mut active = bypass_probe_params();
        active.global.input_gain_db = IoGain::new_const(6.0);
        let mut latest = active;
        latest.global.input_gain_db = IoGain::new_const(-6.0);
        latest.global.output_gain_db = IoGain::new_const(-18.0);
        let mut processor = OttProcessor::new(sample_rate, active).unwrap();
        processor
            .set_control_snapshot(ControlSnapshot {
                params: active,
                bypass_engaged: true,
            })
            .unwrap();
        let _ = render_sine(&mut processor, 0, 8_000, sample_rate);
        assert_eq!(processor.bypass.stage, BypassStage::EngagingGains);
        processor
            .set_control_snapshot(ControlSnapshot {
                params: latest,
                bypass_engaged: false,
            })
            .unwrap();
        assert_eq!(processor.bypass.stage, BypassStage::DisengagingGains);
        assert_eq!(
            processor.global.input_gain_db.target(),
            latest.global.input_gain_db.get()
        );
        assert_eq!(
            processor.global.output_gain_db.target(),
            latest.global.output_gain_db.get()
        );

        processor
            .set_control_snapshot(ControlSnapshot {
                params: latest,
                bypass_engaged: true,
            })
            .unwrap();
        assert_eq!(processor.bypass.stage, BypassStage::EngagingGains);
        assert_eq!(processor.global.input_gain_db.target(), 0.0);
        assert_eq!(processor.global.output_gain_db.target(), 0.0);
    }

    #[test]
    fn latest_pot_target_updates_the_depth_stage_without_restarting_it() {
        let sample_rate = 48_000.0;
        let initial = bypass_probe_params();
        let mut latest = initial;
        latest.global.depth = NormalizedF32::new_const(0.25);
        latest.global.input_gain_db = IoGain::new_const(6.0);
        latest.global.output_gain_db = IoGain::new_const(-18.0);
        let mut processor = OttProcessor::new(sample_rate, initial).unwrap();
        processor
            .set_control_snapshot(ControlSnapshot {
                params: initial,
                bypass_engaged: true,
            })
            .unwrap();
        let _ = render_sine(&mut processor, 0, sample_rate as usize, sample_rate);
        processor
            .set_control_snapshot(ControlSnapshot {
                params: initial,
                bypass_engaged: false,
            })
            .unwrap();
        // Gain restoration takes about 140 ms; at this point depth is already
        // moving, so this specifically exercises a latest target mid-stage.
        let _ = render_sine(&mut processor, sample_rate as usize, 8_000, sample_rate);
        assert_eq!(processor.bypass.stage, BypassStage::DisengagingDepth);
        let held_input_target = processor.global.input_gain_db.target();
        let held_output_target = processor.global.output_gain_db.target();
        processor
            .set_control_snapshot(ControlSnapshot {
                params: latest,
                bypass_engaged: false,
            })
            .unwrap();
        assert_eq!(processor.global.input_gain_db.target(), held_input_target);
        assert_eq!(processor.global.output_gain_db.target(), held_output_target);
        assert_eq!(processor.global.depth.target(), latest.global.depth.get());
        let output = render_sine(
            &mut processor,
            sample_rate as usize + 8_000,
            sample_rate as usize,
            sample_rate,
        );
        assert!(output.iter().all(|sample| sample.is_finite()));
        assert_eq!(processor.bypass.stage, BypassStage::Active);
        assert_eq!(processor.global.depth.target(), latest.global.depth.get());
        assert_eq!(
            processor.global.input_gain_db.target(),
            latest.global.input_gain_db.get()
        );
        assert_eq!(
            processor.global.output_gain_db.target(),
            latest.global.output_gain_db.get()
        );
    }

    #[test]
    fn reset_preserves_the_explicit_bypassed_steady_state() {
        let params = bypass_probe_params();
        let mut processor = OttProcessor::new(48_000.0, params).unwrap();
        processor
            .set_control_snapshot(ControlSnapshot {
                params,
                bypass_engaged: true,
            })
            .unwrap();
        processor.reset(96_000.0).unwrap();
        assert_eq!(processor.bypass.stage, BypassStage::Bypassed);
        assert_eq!(processor.global.depth.current(), 0.0);
        assert_eq!(processor.global.input_gain_db.current(), 0.0);
        assert_eq!(processor.global.output_gain_db.current(), 0.0);
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
    fn band_applies_one_dynamic_gain_to_asymmetric_stereo_input() {
        let sample_rate = 48_000.0;
        let params = Preset::Default.params();
        let mut band = BandProcessor::new(&params.bands.mid, sample_rate);
        let frame = FrameControls {
            time: params.global.time.get(),
            upward: params.global.upward.get(),
            downward: params.global.downward.get(),
            depth: 1.0,
        };

        // A hard-panned, loud signal must still drive one shared gain for the
        // quieter channel (docs/contracts.md §4, ADR 0002).
        let left_in = 0.02;
        let right_in = 0.5;
        for _ in 0..2_000 {
            let (left_out, right_out) = band.process(left_in, right_in, &frame, sample_rate);
            let left_gain = left_out / left_in;
            let right_gain = right_out / right_in;
            let tolerance = 1e-5 * left_gain.abs().max(right_gain.abs()).max(1.0);
            assert!(
                (left_gain - right_gain).abs() <= tolerance,
                "left gain {left_gain} differs from right gain {right_gain}"
            );
        }
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

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(PROPERTY_CASES))]

        #[test]
        fn arbitrary_audio_samples_produce_only_finite_output(
            (input_l, input_r) in arbitrary_stereo_samples(),
        ) {
            let mut processor = OttProcessor::new(48_000.0, Preset::Default.params()).unwrap();
            let mut output_l = vec![0.0; input_l.len()];
            let mut output_r = vec![0.0; input_r.len()];

            prop_assert_eq!(
                processor.process(&input_l, &input_r, &mut output_l, &mut output_r),
                Ok(())
            );
            prop_assert!(output_l.iter().all(|sample| sample.is_finite()));
            prop_assert!(output_r.iter().all(|sample| sample.is_finite()));
        }

        #[test]
        fn mismatched_buffers_leave_outputs_unchanged(
            input_l in arbitrary_samples(),
            input_r in arbitrary_samples(),
            mut output_l in finite_samples(),
            mut output_r in finite_samples(),
        ) {
            prop_assume!(
                input_l.len() != input_r.len()
                    || input_l.len() != output_l.len()
                    || input_l.len() != output_r.len()
            );
            let original_l = output_l.clone();
            let original_r = output_r.clone();
            let mut processor = OttProcessor::new(48_000.0, Preset::SafeStart.params()).unwrap();

            prop_assert_eq!(
                processor.process(&input_l, &input_r, &mut output_l, &mut output_r),
                Err(ProcessError::BufferLengthMismatch)
            );
            prop_assert_eq!(output_l, original_l);
            prop_assert_eq!(output_r, original_r);
        }

        #[test]
        fn arbitrary_chunkings_are_bit_identical(
            (input_l, input_r) in arbitrary_stereo_samples(),
            chunk_hints in prop::collection::vec(any::<u8>(), 1..=16),
        ) {
            let params = Preset::Default.params();
            let mut whole_processor = OttProcessor::new(48_000.0, params).unwrap();
            let mut whole_l = vec![0.0; input_l.len()];
            let mut whole_r = vec![0.0; input_r.len()];
            prop_assert_eq!(
                whole_processor.process(&input_l, &input_r, &mut whole_l, &mut whole_r),
                Ok(())
            );

            let mut chunked_processor = OttProcessor::new(48_000.0, params).unwrap();
            let mut chunked_l = vec![0.0; input_l.len()];
            let mut chunked_r = vec![0.0; input_r.len()];
            let mut offset = 0;
            let mut chunk_index = 0;
            while offset < input_l.len() {
                let remaining = input_l.len() - offset;
                let hint = usize::from(chunk_hints[chunk_index % chunk_hints.len()]);
                let chunk_len = 1 + hint % remaining;
                let end = offset + chunk_len;
                prop_assert_eq!(
                    chunked_processor.process(
                        &input_l[offset..end],
                        &input_r[offset..end],
                        &mut chunked_l[offset..end],
                        &mut chunked_r[offset..end],
                    ),
                    Ok(())
                );
                offset = end;
                chunk_index += 1;
            }

            prop_assert_eq!(chunked_l, whole_l);
            prop_assert_eq!(chunked_r, whole_r);
        }

        #[test]
        fn rejected_parameter_updates_leave_processing_state_unchanged(
            sample_rate in 8_000_u32..=17_000,
            (input_l, input_r) in arbitrary_stereo_samples(),
        ) {
            let sample_rate = sample_rate as f32;
            let params = Preset::Default.params();
            let mut processor = OttProcessor::new(sample_rate, params).unwrap();
            let mut control = OttProcessor::new(sample_rate, params).unwrap();
            let mut invalid_params = params;
            invalid_params.global.crossover = CrossoverSplit::new_const(
                CrossoverFreqLow::new_const(500.0),
                CrossoverFreqHigh::new_const(8_000.0),
            );
            prop_assert!(processor.set_params(invalid_params).is_err());

            let mut output_l = vec![0.0; input_l.len()];
            let mut output_r = vec![0.0; input_r.len()];
            let mut control_l = vec![0.0; input_l.len()];
            let mut control_r = vec![0.0; input_r.len()];
            prop_assert_eq!(
                processor.process(&input_l, &input_r, &mut output_l, &mut output_r),
                Ok(())
            );
            prop_assert_eq!(
                control.process(&input_l, &input_r, &mut control_l, &mut control_r),
                Ok(())
            );
            prop_assert_eq!(output_l, control_l);
            prop_assert_eq!(output_r, control_r);
        }

        #[test]
        fn valid_lifecycle_operation_sequences_keep_output_finite(
            operations in prop::collection::vec((0_u8..3, any::<u8>(), any::<u8>(), 8_000_u32..=384_000, any::<u32>(), any::<u32>()), 1..=64),
        ) {
            let mut params = Preset::Default.params();
            let mut processor = OttProcessor::new(48_000.0, params).unwrap();
            for (operation, first, second, sample_rate, left_bits, right_bits) in operations {
                match operation {
                    0 => {
                        let input_l = [f32::from_bits(left_bits)];
                        let input_r = [f32::from_bits(right_bits)];
                        let mut output_l = [0.0];
                        let mut output_r = [0.0];
                        prop_assert_eq!(
                            processor.process(&input_l, &input_r, &mut output_l, &mut output_r),
                            Ok(())
                        );
                        prop_assert!(output_l[0].is_finite());
                        prop_assert!(output_r[0].is_finite());
                    }
                    1 => {
                        params.global.input_gain_db = IoGain::new_const(f32::from(first % 49) - 24.0);
                        params.global.output_gain_db = IoGain::new_const(f32::from(second % 49) - 24.0);
                        params.global.depth = NormalizedF32::new_const(f32::from(first) / f32::from(u8::MAX));
                        params.global.time = NormalizedF32::new_const(f32::from(second) / f32::from(u8::MAX));
                        prop_assert_eq!(processor.set_params(params), Ok(()));
                    }
                    _ => {
                        prop_assert_eq!(processor.reset(sample_rate as f32), Ok(()));
                    }
                }
            }
        }
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
