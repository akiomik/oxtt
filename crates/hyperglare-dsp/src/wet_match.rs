//! Scaling the resonators to the input, so that the mix knob is a mix knob.
//!
//! # The problem this exists for
//!
//! How loud the bank comes out depends on how much of the input's spectrum
//! happens to land on the grid, and that varies enormously: a bass whose
//! partials sit on the grid excites it strongly, while a source whose energy
//! falls between the resonators barely moves it. Measured on real material,
//! the wet path came out about 20 dB below the dry.
//!
//! A linear crossfade between signals 20 dB apart is not a crossfade. The dry
//! goes on dominating until the wet's share has climbed most of the way to
//! one, so the useful part of the knob is squeezed into its last tenth.
//! Measured, on a percussive source, by spectral flatness — 0.49 at the dry
//! end, and still 0.43 at nine tenths of the way across:
//!
//! ```text
//! colour     0.0    0.5    0.9    0.98   1.0
//! flatness   0.486  0.489  0.428  0.145  0.034
//! ```
//!
//! Nine tenths of the control did nothing.
//!
//! # What it does
//!
//! Follows both levels and scales the wet to the dry, so that half way across
//! is half of each. The bank's own divisors already keep the wet consistent
//! *within* a setting — the voice count, the geometry's density and the decay
//! all divide out — and this is the one that was missing between the wet and
//! the input it is mixed against.
//!
//! Slow on purpose. A fast tracker would follow the dry's shape rather than
//! its level and turn the wet into a copy of the input's envelope, which is
//! the opposite of what a resonator is for: the ring after a transient is the
//! part worth hearing, and a tracker that pulled it down would remove it.
//!
//! # What it does not do
//!
//! It does not run while the input is silent. There is nothing to match to,
//! and a ratio of two decaying envelopes is a way to manufacture a number from
//! noise; the gain simply holds, which also means a tail rings out at the gain
//! it was already at rather than being swept as it decays.

use effectkit::decibels::db_to_amp;

/// How fast the level followers move, in milliseconds.
///
/// Long enough to be a level rather than a shape. Three hundred milliseconds
/// is past the point where a listener hears gain movement as part of the
/// sound.
const FOLLOWER_MS: f32 = 300.0;

/// How fast the correction itself moves, in milliseconds.
///
/// Slower again than the followers, so that a change in what the input is
/// doing reaches the gain as a drift rather than as a step.
const GAIN_MS: f32 = 500.0;

/// Bounds on the correction, in decibels.
///
/// A source with nothing on the grid would otherwise ask for an unbounded
/// boost, and get a bank of resonators lifted out of the noise floor.
const MAX_GAIN_DB: f32 = 24.0;
/// The other bound, for a wet that came out louder than the dry.
const MIN_GAIN_DB: f32 = -24.0;

/// How fast the presence detector opens and closes, in milliseconds.
///
/// Far shorter than the followers, because it answers a different question:
/// not "how loud is the input" but "is there one". A slow answer to that would
/// leave the gain tracking for a second into a tail, chasing the ratio of a
/// level that has stopped against one that is still decaying — and arriving
/// somewhere arbitrary.
const PRESENCE_ATTACK_MS: f32 = 5.0;
/// The other half of it, long enough not to chatter between transients.
const PRESENCE_RELEASE_MS: f32 = 80.0;

/// Below this the input counts as absent and the gain holds. Roughly -60 dBFS.
const DRY_FLOOR: f32 = 1e-3;
/// Floors the divisor so the ratio cannot divide by zero.
const WET_FLOOR: f32 = 1e-9;

/// One-pole coefficients for the followers and the gain.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WetMatchCoeffs {
    follower: f32,
    gain: f32,
    presence_attack: f32,
    presence_release: f32,
}

impl WetMatchCoeffs {
    /// Derives the coefficients for a sample rate.
    #[must_use]
    pub fn new(sample_rate: f32) -> Self {
        Self {
            follower: one_pole(FOLLOWER_MS, sample_rate),
            gain: one_pole(GAIN_MS, sample_rate),
            presence_attack: one_pole(PRESENCE_ATTACK_MS, sample_rate),
            presence_release: one_pole(PRESENCE_RELEASE_MS, sample_rate),
        }
    }
}

