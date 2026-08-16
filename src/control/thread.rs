//! Layer C of the control surface: the control thread and its handoff into
//! the audio callback (see [`crate::control`] for the layering).
//!
//! An MCP3008 read is SPI traffic — a blocking `ioctl` — so it cannot happen
//! inside `AudioProcessHandler::process` (docs/contracts.md §6). This layer is
//! the seam that keeps it out: a plain OS thread polls the [`ControlSource`],
//! drives [`ControlMapping`], and hands finished [`ControlSnapshot`] values to
//! the callback through a `triple_buffer`.
//!
//! The triple buffer is what makes the *reading* side legal, which is the side
//! that matters. `Output::update` is a single `AcqRel` swap plus an index
//! assignment: wait-free, allocation-free, lock-free, and constant-time, so
//! the callback pays the same cost whether or not a knob moved. Its three
//! buffers are allocated once when the buffer is built, and
//! [`ControlSnapshot`] is `Copy` with no `Drop`, so publishing a snapshot
//! neither allocates nor frees anything on either side. This is the "bounded
//! non-blocking queue instead of a new lock" that docs/architecture.md
//! anticipates, with the queue bound at one: the callback wants the knob's
//! newest position, never a backlog of the positions it passed through.
//!
//! Nothing here is on the real-time path — the thread is free to block, sleep,
//! and write to stderr — which is why the crate-wide lints that encode
//! docs/contracts.md §6 are locally allowed below rather than obeyed.

use core::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use triple_buffer::{Input, Output, TripleBuffer};

use crate::params::{ControlSnapshot, OttParams};

use super::mapping::ControlMapping;
use super::raw::ControlSource;

/// How long the control thread waits between hardware reads.
///
/// 2 ms is 500 Hz. The audio callback at 128 frames / 48 kHz runs at about
/// 375 Hz, so polling faster than that cannot make a knob turn feel any more
/// immediate — the callback would simply find the same snapshot twice — while
/// polling much slower would let the callback outrun the control surface and
/// make a fast turn arrive in visible steps. 500 Hz sits just above the
/// callback rate, which costs six MCP3008 conversions per 2 ms (a few
/// hundred microseconds of SPI on the Pi's bus, on a thread that has nothing
/// else to do) and leaves headroom for a smaller JACK buffer.
///
/// This is also the effective time constant of [`ControlMapping`]'s filter,
/// which is defined per read rather than per millisecond: at 500 Hz its
/// 5-read step response is 10 ms.
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(2);

/// How many read failures pass between two stderr reports.
///
/// A disconnected ADC fails on *every* poll, so an unthrottled report would
/// emit 500 lines a second and bury whatever else the terminal is showing.
/// The first failure is always reported, because that is the one that
/// diagnoses the problem; after that, at [`DEFAULT_POLL_INTERVAL`], this
/// works out to one line every 10 seconds — enough to show the fault is
/// ongoing, quiet enough to leave the terminal usable. The exact total is
/// reported once at shutdown from [`ControlHandle::stop_and_join`], the same
/// way xrun counts are (docs/contracts.md §7).
const FAILURE_REPORT_INTERVAL: u64 = 5_000;

/// A running control thread, and the audio callback's end of its handoff.
///
/// Created by [`ControlHandle::spawn`]. The two ends are consumed separately:
/// [`ControlHandle::take_output`] hands the reading end to the audio
/// callback, and the host keeps the handle itself to stop the thread with
/// [`ControlHandle::stop_and_join`] once JACK has been deactivated.
///
/// Dropping a handle without calling `stop_and_join` detaches the thread
/// rather than stopping it. The host only does that on a startup path that is
/// already returning an error, where the process is about to exit anyway.
#[derive(Debug)]
pub struct ControlHandle {
    output: Option<Output<ControlSnapshot>>,
    stop: Arc<AtomicBool>,
    read_failures: Arc<AtomicU64>,
    worker: JoinHandle<()>,
}

