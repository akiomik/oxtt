//! Layer A for the Bela host: turning one frame of analog readings and one
//! switch level into a [`RawControls`] (docs/architecture.md, ADR 0011).
//!
//! Everything here is a free function over plain values rather than a
//! [`ControlSource`](crate::control::ControlSource) implementation. That trait
//! describes the Raspberry Pi's arrangement — hardware owned by the reader,
//! polled from a thread, able to fail — and none of the three is true here:
//! the samples arrive in the block context, `render_pre` already has them, and
//! an analog read cannot fail. The seam between the platform and the mapping
//! layer is the `RawControls` value, not the trait that produces one.
//!
//! Keeping the conversion out of the context also keeps it testable: `bela`
//! offers no supported way to build a `BlockContext` on a development machine
//! (bela-rs#113), so the three lines that touch one live in
//! [`super::app`] and everything with a decision in it lives here.

use crate::control::{POT_POSITION_MAX, PotPosition, Pots, RawControls};

/// The reading a pot wired across the 3.3 V rail produces at its upper stop.
///
/// `analog_read` returns 0.0 to 1.0 for 0 V to 4.096 V — the ADS8166's own
/// reference, which is above the rail the pots are wired to — so a pot at its
/// top reads this fraction of full scale rather than 1.0. Scaling by it is
/// what makes the top of the travel mean depth 1.0 and +24 dB; scaling by
/// full scale instead would stop the pots at 826 of 1023, which is +14.6 dB
/// on the gain pots and a depth that never reaches fully wet.
///
/// The nominal ratio, not the 0.8064 measured on one board on one day
/// (bela-rs `docs/board-facts.md`). The two differ by 0.09%, which is one
/// step of [`POT_POSITION_MAX`] and an eighth of the mapping layer's
/// deadband, so the difference cannot reach a published snapshot. What the
/// measurement does settle is that the *reading* can sit slightly above this
/// nominal ceiling, which is why the clamp below is not optional.
const POT_SUPPLY_FRACTION: f32 = 3.3 / 4.096;

/// The pot position a reading that means nothing falls back to.
///
/// Zero rather than centre: on the gain pots it is -24 dB and on depth it is
/// fully dry, so a channel that is not reading fails quiet. That matches the
/// Raspberry Pi wiring, where the unused ADC channels are tied to ground for
/// the same reason (`src/control/pi.rs`).
const POT_POSITION_FLOOR: PotPosition = PotPosition::new_const(0);

/// Number of analog channels the control surface occupies, `A0` through `A5`.
///
/// The order is [`Pots`]' field order, which is also the Pi's MCP3008 channel
/// order: depth, time, upward, downward, input gain, output gain.
pub const ANALOG_CHANNELS_USED: usize = 6;

/// Converts one analog reading into a pot position.
///
/// Reading order, and why each step is where it is:
///
/// - divide by [`POT_SUPPLY_FRACTION`] first, so the pot's own travel spans
///   the full scale;
/// - clamp, because a reading can exceed the nominal ceiling (see above) and
///   because a floating or mis-wired input can be anything at all;
/// - round to the nearest step rather than truncating, so the top of the
///   travel reaches [`POT_POSITION_MAX`] instead of stopping one short.
///
/// A NaN reading becomes 0 through Rust's saturating float-to-integer cast,
/// which is [`POT_POSITION_FLOOR`] by another route and fails the same way.
#[must_use]
pub fn pot_position(reading: f32) -> PotPosition {
    let fraction = (reading / POT_SUPPLY_FRACTION).clamp(0.0, 1.0);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the clamp above bounds the product to 0..=POT_POSITION_MAX, and a NaN saturates to 0"
    )]
    let position = (fraction * f32::from(POT_POSITION_MAX)).round() as u16;
    // Unreachable in practice — the clamp guarantees the range — so this
    // falls back rather than unwrapping: it runs inside the audio callback,
    // where a panic aborts the process (docs/contracts.md §6). The same
    // pattern as `normalized_or` in `src/control/mapping.rs`.
    PotPosition::try_new(position).unwrap_or(POT_POSITION_FLOOR)
}

