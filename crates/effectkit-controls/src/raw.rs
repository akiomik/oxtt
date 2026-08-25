//! Layer A of the control surface: what a hardware read produces, and the
//! trait that produces it (see [`crate`] for the layering).
//!
//! Nothing here touches hardware. The Raspberry Pi implementation (SPI to the
//! MCP3008, GPIO for the bypass switch) belongs behind the `pi-controls`
//! feature, because `rppal` is Linux-only; these types stay unconditional so
//! that the mapping layer and its tests build on any development machine.

use core::error::Error as StdError;

use nutype::nutype;

use super::conditioning::ConditioningConfig;

/// The position of a pot at its upper stop.
///
/// The scale is 1024 steps because that is what the Raspberry Pi's MCP3008
/// produces directly: a 10-bit successive-approximation conversion spans
/// `0..=2^10 - 1`, with full scale at the reference voltage (3.3 V on the
/// Pi's header). It stays the scale on a platform whose converter is a
/// different width, because the conditioning constants — the deadband above
/// all — are calibrated in these steps.
///
/// **This is a declaration about every surface, not just the two that exist**
/// (ADR 0014). Raising it means re-deriving each surface's `deadband_counts`
/// from its measurement rather than multiplying the existing number.
pub const POT_POSITION_MAX: u16 = 1023;

/// Where a pot is sitting, as a step from zero up to [`POT_POSITION_MAX`].
///
/// A quantised position rather than one converter's output: the Pi's
/// MCP3008 produces this scale directly, and a platform reading its pots
/// some other way maps onto it (`crate::gem`). What travels
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

/// One `T` per potentiometer, named for the ADC channel it is wired to.
///
/// Named fields rather than `[T; 6]` for one reason only: **at the place the
/// assignment is written, each pot can be named the same way the wiring names
/// it.** An array would work everywhere else — `iter().zip()` keeps
/// `clippy::indexing_slicing` out of the picture just as well — but it would
/// turn the six lines that decide what each knob does into six indices, which
/// is the one place in this codebase where a silent mix-up is possible.
///
/// The arity is fixed at six because both surfaces that exist have six pots.
/// A third with a different count gets its own type rather than making this
/// one generic over length.
///
/// **The names are the wiring, not the effect.** `adc0` is the pot on
/// MCP3008 CH0 and on a Gem's `A0`; what that pot *means* is decided once,
/// where the conditioned travel is assigned to parameters, and nowhere else.
/// Naming the fields for OTT's macros instead would put the effect's
/// vocabulary in the one type every effect shares, and would make a
/// two-effect codebase disagree with itself about what `depth` is.
///
/// The cost of the physical names is that a mis-wired assignment is silent —
/// the Time knob moving Depth still makes sound — so the assignment is the
/// one place that can be wrong, and each effect owes a test that pins the
/// channel order down ([`gem`](crate::gem)'s `channel_order_is_pot_order`
/// is the model).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pots<T> {
    /// The pot on ADC channel 0.
    pub adc0: T,
    /// The pot on ADC channel 1.
    pub adc1: T,
    /// The pot on ADC channel 2.
    pub adc2: T,
    /// The pot on ADC channel 3.
    pub adc3: T,
    /// The pot on ADC channel 4.
    pub adc4: T,
    /// The pot on ADC channel 5.
    pub adc5: T,
}

impl<T> Pots<T> {
    /// How many pots a control surface of this shape has.
    ///
    /// The single statement of the arity: a layer A reading six analog
    /// channels reads this rather than declaring a six of its own.
    pub const LEN: usize = 6;

