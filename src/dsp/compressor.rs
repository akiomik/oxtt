//! Dual-threshold gain computer (ADR 0003).
//!
//! `amount` is not a conventional ratio but the change in slope beyond the
//! threshold. 0 means no processing; 1 pins the signal to the threshold
//! under extreme compression.

use crate::dsp::db_to_amp;
use crate::dsp::envelope::BandEnvelope;

/// Lower clamp for the combined gain (docs/contracts.md §4).
pub const MIN_DYNAMIC_GAIN_DB: f32 = -60.0;
/// Upper clamp for the combined gain (docs/contracts.md §4).
pub const MAX_DYNAMIC_GAIN_DB: f32 = 30.0;

/// e.g. `effective_up_amount = clamp(band.up_amount * upward, 0, 1)` (ADR 0003).
#[inline]
#[must_use]
pub fn effective_amount(band_amount: f32, global_amount: f32) -> f32 {
    (band_amount * global_amount).clamp(0.0, 1.0)
}

/// Resolved dynamics settings applied to a single sample.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BandDynamics {
    /// Downward-compression threshold in dB.
    pub lower_threshold_db: f32,
    /// Upward-compression threshold in dB.
    pub upper_threshold_db: f32,
    /// Upward compression amount after applying the global multiplier, `0.0..=1.0`.
    pub effective_up_amount: f32,
    /// Downward compression amount after applying the global multiplier, `0.0..=1.0`.
    pub effective_down_amount: f32,
    /// Attack time in ms, already adjusted for the global `time` control.
    pub attack_ms: f32,
    /// Release time in ms, already adjusted for the global `time` control.
    pub release_ms: f32,
}

/// A single band's stereo-linked dual-threshold compressor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DualThresholdCompressor {
    envelope: BandEnvelope,
}

impl DualThresholdCompressor {
    /// Creates a compressor with its envelope initialized to the threshold midpoint (docs/contracts.md §2).
    #[must_use]
    pub fn new(lower_threshold_db: f32, upper_threshold_db: f32) -> Self {
        Self {
            envelope: BandEnvelope::new(lower_threshold_db, upper_threshold_db),
        }
    }

    /// Immediate reset on `reset` or a sample-rate change (docs/contracts.md §2).
    pub fn reset(&mut self, lower_threshold_db: f32, upper_threshold_db: f32) {
        self.envelope.reset(lower_threshold_db, upper_threshold_db);
    }

    /// Returns `false` if the envelope state has gone non-finite (docs/contracts.md §4).
    #[must_use]
    pub const fn is_finite(&self) -> bool {
        self.envelope.is_finite()
    }

    /// Returns the dynamic gain (linear amplitude) for one sample's detected power `p` (ADR 0003).
    #[inline]
    pub fn process(&mut self, p: f32, dynamics: &BandDynamics, sample_rate: f32) -> f32 {
        self.envelope.update(
            p,
            dynamics.lower_threshold_db,
            dynamics.upper_threshold_db,
            dynamics.attack_ms,
            dynamics.release_ms,
            sample_rate,
        );

        let up_gain_db = dynamics.effective_up_amount
            * (dynamics.lower_threshold_db - self.envelope.low_level_db());
        let down_gain_db = -dynamics.effective_down_amount
            * (self.envelope.high_level_db() - dynamics.upper_threshold_db);

        let dynamic_gain_db =
            (up_gain_db + down_gain_db).clamp(MIN_DYNAMIC_GAIN_DB, MAX_DYNAMIC_GAIN_DB);
        db_to_amp(dynamic_gain_db)
    }
}