/// Assembles one complete control-surface sample.
///
/// `analog_frame` is every analog channel of a single frame, in channel
/// order; only the first [`ANALOG_CHANNELS_USED`] are read, so a board
/// reporting more channels than the surface uses costs nothing. A channel
/// the slice does not reach falls back to [`POT_POSITION_FLOOR`], which is
/// how a misconfigured board fails quiet rather than loud.
///
/// `bypass_engaged` is the switch's logical position, already inverted from
/// its active-low pin level by the caller — the same division of labour as
/// `PiControls::read` (`src/control/pi.rs`). Debouncing it is the mapping
/// layer's job, not this function's.
#[must_use]
pub fn raw_controls(analog_frame: &[f32], bypass_engaged: bool) -> RawControls {
    let mut readings = analog_frame.iter().copied();
    // Field order is channel order, and `Pots` has exactly six fields, so
    // taking them in sequence is the wiring: A0 to depth, A5 to output gain.
    let pots = Pots {
        depth: next_position(&mut readings),
        time: next_position(&mut readings),
        upward: next_position(&mut readings),
        downward: next_position(&mut readings),
        input_gain: next_position(&mut readings),
        output_gain: next_position(&mut readings),
    };
    RawControls {
        pots,
        bypass_engaged,
    }
}

/// Takes the next reading, or the quiet floor if the slice ran out.
fn next_position(readings: &mut impl Iterator<Item = f32>) -> PotPosition {
    readings.next().map_or(POT_POSITION_FLOOR, pot_position)
}

/// Decides which application blocks read the control surface.
///
/// The mapping layer has no clock: its filter coefficient is defined per
/// *read*, and its debounce counts reads, so the caller's read rate is what
/// turns those constants into times (`src/control/mapping.rs`). They were
/// calibrated against the Raspberry Pi's 500 Hz polling, and Bela's callback
/// runs far faster than that — 3000 blocks a second at 48 kHz with a period
/// of 16 — which would shrink the bypass debounce from 28 ms to 5 ms, below
/// the make/break time of the latching switch it exists to ride out.
///
/// So the host reads on every *n*th block instead, with `n` chosen to land
/// near the rate the constants were calibrated for. At 48 kHz and a period of
/// 16 the division is exact: 3000 / 6 is 500 Hz, and the debounce is the Pi's
/// to the millisecond. Other periods land near it rather than on it, which is
/// why [`PollDecimator::effective_hz`] exists for the host to report.
///
/// The alternative — retuning the constants for Bela — would make the mapping
/// layer platform-dependent and put its measured calibration out of reach of
/// the measurement that justified it (ADR 0010, ADR 0011).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PollDecimator {
    every: u32,
    counter: u32,
}

/// The read rate the mapping layer's constants were calibrated against.
///
/// `DEFAULT_POLL_INTERVAL` on the Raspberry Pi is 2 ms, and the deadband,
/// filter coefficient and debounce count in `src/control/mapping.rs` are all
/// justified against jitter measured at that rate
/// (docs/raspberry-pi/control-surface-verification.md).
pub const TARGET_POLL_HZ: f32 = 500.0;

impl PollDecimator {
    /// Reads on every block: the divisor before the block shape is known.
    ///
    /// A conservative starting point rather than a meaningful one — reading
    /// too often costs a little CPU and shortens the debounce, where reading
    /// too rarely would drop knob movement — and in practice unused, because
    /// `setup` replaces it before any block is rendered.
    pub const EVERY_BLOCK: Self = Self {
        every: 1,
        counter: 0,
    };

