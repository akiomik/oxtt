//! The [`BelaApplication`] hyperglare runs under
//! (`docs/hyperglare/contracts.md` §8, ADR 0011).
//!
//! Nothing in this module touches libbela, so it compiles, lints and tests on
//! a development machine — `bela`'s device code is behind a `bela_device` cfg
//! its build script only sets for aarch64 Linux. Only [`super::run`] needs a
//! board.
//!
//! **There is no control surface here**, and that is the difference from
//! `oxtt-bela` rather than an omission. `hyperglare` has more settings than
//! the six pots that panel carries, and which of them a player should reach
//! is undecided; the chord is a command-line argument until the MIDI path
//! exists. What this host is for is the question that comes first: whether
//! the DSP runs on the board at all.

use core::fmt;

use bela::{
    BelaApplication, BlockContext, CleanupContext, MidiInput, MidiMessage, PinMode, RenderContext,
    ResolvedSettings, SetupContext, ThreadInfo,
};

use effectkit::metering::{ClipIndicator, InputMeter};
use hyperglare_dsp::note::note_hz;
use hyperglare_dsp::processor::{HyperglareParams, HyperglareProcessor};

/// How long the clip indicator stays lit after the last clipped frame.
///
/// 0.42 seconds at 48 kHz, which is libbela's own `underrunLedDuration` to the
/// frame — the same number `oxtt-bela` uses, so two effects on one board do
/// not blink at noticeably different speeds.
const CLIP_HOLD_FRAMES: u64 = 20_000;

/// Audio channels hyperglare processes. Stereo in, stereo out; a Gem Stereo
/// has exactly this and nothing else.
const AUDIO_CHANNELS: usize = 2;

/// Resonators the board's bank has room for.
///
/// **A quarter of what the offline renderer carries**, which is a difference
/// worth knowing rather than one to close: `hyperglare-render` sizes for the
/// harmonic geometry at the top of its range, and this sizes for what a board
/// is asked to play. Settings that fit both give the same bank; settings that
/// truncate here and not there give two banks that differ, and `active()` in
/// `--report-on-exit` is where that shows.
///
/// Capacity costs memory and not time — the per-sample loop runs over
/// `active`, not over `N` — so this is about four kilobytes and no cycles.
///
/// It is enough for the octave geometries at any chord worth playing, and not
/// enough for a harmonic series at the top of its range, which truncates.
/// `docs/hyperglare/contracts.md` §2 says what truncation costs.
pub const CAPACITY: usize = 256;

/// The processor the board runs.
pub type Processor = HyperglareProcessor<CAPACITY>;

/// Everything one render thread mutates.
#[derive(Debug, Clone, Copy)]
pub struct HyperglareRenderState {
    processor: Processor,
    /// What this thread's share of the input looked like.
    input: InputMeter,
}

/// The hyperglare application: a processor prototype, the chord it is tuned
/// to, and the counters the run reports at the end.
///
/// Not `Clone` since it may own a MIDI port, which is one device and one
/// reader — copying it would be two readers of one ring.
#[derive(Debug)]
pub struct HyperglareApplication {
    processor: Processor,
    params: HyperglareParams,
    /// The chord, in hertz.
    ///
    /// Owned rather than borrowed because the application outlives whatever
    /// parsed it, and kept so that `setup` can retune once the board has said
    /// what sample rate it settled on.
    notes_hz: Vec<f32>,
    /// The digital channel an LED is wired to, and the hold that makes a
    /// 21 µs event visible on it. `None` is a run with nothing wired.
    clip_led: Option<(usize, ClipIndicator)>,
    report_on_exit: bool,
    /// The MIDI port keys arrive on, if the run asked for one.
    ///
    /// **`None` and a chord in `notes_hz` are the two ways to have notes, and
    /// they are alternatives** (ADR 0021). A run given a port starts with no
    /// keys down, so it is silent at `--color 1.0` until one goes down — which
    /// is correct and looks like a fault, so it is worth saying here.
    midi: Option<MidiInput>,
}

impl HyperglareApplication {
    /// Builds the application from a processor that is already tuned.
    #[must_use]
    pub fn new(
        processor: Processor,
        params: HyperglareParams,
        notes_hz: Vec<f32>,
        clip_led: Option<usize>,
        report_on_exit: bool,
        midi: Option<MidiInput>,
    ) -> Self {
        Self {
            processor,
            params,
            notes_hz,
            clip_led: clip_led.map(|channel| (channel, ClipIndicator::new(CLIP_HOLD_FRAMES))),
            report_on_exit,
            midi,
        }
    }

