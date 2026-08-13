//! Layer A of the control surface: what a hardware read produces, and the
//! trait that produces it (see [`crate::control`] for the layering).
//!
//! Nothing here touches hardware. The Raspberry Pi implementation (SPI to the
//! MCP3008, GPIO for the bypass switch) belongs behind the `pi-controls`
//! feature, because `rppal` is Linux-only; these types stay unconditional so
//! that the mapping layer and its tests build on any development machine.

use core::error::Error as StdError;

use nutype::nutype;

/// The position of a pot at its upper stop.
///
/// The scale is 1024 steps because that is what the Raspberry Pi's MCP3008
/// produces directly: a 10-bit successive-approximation conversion spans
/// `0..=2^10 - 1`, with full scale at the reference voltage (3.3 V on the
/// Pi's header). It stays the scale on a platform whose converter is a
/// different width, because the mapping layer's constants — the deadband
/// above all — are calibrated in these steps.
pub const POT_POSITION_MAX: u16 = 1023;

/// Where a pot is sitting, as a step from zero up to [`POT_POSITION_MAX`].
///
/// A quantised position rather than one converter's output: the Pi's
/// MCP3008 produces this scale directly, and a platform reading its pots
/// some other way maps onto it (`src/bela_host/controls.rs`). What travels
/// through the mapping layer is where the pot is, not how it was measured.
///
/// Only the ceiling needs a validator; `u16` already excludes negative
/// positions, and 0 is a legitimate reading (pot at its lower stop).
///
/// Following the convention in `src/params/value.rs`, the fallible `try_new`
/// is the entry point for untrusted input — here a byte pair off the SPI bus,
/// or a reading out of an audio callback's block — `new_const` is for
/// literals, and `get()` is the accessor.
#[nutype(
    const_fn,
    validate(less_or_equal = POT_POSITION_MAX),
    derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)
)]
pub struct PotPosition(u16);

impl PotPosition {
    /// Returns the wrapped value.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.into_inner()
    }

    /// Wraps a literal position, failing to compile if it is out of range.
    ///
    /// The same shape as `new_const` in `src/params/value.rs`, and here for a
    /// sharper reason than call-site brevity: the Bela host needs a position
    /// to fall back to when a reading means nothing, and it needs it inside
    /// the audio callback, where `try_new(0).unwrap()` would put a panic path
    /// on the real-time path to express something already known at compile
    /// time (docs/contracts.md §6).
    ///
    /// # Panics
    ///
    /// Panics if `value` is above [`POT_POSITION_MAX`].
    #[must_use]
    #[allow(clippy::panic)] // the only way to fail a const-context literal at compile time.
    pub const fn new_const(value: u16) -> Self {
        match Self::try_new(value) {
            Ok(v) => v,
            Err(_) => panic!("PotPosition literal out of range"),
        }
    }
}

/// One `T` per potentiometer: depth, time, upward, downward, input gain, output gain.
///
/// Named fields rather than `[T; 6]`, for the same reason as
/// [`Bands<T>`](crate::bands::Bands) (docs/architecture.md): the control
/// surface has exactly these six pots, so the concept gets one
/// representation from the ADC channel order through to the mapped
/// parameters, and field access cannot go out of range the way an index can
/// (`clippy::indexing_slicing` never enters the picture).
///
/// Field order matches the wiring: MCP3008 CH0..CH5. CH0..CH3 are the four
/// pots `pi-tools` verified against the assembled hardware; CH4 and CH5 are
/// the two gain pots, wired the same way on the same part.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pots<T> {
    /// The Depth pot (CH0), the dry/wet mix.
    pub depth: T,
    /// The Time pot (CH1), the attack/release multiplier.
    pub time: T,
    /// The Upward pot (CH2), the upward-compression multiplier.
    pub upward: T,
    /// The Downward pot (CH3), the downward-compression multiplier.
    pub downward: T,
    /// The Input Gain pot (CH4), the per-effect-band input gain in dB.
    pub input_gain: T,
    /// The Output Gain pot (CH5), the post-sum gain in dB.
    pub output_gain: T,
}

impl<T> Pots<T> {
    /// Applies `f` to every pot, visiting them in ADC channel order: depth,
    /// time, upward, downward, input gain, output gain.
    #[must_use]
    pub fn map<U>(self, mut f: impl FnMut(T) -> U) -> Pots<U> {
        Pots {
            depth: f(self.depth),
            time: f(self.time),
            upward: f(self.upward),
            downward: f(self.downward),
            input_gain: f(self.input_gain),
            output_gain: f(self.output_gain),
        }
    }

    /// Combines two sets of pot values field-wise, in the same order as [`Pots::map`].
    ///
    /// The mapping layer's conditioning is entirely field-wise (filter state
    /// against a new reading, filtered value against the deadband reference),
    /// so pairing by field here keeps that code free of any per-pot repetition.
    #[must_use]
    pub fn zip_with<U, V>(self, other: Pots<U>, mut f: impl FnMut(T, U) -> V) -> Pots<V> {
        Pots {
            depth: f(self.depth, other.depth),
            time: f(self.time, other.time),
            upward: f(self.upward, other.upward),
            downward: f(self.downward, other.downward),
            input_gain: f(self.input_gain, other.input_gain),
            output_gain: f(self.output_gain, other.output_gain),
        }
    }
}