impl ControlHandle {
    /// Starts a control thread polling `source` every `poll_interval`.
    ///
    /// `base` is the CLI parameter set. It seeds both ends with an explicitly
    /// disengaged bypass level, so a callback that reads before the first
    /// successful poll sees exactly the parameters the processor was built
    /// with. [`ControlMapping`] overlays the six pot-driven fields from the
    /// first reading onward.
    ///
    /// [`DEFAULT_POLL_INTERVAL`] is the interval to pass unless a caller has
    /// a specific reason not to.
    #[must_use]
    // `thread::spawn` and the `thread::sleep` in the poll loop are the two
    // things this layer exists to do, on a thread that is not the audio
    // callback; the crate-wide bans encode docs/contracts.md §6, which applies
    // to the callback only.
    #[allow(clippy::disallowed_methods)]
    pub fn spawn<S: ControlSource + Send + 'static>(
        source: S,
        base: OttParams,
        poll_interval: Duration,
    ) -> Self {
        let initial = ControlSnapshot {
            params: base,
            bypass_engaged: false,
        };
        let (publisher, output) = TripleBuffer::new(&initial).split();
        let stop = Arc::new(AtomicBool::new(false));
        let read_failures = Arc::new(AtomicU64::new(0));

        let worker = {
            let stop = Arc::clone(&stop);
            let read_failures = Arc::clone(&read_failures);
            thread::spawn(move || {
                poll_until_stopped(
                    source,
                    ControlMapping::new(base, S::DEADBAND_COUNTS),
                    publisher,
                    &stop,
                    &read_failures,
                    poll_interval,
                );
            })
        };

        Self {
            output: Some(output),
            stop,
            read_failures,
            worker,
        }
    }

    /// Takes the audio callback's end of the handoff, leaving the handle able
    /// only to stop the thread.
    ///
    /// Returns `None` on any later call. A `triple_buffer::Output` is
    /// single-consumer — `update` takes `&mut self` and owns the consumer's
    /// buffer index — so exactly one holder can ever exist, and moving it out
    /// is how that is enforced rather than promised.
    pub const fn take_output(&mut self) -> Option<Output<ControlSnapshot>> {
        self.output.take()
    }

    /// Number of failed hardware reads so far.
    ///
    /// A failed read publishes nothing and does not stop the thread, so this
    /// is the only evidence that the control surface is misbehaving while the
    /// client is still running.
    #[must_use]
    pub fn read_failures(&self) -> u64 {
        self.read_failures.load(Ordering::Relaxed)
    }

    /// Stops the control thread, waits for it, and returns its final
    /// [`read_failures`](Self::read_failures) count.
    ///
    /// Returns up to one `poll_interval` after it is called: the thread
    /// checks the stop flag once per poll, so it wakes from its sleep, sees
    /// the flag, and exits without another read.
    ///
    /// Joining blocks, so this belongs on the host's shutdown path and never
    /// in the audio callback (docs/contracts.md §6).
    // Deliberately not `#[must_use]`: stopping the thread is the reason to
    // call this, and the count is a diagnostic a caller may reasonably ignore.
    #[allow(clippy::must_use_candidate)]
    pub fn stop_and_join(self) -> u64 {
        self.stop.store(true, Ordering::Release);
        // A join error means the control thread panicked. There is nothing
        // left to wait for and nothing useful to do about it here: audio has
        // already stopped by this point, and the count read below is still
        // whatever the thread managed to record.
        let _ = self.worker.join();
        self.read_failures.load(Ordering::Relaxed)
    }
}

/// The control thread's body: read, condition, publish, sleep, repeat.
///
/// A read failure is deliberately not fatal. An intermittent SPI error must
/// not take the control surface down for the rest of the session, and it must
/// never take audio down: publishing nothing leaves the callback on the last
/// good snapshot, which is the pots' last known position — a far better
/// failure mode than either freezing the audio thread or reverting to the CLI
/// defaults.
#[allow(clippy::disallowed_methods)] // `thread::sleep`; see `ControlHandle::spawn`.
fn poll_until_stopped<S: ControlSource>(
    mut source: S,
    mut mapping: ControlMapping,
    mut publisher: Input<ControlSnapshot>,
    stop: &AtomicBool,
    read_failures: &AtomicU64,
    poll_interval: Duration,
) {
    while !stop.load(Ordering::Acquire) {
        match source.read() {
            // `None` means the conditioned value did not move, so there is
            // nothing new for the callback to apply and the buffer is left
            // holding the last published snapshot.
            Ok(raw) => {
                if let Some(params) = mapping.update(raw) {
                    publisher.write(params);
                }
            }
            Err(error) => {
                let previous = read_failures.fetch_add(1, Ordering::Relaxed);
                report_read_failure(previous, &error);
            }
        }

        thread::sleep(poll_interval);
    }
}

