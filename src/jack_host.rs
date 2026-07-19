//! JACK port registration, `ProcessHandler`, and `NotificationHandler` (docs/architecture.md, docs/contracts.md §6, §7).

use std::error::Error;
use std::fmt;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::thread;
use std::time::Duration;

use jack::{AudioIn, AudioOut, Client, ClientOptions, Control, Port, ProcessScope};
use signal_hook::consts::{SIGINT, SIGTERM};
use signal_hook::flag;

use crate::dsp::OttProcessor;
use crate::params::{ConfigError, OttParams};

/// JACK client name and the port names it registers (docs/contracts.md §7).
const CLIENT_NAME: &str = "oxtt";
const PORT_INPUT_L: &str = "input_l";
const PORT_INPUT_R: &str = "input_r";
const PORT_OUTPUT_L: &str = "output_l";
const PORT_OUTPUT_R: &str = "output_r";

/// Errors that can occur connecting to or running under JACK.
#[derive(Debug)]
pub enum HostError {
    /// A JACK client or port operation failed.
    Jack(jack::Error),
    /// The supplied parameters failed validation.
    Config(ConfigError),
    /// Installing the SIGINT/SIGTERM handler failed.
    Signal(io::Error),
}

impl fmt::Display for HostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Jack(e) => write!(f, "JACK error: {e}"),
            Self::Config(e) => write!(f, "invalid parameters: {e}"),
            Self::Signal(e) => write!(f, "failed to install signal handler: {e}"),
        }
    }
}

impl Error for HostError {}

impl From<jack::Error> for HostError {
    fn from(e: jack::Error) -> Self {
        Self::Jack(e)
    }
}

impl From<ConfigError> for HostError {
    fn from(e: ConfigError) -> Self {
        Self::Config(e)
    }
}

impl From<io::Error> for HostError {
    fn from(e: io::Error) -> Self {
        Self::Signal(e)
    }
}

/// Receives JACK shutdown notifications and sample-rate changes (docs/contracts.md §7).
///
/// These callbacks may be invoked from a thread other than the process
/// callback, so they hand off state safely via Atomics instead of a lock
/// (docs/contracts.md §6).
struct Notifications {
    shutdown_requested: Arc<AtomicBool>,
    pending_sample_rate: Arc<AtomicU32>,
}

impl jack::NotificationHandler for Notifications {
    unsafe fn shutdown(&mut self, _status: jack::ClientStatus, _reason: &str) {
        self.shutdown_requested.store(true, Ordering::Release);
    }

    fn sample_rate(&mut self, _: &Client, srate: jack::Frames) -> Control {
        // Use 0 as the "nothing pending" sentinel. JACK's sample rate is never 0 in practice.
        self.pending_sample_rate
            .store(srate.max(1), Ordering::Release);
        Control::Continue
    }
}

/// The audio callback. Prohibits heap allocation, locks, I/O, and panics (docs/contracts.md §6).
struct AudioProcessHandler {
    processor: OttProcessor,
    input_l: Port<AudioIn>,
    input_r: Port<AudioIn>,
    output_l: Port<AudioOut>,
    output_r: Port<AudioOut>,
    pending_sample_rate: Arc<AtomicU32>,
}

impl jack::ProcessHandler for AudioProcessHandler {
    fn process(&mut self, _: &Client, ps: &ProcessScope) -> Control {
        let pending = self.pending_sample_rate.swap(0, Ordering::AcqRel);
        if pending != 0 {
            // On a sample-rate change: recompute all filter coefficients and
            // time coefficients, and reset state (docs/contracts.md §7). Never
            // panics inside the callback, even on failure (docs/contracts.md §6).
            // JACK sample rates stay far below f32's 16.7M exact-integer range.
            #[allow(clippy::cast_precision_loss)]
            let _ = self.processor.reset(pending as f32);
        }

        let in_l = self.input_l.as_slice(ps);
        let in_r = self.input_r.as_slice(ps);

        let ok = {
            let out_l = self.output_l.as_mut_slice(ps);
            let out_r = self.output_r.as_mut_slice(ps);
            self.processor.process(in_l, in_r, out_l, out_r).is_ok()
        };

        if !ok {
            // JACK always passes the same frame count to every port, so this
            // practically never happens, but even if lengths ever mismatch,
            // output silence instead of panicking (docs/contracts.md §6).
            for s in self.output_l.as_mut_slice(ps).iter_mut() {
                *s = 0.0;
            }
            for s in self.output_r.as_mut_slice(ps).iter_mut() {
                *s = 0.0;
            }
        }

        Control::Continue
    }
}

/// Connects to JACK and starts `oxtt`. Blocks until SIGINT/SIGTERM/JACK
/// shutdown is received, then stops safely (docs/contracts.md §7).
///
/// # Errors
///
/// Returns `HostError` if connecting to JACK, registering ports, validating
/// `params`, or installing the SIGINT/SIGTERM handler fails.
pub fn run(params: OttParams) -> Result<(), HostError> {
    let (client, _status) = Client::new(CLIENT_NAME, ClientOptions::default())?;

    // Never auto-connects to physical ports (docs/contracts.md §7).
    let input_l = client.register_port(PORT_INPUT_L, AudioIn::default())?;
    let input_r = client.register_port(PORT_INPUT_R, AudioIn::default())?;
    let output_l = client.register_port(PORT_OUTPUT_L, AudioOut::default())?;
    let output_r = client.register_port(PORT_OUTPUT_R, AudioOut::default())?;

    // Use the sample rate assigned by JACK (docs/contracts.md §7). JACK
    // sample rates stay far below f32's 16.7M exact-integer range.
    #[allow(clippy::cast_precision_loss)]
    let sample_rate = client.sample_rate() as f32;
    let processor = OttProcessor::new(sample_rate, params)?;

    let shutdown_requested = Arc::new(AtomicBool::new(false));
    let pending_sample_rate = Arc::new(AtomicU32::new(0));

    flag::register(SIGINT, Arc::clone(&shutdown_requested))?;
    flag::register(SIGTERM, Arc::clone(&shutdown_requested))?;

    let notifications = Notifications {
        shutdown_requested: Arc::clone(&shutdown_requested),
        pending_sample_rate: Arc::clone(&pending_sample_rate),
    };
    let process_handler = AudioProcessHandler {
        processor,
        input_l,
        input_r,
        output_l,
        output_r,
        pending_sample_rate,
    };

    let active_client = client.activate_async(notifications, process_handler)?;

    while !shutdown_requested.load(Ordering::Acquire) {
        thread::sleep(Duration::from_millis(50));
    }

    active_client.deactivate()?;
    Ok(())
}