    /// Applies `f` to every pot, visiting them in ADC channel order,
    /// `adc0` through `adc5`.
    #[must_use]
    pub fn map<U>(self, mut f: impl FnMut(T) -> U) -> Pots<U> {
        Pots {
            adc0: f(self.adc0),
            adc1: f(self.adc1),
            adc2: f(self.adc2),
            adc3: f(self.adc3),
            adc4: f(self.adc4),
            adc5: f(self.adc5),
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
            adc0: f(self.adc0, other.adc0),
            adc1: f(self.adc1, other.adc1),
            adc2: f(self.adc2, other.adc2),
            adc3: f(self.adc3, other.adc3),
            adc4: f(self.adc4, other.adc4),
            adc5: f(self.adc5, other.adc5),
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
    /// [`SixPotBypassConditioner::update`](crate::SixPotBypassConditioner::update).
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
/// [`gem`](crate::gem) and ADR 0010).
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

    /// How this source's readings are conditioned: the jitter filter, the
    /// deadband, the switch debounce and the rate they were all measured at.
    ///
    /// An associated constant with no default, so that adding a source is
    /// also being asked what its idle jitter is. There is no portable answer:
    /// the two surfaces that exist differ by more than an order of magnitude
    /// in the deadband alone, and a value carried over from the other one
    /// would be either a deadband that chatters or a knob that is needlessly
    /// coarse (ADR 0012).
    ///
    /// The rule the four values have to satisfy together lives on
    /// [`ConditioningConfig`].
    const CONDITIONING: ConditioningConfig;

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
    use crate::surfaces::GEM;

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
            adc0: 1,
            adc1: 2,
            adc2: 3,
            adc3: 4,
            adc4: 5,
            adc5: 6,
        };
        assert_eq!(
            pots.map(|v| v * 10),
            Pots {
                adc0: 10,
                adc1: 20,
                adc2: 30,
                adc3: 40,
                adc4: 50,
                adc5: 60,
            }
        );
    }

    /// The visiting order is part of [`Pots::map`]'s contract: it is the ADC
    /// channel order, so a reader can line the fields up against CH0..CH5.
    #[test]
    fn map_visits_the_pots_in_adc_channel_order() {
        let pots = Pots {
            adc0: "adc0",
            adc1: "adc1",
            adc2: "adc2",
            adc3: "adc3",
            adc4: "adc4",
            adc5: "adc5",
        };

        let mut visited = Vec::new();
        let _ = pots.map(|name| visited.push(name));

        assert_eq!(
            visited,
            ["adc0", "adc1", "adc2", "adc3", "adc4", "adc5"],
            "map must visit the pots in ADC channel order"
        );
    }

    #[test]
    fn zip_with_pairs_values_by_field() {
        let a = Pots {
            adc0: 1,
            adc1: 2,
            adc2: 3,
            adc3: 4,
            adc4: 5,
            adc5: 6,
        };
        let b = Pots {
            adc0: 10,
            adc1: 20,
            adc2: 30,
            adc3: 40,
            adc4: 50,
            adc5: 60,
        };
        assert_eq!(
            a.zip_with(b, |x, y| x + y),
            Pots {
                adc0: 11,
                adc1: 22,
                adc2: 33,
                adc3: 44,
                adc4: 55,
                adc5: 66,
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

        /// The Bela Gem's, because it is a measured surface and this fake is
        /// not; which of the two it borrows makes no difference to what is
        /// under test here.
        const CONDITIONING: ConditioningConfig = GEM;

        fn read(&mut self) -> Result<RawControls, Self::Error> {
            self.reads = self.reads.saturating_add(1);
            Ok(self.reading)
        }
    }

    #[test]
    fn a_fake_source_can_stand_in_for_hardware() {
        let reading = RawControls {
            pots: Pots {
                adc0: PotPosition::try_new(1).unwrap(),
                adc1: PotPosition::try_new(2).unwrap(),
                adc2: PotPosition::try_new(3).unwrap(),
                adc3: PotPosition::try_new(4).unwrap(),
                adc4: PotPosition::try_new(5).unwrap(),
                adc5: PotPosition::try_new(6).unwrap(),
            },
            bypass_engaged: true,
        };
        let mut source = FakeSource { reading, reads: 0 };

        assert_eq!(source.read(), Ok(reading));
        assert_eq!(source.read(), Ok(reading));
        assert_eq!(source.reads, 2);
    }
}