    /// Drains the block's MIDI into the render states' processors.
    ///
    /// **Into the states and not into the prototype.** Each render thread got
    /// a copy of the processor at setup, and the copies are what play; the
    /// prototype has never seen a sample. Pressing a key on it as well would
    /// not be redundant, it would be wrong — the allocator chooses by how much
    /// a voice is holding, and a processor that has processed nothing holds
    /// nothing, so it would answer with a different voice and the two would
    /// drift apart.
    ///
    /// Called from `render_pre`, which `bela`'s `MidiInput` documents as the
    /// place: the ring has one reader, and `render` holds the application as
    /// `&self` on every render thread at once.
    ///
    /// **Every channel, and velocity ignored.** There is one source and one
    /// instrument here; filtering and touch are additive and neither reaches
    /// the voice table. A note on at velocity zero is a release, which is what
    /// most keyboards send instead of a note off.
    fn read_midi(&mut self, states: &mut [HyperglareRenderState]) {
        let Some(midi) = self.midi.as_mut() else {
            return;
        };
        while let Some(message) = midi.read() {
            let (note, down) = match message {
                MidiMessage::NoteOn { note, velocity, .. } => (note, velocity.get() > 0),
                MidiMessage::NoteOff { note, .. } => (note, false),
                _ => continue,
            };
            let hz = note_hz(note.get());
            for state in states.iter_mut() {
                if down {
                    state.processor.note_on(hz);
                } else {
                    state.processor.note_off(hz);
                }
            }
        }
    }

    /// What the run measured, gathered where the counters still exist.
    #[must_use]
    pub fn diagnostics(
        &self,
        states: &[HyperglareRenderState],
        context: &CleanupContext,
    ) -> RunDiagnostics {
        RunDiagnostics {
            input: input_meter(states),
            // From the states rather than from the prototype: the copies are
            // what played, and MIDI only ever reached them.
            active_resonators: states.first().map_or(0, |state| state.processor.active()),
            held_voices: states.first().map_or(0, |state| state.processor.held_voices()),
            underruns: context.underrun_count(),
            audio_frames_elapsed: context.audio_frames_elapsed(),
            cpu_percentage: context.cpu_usage().map(|usage| usage.percentage()),
        }
    }
}

impl BelaApplication for HyperglareApplication {
    type RenderState = HyperglareRenderState;

    /// Refuses a configuration hyperglare will not run under, before any
    /// audio system is built.
    ///
    /// Refusing here rather than from `setup` for the reason `oxtt-bela`
    /// documents: `setup` runs inside `Bela_initAudio` with the hardware
    /// already up, so a refusal there fails the initialisation and leaves the
    /// process unable to build another audio system (bela-rs#112).
    fn validate_settings(&self, settings: &ResolvedSettings<'_>) -> Result<(), &'static str> {
        // A block is split across render threads by frame range, and every
        // resonator carries state from one frame to the next — that state is
        // the tail, and the tail is the effect. A second thread would start
        // mid-ring from a state that never saw the frames before it.
        if settings.thread_count() != 1 {
            return Err(
                "hyperglare renders on one thread: its resonators carry their tails across frames",
            );
        }

        // `pin_mode` and `digital_write` panic on a channel the board does not
        // have, and `render_pre` calls both every block. Refused here so an
        // out-of-range `--clip-led` ends the program with a message instead of
        // ending it from inside the audio callback.
        if let Some((channel, _)) = self.clip_led {
            let digital = usize::try_from(settings.num_digital_channels()).unwrap_or(0);
            if let Some(reason) = clip_led_refusal(channel, settings.use_digital(), digital) {
                return Err(reason);
            }
        }
        Ok(())
    }

    /// Retunes for the rate the board settled on, and refuses a board that is
    /// not stereo.
    fn setup(&mut self, context: &SetupContext) -> bool {
        if context.audio_in_channels() < AUDIO_CHANNELS
            || context.audio_out_channels() < AUDIO_CHANNELS
        {
            return false;
        }

        // The rate the board settled on, which every coefficient in the bank
        // and the band split was derived for. `set_sample_rate` rebuilds all
        // of them and retunes the chord from the notes kept above.
        self.processor.set_sample_rate(context.audio_sample_rate());
        self.processor.apply_params(&self.params, &self.notes_hz);
        true
    }

    /// Hands each render thread its own copy of the processor.
    ///
    /// A copy rather than a construction: the prototype is already tuned, and
    /// with `thread_count` pinned to one by `validate_settings` there is
    /// exactly one of these.
    fn create_render_state(
        &mut self,
        _thread: ThreadInfo,
        _context: &SetupContext,
    ) -> HyperglareRenderState {
        HyperglareRenderState {
            processor: self.processor,
            input: InputMeter::new(),
        }
    }

    /// Drives the clip indicator, which is the only thing that happens per
    /// block rather than per frame.
    fn render_pre(&mut self, states: &mut [HyperglareRenderState], context: &mut BlockContext) {
        self.read_midi(states);

        if let Some((channel, indicator)) = self.clip_led.as_mut() {
            let clipped = input_meter(states).clipped_frames();
            let frames = u64::try_from(context.audio_frames()).unwrap_or(u64::MAX);
            let lit = indicator.update(clipped, frames);
            // Both re-applied every block: a direction and value set once can
            // fail to reach the pin at larger period sizes (bela-rs
            // `docs/board-facts.md`).
            context.pin_mode(0, *channel, PinMode::Output);
            context.digital_write(0, *channel, lit);
        }
    }

    fn render(&self, state: &mut HyperglareRenderState, context: &mut RenderContext) {
        let mut io = context.audio_io();
        for (input, output) in io.frames() {
            // A frame short of stereo is silence rather than a panic; `setup`
            // has already refused a board that would do this.
            let [left_in, right_in, ..] = *input else {
                continue;
            };

            state.input.observe(left_in, right_in);

            let (left_out, right_out) = state.processor.process_frame(left_in, right_in);
            if let [left, right, ..] = output {
                *left = left_out;
                *right = right_out;
            }
        }
    }

    /// Reports what the run measured.
    ///
    /// Printed from here rather than from the host because
    /// [`bela::Bela::until_stopped`] consumes the audio system without handing
    /// the application back, so the counters do not outlive it.
    #[expect(
        clippy::disallowed_macros,
        reason = "cleanup runs after audio has stopped, outside the real-time callbacks docs/effectkit/realtime.md governs; same exemption as src/main.rs"
    )]
    fn cleanup(&mut self, states: &mut [HyperglareRenderState], context: &CleanupContext) {
        if self.report_on_exit {
            eprintln!("{}", self.diagnostics(states, context));
        }
    }
}

