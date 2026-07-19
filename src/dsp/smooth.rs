//! Sample-rate-independent per-sample parameter smoothing (docs/architecture.md).
//!
//! Applies one-pole smoothing, `current[n] = c * current[n-1] + (1 - c) * target`,
//! fixed at 20ms. After 20ms the difference from the target is about 36.8%
//! (`1/e`) of the initial difference.

/// Smoothing time constant (docs/architecture.md).
pub const SMOOTHING_TIME_MS: f32 = 20.0;

/// Derives the one-pole coefficient from `SMOOTHING_TIME_MS` and sample_rate.
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
    pub fn set_target(&mut self, target: f32) {
        self.target = target;
    }

    /// Immediately resets both current and target to the same value (initial construction, `reset`).
    pub fn snap(&mut self, value: f32) {
        self.current = value;
        self.target = value;
    }

    pub fn current(&self) -> f32 {
        self.current
    }

    pub fn target(&self) -> f32 {
        self.target
    }

    /// Advances `current` toward `target` by one sample, returning the updated value.
    #[inline]
    pub fn tick(&mut self) -> f32 {
        self.current = self.coefficient * self.current + (1.0 - self.coefficient) * self.target;
        self.current
    }
}

/// Wrapper for crossover frequencies that smooths on a logarithmic frequency scale (docs/architecture.md).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LogSmoothed {
    inner: Smoothed,
}

impl LogSmoothed {
    pub fn new(value_hz: f32, sample_rate: f32) -> Self {
        Self {
            inner: Smoothed::new(value_hz.ln(), sample_rate),
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.inner.set_sample_rate(sample_rate);
    }

    pub fn set_target_hz(&mut self, hz: f32) {
        self.inner.set_target(hz.ln());
    }

    pub fn snap_hz(&mut self, hz: f32) {
        self.inner.snap(hz.ln());
    }

    pub fn current_hz(&self) -> f32 {
        self.inner.current().exp()
    }

    /// Advances by one sample and returns the current value in Hz.
    #[inline]
    pub fn tick_hz(&mut self) -> f32 {
        self.inner.tick().exp()
    }
}

#[cfg(test)]
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
        // f32's one-pole recursion stalls once the update falls below one ULP.
        // Here we confirm sufficient convergence by relative error (within
        // 0.1%) rather than absolute error.
        let sample_rate = 48_000.0;
        let mut s = LogSmoothed::new(120.0, sample_rate);
        s.set_target_hz(2500.0);
        for _ in 0..(sample_rate as usize) {
            s.tick_hz();
        }
        let relative_error = (s.current_hz() - 2500.0).abs() / 2500.0;
        assert!(
            relative_error < 1e-3,
            "relative error {relative_error} too large"
        );
    }

    #[test]
    fn sample_rate_change_does_not_cause_startup_fade() {
        let mut s = Smoothed::new(3.0, 48_000.0);
        s.set_sample_rate(96_000.0);
        assert_eq!(s.current(), 3.0);
        assert_eq!(s.target(), 3.0);
    }
}
