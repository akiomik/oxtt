//! The [`BelaApplication`] oxtt runs under: what each callback does, and
//! where the state lives (docs/architecture.md, docs/contracts.md §6, §9).
//!
//! Nothing in this module touches libbela, so it compiles, lints and tests on
//! a development machine — `bela`'s device code is behind a `bela_device` cfg
//! its build script only sets for aarch64 Linux. Only [`super::run`] needs a
//! board.

use core::fmt;

use bela::{
    BelaApplication, BlockContext, CleanupContext, RenderContext, ResolvedSettings, SetupContext,
    ThreadInfo,
};

use super::controls::{ANALOG_CHANNELS_USED, PollDecimator, raw_controls};
use crate::control::ControlMapping;
use crate::dsp::OttProcessor;
use crate::params::OttParams;

/// Analog frame the control surface is read from.
///
/// The first of the block: the pots are levels, not events, and a block is
/// 0.33 ms at 48 kHz with a period of 16 — far below anything a hand does —
/// so which frame within it is read cannot matter. Reading one frame rather
/// than averaging the block is deliberate: the mapping layer already filters,
/// and averaging here would filter twice with only one of the two calibrated
/// (`src/control/mapping.rs`).
const POLL_FRAME: usize = 0;

/// Digital channel (`D0`) the latching bypass switch is wired to.
///
/// Unlike the Raspberry Pi's GPIO17 this pin has no internal pull-up, so the
/// wiring supplies an external one to 3.3 V with the switch shorting to
/// ground (docs/bela/control-surface-setup.md). The active-low convention is
/// then identical to the Pi's, and a broken connection reads as "not
/// bypassed" — the effect keeps working rather than silently dropping out.
const BYPASS_CHANNEL: usize = 0;

/// Digital channels the control surface occupies: just the one the bypass
/// switch is on, so `D0` has to exist for `digital_read` to reach it.
const DIGITAL_CHANNELS_USED: usize = BYPASS_CHANNEL + 1;

/// Audio channels oxtt processes. Stereo in, stereo out; a Gem Stereo has
/// exactly this and nothing else.
const AUDIO_CHANNELS: usize = 2;

/// Everything one render thread mutates.
///
/// Just the processor: [`OttProcessor`] is `Copy`, holds all of its state
/// inline, and works a frame at a time, so there is no block-sized scratch
/// buffer to own (see [`OttApplication::render`]).
#[derive(Debug, Clone, Copy)]
pub struct OttRenderState {
    processor: OttProcessor,
}

/// The oxtt application: a processor prototype, the control surface's mapping
/// layer, and the counters the run reports at the end.
///
/// The mapping layer lives here rather than in [`OttRenderState`] because
/// there is one control surface, not one per render thread — and because
/// `render_pre` holds `&mut self` and `&mut [OttRenderState]` at the same
/// time, which is what lets a snapshot go straight from the mapping layer
/// into the processors with no queue, no atomic and no thread in between.
/// That is the whole of layer C on the Raspberry Pi (ADR 0010, ADR 0011).
#[derive(Debug, Clone, Copy)]
pub struct OttApplication {
    processor: OttProcessor,
    /// The parameters the processor was built from, kept because
    /// `validate_settings` has to re-check them against the sample rate the
    /// settings ask for and `OttProcessor` does not hand its targets back.
    params: OttParams,
    mapping: Option<ControlMapping>,
    poll: PollDecimator,
    publishes: u64,
    rejects: u64,
    report_on_exit: bool,
}

impl OttApplication {
    /// Builds the application around an already-validated processor.
    ///
    /// The processor is constructed by the caller, before any audio system
    /// exists, so a parameter mistake is reported as itself rather than as a
    /// failed initialisation (`super::run`).
    ///
    /// `controls` asks for the physical control surface. With it off, the
    /// parameters stay exactly what the command line said for the whole run.
    ///
    /// `report_on_exit` prints [`RunDiagnostics`] once the run has finished.
    /// The host cannot print them for us: [`bela::Bela::until_stopped`]
    /// consumes the audio system and never hands the application back, so
    /// `cleanup` is the last place the counters exist.
    #[must_use]
    pub const fn new(
        processor: OttProcessor,
        params: OttParams,
        controls: bool,
        report_on_exit: bool,
    ) -> Self {
        Self {
            processor,
            params,
            // Seeded with the same parameters the processor starts from, so
            // the fields no pot drives keep their command-line values and the
            // six that are pot-driven become the hardware's from its first
            // reading onward (`ControlMapping::new`).
            mapping: if controls {
                Some(ControlMapping::new(params))
            } else {
                None
            },
            // Replaced in `setup` once the board has reported its block
            // shape. Reading every block until then is the conservative
            // starting point, and no block is rendered before `setup` runs.
            poll: PollDecimator::EVERY_BLOCK,
            publishes: 0,
            rejects: 0,
            report_on_exit,
        }
    }

