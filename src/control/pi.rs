//! Layer A on a Raspberry Pi: four pots on an MCP3008 over SPI, and a bypass
//! switch on a GPIO pin (see [`crate::control`] for the layering).
//!
//! Behind the `pi-controls` feature, because `rppal` is Linux-only and the
//! rest of the control surface has to keep building on a development machine.
//!
//! The wiring and the read sequence below are not a proposal: they are what
//! the `pi-tools` binary in this workspace verified against the assembled
//! hardware, where all four channels track their pots across the full
//! `0..=1023` range and the switch is detected. This module reproduces that
//! read exactly — same bus, same mode, same clock, same three-byte
//! conversation — so that a fault here is a fault in code rather than a
//! question about the breadboard. Any change to the constants or to
//! [`command_for`]/[`decode_response`] should be re-verified with that tool.
//!
//! Nothing here is on the real-time path. A conversion is a blocking `ioctl`,
//! which is exactly why it is polled from the control thread
//! ([`ControlHandle`](crate::control::ControlHandle)) instead of the audio
//! callback (docs/contracts.md §6, ADR 0009).

use rppal::gpio::{Gpio, InputPin, Level};
// Aliased rather than qualified at the use site: `rppal::spi::Error` is three
// segments, which `clippy::absolute_paths` rejects, and `SpiError`/`GpioError`
// say which bus failed anyway.
use rppal::gpio::Error as GpioError;
use rppal::spi::Error as SpiError;
use rppal::spi::{Bus, Mode, SlaveSelect, Spi};
use thiserror::Error;

use super::raw::{AdcCount, AdcCountError, ControlSource, Pots, RawControls};

/// GPIO pin (BCM numbering) the bypass switch is wired to.
///
/// Read with the `SoC`'s internal pull-up and the switch shorting the pin to
/// ground, so the *electrical* level is inverted with respect to
/// [`RawControls::bypass_pressed`]; no external resistor is on the board.
const BYPASS_GPIO: u8 = 17;

/// SPI clock rate for the MCP3008 conversation.
///
/// Deliberately well under the MCP3008's ~1.35–2 MHz ceiling at 3.3 V
/// (datasheet): breadboard jumpers pick up noise readily at higher clock
/// rates, and there is nothing to buy by going faster. Four conversions are 12
/// bytes, about 200 µs at this rate, against a
/// [`DEFAULT_POLL_INTERVAL`](super::DEFAULT_POLL_INTERVAL) of 2 ms on a thread
/// that has nothing else to do.
const SPI_CLOCK_HZ: u32 = 500_000;

/// The MCP3008 samples on the rising edge with the clock idling low, which is
/// SPI mode 0.
const SPI_MODE: Mode = Mode::Mode0;

/// MCP3008 channel wired to the Depth pot (dry/wet mix).
const CHANNEL_DEPTH: u8 = 0;
/// MCP3008 channel wired to the Time pot (attack/release multiplier).
const CHANNEL_TIME: u8 = 1;
/// MCP3008 channel wired to the Upward pot (upward-compression multiplier).
const CHANNEL_UPWARD: u8 = 2;
/// MCP3008 channel wired to the Downward pot (downward-compression multiplier).
const CHANNEL_DOWNWARD: u8 = 3;

/// The MCP3008's start bit, sent alone in the first byte.
///
/// Byte-aligning the start bit this way is what leaves the 10-bit result
/// split across the last two bytes of the response in the shape
/// [`decode_response`] expects.
const START_BIT: u8 = 0x01;

/// SGL/DIFF set: a single-ended conversion of one channel against `AGND`,
/// rather than a differential pair.
///
/// Occupies the bit just above the three channel-select bits, so
/// `SINGLE_ENDED | channel` is the complete four-bit configuration nibble.
const SINGLE_ENDED: u8 = 0x08;

/// Mask for the two significant bits (B9, B8) in the response's first payload
/// byte; every bit above them is undefined and must not reach the count.
const HIGH_BITS_MASK: u8 = 0x03;

/// Builds the three bytes to clock out for a single-ended conversion of `channel`.
///
/// The MCP3008 wants a start bit, then SGL/DIFF plus three channel-select
/// bits, then a running clock while it shifts the result back. Sending
/// `[START_BIT, config << 4, 0x00]` puts the configuration nibble in the upper
/// half of the second byte, where the converter expects it directly after the
/// start bit, and clocks out the result during the third byte.
const fn command_for(channel: u8) -> [u8; 3] {
    // `rotate_left` rather than `<<`: the configuration nibble is at most
    // `0x0F` (`SINGLE_ENDED | channel` with `channel <= 7`), so its high
    // nibble is zero and a rotate moves no set bit off the top — the two are
    // identical here, but the rotate is total and so needs no overflow proof
    // from `clippy::arithmetic_side_effects`.
    [START_BIT, (SINGLE_ENDED | channel).rotate_left(4), 0x00]
}