/// What a completed run measured.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RunDiagnostics {
    /// What arrived at the input.
    pub input: InputMeter,
    /// How many resonators the chord and the geometry produced.
    ///
    /// **The number that decides whether this fits.** The per-sample cost is
    /// two biquads per band plus one state-variable filter per *active*
    /// resonator, so a CPU figure means nothing without it.
    pub active_resonators: usize,
    /// How many voices are holding a note.
    ///
    /// The chord's own size, which `active_resonators` stopped being when the
    /// bank started reserving a block per voice (ADR 0021).
    pub held_voices: usize,
    /// Blocks the audio system reported as late.
    ///
    /// **The number that says whether it fits.** A CPU figure under an
    /// underrun is a figure for a run that did not keep up.
    pub underruns: u32,
    /// Audio frames the run processed, so the underruns have a denominator.
    pub audio_frames_elapsed: u64,
    /// Measured CPU load as a percentage, if the run asked for monitoring.
    pub cpu_percentage: Option<f32>,
}

/// Why a clip-indicator channel cannot be used, if it cannot.
///
/// Separate from [`BelaApplication::validate_settings`] because a
/// `ResolvedSettings` is deliberately not constructible outside the audio
/// system — what makes it *resolved* is where it comes from — so the rule
/// itself is what gets tested.
///
/// Shorter than `oxtt-bela`'s by one clause: there is no control surface here,
/// so no channel is reserved for a bypass switch and every channel the board
/// delivers is free.
const fn clip_led_refusal(
    channel: usize,
    use_digital: bool,
    digital_channels: usize,
) -> Option<&'static str> {
    if !use_digital || digital_channels <= channel {
        return Some("the clip indicator needs a digital channel the board delivers");
    }
    None
}

/// The input meters of every render thread, combined.
fn input_meter(states: &[HyperglareRenderState]) -> InputMeter {
    states
        .iter()
        .map(|state| state.input)
        .fold(InputMeter::new(), InputMeter::merged)
}

impl fmt::Display for RunDiagnostics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "hyperglare-bela: active_resonators={} held_voices={} underruns={} \
             audio_frames_elapsed={}",
            self.active_resonators, self.held_voices, self.underruns, self.audio_frames_elapsed
        )?;
        if let Some(percentage) = self.cpu_percentage {
            write!(f, " cpu_percentage={percentage:.1}")?;
        }
        write!(
            f,
            " input_peak_dbfs={:.2} clipped_frames={}",
            self.input.peak_dbfs(),
            self.input.clipped_frames()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::clip_led_refusal;

    #[test]
    fn a_channel_the_board_delivers_is_accepted() {
        assert!(clip_led_refusal(0, true, 16).is_none());
        assert!(clip_led_refusal(15, true, 16).is_none());
    }

    #[test]
    fn a_channel_past_the_board_is_refused() {
        // The bound is exclusive: sixteen channels are numbered 0 through 15,
        // and it is channel 16 that would index past the mask `pin_mode`
        // builds and panic inside `render_pre`.
        assert!(clip_led_refusal(16, true, 16).is_some());
        assert!(clip_led_refusal(usize::MAX, true, 16).is_some());
    }

    #[test]
    fn no_channel_is_usable_without_digital_io() {
        assert!(clip_led_refusal(0, false, 16).is_some());
    }
}
