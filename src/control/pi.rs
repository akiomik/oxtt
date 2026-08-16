//! Layer A on a Raspberry Pi: six pots on an MCP3008 over SPI, and a bypass
//! switch on a GPIO pin (see [`crate::control`] for the layering).
//!
//! Behind the `pi-controls` feature, because `rppal` is Linux-only and the
//! rest of the control surface has to keep building on a development machine.
//!
//! The read sequence below is not a proposal: it is what the `pi-tools` binary
//! in this workspace verified against the assembled hardware, where CH0..CH3
//! track their pots across the full `0..=1023` range and the switch is
//! detected. This module reproduces that read exactly — same bus, same mode,
//! same clock, same three-byte conversation — so that a fault here is a fault
//! in code rather than a question about the breadboard. The two gain pots on
//! CH4 and CH5 are the same part wired the same way and read by the same code
//! path, but they postdate that verification and have not themselves been on a
//! bench. Any change to the constants or to
//! [`command_for`]/[`decode_response`] should be re-verified with that tool.
//!
//! CH6 and CH7 are unwired. They must be **tied to ground**, not left
//! floating: a floating CMOS input reads whatever it has capacitively picked
//! up, so an unconnected channel returns noise rather than a stable value. That
//! costs nothing while no code reads those channels, but it is the habit to
//! keep, because on this board the consequence of getting it wrong is now
//! concrete — the two channels that *were* unwired until recently are the gain
//! channels, and a floating gain channel is a gain that wanders at random.
//! Grounded, a channel reads 0 counts, which on the gain mapping is -24 dB:
//! quiet, so a mis-wired or dead channel fails towards silence rather than
//! towards a loud surprise.
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

use super::raw::{ControlSource, PotPosition, PotPositionError, Pots, RawControls};

/// GPIO pin (BCM numbering) the bypass switch is wired to.
///
/// Read with the `SoC`'s internal pull-up and the switch shorting the pin to
/// ground, so the *electrical* level is inverted with respect to
/// [`RawControls::bypass_engaged`]; no external resistor is on the board. The
/// part is a latching (alternate-action) switch, so the pin sits at whichever
/// level the switch was last left in rather than pulsing low for a press.
const BYPASS_GPIO: u8 = 17;

/// SPI clock rate for the MCP3008 conversation.
///
/// Deliberately well under the MCP3008's ~1.35–2 MHz ceiling at 3.3 V
/// (datasheet): breadboard jumpers pick up noise readily at higher clock
/// rates, and there is nothing to buy by going faster. Six conversions are 18
/// bytes, about 300 µs at this rate, against a
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
/// MCP3008 channel wired to the Input Gain pot (per-effect-band input gain in dB).
const CHANNEL_INPUT_GAIN: u8 = 4;
/// MCP3008 channel wired to the Output Gain pot (post-sum gain in dB).
const CHANNEL_OUTPUT_GAIN: u8 = 5;

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
    // is `POT_POSITION_MAX`, so every possible response decodes to a valid
    // `PotPosition` (exhaustively asserted in the tests below).
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
    /// A decoded conversion was out of range for [`PotPosition`].
    #[error("MCP3008 conversion out of range: {0}")]
    Count(#[from] PotPositionError),
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
    fn read_channel(&self, channel: u8) -> Result<PotPosition, PiControlError> {
        let mut response = [0_u8; 3];
        self.spi.transfer(&mut response, &command_for(channel))?;

        // `decode_response` masks to 10 bits, so its result cannot exceed
        // `POT_POSITION_MAX` and this conversion cannot actually fail. It is
        // still propagated rather than unwrapped: the alternative is a panic
        // on the control thread if that reasoning ever stops holding, and
        // `PiControlError::Count` turns the same event into a counted,
        // reported read failure that leaves audio running.
        Ok(PotPosition::try_new(decode_response(response))?)
    }
}

impl ControlSource for PiControls {
    type Error = PiControlError;

    /// Eight counts, from a worst-case raw σ of 6.39 measured across all six
    /// channels at both full and mid travel
    /// (`docs/raspberry-pi/control-surface-verification.md`).
    ///
    /// The mapping layer's rule is `DEADBAND_COUNTS >= σ` of the raw jitter,
    /// so 8.0 ≥ 6.39 holds with room to spare — but not much: this is the
    /// noisier of the two surfaces by a wide margin, and the figure has no
    /// slack for a quieter one to inherit. It is 0.8% of travel, roughly 128
    /// distinct positions across a sweep, and `8 / 1023 * 48` ≈ 0.375 dB on
    /// the two gain pots.
    const DEADBAND_COUNTS: f32 = 8.0;

    /// Reads the six channels and the switch as one sample.
    ///
    /// The conversions are sequential and each is a separate SPI transaction —
    /// the MCP3008 has one converter and no scan mode — so the six counts are
    /// staggered by tens of microseconds. That is far below anything a hand
    /// can do to a knob, so they are treated as simultaneous.
    fn read(&mut self) -> Result<RawControls, Self::Error> {
        let pots = Pots {
            depth: self.read_channel(CHANNEL_DEPTH)?,
            time: self.read_channel(CHANNEL_TIME)?,
            upward: self.read_channel(CHANNEL_UPWARD)?,
            downward: self.read_channel(CHANNEL_DOWNWARD)?,
            input_gain: self.read_channel(CHANNEL_INPUT_GAIN)?,
            output_gain: self.read_channel(CHANNEL_OUTPUT_GAIN)?,
        };

        // Active-low against the internal pull-up: the switch shorts the pin
        // to ground, so Low is the switch closed. The part latches, so that is
        // a resting position rather than a press — the pin holds its level
        // until a hand moves the switch back. `RawControls::bypass_engaged` is
        // the switch's logical position, so the level is inverted here and
        // nowhere else. Every poll reads the raw level; debouncing it belongs
        // to `ControlMapping`, which has no hardware to know about.
        let bypass_engaged = self.bypass.read() == Level::Low;

        Ok(RawControls {
            pots,
            bypass_engaged,
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
    use crate::control::POT_POSITION_MAX;

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
        assert_eq!(
            command_for(CHANNEL_INPUT_GAIN),
            [0x01, 0xC0, 0x00],
            "CH4 must be requested single-ended"
        );
        assert_eq!(
            command_for(CHANNEL_OUTPUT_GAIN),
            [0x01, 0xD0, 0x00],
            "CH5 must be requested single-ended"
        );
    }

    #[test]
    fn the_command_covers_the_channels_this_wiring_does_not_use() {
        // Nothing calls these today — CH6 and CH7 are the two the board leaves
        // unwired (and grounded; see the module doc) — but the encoding is the
        // datasheet's, not this board's: the top channel must still land in the
        // same nibble.
        assert_eq!(
            command_for(6),
            [0x01, 0xE0, 0x00],
            "CH6 must set the two high channel-select bits"
        );
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
            POT_POSITION_MAX,
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
        // to 10 bits means `PotPosition::try_new` cannot reject a decoded
        // response, whatever the bus returns.
        for high in 0..=u8::MAX {
            for low in 0..=u8::MAX {
                let count = decode_response([0xFF, high, low]);
                assert!(
                    count <= POT_POSITION_MAX,
                    "decoded {count} from high={high:#04x} low={low:#04x}, above the 10-bit ceiling"
                );
                assert!(
                    PotPosition::try_new(count).is_ok(),
                    "PotPosition rejected {count}, decoded from high={high:#04x} low={low:#04x}"
                );
            }
        }
    }
}
