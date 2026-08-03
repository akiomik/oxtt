//! Wiring verification for oxtt's stage-3 physical controls
//! (`tmp/spec.md` §6): reads MCP3008 channels 0-3 (Depth, Time, Upward,
//! Downward) over SPI0 and the Bypass switch on GPIO17, printing raw values
//! to stdout in a loop.
//!
//! This deliberately does nothing else — no calibration, no normalization,
//! no oxtt integration — so the electrical/wiring layer can be verified
//! before any of that is built on top of it.

use std::error::Error;
use std::io;
use std::thread;
use std::time::Duration;

use rppal::gpio::{Gpio, Level};
use spidev::{SpiModeFlags, Spidev, SpidevOptions, SpidevTransfer};

// On the Raspberry Pi 5, the RP1 southbridge exposes the header's SPI0/CE0
// (GPIO8-11) through spidev bus 10, not bus 0 as on earlier Pi models --
// `/dev/spidev0.0` doesn't exist there. `rppal`'s `Spi` type only opens
// `/dev/spidev{0..=6}.{ss}`, so this talks to spidev directly instead.
// Confirm with `ls /dev/spidev*` if this path doesn't match your board.
const SPI_DEVICE: &str = "/dev/spidev10.0";
const BYPASS_GPIO: u8 = 17;
const SPI_CLOCK_HZ: u32 = 1_000_000;
const POLL_INTERVAL: Duration = Duration::from_millis(200);

/// Reads one MCP3008 channel (0-7) in single-ended mode and returns the raw
/// 10-bit count (0-1023).
fn read_channel(spi: &Spidev, channel: u8) -> io::Result<u16> {
    let tx = [0x01, (0x08 | channel) << 4, 0x00];
    let mut rx = [0u8; 3];
    let mut transfer = SpidevTransfer::read_write(&tx, &mut rx);
    spi.transfer(&mut transfer)?;
    Ok((u16::from(rx[1] & 0x03) << 8) | u16::from(rx[2]))
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut spi = Spidev::open(SPI_DEVICE)?;
    let options = SpidevOptions::new()
        .bits_per_word(8)
        .max_speed_hz(SPI_CLOCK_HZ)
        .mode(SpiModeFlags::SPI_MODE_0)
        .build();
    spi.configure(&options)?;

    let bypass_pin = Gpio::new()?.get(BYPASS_GPIO)?.into_input_pullup();

    println!(
        "Reading MCP3008 CH0-3 ({SPI_DEVICE}) and Bypass (GPIO{BYPASS_GPIO}). Ctrl+C to stop."
    );

    loop {
        let depth = read_channel(&spi, 0)?;
        let time = read_channel(&spi, 1)?;
        let upward = read_channel(&spi, 2)?;
        let downward = read_channel(&spi, 3)?;
        let bypass = match bypass_pin.read() {
            Level::Low => "pressed",
            Level::High => "released",
        };

        println!("Depth={depth} Time={time} Upward={upward} Downward={downward} Bypass={bypass}");

        thread::sleep(POLL_INTERVAL);
    }
}
