//! Stereo-linked power envelope follower (ADR 0002) and time conversion.

use crate::dsp::{db_to_amp, power_to_db};

/// State below this is flushed to 0 to avoid denormals.
const DENORMAL_FLOOR: f32 = 1e-30;

/// Minimum number of samples for attack/release.
const MIN_TIME_SAMPLES: f32 = 5.0;

/// Instantaneous detected power shared by L/R: `p[n] = max(left[n]^2, right[n]^2)` (ADR 0002).
#[inline]
#[must_use]
pub fn detector_power(left: f32, right: f32) -> f32 {
    (left * left).max(right * right)
}

/// `coefficient(t_ms) = exp(-1 / max(t_ms * 0.001 * sample_rate, 1))`.
#[inline]
#[must_use]
pub fn envelope_coefficient(time_ms: f32, sample_rate: f32) -> f32 {
    let n = (time_ms * 0.001 * sample_rate).max(1.0);
    (-1.0_f32 / n).exp()
}

#[inline]
fn flush_denormal(x: f32) -> f32 {
    if x.abs() < DENORMAL_FLOOR { 0.0 } else { x }
}

/// A single one-pole envelope update. Uses the attack coefficient when
/// `p[n] > env[n-1]`, otherwise the release coefficient.
#[inline]
#[must_use]
pub fn update_envelope(
    p: f32,
    prev_env: f32,
    attack_ms: f32,
    release_ms: f32,
    sample_rate: f32,
) -> f32 {
    let c = if p > prev_env {
        envelope_coefficient(attack_ms, sample_rate)
    } else {
        envelope_coefficient(release_ms, sample_rate)
    };
    flush_denormal(c.mul_add(prev_env, (1.0 - c) * p))
}

/// Derives the attack/release time multiplier from `time (0..1)`.
///
/// `time = 0.5` gives a multiplier of 1; 0 gives ~0.0183; 1 gives ~54.6.
#[inline]
#[must_use]
pub fn time_multiplier(time: f32) -> f32 {
    8.0f32.mul_add(time, -4.0).exp()
}

/// Derives the effective attack/release ms from the band's base values and `time`.
#[inline]
#[must_use]
pub fn attack_release_ms(
    base_attack_ms: f32,
    base_release_ms: f32,
    time: f32,
    sample_rate: f32,
) -> (f32, f32) {
    let multiplier = time_multiplier(time);
    let floor_ms = MIN_TIME_SAMPLES / sample_rate * 1000.0;
    let attack_ms = (base_attack_ms * multiplier).max(floor_ms);
    let release_ms = (base_release_ms * multiplier).max(floor_ms);
    (attack_ms, release_ms)
}

/// Band detector holding two independent envelope states, one for the upward
/// (low) side and one for the downward (high) side (ADR 0002).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BandEnvelope {
    low_env: f32,
    high_env: f32,
}

impl BandEnvelope {
    /// Initial construction. Snaps immediately to the boundary power,
    /// preventing startup at maximum gain from a state with no history
    /// (docs/contracts.md §2).
    #[must_use]
    pub fn new(lower_threshold_db: f32, upper_threshold_db: f32) -> Self {
        let mut e = Self {
            low_env: 0.0,
            high_env: 0.0,
        };
        e.reset(lower_threshold_db, upper_threshold_db);
        e
    }

    /// Immediate reset on `reset` or a sample-rate change (docs/contracts.md §2).
    /// The normal `set_params` path does not call this; it lets state converge
    /// via `update` instead.
    pub fn reset(&mut self, lower_threshold_db: f32, upper_threshold_db: f32) {
        self.low_env = db_to_amp(lower_threshold_db).powi(2);
        self.high_env = db_to_amp(upper_threshold_db).powi(2);
    }

    /// Returns `false` if either envelope has gone non-finite (docs/contracts.md §4).
    #[must_use]
    pub const fn is_finite(&self) -> bool {
        self.low_env.is_finite() && self.high_env.is_finite()
    }

    /// Returns the low envelope's power in dB.
    #[must_use]
    pub fn low_level_db(&self) -> f32 {
        power_to_db(self.low_env)
    }

    /// Returns the high envelope's power in dB.
    #[must_use]
    pub fn high_level_db(&self) -> f32 {
        power_to_db(self.high_env)
    }

    /// Updates both envelopes from one sample's detected power `p` (ADR 0003).
    #[inline]
    pub fn update(
        &mut self,
        p: f32,
        lower_threshold_db: f32,
        upper_threshold_db: f32,
        attack_ms: f32,
        release_ms: f32,
        sample_rate: f32,
    ) {
        let lower_power = db_to_amp(lower_threshold_db) * db_to_amp(lower_threshold_db);
        let upper_power = db_to_amp(upper_threshold_db) * db_to_amp(upper_threshold_db);

        let raw_low = update_envelope(p, self.low_env, attack_ms, release_ms, sample_rate);
        self.low_env = raw_low.min(lower_power);

        let raw_high = update_envelope(p, self.high_env, attack_ms, release_ms, sample_rate);
        self.high_env = raw_high.max(upper_power);
    }
}

#[cfg(test)]
// Step counts derived from ms*sample_rate stay well within f32/usize's exact
// range, so narrowing casts here are intentional, not precision bugs.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
mod tests {
    use super::*;

    #[test]
    fn detector_power_is_symmetric_peak_linked() {
        let expected = 0.75_f32.powi(2);
        let left_dominant = detector_power(-0.75, 0.25);
        let right_dominant = detector_power(0.25, -0.75);

        assert!((left_dominant - expected).abs() <= f32::EPSILON);
        assert!((right_dominant - expected).abs() <= f32::EPSILON);
    }