    /// Chooses a divisor from the block rate the audio system reports.
    ///
    /// Never divides by less than one, so a host slower than
    /// [`TARGET_POLL_HZ`] reads on every block rather than being asked to
    /// read more often than it is called.
    #[must_use]
    pub fn for_block_rate(sample_rate: f32, frames_per_block: usize) -> Self {
        #[expect(
            clippy::cast_precision_loss,
            reason = "a block is thousands of frames at most; f32 is exact well past that"
        )]
        let blocks_per_second = sample_rate / frames_per_block.max(1) as f32;
        let ratio = (blocks_per_second / TARGET_POLL_HZ).round();
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "clamped to at least 1 below, and a non-finite ratio saturates to 0 and is lifted by the same max"
        )]
        let every = (ratio as u32).max(1);
        Self { every, counter: 0 }
    }

    /// Advances one block and reports whether this one reads.
    ///
    /// The first block reads, so a run is never one whole divisor long before
    /// the control surface has any say.
    pub const fn tick(&mut self) -> bool {
        let due = self.counter == 0;
        // Saturating rather than plain, for `clippy::arithmetic_side_effects`
        // (docs/contracts.md §6): the wrap below keeps the counter under
        // `every`, so saturation is unreachable and costs nothing.
        self.counter = self.counter.saturating_add(1);
        if self.counter >= self.every {
            self.counter = 0;
        }
        due
    }

    /// The divisor in use: one read every this many application blocks.
    #[must_use]
    pub const fn every(self) -> u32 {
        self.every
    }

    /// The read rate this divisor actually produces, for the host to report.
    ///
    /// Worth printing because it is only exactly [`TARGET_POLL_HZ`] at some
    /// period sizes; at others the debounce and filter time constants scale
    /// with the difference.
    #[must_use]
    pub fn effective_hz(self, sample_rate: f32, frames_per_block: usize) -> f32 {
        #[expect(clippy::cast_precision_loss, reason = "same bounds as for_block_rate")]
        let blocks_per_second = sample_rate / frames_per_block.max(1) as f32;
        blocks_per_second / f32::from(u16::try_from(self.every).unwrap_or(u16::MAX))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn the_bottom_of_the_travel_is_the_bottom_of_the_scale() {
        assert_eq!(pot_position(0.0).get(), 0);
    }

    #[test]
    fn the_nominal_top_of_the_travel_reaches_full_scale() {
        assert_eq!(pot_position(POT_SUPPLY_FRACTION).get(), POT_POSITION_MAX);
    }

    /// The measured ceiling sits 0.09% above the nominal one, so without the
    /// clamp this reading would round to 1024 and be rejected.
    #[test]
    fn a_reading_above_the_nominal_ceiling_is_clamped_not_rejected() {
        assert_eq!(pot_position(0.8064).get(), POT_POSITION_MAX);
        assert_eq!(pot_position(1.0).get(), POT_POSITION_MAX);
    }

    #[test]
    fn readings_that_mean_nothing_fail_quiet() {
        assert_eq!(pot_position(-1.0), POT_POSITION_FLOOR);
        assert_eq!(pot_position(f32::NAN), POT_POSITION_FLOOR);
        assert_eq!(pot_position(f32::NEG_INFINITY), POT_POSITION_FLOOR);
        assert_eq!(pot_position(f32::INFINITY).get(), POT_POSITION_MAX);
    }

    #[test]
    fn the_middle_of_the_travel_is_the_middle_of_the_scale() {
        let middle = pot_position(POT_SUPPLY_FRACTION / 2.0).get();
        assert_eq!(
            middle, 512,
            "expected the midpoint of 0..=1023, got {middle}"
        );
    }

    #[test]
    fn channel_order_is_pot_order() {
        let frame = [0.0, 0.1, 0.2, 0.3, 0.4, POT_SUPPLY_FRACTION];
        let raw = raw_controls(&frame, false);
        assert_eq!(raw.pots.depth.get(), 0);
        assert_eq!(raw.pots.output_gain.get(), POT_POSITION_MAX);
        assert!(raw.pots.time < raw.pots.upward);
        assert!(raw.pots.upward < raw.pots.downward);
        assert!(raw.pots.downward < raw.pots.input_gain);
    }

    #[test]
    fn channels_past_the_sixth_are_ignored() {
        let six = [0.1, 0.2, 0.3, 0.4, 0.5, 0.6];
        let eight = [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8];
        assert_eq!(raw_controls(&six, false), raw_controls(&eight, false));
    }

    /// A board configured with fewer analog inputs than the surface needs is
    /// refused before the audio system exists (`super::app`), so this is the
    /// belt to that braces: whatever reaches here still cannot panic.
    #[test]
    fn a_short_frame_fills_the_rest_with_the_quiet_floor() {
        let raw = raw_controls(&[POT_SUPPLY_FRACTION, POT_SUPPLY_FRACTION], false);
        assert_eq!(raw.pots.depth.get(), POT_POSITION_MAX);
        assert_eq!(raw.pots.time.get(), POT_POSITION_MAX);
        assert_eq!(raw.pots.upward, POT_POSITION_FLOOR);
        assert_eq!(raw.pots.output_gain, POT_POSITION_FLOOR);

        let raw = raw_controls(&[], true);
        assert_eq!(raw.pots.depth, POT_POSITION_FLOOR);
        assert!(raw.bypass_engaged);
    }

    #[test]
    fn the_bypass_level_is_carried_through_untouched() {
        let frame = [0.0; ANALOG_CHANNELS_USED];
        assert!(!raw_controls(&frame, false).bypass_engaged);
        assert!(raw_controls(&frame, true).bypass_engaged);
    }

    /// The reason 48 kHz was chosen over the board's 44.1 kHz default: the
    /// divisor is exact, so the mapping layer's constants keep the timings
    /// they were measured with (ADR 0011).
    #[test]
    fn forty_eight_kilohertz_at_the_default_period_divides_exactly() {
        let decimator = PollDecimator::for_block_rate(48_000.0, 16);
        assert_eq!(decimator.every(), 6);
        assert_eq!(decimator.effective_hz(48_000.0, 16), TARGET_POLL_HZ);
    }

    #[test]
    fn larger_periods_land_near_the_target_rather_than_on_it() {
        // 750 blocks/s / 2 = 375 Hz.
        let decimator = PollDecimator::for_block_rate(48_000.0, 64);
        assert_eq!(decimator.every(), 2);
        assert_eq!(decimator.effective_hz(48_000.0, 64), 375.0);

        // 375 blocks/s is already below the target, so every block reads.
        let decimator = PollDecimator::for_block_rate(48_000.0, 128);
        assert_eq!(decimator.every(), 1);
        assert_eq!(decimator.effective_hz(48_000.0, 128), 375.0);
    }

    #[test]
    fn a_host_slower_than_the_target_reads_every_block() {
        let decimator = PollDecimator::for_block_rate(48_000.0, 1024);
        assert_eq!(decimator.every(), 1);
    }

    /// None of these can come from a running audio system, but the divisor
    /// must never be zero whatever it is handed: `tick` compares against it,
    /// and a zero would make every block both due and not due.
    #[test]
    fn nonsense_block_shapes_still_produce_a_usable_divisor() {
        // A zero-frame block is read as one frame, so this is 48000 / 500.
        assert_eq!(PollDecimator::for_block_rate(48_000.0, 0).every(), 96);
        // No blocks at all, and a rate that is not a number, both floor to
        // reading every block rather than dividing by zero or saturating.
        assert_eq!(PollDecimator::for_block_rate(0.0, 16).every(), 1);
        assert_eq!(PollDecimator::for_block_rate(f32::NAN, 16).every(), 1);
        assert_eq!(PollDecimator::for_block_rate(-48_000.0, 16).every(), 1);
    }

    #[test]
    fn the_first_block_reads_and_then_every_nth() {
        let mut decimator = PollDecimator::for_block_rate(48_000.0, 16);
        let reads = [(); 13].map(|()| decimator.tick());
        assert_eq!(
            reads,
            [
                true, false, false, false, false, false, // block 0 reads
                true, false, false, false, false, false, // block 6 reads
                true,  // block 12 reads
            ]
        );
    }

    #[test]
    fn a_divisor_of_one_reads_every_block() {
        let mut decimator = PollDecimator::for_block_rate(48_000.0, 128);
        assert!(decimator.tick());
        assert!(decimator.tick());
        assert!(decimator.tick());
    }
}
