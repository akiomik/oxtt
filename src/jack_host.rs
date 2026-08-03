//! JACK port registration, `ProcessHandler`, and `NotificationHandler` (docs/architecture.md, docs/contracts.md §6, §7).

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use jack::{AudioIn, AudioOut, Client, ClientOptions, Control, Port, ProcessScope};
use signal_hook::consts::{SIGINT, SIGTERM};
use signal_hook::flag;
use thiserror::Error;

use crate::control::ControlHandle;
use crate::dsp::OttProcessor;
use crate::params::{ConfigError, OttParams};

/// JACK client name and the port names it registers (docs/contracts.md §7).
const CLIENT_NAME: &str = "oxtt";
const PORT_INPUT_L: &str = "input_l";
const PORT_INPUT_R: &str = "input_r";
const PORT_OUTPUT_L: &str = "output_l";
const PORT_OUTPUT_R: &str = "output_r";

/// Errors that can occur connecting to or running under JACK.
#[derive(Debug, Error)]
pub enum HostError {
    /// A JACK client or port operation failed.
    #[error("JACK error: {0}")]
    Jack(#[from] jack::Error),
    /// The supplied parameters failed validation.
    #[error("invalid parameters: {0}")]
    Config(#[from] ConfigError),
    /// Installing the SIGINT/SIGTERM handler failed.
    #[error("failed to install signal handler: {0}")]
    Signal(#[from] io::Error),
}

/// Diagnostics collected by the JACK host during one completed run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunSummary {
    xrun_count: u64,
    control_read_failures: Option<u64>,
}

impl RunSummary {
    /// Number of xrun notifications delivered by JACK while the client was active.
    #[must_use]
    pub const fn xrun_count(self) -> u64 {
        self.xrun_count
    }

    /// Number of control-surface reads that failed while the client was
    /// active, or `None` if the run had no control surface at all.
    ///
    /// The distinction matters to the caller: a healthy control thread and an
    /// absent one both failed zero reads, but only one of them is worth
    /// printing. Reported after the fact for the same reason as `xrun_count`:
    /// the control thread throttles its own stderr output, so this is the only
    /// exact figure.
    #[must_use]
    pub const fn control_read_failures(self) -> Option<u64> {
        self.control_read_failures
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
    xrun_count: Arc<AtomicU64>,
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

    fn xrun(&mut self, _: &Client) -> Control {
        // This notification can run on a JACK-managed thread. Counting is
        // intentionally the only work here; formatting/reporting happens in
        // the CLI after the host has stopped.
        self.xrun_count.fetch_add(1, Ordering::Relaxed);
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
    /// The reading end of the control thread's handoff, absent for a build
    /// with no control surface attached. Reading it is wait-free and
    /// allocation-free, so it is legal here (docs/contracts.md §6).
    control: Option<triple_buffer::Output<OttParams>>,
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

        // Strictly after the reset above: `reset` rebuilds the processor from
        // the targets it already holds (docs/contracts.md §2), so a snapshot
        // applied before it would be thrown away on a sample-rate change.
        if let Some(control) = self.control.as_mut() {
            // `update` is the swap; it returns whether a new snapshot actually
            // arrived, so `set_params` runs when a knob moved rather than on
            // every cycle. `peek_output_buffer` then reads what was just
            // swapped in without swapping again.
            if control.update() {
                // A rejected update leaves the processor unchanged
                // (docs/contracts.md §2), and the callback has no way to
                // report an error in any case (docs/contracts.md §6) — so
                // there is nothing to do with the result but drop it. Every
                // snapshot the mapping layer produces is built from validated
                // base parameters, so a rejection would mean the sample rate
                // changed underneath it, and the next reading corrects that.
                let _ = self.processor.set_params(*control.peek_output_buffer());
            }
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
/// `control` is an already-running control thread, or `None` for a build with
/// no control surface — a plain desktop JACK client, or any host without the
/// hardware. It arrives as a concrete [`ControlHandle`] rather than a generic
/// [`ControlSource`](crate::control::ControlSource), so which hardware is
/// behind it stays entirely the caller's concern.
///
/// # Errors
///
/// Returns a [`RunSummary`] after a normal shutdown. Returns `HostError` if
/// connecting to JACK, registering ports, validating `params`, installing the
/// SIGINT/SIGTERM handler, or deactivating the client fails.
pub fn run(params: OttParams, mut control: Option<ControlHandle>) -> Result<RunSummary, HostError> {
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
    let xrun_count = Arc::new(AtomicU64::new(0));

    flag::register(SIGINT, Arc::clone(&shutdown_requested))?;
    flag::register(SIGTERM, Arc::clone(&shutdown_requested))?;

    let notifications = Notifications {
        shutdown_requested: Arc::clone(&shutdown_requested),
        pending_sample_rate: Arc::clone(&pending_sample_rate),
        xrun_count: Arc::clone(&xrun_count),
    };
    let process_handler = AudioProcessHandler {
        processor,
        input_l,
        input_r,
        output_l,
        output_r,
        pending_sample_rate,
        control: control.as_mut().and_then(ControlHandle::take_output),
    };

    let active_client = client.activate_async(notifications, process_handler)?;

    while !shutdown_requested.load(Ordering::Acquire) {
        // Main thread's shutdown-poll loop, not the audio callback: the
        // real-time callback contract (docs/contracts.md §6) doesn't apply here.
        #[allow(clippy::disallowed_methods)]
        thread::sleep(Duration::from_millis(50));
    }

    // Stop the control thread only after JACK has let go of the callback, so
    // nothing is publishing into a buffer whose reader is being torn down.
    // Held rather than propagated so that a failing `deactivate` still joins
    // the thread instead of leaving it polling the hardware forever.
    let deactivated = active_client.deactivate();
    let control_read_failures = control.map(ControlHandle::stop_and_join);
    deactivated?;

    Ok(RunSummary {
        xrun_count: xrun_count.load(Ordering::Relaxed),
        control_read_failures,
    })
}

#[cfg(test)]
mod tests {
    use super::RunSummary;

    #[test]
    fn run_summary_exposes_its_diagnostic_counts() {
        let summary = RunSummary {
            xrun_count: 3,
            control_read_failures: Some(7),
        };
        assert_eq!(summary.xrun_count(), 3);
        assert_eq!(summary.control_read_failures(), Some(7));
    }

    #[test]
    fn a_run_without_a_control_surface_reports_no_count_at_all() {
        // Not `Some(0)`: the CLI decides whether to print the line from this,
        // and a desktop run must keep its exit report byte-for-byte what it
        // has always been (the Pi verification scripts match it exactly).
        let summary = RunSummary {
            xrun_count: 0,
            control_read_failures: None,
        };
        assert_eq!(summary.control_read_failures(), None);
    }
}