    #[test]
    fn step_response_is_monotonic_towards_target() {
        let sample_rate = 48_000.0;
        let mut env = 0.0_f32;
        let target_power = 1.0_f32;
        let mut prev_diff = (target_power - env).abs();
        for _ in 0..1000 {
            env = update_envelope(target_power, env, 5.0, 50.0, sample_rate);
            let diff = (target_power - env).abs();
            assert!(
                diff <= prev_diff + f32::EPSILON,
                "envelope should move monotonically towards target"
            );
            prev_diff = diff;
        }
    }

    #[test]
    fn attack_is_faster_than_release_for_equal_time_constants() {
        let sample_rate = 48_000.0;
        // Confirm that with the same time constant, attack is used when p>prev
        // and release when p<prev.
        let attack_ms = 1.0_f32;
        let release_ms = 100.0_f32;

        // Running for 5x the attack time constant (in samples) converges nearly completely.
        let attack_samples = (attack_ms * 0.001 * sample_rate).max(1.0);
        let steps = (attack_samples * 5.0) as usize;

        let mut rising = 0.0_f32;
        for _ in 0..steps {
            rising = update_envelope(1.0, rising, attack_ms, release_ms, sample_rate);
        }
        let mut falling = 1.0_f32;
        for _ in 0..steps {
            falling = update_envelope(0.0, falling, attack_ms, release_ms, sample_rate);
        }

        // attack (1ms) should track much faster than release (100ms).
        assert!(
            rising > 0.99,
            "fast attack should have nearly reached target, got {rising}"
        );
        assert!(
            falling > 0.9,
            "slow release should not have decayed much over the same duration, got {falling}"
        );
    }

    #[test]
    fn millisecond_response_matches_across_sample_rates() {
        // With a 5ms time constant, different sample rates should converge to a
        // similar degree after the same number of ms.
        for sample_rate in [44_100.0, 48_000.0, 96_000.0, 192_000.0] {
            let time_ms = 5.0_f32;
            let steps = (time_ms * 0.001 * sample_rate).round() as usize;
            let mut env = 0.0_f32;
            for _ in 0..steps {
                env = update_envelope(1.0, env, time_ms, time_ms, sample_rate);
            }
            let expected = 1.0 - 1.0_f32.exp().recip();
            assert!(
                (env - expected).abs() < 0.02,
                "sample_rate {sample_rate}: env {env} should be close to {expected}"
            );
        }
    }

    #[test]
    fn init_and_reset_snap_to_threshold_powers() {
        let lower_db = -35.0;
        let upper_db = -28.0;
        let env = BandEnvelope::new(lower_db, upper_db);

        let expected_low_db = power_to_db(db_to_amp(lower_db) * db_to_amp(lower_db));
        let expected_high_db = power_to_db(db_to_amp(upper_db) * db_to_amp(upper_db));

        assert!((env.low_level_db() - expected_low_db).abs() < 1e-3);
        assert!((env.high_level_db() - expected_high_db).abs() < 1e-3);
        assert!((env.low_level_db() - lower_db).abs() < 1e-3);
        assert!((env.high_level_db() - upper_db).abs() < 1e-3);
    }

    #[test]
    fn reset_after_use_snaps_back_to_threshold_powers() {
        let mut env = BandEnvelope::new(-35.0, -28.0);

        // low_env is clamped to at most lower_power, so pulling it down requires
        // feeding in a quiet signal below the threshold.
        for _ in 0..1000 {
            env.update(0.0, -35.0, -28.0, 5.0, 50.0, 48_000.0);
        }
        assert!(
            env.low_level_db() < -35.0 - 1.0,
            "quiet signal should pull low_env below lower threshold, got {}",
            env.low_level_db()
        );

        env.reset(-35.0, -28.0);
        assert!((env.low_level_db() - (-35.0)).abs() < 1e-3);
        assert!((env.high_level_db() - (-28.0)).abs() < 1e-3);

        // high_env is clamped to at least upper_power, so raising it requires
        // feeding in a loud signal above the threshold.
        let loud_amp = db_to_amp(0.0);
        let p = loud_amp * loud_amp;
        for _ in 0..1000 {
            env.update(p, -35.0, -28.0, 5.0, 50.0, 48_000.0);
        }
        assert!(
            env.high_level_db() > -28.0 + 1.0,
            "loud signal should push high_env above upper threshold, got {}",
            env.high_level_db()
        );

        env.reset(-35.0, -28.0);
        assert!((env.low_level_db() - (-35.0)).abs() < 1e-3);
        assert!((env.high_level_db() - (-28.0)).abs() < 1e-3);
    }

    #[test]
    fn time_multiplier_reference_points() {
        assert!((time_multiplier(0.5) - 1.0).abs() < 1e-6);
        assert!((time_multiplier(0.0) - 0.0183_f32).abs() < 0.001);
        assert!((time_multiplier(1.0) - 54.6_f32).abs() < 0.1);
    }

    #[test]
    fn attack_release_ms_respects_sample_rate_floor() {
        let sample_rate = 48_000.0;
        let (attack, release) = attack_release_ms(2.8, 40.0, 0.0, sample_rate);
        let floor_ms = MIN_TIME_SAMPLES / sample_rate * 1000.0;
        assert!(attack >= floor_ms - 1e-6);
        assert!(release >= floor_ms - 1e-6);
    }
}