    /// How many control snapshots reached the processors.
    #[must_use]
    pub const fn publishes(&self) -> u64 {
        self.publishes
    }

    /// How many snapshots the processor refused as invalid.
    ///
    /// Zero for a run whose parameters were validated up front and whose
    /// sample rate never changed, which is every Bela run; non-zero would
    /// mean the mapping layer produced something the processor's own
    /// validation disagreed with, and is worth seeing for that reason alone.
    #[must_use]
    pub const fn rejects(&self) -> u64 {
        self.rejects
    }

    /// Whether this run has a control surface at all.
    #[must_use]
    pub const fn has_controls(&self) -> bool {
        self.mapping.is_some()
    }

    /// The read divisor `setup` settled on, for the host to report.
    #[must_use]
    pub const fn poll(&self) -> PollDecimator {
        self.poll
    }
}

impl BelaApplication for OttApplication {
    type RenderState = OttRenderState;

    /// Refuses a configuration oxtt will not run under, before any audio
    /// system is built.
    ///
    /// Everything checkable here is checked here rather than in
    /// [`setup`](BelaApplication::setup): `setup` runs inside
    /// `Bela_initAudio` with the hardware already up, so refusing from there
    /// fails the initialisation and leaves the process unable to build
    /// another audio system (bela-rs#112). Refusing here is an ordinary
    /// error.
    ///
    /// What this sees is what will be *asked* of libbela, not what the board
    /// will deliver — the audio channel counts are not among the settings for
    /// that reason, and are checked in `setup` instead.
    fn validate_settings(&self, settings: &ResolvedSettings<'_>) -> Result<(), &'static str> {
        // A block is split across render threads by frame range, and every
        // filter and envelope in the processor carries state from one frame
        // to the next, so a second thread would start mid-signal from a state
        // that never saw the frames before it. The crossover alone is twelve
        // biquads per channel (ADR 0001).
        if settings.thread_count() != 1 {
            return Err("oxtt renders on one thread: its filters carry state across frames");
        }

        if self.mapping.is_some() {
            let analog_in = usize::try_from(settings.num_analog_in_channels()).unwrap_or(0);
            if !settings.use_analog() || analog_in < ANALOG_CHANNELS_USED {
                return Err("the control surface needs six analog inputs (A0-A5)");
            }
            let digital = usize::try_from(settings.num_digital_channels()).unwrap_or(0);
            if !settings.use_digital() || digital < DIGITAL_CHANNELS_USED {
                return Err(
                    "the control surface needs one digital input (D0) for the bypass switch",
                );
            }
        }

        // The Nyquist-relative crossover limit is the one parameter check
        // that needs a sample rate, so it is the one the command line could
        // not make on its own (`src/cli.rs`). Making it here means an
        // out-of-range crossover ends the program with its own message
        // instead of a failed initialisation.
        self.params
            .validate(settings.audio_sample_rate())
            .map_err(|_| "the crossover frequencies are too high for this sample rate")?;

        Ok(())
    }

    /// Confirms the board delivered stereo, and adapts to the block it
    /// reports.
    ///
    /// Only the audio channel counts are left to check here, because they are
    /// the one thing `validate_settings` cannot see: libbela ignores the
    /// requested counts and the delivered ones do not exist until
    /// `Bela_initAudio` has run. Refusing costs the process its audio system
    /// (see `validate_settings`), which is acceptable for exactly this case —
    /// oxtt exits on it either way and never tries a second configuration.
    fn setup(&mut self, context: &SetupContext) -> bool {
        if context.audio_in_channels() < AUDIO_CHANNELS
            || context.audio_out_channels() < AUDIO_CHANNELS
        {
            return false;
        }

        // The rate the board settled on, which is what the processor's filter
        // coefficients have to be built for (docs/contracts.md §2).
        if self.processor.reset(context.audio_sample_rate()).is_err() {
            return false;
        }

        self.poll =
            PollDecimator::for_block_rate(context.audio_sample_rate(), context.audio_frames());
        true
    }

    /// Hands each render thread its own copy of the processor.
    ///
    /// A copy rather than a construction, because `OttProcessor::new` can
    /// fail and this cannot: the prototype was built and validated before any
    /// of this ran. With `thread_count` pinned to 1 by `validate_settings`
    /// there is exactly one.
    fn create_render_state(
        &mut self,
        _thread: ThreadInfo,
        _context: &SetupContext,
    ) -> OttRenderState {
        OttRenderState {
            processor: self.processor,
        }
    }

    /// Reads the control surface and pushes any new snapshot into the
    /// processors.
    ///
    /// This is layer A and the whole of the handoff, in the one callback that
    /// holds both the mapping layer (`&mut self`) and the processors
    /// (`&mut [OttRenderState]`). On the Raspberry Pi the same handoff needs
    /// a thread, a triple buffer and a poll interval, because its audio
    /// callback cannot read SPI (ADR 0010).
    ///
    /// Real-time safe: reading a slice, six float conversions, the mapping
    /// layer (itself proven panic-free), and a validated assignment. No
    /// allocation, no lock, no I/O (docs/contracts.md §6).
    fn render_pre(&mut self, states: &mut [OttRenderState], context: &mut BlockContext) {
        if !self.poll.tick() {
            return;
        }
        let Some(mapping) = self.mapping.as_mut() else {
            return;
        };

        // `analog_in` is the whole block interleaved by channel; the first
        // chunk is every channel of `POLL_FRAME`. Taken as a slice rather
        // than through `analog_read` so that nothing here can panic on an
        // index (bela-rs#114): `chunks_exact` refuses a zero-sized chunk by
        // returning nothing, and `nth` refuses a short block the same way.
        let channels = context.analog_in_channels();
        let frame = context
            .analog_in()
            .chunks_exact(channels.max(1))
            .nth(POLL_FRAME)
            .unwrap_or_default();

        // The switch is wired active-low against an external pull-up, so the
        // pin being low is the switch being closed is the effect being
        // bypassed. Inverting here, at the one place that reads the pin,
        // matches `PiControls::read`.
        let bypass_engaged = !context.digital_read(POLL_FRAME, BYPASS_CHANNEL);

        let Some(snapshot) = mapping.update(raw_controls(frame, bypass_engaged)) else {
            return;
        };
        // Saturating for `clippy::arithmetic_side_effects` (docs/contracts.md
        // §6). A `u64` at 500 publishes a second would take half a billion
        // years to reach the ceiling, so this is a lint's shape rather than a
        // behaviour.
        self.publishes = self.publishes.saturating_add(1);
        for state in states.iter_mut() {
            if state.processor.set_control_snapshot(snapshot).is_err() {
                self.rejects = self.rejects.saturating_add(1);
            }
        }
    }

    /// Processes the block, one frame at a time, in place.
    ///
    /// `audio_io` pairs the input with the output so both can be held at
    /// once, which `audio_in`/`audio_out` cannot do — the first borrows the
    /// context shared and the second uniquely (bela-rs#110). `frames` then
    /// walks the two together, which also removes the one arithmetic mistake
    /// this loop invites: on a `RenderContext` the input is indexed from
    /// block frame 0 and the output from this thread's first frame, and those
    /// coincide only because `thread_count` is 1.
    ///
    /// Real-time safe: no allocation, no lock, no I/O, and no index — the
    /// slice patterns below cannot go out of bounds, and `process_frame` is
    /// proven panic-free (docs/contracts.md §6).
    fn render(&self, state: &mut OttRenderState, context: &mut RenderContext) {
        let mut io = context.audio_io();
        for (input, output) in io.frames() {
            // A frame short of stereo is silence rather than a panic; `setup`
            // has already refused a board that would do this.
            let [left_in, right_in, ..] = *input else {
                continue;
            };
            let (left_out, right_out) = state.processor.process_frame(left_in, right_in);
            if let [left, right, ..] = output {
                *left = left_out;
                *right = right_out;
            }
        }
    }

    /// Reports what the run measured.
    ///
    /// Printing from here rather than from the host is forced by the shape of
    /// the API — [`bela::Bela::until_stopped`] consumes the audio system and
    /// does not return the application — but it is safe: `cleanup` runs after
    /// audio has stopped, outside every real-time callback, so the no-I/O
    /// rule in docs/contracts.md §6 does not reach it.
    #[expect(
        clippy::disallowed_macros,
        reason = "cleanup runs after audio has stopped, outside the real-time callbacks docs/contracts.md §6 governs; same exemption as src/main.rs"
    )]
    fn cleanup(&mut self, _states: &mut [OttRenderState], context: &CleanupContext) {
        if self.report_on_exit {
            eprintln!("{}", self.diagnostics(context));
        }
    }
}