/// Reports a read failure to stderr, throttled to [`FAILURE_REPORT_INTERVAL`].
///
/// `failures_before` is the count *excluding* this failure, so the first one
/// always reports.
// stderr from the control thread, which is not the audio callback; the
// crate-wide ban on these macros encodes docs/contracts.md §6, which applies
// to the callback only. The same reasoning as `main.rs`.
#[allow(clippy::disallowed_macros)]
fn report_read_failure(failures_before: u64, error: &impl fmt::Display) {
    // `checked_rem` rather than `%` only to keep the expression free of a
    // division that clippy has to prove cannot trap; the divisor is a nonzero
    // constant.
    if failures_before.checked_rem(FAILURE_REPORT_INTERVAL) == Some(0) {
        eprintln!(
            "oxtt: control read failed ({} so far): {error}",
            failures_before.saturating_add(1)
        );
    }
}

#[cfg(test)]
// Tests assert on exact conditioned values and fail loudly on a timeout, so
// unwrapping known-good fixtures and panicking on a missed deadline are the
// intent here rather than an oversight. The float comparisons are exact for
// the same reason as in `mapping.rs`: a published value is a hand-computable
// count divided by `POT_POSITION_MAX`, not the result of accumulated arithmetic.
#[allow(clippy::unwrap_used, clippy::panic, clippy::float_cmp)]
mod tests {
    use core::convert::Infallible;
    use core::error::Error as StdError;
    use core::sync::atomic::AtomicU16;
    use std::time::Instant;

    use super::*;
    use crate::control::{POT_POSITION_MAX, PotPosition, Pots, RawControls};
    use crate::dsp::OttProcessor;
    use crate::params::Preset;

    /// Long enough that a loaded CI machine cannot hit it by being slow, short
    /// enough that a genuinely stuck thread fails the run rather than hanging it.
    const TEST_TIMEOUT: Duration = Duration::from_secs(5);
    /// Faster than [`DEFAULT_POLL_INTERVAL`] so the tests below finish quickly;
    /// nothing under test depends on the interval's value.
    const TEST_POLL_INTERVAL: Duration = Duration::from_millis(1);
    /// What "promptly" means for a stop that should cost one poll interval.
    const STOP_BUDGET: Duration = Duration::from_millis(500);
    const SAMPLE_RATE: f32 = 48_000.0;

    /// Spins until `condition` holds, failing with `what` instead of hanging.
    ///
    /// Polling with a deadline rather than sleeping a guessed duration: the
    /// control thread's timing is not observable from here, so any fixed wait
    /// would be either flaky or needlessly slow.
    fn wait_until(what: &str, mut condition: impl FnMut() -> bool) {
        let start = Instant::now();
        while start.elapsed() < TEST_TIMEOUT {
            if condition() {
                return;
            }
            thread::yield_now();
        }
        panic!("timed out after {TEST_TIMEOUT:?} waiting for {what}");
    }

    fn reading(raw: u16) -> RawControls {
        let count = PotPosition::try_new(raw).unwrap();
        RawControls {
            pots: Pots {
                depth: count,
                time: count,
                upward: count,
                downward: count,
                input_gain: count,
                output_gain: count,
            },
            bypass_engaged: false,
        }
    }

    fn normalized(raw: u16) -> f32 {
        f32::from(raw) / f32::from(POT_POSITION_MAX)
    }

    /// A pot the test can turn, standing in for the MCP3008.
    struct TurnablePot {
        position: Arc<AtomicU16>,
        reads: Arc<AtomicU64>,
    }

    impl TurnablePot {
        fn at(raw: u16) -> (Self, Arc<AtomicU16>, Arc<AtomicU64>) {
            let position = Arc::new(AtomicU16::new(raw));
            let reads = Arc::new(AtomicU64::new(0));
            let source = Self {
                position: Arc::clone(&position),
                reads: Arc::clone(&reads),
            };
            (source, position, reads)
        }
    }

    impl ControlSource for TurnablePot {
        type Error = Infallible;

        /// The Raspberry Pi's figure: the widest of the real ones, so a
        /// fake conditioned against it is conditioned conservatively.
        const DEADBAND_COUNTS: f32 = 8.0;

        fn read(&mut self) -> Result<RawControls, Self::Error> {
            self.reads.fetch_add(1, Ordering::Release);
            Ok(reading(self.position.load(Ordering::Acquire)))
        }
    }

    #[derive(Debug)]
    struct DisconnectedAdc;