/// Decodes an MCP3008 response into a raw 10-bit count.
///
/// The first byte is clocked out while the command is still going in and
/// carries nothing; the result straddles the other two, with B9 and B8 in the
/// low bits of the second and B7..B0 in the third.
const fn decode_response(response: [u8; 3]) -> u16 {
    // Destructured rather than indexed, so the read stays clear of
    // `clippy::indexing_slicing` without an allow.
    let [_command_echo, high, low] = response;
    // `from_be_bytes` rather than a shift-and-or: it is the same value with no
    // arithmetic to prove non-trapping, and it makes the byte order explicit.
    // Masking the high byte first bounds the result at `0x03FF` = 1023, which
    // is `ADC_MAX_COUNT`, so every possible response decodes to a valid
    // `AdcCount` (exhaustively asserted in the tests below).
    u16::from_be_bytes([high & HIGH_BITS_MASK, low])
}

/// How a Raspberry Pi control-surface read can fail.
///
/// The three causes are kept apart because they are diagnosed differently: an
/// SPI failure points at the MCP3008 wiring or a disabled SPI0, a GPIO failure
/// at the pin or at permissions, and a count violation at this module's own
/// decoding — the hardware cannot produce one (see [`PiControls::read_channel`]).
#[derive(Debug, Error)]
pub enum PiControlError {
    /// The SPI bus could not be opened, or a conversion transfer failed.
    #[error("MCP3008 SPI access failed: {0}")]
    Spi(#[from] SpiError),
    /// The bypass switch's GPIO pin could not be acquired or read.
    #[error("bypass switch GPIO{BYPASS_GPIO} access failed: {0}")]
    Gpio(#[from] GpioError),
    /// A decoded conversion was out of range for [`AdcCount`].
    #[error("MCP3008 conversion out of range: {0}")]
    Count(#[from] AdcCountError),
}

/// Layer A for the Raspberry Pi: an MCP3008 on SPI0/CE0 and a bypass switch on
/// GPIO17.
///
/// Owns the bus and the pin for the lifetime of the process; both are released
/// on drop by `rppal`. Polled from the control thread, so it is free to block
/// (docs/contracts.md §6).
#[derive(Debug)]
pub struct PiControls {
    spi: Spi,
    bypass: InputPin,
}

impl PiControls {
    /// Acquires SPI0/CE0 and the bypass switch's GPIO pin.
    ///
    /// Failing here is fatal at startup rather than something to retry: it
    /// means SPI0 is not enabled, the process cannot reach `/dev/spidev0.0` or
    /// the GPIO character device, or the pin is already claimed — none of
    /// which a later poll would find fixed. A *read* failure once running is
    /// the opposite case and is deliberately survivable (see
    /// [`ControlHandle::spawn`](crate::control::ControlHandle::spawn)).
    ///
    /// # Errors
    ///
    /// Returns [`PiControlError::Spi`] if the SPI bus cannot be opened and
    /// [`PiControlError::Gpio`] if the pin cannot be acquired.
    pub fn new() -> Result<Self, PiControlError> {
        let spi = Spi::new(Bus::Spi0, SlaveSelect::Ss0, SPI_CLOCK_HZ, SPI_MODE)?;
        // The pull-up is internal to the SoC, so the switch needs nothing but
        // a connection to ground; see `BYPASS_GPIO`.
        let bypass = Gpio::new()?.get(BYPASS_GPIO)?.into_input_pullup();

        Ok(Self { spi, bypass })
    }

    /// Runs one conversion on `channel` and returns its count.
    fn read_channel(&self, channel: u8) -> Result<AdcCount, PiControlError> {
        let mut response = [0_u8; 3];
        self.spi.transfer(&mut response, &command_for(channel))?;

        // `decode_response` masks to 10 bits, so its result cannot exceed
        // `ADC_MAX_COUNT` and this conversion cannot actually fail. It is
        // still propagated rather than unwrapped: the alternative is a panic
        // on the control thread if that reasoning ever stops holding, and
        // `PiControlError::Count` turns the same event into a counted,
        // reported read failure that leaves audio running.
        Ok(AdcCount::try_new(decode_response(response))?)
    }
}

impl ControlSource for PiControls {
    type Error = PiControlError;

    /// Reads the four channels and the switch as one sample.
    ///
    /// The conversions are sequential and each is a separate SPI transaction —
    /// the MCP3008 has one converter and no scan mode — so the four counts are
    /// staggered by tens of microseconds. That is far below anything a hand
    /// can do to a knob, so they are treated as simultaneous.
    fn read(&mut self) -> Result<RawControls, Self::Error> {
        let pots = Pots {
            depth: self.read_channel(CHANNEL_DEPTH)?,
            time: self.read_channel(CHANNEL_TIME)?,
            upward: self.read_channel(CHANNEL_UPWARD)?,
            downward: self.read_channel(CHANNEL_DOWNWARD)?,
        };

        // Active-low against the internal pull-up: the switch shorts the pin
        // to ground, so Low is the switch closed. `RawControls::bypass_pressed`
        // is the switch's logical state, so the level is inverted here and
        // nowhere else. Every poll reads the raw level; debouncing it and
        // latching the effect bypass belong to `ControlMapping`, which has no
        // hardware to know about.
        let bypass_pressed = self.bypass.read() == Level::Low;

        Ok(RawControls {
            pots,
            bypass_pressed,
        })
    }
}

#[cfg(test)]
// The pure halves of the read: what goes out on the bus, and how what comes
// back becomes a count. Everything either side of them needs an MCP3008 and a
// Linux kernel, and is deliberately not faked — a mock of `rppal` would prove
// only that the mock matches this code.
mod tests {
    use super::*;
    use crate::control::ADC_MAX_COUNT;

    #[test]
    fn the_command_selects_each_wired_channel() {
        // Start bit, then `1` (single-ended) followed by the three-bit channel
        // number in the upper nibble of the second byte.
        assert_eq!(
            command_for(CHANNEL_DEPTH),
            [0x01, 0x80, 0x00],
            "CH0 must be requested single-ended"
        );
        assert_eq!(
            command_for(CHANNEL_TIME),
            [0x01, 0x90, 0x00],
            "CH1 must be requested single-ended"
        );
        assert_eq!(
            command_for(CHANNEL_UPWARD),
            [0x01, 0xA0, 0x00],
            "CH2 must be requested single-ended"
        );
        assert_eq!(
            command_for(CHANNEL_DOWNWARD),
            [0x01, 0xB0, 0x00],
            "CH3 must be requested single-ended"
        );
    }

    #[test]
    fn the_command_covers_the_channels_this_wiring_does_not_use() {
        // Nothing calls these today, but the encoding is the datasheet's, not
        // this board's: the top channel must still land in the same nibble.
        assert_eq!(
            command_for(7),
            [0x01, 0xF0, 0x00],
            "CH7 must set all three channel-select bits"
        );
    }

    #[test]
    fn a_full_scale_response_decodes_to_the_adc_ceiling() {
        assert_eq!(
            decode_response([0x00, 0xFF, 0xFF]),
            ADC_MAX_COUNT,
            "an all-ones response is full scale, not an overflow"
        );
    }

    #[test]
    fn the_undefined_bits_above_the_count_are_masked_off() {
        // Only B9 and B8 of the first payload byte belong to the result; the
        // null bit and the tail of the command echo sit above them, and a
        // response with every one of those set must still decode to zero.
        assert_eq!(
            decode_response([0xFF, 0xFC, 0x00]),
            0,
            "bits above B9 must not reach the count"
        );
        assert_eq!(
            decode_response([0xFF, 0xFD, 0x00]),
            256,
            "B8 must survive the mask that drops the bits above it"
        );
    }

    #[test]
    fn a_response_decodes_to_the_count_its_two_payload_bytes_spell() {
        assert_eq!(decode_response([0x00, 0x00, 0x00]), 0, "zero scale");
        assert_eq!(decode_response([0x00, 0x02, 0x00]), 512, "mid scale");
        assert_eq!(decode_response([0x00, 0x01, 0x2C]), 300, "B9 clear, B8 set");
    }

    #[test]
    fn the_first_byte_never_affects_the_count() {
        // It is clocked out while the command is still going in, so whatever
        // it happens to contain must not move the reading.
        for echo in 0..=u8::MAX {
            assert_eq!(
                decode_response([echo, 0x01, 0x2C]),
                300,
                "the command echo byte must be ignored, echo={echo:#04x}"
            );
        }
    }

    #[test]
    fn every_possible_response_decodes_to_a_valid_count() {
        // The exhaustive form of the claim `read_channel` relies on: masking
        // to 10 bits means `AdcCount::try_new` cannot reject a decoded
        // response, whatever the bus returns.
        for high in 0..=u8::MAX {
            for low in 0..=u8::MAX {
                let count = decode_response([0xFF, high, low]);
                assert!(
                    count <= ADC_MAX_COUNT,
                    "decoded {count} from high={high:#04x} low={low:#04x}, above the 10-bit ceiling"
                );
                assert!(
                    AdcCount::try_new(count).is_ok(),
                    "AdcCount rejected {count}, decoded from high={high:#04x} low={low:#04x}"
                );
            }
        }
    }
}