/// What a completed run measured, gathered where the counters still exist.
///
/// Collected in `cleanup` because that is the last callback with both the
/// context and the application in hand; printing happens in `main`, outside
/// every callback.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RunDiagnostics {
    underrun_count: u32,
    audio_frames_elapsed: u64,
    cpu_percentage: Option<f32>,
    publishes: Option<u64>,
    rejects: Option<u64>,
}

impl RunDiagnostics {
    /// Blocks libbela reported as underrun while the run was going.
    #[must_use]
    pub const fn underrun_count(self) -> u32 {
        self.underrun_count
    }

    /// Audio frames the run processed, which is how long it ran.
    #[must_use]
    pub const fn audio_frames_elapsed(self) -> u64 {
        self.audio_frames_elapsed
    }

    /// The last CPU reading, or `None` if CPU monitoring was not enabled.
    #[must_use]
    pub const fn cpu_percentage(self) -> Option<f32> {
        self.cpu_percentage
    }

    /// Control snapshots published, or `None` if the run had no control
    /// surface.
    ///
    /// The same distinction `jack_host::RunSummary::control_read_failures`
    /// draws: a surface that published nothing and no surface at all are not
    /// the same thing, and only one of them is worth printing.
    #[must_use]
    pub const fn publishes(self) -> Option<u64> {
        self.publishes
    }