/// `exp(-1 / samples)`, floored so a degenerate sample rate cannot divide by
/// zero.
#[inline]
fn one_pole(time_ms: f32, sample_rate: f32) -> f32 {
    let samples = (time_ms * 0.001 * sample_rate).max(1.0);
    (-1.0 / samples).exp()
}

/// Follows the dry and the wet, and reports the gain that matches them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WetMatch {
    dry: f32,
    wet: f32,
    presence: f32,
    gain: f32,
}

impl Default for WetMatch {
    fn default() -> Self {
        Self::new()
    }
}

impl WetMatch {
    /// A matcher at rest, with unity gain.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            dry: 0.0,
            wet: 0.0,
            presence: 0.0,
            gain: 1.0,
        }
    }

    /// Clears the followers and returns the gain to unity.
    pub const fn reset(&mut self) {
        self.dry = 0.0;
        self.wet = 0.0;
        self.presence = 0.0;
        self.gain = 1.0;
    }

    /// The gain currently being applied.
    #[must_use]
    pub const fn gain(&self) -> f32 {
        self.gain
    }

    /// Observes one frame and returns the wet, scaled to the dry.
    ///
    /// `amount` interpolates the correction in decibels: `0.0` leaves the wet
    /// exactly as the bank produced it, `1.0` matches it to the dry. The
    /// middle is a real setting rather than a fudge — a source that wants some
    /// of the bank's own dynamics back asks for less than all of it.
    #[inline]
    pub fn process(&mut self, dry: f32, wet: f32, amount: f32, coeffs: &WetMatchCoeffs) -> f32 {
        let level = dry.abs();
        let c = coeffs.follower;
        self.dry = c.mul_add(self.dry - level, level);
        self.wet = c.mul_add(self.wet - wet.abs(), wet.abs());

        // Fast, because this asks whether there *is* an input rather than how
        // loud it is.
        let p = if level > self.presence {
            coeffs.presence_attack
        } else {
            coeffs.presence_release
        };
        self.presence = p.mul_add(self.presence - level, level);

        // Only while there is an input to match to. A ratio of two decaying
        // envelopes says nothing, and sweeping the gain across a tail would
        // reshape the one part of the output that is the effect's own.
        if self.presence > DRY_FLOOR {
            let target = (self.dry / self.wet.max(WET_FLOOR))
                .clamp(db_to_amp(MIN_GAIN_DB), db_to_amp(MAX_GAIN_DB));
            self.gain = coeffs.gain.mul_add(self.gain - target, target);
        }

        let amount = amount.clamp(0.0, 1.0);
        // Interpolated in decibels rather than linearly, so that half of a
        // 20 dB correction is 10 dB and not a fifth of the way to it.
        let applied = powf_from_db(amount * amp_to_db(self.gain));
        wet * applied
    }
}

/// `20·log10(amp)`, floored so a zero gain does not produce an infinity.
#[inline]
fn amp_to_db(amp: f32) -> f32 {
    20.0 * amp.max(WET_FLOOR).log10()
}

/// `10^(db/20)`.
#[inline]
fn powf_from_db(db: f32) -> f32 {
    db_to_amp(db)
}

#[cfg(test)]
// Test-only arithmetic over sample indices: small counts, positive by
// construction.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::arithmetic_side_effects,
    clippy::suboptimal_flops
)]
mod tests {
    use core::f32::consts::PI;

    use super::*;

    const SR: f32 = 48_000.0;

    /// Runs a dry and a wet at fixed levels and returns the settled gain.
    fn settle(dry_level: f32, wet_level: f32, amount: f32) -> (f32, f32) {
        let coeffs = WetMatchCoeffs::new(SR);
        let mut matcher = WetMatch::new();
        let mut last = 0.0;
        for i in 0..(SR as usize * 4) {
            let phase = 2.0 * PI * 220.0 * i as f32 / SR;
            let dry = phase.sin() * dry_level;
            let wet = (phase * 1.5).sin() * wet_level;
            last = matcher.process(dry, wet, amount, &coeffs);
        }
        (matcher.gain(), last)
    }

