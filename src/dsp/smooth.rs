//! Sample-rate-independent per-sample parameter smoothing (docs/architecture.md).
//!
//! Applies one-pole smoothing, `current[n] = c * current[n-1] + (1 - c) * target`,
//! fixed at 20ms. After 20ms the difference from the target is about 36.8%
//! (`1/e`) of the initial difference.

use std::f64::consts::LN_2;

/// Smoothing time constant (docs/architecture.md).
pub const SMOOTHING_TIME_MS: f32 = 20.0;

/// Remaining logarithmic crossover-frequency difference at which smoothing snaps to its target.
///
/// One cent is `1/1200` of an octave. At 0.1 cent, the final snap is far
/// below a perceptible pitch/frequency difference, but gives a finite
/// settled state so the real-time path can stop recalculating coefficients.
pub const CROSSOVER_SETTLE_CENTS: f64 = 0.1;

const CROSSOVER_SETTLE_LOG_HZ: f64 = CROSSOVER_SETTLE_CENTS * LN_2 / 1200.0;

fn log_smoothing_coefficient(sample_rate: f32) -> f64 {
    (-1.0 / (f64::from(SMOOTHING_TIME_MS) * 0.001 * f64::from(sample_rate))).exp()
}

/// Derives the one-pole coefficient from `SMOOTHING_TIME_MS` and `sample_rate`.
#[must_use]
pub fn smoothing_coefficient(sample_rate: f32) -> f32 {
    (-1.0 / (SMOOTHING_TIME_MS * 0.001 * sample_rate)).exp()
}

/// A current/target pair that applies one-pole smoothing to a linear value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Smoothed {
    current: f32,
    target: f32,
    coefficient: f32,
}

impl Smoothed {
    /// Constructs with an immediate snap to `value` (no startup fade).
    #[must_use]
    pub fn new(value: f32, sample_rate: f32) -> Self {
        Self {
            current: value,
            target: value,
            coefficient: smoothing_coefficient(sample_rate),
        }
    }

    /// Recomputes the coefficient on a sample-rate change. Does not change current/target.
    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.coefficient = smoothing_coefficient(sample_rate);
    }

    /// Updates the smoothing target value.
    pub const fn set_target(&mut self, target: f32) {
        self.target = target;
    }

    /// Immediately resets both current and target to the same value (initial construction, `reset`).
    pub const fn snap(&mut self, value: f32) {
        self.current = value;
        self.target = value;
    }

    /// Returns the current (smoothed) value.
    #[must_use]
    pub const fn current(&self) -> f32 {
        self.current
    }

    /// Returns the smoothing target value.
    #[must_use]
    pub const fn target(&self) -> f32 {
        self.target
    }

    /// Advances `current` toward `target` by one sample, returning the updated value.
    #[inline]
    pub fn tick(&mut self) -> f32 {
        self.current = self
            .coefficient
            .mul_add(self.current, (1.0 - self.coefficient) * self.target);
        self.current
    }
}

/// Wrapper for crossover frequencies that smooths on a logarithmic frequency scale (docs/architecture.md).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LogSmoothed {
    current_log_hz: f64,
    target_log_hz: f64,
    coefficient: f64,
}

impl LogSmoothed {
    /// Constructs with an immediate snap to `value_hz` (no startup fade).
    #[must_use]
    pub fn new(value_hz: f32, sample_rate: f32) -> Self {
        let value_log_hz = f64::from(value_hz).ln();
        Self {
            current_log_hz: value_log_hz,
            target_log_hz: value_log_hz,
            coefficient: log_smoothing_coefficient(sample_rate),
        }
    }