#[cfg(test)]
// These tests compare exact deterministic gain values (e.g. silence in -> silence out),
// so float_cmp noise here is expected rather than a real risk.
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use crate::dsp::envelope::detector_power;

    fn steady_state_gain(dynamics: &BandDynamics, level_db: f32, sample_rate: f32) -> f32 {
        let mut comp =
            DualThresholdCompressor::new(dynamics.lower_threshold_db, dynamics.upper_threshold_db);
        let amp = db_to_amp(level_db);
        let p = detector_power(amp, amp);
        let mut gain = 1.0;
        for _ in 0..20_000 {
            gain = comp.process(p, dynamics, sample_rate);
        }
        gain
    }

    fn default_dynamics() -> BandDynamics {
        BandDynamics {
            lower_threshold_db: -35.0,
            upper_threshold_db: -28.0,
            effective_up_amount: 0.8,
            effective_down_amount: 0.9,
            attack_ms: 2.8,
            release_ms: 40.0,
        }
    }

    #[test]
    fn gain_is_0db_inside_thresholds() {
        let dynamics = default_dynamics();
        let sample_rate = 48_000.0;
        let mid_level_db = f32::midpoint(dynamics.lower_threshold_db, dynamics.upper_threshold_db);
        let gain = steady_state_gain(&dynamics, mid_level_db, sample_rate);
        assert!(
            (gain - 1.0).abs() < 1e-3,
            "gain {gain} should be ~0 dB inside thresholds"
        );
    }

    #[test]
    fn gain_is_0db_everywhere_when_amounts_are_zero() {
        let mut dynamics = default_dynamics();
        dynamics.effective_up_amount = 0.0;
        dynamics.effective_down_amount = 0.0;
        let sample_rate = 48_000.0;

        for level_db in [-80.0, -50.0, -35.0, -28.0, -10.0, 0.0] {
            let gain = steady_state_gain(&dynamics, level_db, sample_rate);
            assert!(
                (gain - 1.0).abs() < 1e-3,
                "level {level_db} dB: gain {gain} should be 0 dB when amount=0"
            );
        }
    }

    #[test]
    fn gain_is_positive_below_lower_threshold() {
        let dynamics = default_dynamics();
        let sample_rate = 48_000.0;
        let gain = steady_state_gain(&dynamics, -60.0, sample_rate);
        assert!(
            gain > 1.0,
            "gain {gain} should be > 0 dB below lower threshold"
        );
    }

    #[test]
    fn gain_is_negative_above_upper_threshold() {
        let dynamics = default_dynamics();
        let sample_rate = 48_000.0;
        let gain = steady_state_gain(&dynamics, 0.0, sample_rate);
        assert!(
            gain < 1.0,
            "gain {gain} should be < 0 dB above upper threshold"
        );
    }

    #[test]
    fn gain_clamp_does_not_exceed_limits() {
        let mut dynamics = default_dynamics();
        dynamics.effective_up_amount = 1.0;
        dynamics.effective_down_amount = 1.0;
        let sample_rate = 48_000.0;

        let gain_up = steady_state_gain(&dynamics, -120.0, sample_rate);
        let gain_up_db = 20.0 * gain_up.log10();
        assert!(
            gain_up_db <= MAX_DYNAMIC_GAIN_DB + 1e-3,
            "gain {gain_up_db} dB exceeds +30 dB clamp"
        );

        let gain_down = steady_state_gain(&dynamics, 6.0, sample_rate);
        let gain_down_db = 20.0 * gain_down.log10();
        assert!(
            gain_down_db >= MIN_DYNAMIC_GAIN_DB - 1e-3,
            "gain {gain_down_db} dB exceeds -60 dB clamp"
        );
    }

    #[test]
    fn silence_stays_silent_even_with_max_upward_gain() {
        // Silence stays silent even after +30 dB (docs/contracts.md §4).
        let mut dynamics = default_dynamics();
        dynamics.effective_up_amount = 1.0;
        dynamics.effective_down_amount = 0.0;
        let sample_rate = 48_000.0;

        let mut comp =
            DualThresholdCompressor::new(dynamics.lower_threshold_db, dynamics.upper_threshold_db);
        let mut output = 0.0_f32;
        for _ in 0..20_000 {
            let gain = comp.process(0.0, &dynamics, sample_rate);
            output = 0.0 * gain;
        }
        assert_eq!(output, 0.0);
    }

    #[test]
    fn effective_amount_clamps_to_unit_range() {
        assert_eq!(effective_amount(0.8, 2.0), 1.0);
        assert_eq!(effective_amount(0.8, -1.0), 0.0);
        assert!((effective_amount(0.8, 0.5) - 0.4).abs() < 1e-6);
    }

    #[test]
    fn init_state_yields_0db_gain() {
        // The dynamic gain computed right after initial construction is 0 dB (docs/contracts.md §2).
        let dynamics = default_dynamics();
        let mut comp =
            DualThresholdCompressor::new(dynamics.lower_threshold_db, dynamics.upper_threshold_db);
        let mid_level_db = f32::midpoint(dynamics.lower_threshold_db, dynamics.upper_threshold_db);
        let amp = db_to_amp(mid_level_db);
        let p = detector_power(amp, amp);
        let gain = comp.process(p, &dynamics, 48_000.0);
        // With the boundary power as the initial value, the first sample is also very close to 0 dB.
        assert!(
            (gain - 1.0).abs() < 0.05,
            "first-sample gain {gain} should start near 0 dB"
        );
    }
}
