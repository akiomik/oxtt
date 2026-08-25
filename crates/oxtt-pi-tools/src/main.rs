//! Wiring verification for oxtt's physical control surface: reads MCP3008
//! channels 0-5 (Depth, Time, Upward, Downward, Input Gain, Output Gain) over
//! SPI0 and the Bypass switch on GPIO17, printing raw and converted values to
//! stdout in a loop. The same wiring, and the reasoning behind each constant,
//! is documented in `src/control/pi.rs`, which reproduces this read inside
//! `oxtt` itself.
//!
//! The conversions here are display-only sanity checks, not the real
//! `NormalizedF32`/`IoGain` conversions in `src/control/mapping.rs` -- this
//! tool stays independent of the `oxtt` crate by design, so it can be run on a
//! Pi with nothing else working. They mirror that module's arithmetic so the
//! numbers on screen are the numbers the effect will act on:
//!
//! - CH0-CH3 are a plain linear scale (raw / 1023). No per-pot calibration
//!   offsets: the potentiometers are linear-taper (B-curve) and their range,
//!   measured with this tool on the assembled hardware, already covers the full
//!   0-1023 scale.
//! - CH4-CH5 map that same scale onto -24..+24 dB, so unity gain is the centre
//!   of the pot's rotation.
//!
//! The Bypass switch is a latching (alternate-action) part, so the pin reports
//! which position it is resting in rather than a press; the display says
//! `engaged`/`disengaged` accordingly.

use std::error::Error;
use std::io::{self, Write};
use std::thread;
use std::time::Duration;

use rppal::gpio::{Gpio, Level};
use rppal::spi::{Bus, Mode, SlaveSelect, Spi};

const BYPASS_GPIO: u8 = 17;
// Lower than the MCP3008's ~1.35-2 MHz ceiling at 3.3V (MCP3008 datasheet):
// breadboard jumpers pick up noise more readily at higher clock rates, and
// this tool cares about clean wiring verification, not throughput.
const SPI_CLOCK_HZ: u32 = 500_000;
const POLL_INTERVAL: Duration = Duration::from_millis(200);
const ADC_MAX: f32 = 1023.0;

/// Lower stop of the gain pots, in dB, and the span they sweep. Mirrors
/// `GAIN_MIN_DB`/`GAIN_SPAN_DB` in `src/control/mapping.rs`.
const GAIN_MIN_DB: f32 = -24.0;
const GAIN_SPAN_DB: f32 = 48.0;

/// Reads one MCP3008 channel (0-7) in single-ended mode and returns the raw
/// 10-bit count (0-1023).
fn read_channel(spi: &Spi, channel: u8) -> Result<u16, rppal::spi::Error> {
    let write = [0x01, (0x08 | channel) << 4, 0x00];
    let mut read = [0u8; 3];
    spi.transfer(&mut read, &write)?;
    Ok((u16::from(read[1] & 0x03) << 8) | u16::from(read[2]))
}

/// Linear raw-to-normalized scale for CH0-CH3 (see the module doc).
fn normalize(raw: u16) -> f32 {
    f32::from(raw) / ADC_MAX
}

/// Raw-to-dB scale for the two gain channels (see the module doc).
fn gain_db(raw: u16) -> f32 {
    normalize(raw).mul_add(GAIN_SPAN_DB, GAIN_MIN_DB)
}

fn main() -> Result<(), Box<dyn Error>> {
    let spi = Spi::new(Bus::Spi0, SlaveSelect::Ss0, SPI_CLOCK_HZ, Mode::Mode0)?;
    let bypass_pin = Gpio::new()?.get(BYPASS_GPIO)?.into_input_pullup();

    let stdout = io::stdout();
    let mut out = stdout.lock();

    if writeln!(
        out,
        "Reading MCP3008 CH0-5 and Bypass (GPIO{BYPASS_GPIO}). Ctrl+C to stop."
    )
    .is_err()
    {
        return Ok(());
    }

    loop {
        let depth = read_channel(&spi, 0)?;
        let time = read_channel(&spi, 1)?;
        let upward = read_channel(&spi, 2)?;
        let downward = read_channel(&spi, 3)?;
        let input_gain = read_channel(&spi, 4)?;
        let output_gain = read_channel(&spi, 5)?;
        let bypass = match bypass_pin.read() {
            Level::Low => "engaged",
            Level::High => "disengaged",
        };

        // One sample per line, with every value as `Name=raw`, so the awk
        // reduction in docs/raspberry-pi/control-surface-verification.md can
        // pick out any channel by name.
        let wrote = writeln!(
            out,
            "Depth={depth} ({:.3}) Time={time} ({:.3}) Upward={upward} ({:.3}) Downward={downward} ({:.3}) InputGain={input_gain} ({:+.1} dB) OutputGain={output_gain} ({:+.1} dB) Bypass={bypass}",
            normalize(depth),
            normalize(time),
            normalize(upward),
            normalize(downward),
            gain_db(input_gain),
            gain_db(output_gain)
        );
        // The reader closed the pipe (e.g. `head -n N` in a capture script) --
        // that's an expected way for a consumer to stop listening, not a
        // fault, so exit quietly instead of panicking on the write error.
        if wrote.is_err() {
            return Ok(());
        }

        thread::sleep(POLL_INTERVAL);
    }
}
