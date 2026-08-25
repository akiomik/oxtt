//! What arrived at a host's input, measured while it arrives.
//!
//! This lives beside the DSP rather than inside a host because it describes
//! the signal, not the platform: the loudest sample of a run and the number of
//! frames that hit full scale mean the same thing wherever the run happened.
//!
//! Only the Bela host reads it today, and the reason is a fact about the
//! hardware in front of each host rather than about the hosts themselves. A
//! JACK run is fed by an audio interface that has its own input meter and clip
//! indicator, so a level can be set by looking at the hardware; that is why
//! this problem never appeared there. A Gem's input gain is a codec register
//! with nothing to watch — clipping is silent and shows up only as a peak that
//! stops moving. A Raspberry Pi 5 behind an I2S HAT is in the second position,
//! not the first, and that platform is still on ADR 0009's table, so the
//! threshold, the decibel convention and the figures are defined once here
//! instead of inside `bela_host`.

/// Input magnitude at or above which a frame counts as clipped.
///
/// Just below full scale rather than at it. A converter that has run out of
/// range holds its extreme code, and what a host is handed is that code scaled
/// to float — for a 16-bit converter the positive extreme is `32767/32768`,
/// which never equals 1.0. The value to look for is therefore "as high as this
/// input goes", not a specific number.
pub const CLIP_THRESHOLD: f32 = 0.999;

/// The loudest input a run saw, and how much of it was off the top of the
/// scale.
///
/// Kept as a linear peak and converted once when reported: a logarithm per
/// frame would be real-time work for a figure only read after the run.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct InputMeter {
    peak: f32,
    clipped_frames: u64,
}

impl InputMeter {
    /// A meter that has seen nothing.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            peak: 0.0,
            clipped_frames: 0,
        }
    }

    /// Folds one stereo frame in.
    ///
    /// The louder of the two channels is enough for both figures: the peak is
    /// a maximum over everything, and a frame in which either channel clipped
    /// is a clipped frame. Splitting them per channel would say more, but not
    /// anything actionable — the input gain that would answer it is set for
    /// both channels at once.
    ///
    /// Real-time safe: two comparisons, no allocation, no branch that can
    /// panic (docs/contracts.md §6).
    #[inline]
    pub fn observe(&mut self, left: f32, right: f32) {
        let magnitude = left.abs().max(right.abs());
        if magnitude > self.peak {
            self.peak = magnitude;
        }
        if magnitude >= CLIP_THRESHOLD {
            // Saturating for `clippy::arithmetic_side_effects`
            // (docs/contracts.md §6). A `u64` of frames outlives the hardware
            // by a wide margin.
            self.clipped_frames = self.clipped_frames.saturating_add(1);
        }
    }

    /// Combines two meters, for a host whose callback state is per thread.
    ///
    /// The peak is the larger and the clipped frames add up, which is what
    /// makes this usable as a `fold` over however many render states a host
    /// hands back.
    #[must_use]
    pub const fn merged(self, other: Self) -> Self {
        Self {
            peak: self.peak.max(other.peak),
            clipped_frames: self.clipped_frames.saturating_add(other.clipped_frames),
        }
    }

    /// The loudest magnitude seen, linear.
    #[must_use]
    pub const fn peak(self) -> f32 {
        self.peak
    }

    /// The loudest magnitude seen, in dBFS, or negative infinity if the input
    /// was digitally silent.
    ///
    /// Unfloored, unlike the DSP's `power_to_db`: this is a measurement being
    /// reported rather than a value being fed back into a filter, and a floor
    /// would read as a signal that was not there. The offline renderer's
    /// `amplitude_to_db` makes the same choice.
    #[must_use]
    pub fn peak_dbfs(self) -> f32 {
        if self.peak > 0.0 {
            20.0 * self.peak.log10()
        } else {
            f32::NEG_INFINITY
        }
    }

    /// Frames in which either channel reached [`CLIP_THRESHOLD`].
    ///
    /// Zero is the only value that leaves [`peak_dbfs`](Self::peak_dbfs)
    /// meaning what it says; anything else makes the peak a floor rather than
    /// a measurement, because the converter ran out of range before the signal
    /// did.
    #[must_use]
    pub const fn clipped_frames(self) -> u64 {
        self.clipped_frames
    }
}

/// Turns clipped frames into something a human can see.
///
/// A clipped frame is 21 µs at 48 kHz, and an indicator lit for 21 µs is an
/// indicator that is never seen. This holds it on for a fixed number of frames
/// after the last one, retriggering while clipping continues, which is the
/// same shape libbela gives its own underrun LED — that one holds for 20000
/// frames, and matching it means the two indicators on a board behave alike.
///
/// Counted in frames rather than in blocks or seconds so that the hold is the
/// same length whatever the period size, and so that nothing here needs a
/// clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipIndicator {
    hold_frames: u64,
    remaining: u64,
    seen: u64,
}

impl ClipIndicator {
    /// An unlit indicator that will hold for `hold_frames` after each clip.
    #[must_use]
    pub const fn new(hold_frames: u64) -> Self {
        Self {
            hold_frames,
            remaining: 0,
            seen: 0,
        }
    }

    /// Advances by one block and says whether the indicator should be lit.
    ///
    /// `clipped_frames` is the meter's running total, not a per-block count:
    /// the meter accumulates over the run and this watches it for movement,
    /// which is what lets the two live in different places.
    ///
    /// Real-time safe: two comparisons and a subtraction (docs/contracts.md
    /// §6).
    pub const fn update(&mut self, clipped_frames: u64, block_frames: u64) -> bool {
        if clipped_frames > self.seen {
            self.seen = clipped_frames;
            self.remaining = self.hold_frames;
        } else {
            self.remaining = self.remaining.saturating_sub(block_frames);
        }
        self.lit()
    }

