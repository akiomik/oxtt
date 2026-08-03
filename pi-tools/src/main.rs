//! Wiring verification for oxtt's physical control surface: reads MCP3008
//! channels 0-3 (Depth, Time, Upward, Downward) over SPI0 and the Bypass
//! switch on GPIO17, printing raw and normalized values to stdout in a loop.
//! The same wiring, and the reasoning behind each constant, is documented in
//! `src/control/pi.rs`, which reproduces this read inside `oxtt` itself.
//!
//! Normalization is a plain linear scale (raw / 1023), with no per-pot
//! calibration offsets: the potentiometers are linear-taper (B-curve) and
//! their range, measured with this tool on the assembled hardware, already
//! covers the full 0-1023 scale. This is a display-only sanity check, not the
//! real `NormalizedF32` (`src/params`) conversion -- that, and the same read
//! against the same wiring, now live in `oxtt` proper under `src/control/`,
//! while this tool stays independent of the `oxtt` crate by design so it can
//! be run on a Pi with nothing else working.

use std::error::Error;
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

/// Reads one MCP3008 channel (0-7) in single-ended mode and returns the raw
/// 10-bit count (0-1023).
fn read_channel(spi: &Spi, channel: u8) -> Result<u16, rppal::spi::Error> {
    let write = [0x01, (0x08 | channel) << 4, 0x00];
    let mut read = [0u8; 3];
    spi.transfer(&mut read, &write)?;
    Ok((u16::from(read[1] & 0x03) << 8) | u16::from(read[2]))
}

/// Linear raw-to-normalized scale (see the module doc for why no
/// per-pot calibration is needed).
fn normalize(raw: u16) -> f32 {
    f32::from(raw) / ADC_MAX
}

fn main() -> Result<(), Box<dyn Error>> {
    let spi = Spi::new(Bus::Spi0, SlaveSelect::Ss0, SPI_CLOCK_HZ, Mode::Mode0)?;
    let bypass_pin = Gpio::new()?.get(BYPASS_GPIO)?.into_input_pullup();

    println!("Reading MCP3008 CH0-3 and Bypass (GPIO{BYPASS_GPIO}). Ctrl+C to stop.");

    loop {
        let depth = read_channel(&spi, 0)?;
        let time = read_channel(&spi, 1)?;
        let upward = read_channel(&spi, 2)?;
        let downward = read_channel(&spi, 3)?;
        let bypass = match bypass_pin.read() {
            Level::Low => "pressed",
            Level::High => "released",
        };

        println!(
            "Depth={depth} ({:.3}) Time={time} ({:.3}) Upward={upward} ({:.3}) Downward={downward} ({:.3}) Bypass={bypass}",
            normalize(depth),
            normalize(time),
            normalize(upward),
            normalize(downward)
        );

        thread::sleep(POLL_INTERVAL);
    }
}
