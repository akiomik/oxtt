//! Layer A of the control surface: what a hardware read produces, and the
//! trait that produces it (see [`crate::control`] for the layering).
//!
//! Nothing here touches hardware. The Raspberry Pi implementation (SPI to the
//! MCP3008, GPIO for the bypass switch) belongs behind the `pi-controls`
//! feature, because `rppal` is Linux-only; these types stay unconditional so
//! that the mapping layer and its tests build on any development machine.

use core::error::Error as StdError;

use nutype::nutype;

/// The largest count an MCP3008 conversion can produce.
///
/// The MCP3008 is a 10-bit successive-approximation converter, so a
/// single-ended conversion spans `0..=2^10 - 1` and full scale is the
/// reference voltage (3.3 V on the Pi's header).
pub const ADC_MAX_COUNT: u16 = 1023;

/// One MCP3008 conversion result: a 10-bit count, from zero up to [`ADC_MAX_COUNT`].
///
/// Only the ceiling needs a validator; `u16` already excludes negative
/// counts, and 0 is a legitimate reading (pot at its lower stop).
///
/// Following the convention in `src/params/value.rs`, the fallible `try_new`
/// is the entry point for untrusted input — here a byte pair off the SPI bus
/// rather than a CLI argument — and `get()` is the accessor. There is no
/// `new_const`: no count is ever written as a literal outside tests.
#[nutype(
    const_fn,
    validate(less_or_equal = ADC_MAX_COUNT),
    derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)
)]
pub struct AdcCount(u16);

impl AdcCount {
    /// Returns the wrapped value.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.into_inner()
    }
}

/// One `T` per potentiometer: depth, time, upward, downward.
///
/// Named fields rather than `[T; 4]`, for the same reason as
/// [`Bands<T>`](crate::bands::Bands) (docs/architecture.md): the control
/// surface has exactly these four pots, so the concept gets one
/// representation from the ADC channel order through to the mapped
/// parameters, and field access cannot go out of range the way an index can
/// (`clippy::indexing_slicing` never enters the picture).
///
/// Field order matches the wiring verified by `pi-tools`: MCP3008 CH0..CH3.
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
}

impl<T> Pots<T> {
    /// Applies `f` to every pot, visiting depth, time, upward, downward in that order.
    #[must_use]
    pub fn map<U>(self, mut f: impl FnMut(T) -> U) -> Pots<U> {
        Pots {
            depth: f(self.depth),
            time: f(self.time),
            upward: f(self.upward),
            downward: f(self.downward),
        }
    }

    /// Combines two sets of pot values field-wise, in the same order as [`Pots::map`].
    ///
    /// The mapping layer's conditioning is entirely field-wise (filter state
    /// against a new reading, filtered value against the last published one),
    /// so pairing by field here keeps that code free of any per-pot repetition.
    #[must_use]
    pub fn zip_with<U, V>(self, other: Pots<U>, mut f: impl FnMut(T, U) -> V) -> Pots<V> {
        Pots {
            depth: f(self.depth, other.depth),
            time: f(self.time, other.time),
            upward: f(self.upward, other.upward),
            downward: f(self.downward, other.downward),
        }
    }
}

/// One complete sample of the control surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawControls {
    /// The four pot readings.
    pub pots: Pots<AdcCount>,
    /// Whether the bypass switch is currently held down.
    ///
    /// The switch is wired active-low against an internal pull-up, so the
    /// electrical level is inverted by the reading layer: this field is the
    /// switch's logical state, not its pin level. Debouncing and the latch
    /// this drives are not part of this field's meaning — see
    /// [`ControlMapping::update`](crate::control::ControlMapping::update).
    pub bypass_pressed: bool,
}

/// Layer A: a source of [`RawControls`] readings.
///
/// This is not a general platform-abstraction layer, and should not grow into
/// one. It exists for exactly two reasons:
///
/// - a fake source lets the mapping layer and (later) the control thread be
///   exercised on a development machine, with no MCP3008 and no Linux;
/// - it confines what a platform port has to rewrite to the hardware read
///   itself — the conditioning, the parameter mapping, and the transport all
///   stay put.
///
/// Implementations are free to block or allocate: a source is polled from the
/// control thread on the Pi. A Bela port reading inside `render()` is the
/// exception and must respect docs/contracts.md §6 in its own implementation.
pub trait ControlSource {
    /// How this source's hardware read can fail.
    type Error: StdError;

    /// Reads all four pots and the bypass switch as one sample.
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
    fn adc_count_accepts_the_full_10_bit_range_and_rejects_above_it() {
        assert!(AdcCount::try_new(0).is_ok());
        assert!(AdcCount::try_new(ADC_MAX_COUNT).is_ok());
        assert!(AdcCount::try_new(ADC_MAX_COUNT + 1).is_err());
        assert_eq!(AdcCount::try_new(512).unwrap().get(), 512);
    }

    #[test]
    fn map_applies_the_function_to_every_pot() {
        let pots = Pots {
            depth: 1,
            time: 2,
            upward: 3,
            downward: 4,
        };
        assert_eq!(
            pots.map(|v| v * 10),
            Pots {
                depth: 10,
                time: 20,
                upward: 30,
                downward: 40,
            }
        );
    }

    #[test]
    fn zip_with_pairs_values_by_field() {
        let a = Pots {
            depth: 1,
            time: 2,
            upward: 3,
            downward: 4,
        };
        let b = Pots {
            depth: 10,
            time: 20,
            upward: 30,
            downward: 40,
        };
        assert_eq!(
            a.zip_with(b, |x, y| x + y),
            Pots {
                depth: 11,
                time: 22,
                upward: 33,
                downward: 44,
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
                depth: AdcCount::try_new(1).unwrap(),
                time: AdcCount::try_new(2).unwrap(),
                upward: AdcCount::try_new(3).unwrap(),
                downward: AdcCount::try_new(4).unwrap(),
            },
            bypass_pressed: true,
        };
        let mut source = FakeSource { reading, reads: 0 };

        assert_eq!(source.read(), Ok(reading));
        assert_eq!(source.read(), Ok(reading));
        assert_eq!(source.reads, 2);
    }
}