    /// Whether the indicator is currently held on.
    #[must_use]
    pub const fn lit(self) -> bool {
        self.remaining > 0
    }
}

#[cfg(test)]
#[expect(
    clippy::float_cmp,
    reason = "every peak asserted here is a magnitude the meter stored verbatim, so exact equality is the property under test"
)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_meter_has_seen_nothing() {
        let meter = InputMeter::new();
        assert_eq!(meter.peak(), 0.0);
        assert_eq!(meter.clipped_frames(), 0);
        assert_eq!(meter, InputMeter::default());
    }

    #[test]
    fn the_peak_is_the_largest_magnitude_of_either_channel() {
        let mut meter = InputMeter::new();
        meter.observe(0.2, -0.4);
        meter.observe(-0.1, 0.3);
        assert_eq!(meter.peak(), 0.4);
    }

    /// The acceptance condition for the reported figure: it has to track the
    /// input one for one, so that turning an input gain down 6 dB moves it
    /// 6 dB.
    #[test]
    fn the_peak_in_decibels_tracks_amplitude_one_for_one() {
        let peak_of = |amplitude: f32| {
            let mut meter = InputMeter::new();
            meter.observe(amplitude, 0.0);
            meter.peak_dbfs()
        };
        assert!((peak_of(1.0) - 0.0).abs() < 1e-4);
        assert!((peak_of(0.5) - -6.0206).abs() < 1e-3);
        assert!((peak_of(0.25) - -12.0412).abs() < 1e-3);
    }

    /// A silent input has no level, and printing one would claim a level that
    /// was not there.
    #[test]
    fn digital_silence_has_no_decibel_value() {
        let mut meter = InputMeter::new();
        meter.observe(0.0, 0.0);
        assert_eq!(meter.peak_dbfs(), f32::NEG_INFINITY);
    }

    #[test]
    fn a_frame_at_full_scale_counts_once_however_many_channels_hit_it() {
        let mut meter = InputMeter::new();
        meter.observe(1.0, 1.0);
        meter.observe(-1.0, 0.0);
        meter.observe(0.5, 0.5);
        assert_eq!(meter.clipped_frames(), 2);
    }

    /// The threshold is below full scale on purpose: a 16-bit converter's
    /// positive extreme is `32767/32768`, which is not 1.0.
    #[test]
    fn the_top_code_of_a_16_bit_converter_counts_as_clipped() {
        let mut meter = InputMeter::new();
        meter.observe(32767.0 / 32768.0, 0.0);
        assert_eq!(meter.clipped_frames(), 1);
    }

    #[test]
    fn a_signal_below_the_threshold_does_not_count() {
        let mut meter = InputMeter::new();
        meter.observe(0.998, -0.998);
        assert_eq!(meter.clipped_frames(), 0);
    }

    #[test]
    fn merging_takes_the_larger_peak_and_adds_the_clipped_frames() {
        let mut quiet = InputMeter::new();
        quiet.observe(0.25, 0.0);
        let mut loud = InputMeter::new();
        loud.observe(1.0, 0.0);
        loud.observe(1.0, 0.0);

        let merged = quiet.merged(loud);
        assert_eq!(merged.peak(), 1.0);
        assert_eq!(merged.clipped_frames(), 2);
        assert_eq!(merged, loud.merged(quiet));
    }

    #[test]
    fn merging_nothing_leaves_a_meter_alone() {
        let mut meter = InputMeter::new();
        meter.observe(0.5, 0.0);
        assert_eq!(meter.merged(InputMeter::new()), meter);
    }

    #[test]
    fn an_indicator_starts_unlit_and_stays_unlit_without_clipping() {
        let mut led = ClipIndicator::new(100);
        assert!(!led.lit());
        assert!(!led.update(0, 16));
        assert!(!led.update(0, 16));
    }

    /// The whole point: one clipped frame out of a block has to be visible
    /// long after the block it happened in.
    #[test]
    fn one_clipped_frame_lights_it_for_the_whole_hold() {
        let mut led = ClipIndicator::new(64);
        assert!(led.update(1, 16), "the block the clip arrived in");
        // 64 frames of hold against 16-frame blocks: three more blocks lit,
        // and the fourth dark.
        for block in 1..=3 {
            assert!(led.update(1, 16), "block {block} after the clip");
        }
        assert!(!led.update(1, 16));
    }

    #[test]
    fn continued_clipping_retriggers_the_hold() {
        let mut led = ClipIndicator::new(100);
        led.update(1, 16);
        for count in 2..20 {
            assert!(led.update(count, 16), "clipping at block {count}");
        }
    }

    /// The meter's total only ever rises, so a total that has not moved means
    /// no new clipping — not that the run restarted.
    #[test]
    fn a_total_that_stops_moving_lets_the_hold_expire() {
        let mut led = ClipIndicator::new(32);
        led.update(7, 16);
        assert!(led.update(7, 16));
        assert!(!led.update(7, 16));
    }

    /// A block longer than the whole hold must not wrap the counter round.
    #[test]
    fn a_block_longer_than_the_hold_just_ends_it() {
        let mut led = ClipIndicator::new(64);
        led.update(1, 16);
        assert!(!led.update(1, 4096));
    }
}