    impl fmt::Display for DisconnectedAdc {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("no response from the ADC")
        }
    }

    impl StdError for DisconnectedAdc {}

    /// Fails its first `failures` reads, then behaves like a pot at `raw`.
    struct FlakyAdc {
        remaining_failures: u64,
        raw: u16,
        reads: Arc<AtomicU64>,
    }

    impl ControlSource for FlakyAdc {
        type Error = DisconnectedAdc;

        /// The Raspberry Pi's figure: the widest of the real ones, so a
        /// fake conditioned against it is conditioned conservatively.
        const DEADBAND_COUNTS: f32 = 8.0;

        fn read(&mut self) -> Result<RawControls, Self::Error> {
            self.reads.fetch_add(1, Ordering::Release);
            if self.remaining_failures == 0 {
                return Ok(reading(self.raw));
            }
            self.remaining_failures = self.remaining_failures.saturating_sub(1);
            Err(DisconnectedAdc)
        }
    }

    /// Runs `f` with the reading end of a freshly spawned handle, then stops
    /// the thread and returns its read-failure count.
    fn with_control<S: ControlSource + Send + 'static>(
        source: S,
        f: impl FnOnce(&mut Output<ControlSnapshot>, &ControlHandle),
    ) -> u64 {
        let mut handle =
            ControlHandle::spawn(source, Preset::SafeStart.params(), TEST_POLL_INTERVAL);
        let mut output = handle
            .take_output()
            .expect("a fresh handle must hand out its output");
        f(&mut output, &handle);
        handle.stop_and_join()
    }

    #[test]
    fn the_output_starts_at_the_base_parameters() {
        let base = Preset::Default.params();
        let (source, _position, _reads) = TurnablePot::at(0);
        let mut handle = ControlHandle::spawn(source, base, TEST_POLL_INTERVAL);
        let output = handle
            .take_output()
            .expect("a fresh handle must hand out its output");

        // Before `update`, the callback sees exactly what the processor was
        // built with rather than an uninitialized or defaulted snapshot.
        assert_eq!(
            output.output_buffer().params,
            base,
            "the buffer must start seeded with the CLI parameters"
        );
        handle.stop_and_join();
    }

    #[test]
    fn a_turning_pot_reaches_the_audio_side() {
        let (source, position, _reads) = TurnablePot::at(100);
        with_control(source, |output, _handle| {
            wait_until("the initial position to be published", || {
                output.update() && output.output_buffer().params.global.depth.get() > 0.0
            });

            position.store(900, Ordering::Release);
            wait_until("the turned position to reach the output", || {
                output.update();
                output.output_buffer().params.global.depth.get() > normalized(880)
            });

            // The turn drives all six pots, not just the one asserted above.
            let params = *output.output_buffer();
            assert!(
                params.params.global.time.get() > normalized(880)
                    && params.params.global.upward.get() > normalized(880)
                    && params.params.global.downward.get() > normalized(880),
                "every effect pot must track the turn, got {params:?}"
            );
            assert!(
                params.params.global.input_gain_db.get() > 15.0
                    && params.params.global.output_gain_db.get() > 15.0,
                "both gain pots must track the turn, got {params:?}"
            );
        });
    }

    #[test]
    fn a_motionless_pot_publishes_only_its_first_reading() {
        let (source, _position, reads) = TurnablePot::at(400);
        with_control(source, |output, _handle| {
            wait_until("the first reading to be published", || output.update());
            assert_eq!(
                output.output_buffer().params.global.depth.get(),
                normalized(400),
                "the first publish must carry the pot's position"
            );

            // Let the thread poll many more times; a still pot must fall in
            // `ControlMapping`'s deadband every time and publish nothing.
            let polled_by_now = reads.load(Ordering::Acquire).saturating_add(50);
            wait_until("the thread to poll 50 more times", || {
                reads.load(Ordering::Acquire) >= polled_by_now
            });
            assert!(
                !output.update(),
                "a motionless pot must not publish again after its first reading"
            );
        });
    }

    #[test]
    fn read_failures_are_counted_without_stopping_the_thread() {
        let reads = Arc::new(AtomicU64::new(0));
        let source = FlakyAdc {
            remaining_failures: 5,
            raw: 700,
            reads: Arc::clone(&reads),
        };

        let failures = with_control(source, |output, handle| {
            wait_until("the failing reads to be counted", || {
                handle.read_failures() >= 5
            });

            // The thread survived them: it goes on to read successfully and
            // publish, which is the whole point of not treating a read error
            // as fatal.
            wait_until("a publish after the failures", || {
                output.update()
                    && output.output_buffer().params.global.depth.get() == normalized(700)
            });
        });

        assert_eq!(
            failures, 5,
            "every failed read must be counted exactly once"
        );
    }

    #[test]
    fn a_permanently_failing_source_neither_panics_nor_publishes() {
        let reads = Arc::new(AtomicU64::new(0));
        let source = FlakyAdc {
            remaining_failures: u64::MAX,
            raw: 0,
            reads: Arc::clone(&reads),
        };

        let failures = with_control(source, |output, handle| {
            wait_until("a run of failed reads", || handle.read_failures() >= 20);
            assert!(
                !output.update(),
                "a source that never succeeds must never publish"
            );
        });

        assert!(
            failures >= 20,
            "the failure count must survive to shutdown, got {failures}"
        );
    }

    #[test]
    fn stop_and_join_terminates_promptly() {
        let (source, _position, reads) = TurnablePot::at(500);
        let handle = ControlHandle::spawn(source, Preset::SafeStart.params(), TEST_POLL_INTERVAL);
        wait_until("the thread to start polling", || {
            reads.load(Ordering::Acquire) > 0
        });

        let start = Instant::now();
        let failures = handle.stop_and_join();
        let elapsed = start.elapsed();

        // The thread checks the flag once per poll, so joining costs about one
        // interval plus scheduling. The bound is generous against a loaded
        // machine but still orders of magnitude below a thread that only
        // noticed the flag by accident, or never did.
        assert!(
            elapsed < STOP_BUDGET,
            "stop_and_join took {elapsed:?}, which is not prompt"
        );
        assert_eq!(failures, 0, "an infallible source must report no failures");
        // `join` returning is itself the proof the thread is gone; the read
        // count is only here to show it had really started.
        assert!(
            reads.load(Ordering::Acquire) > 0,
            "the thread must have polled before being stopped"
        );
    }

    #[test]
    fn the_output_can_only_be_taken_once() {
        let (source, _position, _reads) = TurnablePot::at(0);
        let mut handle =
            ControlHandle::spawn(source, Preset::SafeStart.params(), TEST_POLL_INTERVAL);

        assert!(
            handle.take_output().is_some(),
            "the first take must hand out the output"
        );
        assert!(
            handle.take_output().is_none(),
            "the output must not be handed out twice"
        );
        handle.stop_and_join();
    }

    /// The chain the audio callback actually runs, minus JACK: `update` gates
    /// the work, `output_buffer` reads the snapshot without a second
    /// swap, and `set_control_snapshot` applies it (docs/contracts.md §2,
    /// §6). Proves
    /// the whole control surface end to end on a development machine.
    #[test]
    fn the_callbacks_consume_pattern_drives_the_processor() {
        let base = Preset::SafeStart.params();
        let mut processor = OttProcessor::new(SAMPLE_RATE, base).unwrap();

        let (source, position, _reads) = TurnablePot::at(100);
        let mut handle = ControlHandle::spawn(source, base, TEST_POLL_INTERVAL);
        let mut output = handle
            .take_output()
            .expect("a fresh handle must hand out its output");

        position.store(1023, Ordering::Release);
        let mut applied = 0_u32;
        wait_until("the processor to be handed the turned position", || {
            // Byte-for-byte the callback's snapshot step.
            if output.update() {
                let accepted = processor
                    .set_control_snapshot(*output.output_buffer())
                    .is_ok();
                assert!(accepted, "a mapped snapshot must always validate");
                applied = applied.saturating_add(1);
            }
            output.output_buffer().params.global.depth.get() > normalized(1000)
        });
        assert!(applied > 0, "the callback pattern must apply at least once");

        // The processor keeps running on the applied targets: audio still
        // flows, which is the observable end of the chain.
        let input = [0.5_f32; 64];
        let mut out_l = [0.0_f32; 64];
        let mut out_r = [0.0_f32; 64];
        processor
            .process(&input, &input, &mut out_l, &mut out_r)
            .expect("equal-length buffers must process");
        assert!(
            out_l.iter().chain(out_r.iter()).all(|s| s.is_finite()),
            "output must stay finite after a control update"
        );

        handle.stop_and_join();
    }
}