    /// Snapshots the processor refused, or `None` if there was no control
    /// surface.
    #[must_use]
    pub const fn rejects(self) -> Option<u64> {
        self.rejects
    }
}

/// One line per figure, each `oxtt: name=value`.
///
/// The same shape as the JACK host's `oxtt: xrun_count=N`, and for the same
/// reason: the verification scripts under `scripts/` match these lines whole,
/// so each keeps its exact wording and stays alone on its line.
///
/// A figure that does not apply is left out rather than printed as zero — a
/// run with no control surface published nothing, but so did a broken one.
impl fmt::Display for RunDiagnostics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "oxtt: underrun_count={}", self.underrun_count)?;
        write!(
            f,
            "\noxtt: audio_frames_elapsed={}",
            self.audio_frames_elapsed
        )?;
        if let Some(percentage) = self.cpu_percentage {
            write!(f, "\noxtt: cpu_percentage={percentage:.1}")?;
        }
        if let Some(publishes) = self.publishes {
            write!(f, "\noxtt: control_publishes={publishes}")?;
        }
        if let Some(rejects) = self.rejects {
            write!(f, "\noxtt: control_rejects={rejects}")?;
        }
        Ok(())
    }
}

impl OttApplication {
    /// Reads the run's diagnostics out of the cleanup context.
    ///
    /// Separate from [`BelaApplication::cleanup`] so that the figures can be
    /// built and checked without printing them.
    #[must_use]
    pub fn diagnostics(&self, context: &CleanupContext) -> RunDiagnostics {
        RunDiagnostics {
            underrun_count: context.underrun_count(),
            audio_frames_elapsed: context.audio_frames_elapsed(),
            cpu_percentage: context.cpu_usage().map(|usage| usage.percentage()),
            publishes: self.has_controls().then_some(self.publishes),
            rejects: self.has_controls().then_some(self.rejects),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::params::Preset;

    fn diagnostics() -> RunDiagnostics {
        RunDiagnostics {
            underrun_count: 0,
            audio_frames_elapsed: 480_000,
            cpu_percentage: None,
            publishes: None,
            rejects: None,
        }
    }

    #[test]
    fn a_plain_run_reports_only_what_applies() {
        assert_eq!(
            diagnostics().to_string(),
            "oxtt: underrun_count=0\noxtt: audio_frames_elapsed=480000"
        );
    }

    #[test]
    fn a_run_with_a_control_surface_reports_its_counters() {
        let report = RunDiagnostics {
            publishes: Some(12),
            rejects: Some(0),
            ..diagnostics()
        };
        assert_eq!(
            report.to_string(),
            "oxtt: underrun_count=0\noxtt: audio_frames_elapsed=480000\noxtt: control_publishes=12\noxtt: control_rejects=0"
        );
    }

    #[test]
    fn cpu_is_reported_only_when_it_was_measured() {
        let report = RunDiagnostics {
            cpu_percentage: Some(31.25),
            ..diagnostics()
        };
        assert!(report.to_string().contains("\noxtt: cpu_percentage=31.2"));
        assert!(!diagnostics().to_string().contains("cpu_percentage"));
    }

    #[test]
    fn the_control_surface_is_off_unless_asked_for() {
        let params = Preset::SafeStart.params();
        let processor = OttProcessor::new(48_000.0, params).unwrap();
        assert!(!OttApplication::new(processor, params, false, false).has_controls());
        assert!(OttApplication::new(processor, params, true, false).has_controls());
    }
}