    /// The point of the whole thing: a wet 20 dB down comes back up.
    #[test]
    fn a_quiet_wet_is_matched_to_the_dry() {
        let (gain, _) = settle(0.5, 0.05, 1.0);
        let db = 20.0 * gain.log10();
        assert!(
            (db - 20.0).abs() < 1.5,
            "a wet 20 dB down should be corrected by about 20 dB, got {db}"
        );
    }

    /// And a wet that came out louder is brought down, so the correction is
    /// not a one-way boost.
    #[test]
    fn a_loud_wet_is_brought_down() {
        let (gain, _) = settle(0.05, 0.5, 1.0);
        let db = 20.0 * gain.log10();
        assert!((db + 20.0).abs() < 1.5, "expected about -20 dB, got {db}");
    }

    /// The amount interpolates in decibels, so half of a correction is half of
    /// it in the unit a listener hears.
    #[test]
    fn the_amount_interpolates_in_decibels() {
        let full = 20.0 * settle(0.5, 0.05, 1.0).0.log10();
        let (_, out_half) = settle(0.5, 0.05, 0.5);
        let (_, out_none) = settle(0.5, 0.05, 0.0);
        let half_db = 20.0 * (out_half.abs().max(1e-12) / out_none.abs().max(1e-12)).log10();
        assert!(
            (half_db - full / 2.0).abs() < 1.5,
            "half the amount should be half the decibels: {half_db} vs {}",
            full / 2.0
        );
    }

    /// Zero amount leaves the wet exactly alone, so the raw behaviour is still
    /// reachable.
    #[test]
    fn zero_amount_is_a_bypass() {
        let coeffs = WetMatchCoeffs::new(SR);
        let mut matcher = WetMatch::new();
        #[allow(clippy::float_cmp)]
        for i in 0..4_800 {
            let phase = 2.0 * PI * 220.0 * i as f32 / SR;
            let wet = phase.sin() * 0.01;
            assert_eq!(matcher.process(phase.sin() * 0.5, wet, 0.0, &coeffs), wet);
        }
    }

    /// The gain holds while the input is silent, rather than chasing the ratio
    /// of two decaying envelopes.
    ///
    /// Held from the moment the presence detector closes, which is tens of
    /// milliseconds rather than instantly — so this checks that it has stopped
    /// moving, not that it never moved.
    #[test]
    fn the_gain_holds_through_silence() {
        let coeffs = WetMatchCoeffs::new(SR);
        let mut matcher = WetMatch::new();
        for i in 0..(SR as usize * 3) {
            let phase = 2.0 * PI * 220.0 * i as f32 / SR;
            matcher.process(phase.sin() * 0.5, (phase * 1.5).sin() * 0.05, 1.0, &coeffs);
        }

        // Half a second into the tail, by which time the detector has closed.
        let mut early = 0.0;
        for i in 0..(SR as usize * 2) {
            let decay = (-(i as f32) / SR).exp();
            matcher.process(0.0, decay * 0.05, 1.0, &coeffs);
            if i == SR as usize / 2 {
                early = matcher.gain();
            }
        }
        assert!(
            (matcher.gain() / early - 1.0).abs() < 1e-4,
            "the gain went on moving through the tail: {early} -> {}",
            matcher.gain()
        );
    }

    /// Silence in still gives silence out: the gain multiplies a zero.
    #[test]
    fn silence_survives_the_correction() {
        let coeffs = WetMatchCoeffs::new(SR);
        let mut matcher = WetMatch::new();
        #[allow(clippy::float_cmp)]
        for _ in 0..4_800 {
            assert_eq!(matcher.process(0.0, 0.0, 1.0, &coeffs), 0.0);
        }
    }

    /// A wet of nothing asks for an unbounded boost and does not get one.
    #[test]
    fn the_correction_is_bounded() {
        let (gain, _) = settle(0.5, 0.0, 1.0);
        assert!(
            20.0 * gain.log10() <= MAX_GAIN_DB + 0.1,
            "the boost should be bounded, got {}",
            20.0 * gain.log10()
        );
        assert!(gain.is_finite());
    }
}