/// One complete sample of the control surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawControls {
    /// The six pot readings.
    pub pots: Pots<PotPosition>,
    /// Which position the bypass switch is resting in: `true` for bypassed.
    ///
    /// The panel part is a mechanically *latching* (alternate-action) switch,
    /// so there is no press to observe — the switch stays where it was last
    /// put, and its position is the bypass state itself rather than a stimulus
    /// that toggles one. Every poll reports the position the switch is in at
    /// that instant.
    ///
    /// The switch is wired active-low against an internal pull-up, so the
    /// electrical level is still inverted by the reading layer: this field is
    /// the switch's logical position, not its pin level. Debouncing that
    /// position is not part of this field's meaning — see
    /// [`ControlMapping::update`](crate::control::ControlMapping::update).
    pub bypass_engaged: bool,
}

/// A source of [`RawControls`] readings that owns its hardware and is polled
/// from a thread of its own.
///
/// This is the shape the Raspberry Pi's control surface has, not a platform
/// seam, and it should not grow into one. `read` takes `&mut self` because
/// the SPI bus and the GPIO line live inside the implementation, and it
/// returns a `Result` because an SPI transfer can fail — neither is true of
/// every platform. The Bela host reads its pots out of the block context it
/// is handed, so it has no `self` to own them and nothing to fail; it builds
/// a [`RawControls`] directly instead of implementing this trait (see
/// `src/bela_host/controls.rs` and ADR 0010).
///
/// What both platforms share is the *value*: [`RawControls`] is the seam
/// between the hardware read and the mapping layer, and that is where the
/// portability lives.
///
/// It still earns its place here: a fake source lets the mapping layer and
/// the control thread be exercised on a development machine, with no MCP3008
/// and no Linux.
///
/// Implementations are free to block or allocate: a source is polled from the
/// control thread on the Pi, never from the audio callback.
pub trait ControlSource {
    /// How this source's hardware read can fail.
    type Error: StdError;

    /// Reads all six pots and the bypass switch as one sample.
    ///
    /// # Errors
    ///
    /// Returns `Self::Error` if the underlying hardware read fails.
    fn read(&mut self) -> Result<RawControls, Self::Error>;
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use core::convert::Infallible;

    use super::*;

    #[test]
    fn pot_position_accepts_the_full_scale_and_rejects_above_it() {
        assert!(PotPosition::try_new(0).is_ok());
        assert!(PotPosition::try_new(POT_POSITION_MAX).is_ok());
        assert!(PotPosition::try_new(POT_POSITION_MAX + 1).is_err());
        assert_eq!(PotPosition::try_new(512).unwrap().get(), 512);
    }

    #[test]
    fn map_applies_the_function_to_every_pot() {
        let pots = Pots {
            depth: 1,
            time: 2,
            upward: 3,
            downward: 4,
            input_gain: 5,
            output_gain: 6,
        };
        assert_eq!(
            pots.map(|v| v * 10),
            Pots {
                depth: 10,
                time: 20,
                upward: 30,
                downward: 40,
                input_gain: 50,
                output_gain: 60,
            }
        );
    }

    /// The visiting order is part of [`Pots::map`]'s contract: it is the ADC
    /// channel order, so a reader can line the fields up against CH0..CH5.
    #[test]
    fn map_visits_the_pots_in_adc_channel_order() {
        let pots = Pots {
            depth: "depth",
            time: "time",
            upward: "upward",
            downward: "downward",
            input_gain: "input_gain",
            output_gain: "output_gain",
        };

        let mut visited = Vec::new();
        let _ = pots.map(|name| visited.push(name));

        assert_eq!(
            visited,
            [
                "depth",
                "time",
                "upward",
                "downward",
                "input_gain",
                "output_gain"
            ],
            "map must visit the pots in MCP3008 channel order"
        );
    }

    #[test]
    fn zip_with_pairs_values_by_field() {
        let a = Pots {
            depth: 1,
            time: 2,
            upward: 3,
            downward: 4,
            input_gain: 5,
            output_gain: 6,
        };
        let b = Pots {
            depth: 10,
            time: 20,
            upward: 30,
            downward: 40,
            input_gain: 50,
            output_gain: 60,
        };
        assert_eq!(
            a.zip_with(b, |x, y| x + y),
            Pots {
                depth: 11,
                time: 22,
                upward: 33,
                downward: 44,
                input_gain: 55,
                output_gain: 66,
            }
        );
    }

    /// The development-machine stand-in the trait exists for: canned
    /// readings, no MCP3008 and no Linux involved. `Infallible` as the error
    /// type is itself part of what is being checked — a source that cannot
    /// fail must not be forced to invent an error.
    struct FakeSource {
        reading: RawControls,
        reads: usize,
    }

    impl ControlSource for FakeSource {
        type Error = Infallible;

        fn read(&mut self) -> Result<RawControls, Self::Error> {
            self.reads = self.reads.saturating_add(1);
            Ok(self.reading)
        }
    }

    #[test]
    fn a_fake_source_can_stand_in_for_hardware() {
        let reading = RawControls {
            pots: Pots {
                depth: PotPosition::try_new(1).unwrap(),
                time: PotPosition::try_new(2).unwrap(),
                upward: PotPosition::try_new(3).unwrap(),
                downward: PotPosition::try_new(4).unwrap(),
                input_gain: PotPosition::try_new(5).unwrap(),
                output_gain: PotPosition::try_new(6).unwrap(),
            },
            bypass_engaged: true,
        };
        let mut source = FakeSource { reading, reads: 0 };

        assert_eq!(source.read(), Ok(reading));
        assert_eq!(source.read(), Ok(reading));
        assert_eq!(source.reads, 2);
    }
}