    /// Recomputes the coefficient on a sample-rate change. Does not change current/target.
    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.coefficient = log_smoothing_coefficient(sample_rate);
    }

    /// Updates the smoothing target frequency, in Hz.
    pub fn set_target_hz(&mut self, hz: f32) {
        self.target_log_hz = f64::from(hz).ln();
    }

    /// Immediately resets both current and target to `hz` (initial construction, `reset`).
    pub fn snap_hz(&mut self, hz: f32) {
        let log_hz = f64::from(hz).ln();
        self.current_log_hz = log_hz;
        self.target_log_hz = log_hz;
    }

    /// Returns the current (smoothed) value, in Hz.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)] // Validated crossover targets are far inside f32's finite range.
    pub fn current_hz(&self) -> f32 {
        self.current_log_hz.exp() as f32
    }

    /// Returns whether the current value has reached the target exactly.
    #[must_use]
    #[allow(clippy::float_cmp)] // Exact equality is the explicit finite settled-state sentinel.
    pub const fn is_settled(&self) -> bool {
        self.current_log_hz == self.target_log_hz
    }

    /// Advances the smoother and returns the effective frequency only when it changed.
    ///
    /// When the remaining log-frequency distance is within
    /// [`CROSSOVER_SETTLE_CENTS`], this snaps to the target. The exact settled
    /// state lets callers avoid recalculating filter coefficients indefinitely
    /// for a static crossover setting.
    #[inline]
    pub fn tick_hz_if_changed(&mut self) -> Option<f32> {
        if self.is_settled() {
            return None;
        }

        let target = self.target_log_hz;
        let next = self
            .coefficient
            .mul_add(self.current_log_hz, (1.0 - self.coefficient) * target);
        if (target - next).abs() <= CROSSOVER_SETTLE_LOG_HZ {
            self.current_log_hz = target;
        } else {
            self.current_log_hz = next;
        }
        Some(self.current_hz())
    }

    /// Advances by one sample and returns the current value in Hz.
    #[inline]
    pub fn tick_hz(&mut self) -> f32 {
        self.tick_hz_if_changed()
            .unwrap_or_else(|| self.current_hz())
    }
}

#[cfg(test)]
// Step counts derived from ms/sample_rate stay well within f32/usize's exact
// range, so narrowing casts here are intentional, not precision bugs.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::float_cmp
)]
mod tests {
    use super::*;

    #[test]
    fn reaches_36_8_percent_of_initial_difference_after_20ms() {
        let sample_rate = 48_000.0;
        let mut s = Smoothed::new(0.0, sample_rate);
        s.set_target(1.0);

        let steps = (0.020 * sample_rate).round() as usize;
        for _ in 0..steps {
            s.tick();
        }

        let remaining_diff = 1.0 - s.current();
        assert!(
            (remaining_diff - 1.0_f32.exp().recip()).abs() < 0.01,
            "remaining diff {remaining_diff} should be close to 1/e"
        );
    }

    #[test]
    fn snap_sets_current_and_target_immediately() {
        let mut s = Smoothed::new(0.0, 48_000.0);
        s.set_target(10.0);
        s.tick();
        assert_ne!(s.current(), 10.0);

        s.snap(5.0);
        assert_eq!(s.current(), 5.0);
        assert_eq!(s.target(), 5.0);
        assert_eq!(s.tick(), 5.0);
    }

    #[test]
    fn result_is_independent_of_chunking() {
        let sample_rate = 48_000.0;
        let mut a = Smoothed::new(0.0, sample_rate);
        a.set_target(1.0);
        let mut b = Smoothed::new(0.0, sample_rate);
        b.set_target(1.0);

        for _ in 0..100 {
            a.tick();
        }
        // Splitting the same 100 samples as e.g. 64+36 should still give the same result.
        for _ in 0..64 {
            b.tick();
        }
        for _ in 0..36 {
            b.tick();
        }

        assert_eq!(a.current(), b.current());
    }

    #[test]
    fn log_smoothed_converges_to_target_hz() {
        let sample_rate = 48_000.0;
        let mut s = LogSmoothed::new(120.0, sample_rate);
        s.set_target_hz(2500.0);

        // The first step must remain a smooth transition, not an immediate snap.
        let first = s
            .tick_hz_if_changed()
            .expect("a changed target must start a crossover transition");
        assert!(first > 120.0 && first < 2500.0);
        assert!(!s.is_settled());

        for _ in 0..(sample_rate as usize) {
            if s.tick_hz_if_changed().is_none() {
                break;
            }
        }
        assert!(
            s.is_settled(),
            "current={} target={} delta={} epsilon={CROSSOVER_SETTLE_LOG_HZ}",
            s.current_log_hz,
            s.target_log_hz,
            (s.current_log_hz - s.target_log_hz).abs(),
        );
        assert_eq!(s.current_hz(), 2500.0);
        assert_eq!(s.tick_hz_if_changed(), None);
    }

    #[test]
    fn sample_rate_change_does_not_cause_startup_fade() {
        let mut s = Smoothed::new(3.0, 48_000.0);
        s.set_sample_rate(96_000.0);
        assert_eq!(s.current(), 3.0);
        assert_eq!(s.target(), 3.0);
    }
}
