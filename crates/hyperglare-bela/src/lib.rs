//! Bela audio-system setup and lifecycle for `hyperglare`
//! (`docs/hyperglare/contracts.md` §8, ADR 0011).
//!
//! The counterpart of `oxtt-bela`: it builds the settings, brings the audio
//! system up, sets the codec levels, waits, and reports. The processing itself
//! is in [`app`], which is deliberately free of libbela so that it compiles
//! and tests on a development machine — only [`run`] needs a board, and it is
//! the only thing here behind `cfg(bela_device)`.
//!
//! **What this host is for is one question**: whether the DSP that ADRs 0016
//! to 0020 settled runs on the board at all, and at what cost. Two of those
//! decisions — the band count and the grid's ceiling — were accepted with the
//! CPU explicitly unmeasured, because `hyperglare` had never been on a board.

pub mod app;
pub mod cli;

use core::num::NonZeroU32;

use thiserror::Error;

pub use app::{CAPACITY, HyperglareApplication, HyperglareRenderState, Processor, RunDiagnostics};
pub use cli::BelaCli;

/// Audio sample rate hyperglare asks a Bela for.
///
/// The same rate `oxtt-bela` asks for, so that a CPU figure from one is
/// comparable with the other and both are comparable with the Raspberry Pi
/// measurements (ADR 0008, ADR 0011). Nothing in this effect needs 48 kHz for
/// itself — the grid stops at 5 kHz — but a number nobody can compare is a
/// number that has to be measured again.
pub const SAMPLE_RATE_HZ: NonZeroU32 = NonZeroU32::new(48_000).expect("48000 is not zero");

/// Audio frames per block hyperglare asks for, which is also the board's
/// default.
///
/// The smallest block the board supports is 2 frames, and periods of 1 and 3
/// fail inside the PRU with eight analog inputs configured, so 16 is both the
/// default and the smallest size worth asking for. It is 0.33 ms at
/// [`SAMPLE_RATE_HZ`].
pub const PERIOD_SIZE: NonZeroU32 = NonZeroU32::new(16).expect("16 is not zero");

/// Analog input channels hyperglare asks for: the board's default of eight.
///
/// **Asked for and not read.** This host has no control surface, so it needs
/// none of them — but a Gem Stereo has no analog *outputs*, and a request
/// whose input and output counts disagree fails `Bela_initAudio` outright
/// (bela-rs `docs/board-facts.md`). The configuration that is known to come up
/// on this board is the one `oxtt-bela` was measured with, and the first run
/// of a new effect is the wrong place to find out whether a different one
/// also works.
pub const ANALOG_IN_CHANNELS: u32 = 8;

/// Errors that can occur bringing up or running under a Bela audio system.
#[derive(Debug, Error)]
pub enum HostError {
    /// The audio system could not be built, started, or run.
    #[error("Bela error: {0}")]
    Bela(#[from] bela::Error),
}

/// Where a run's notes come from.
///
/// **The two are alternatives and this is what makes that structural** rather
/// than a rule a caller has to remember (ADR 0021). A fixed chord and a MIDI
/// port cannot both be given, so a run cannot start with a default chord
/// sounding underneath the keys somebody is playing.
#[derive(Debug, Clone, PartialEq)]
pub enum ChordSource {
    /// The chord named on the command line, held for the whole run.
    Fixed(Vec<f32>),
    /// Keys from an ALSA port, named as `amidi -l` names it plus a subdevice —
    /// `hw:0,0,0`.
    ///
    /// The run starts with nothing down, so it is silent at `--color 1.0`
    /// until a key goes down. That is correct and it looks like a fault, which
    /// is why it is said here as well as in the help.
    Midi(String),
}

impl ChordSource {
    /// The chord to start with: the fixed one, or none at all.
    ///
    /// Allocates, so it belongs before the audio system and not on a callback.
    #[must_use]
    pub fn initial_notes(&self) -> Vec<f32> {
        match self {
            Self::Fixed(notes) => notes.clone(),
            Self::Midi(_) => Vec::new(),
        }
    }
}

/// How a run was configured, for the caller to choose and the host to apply.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RunOptions {
    /// Audio frames per block.
    pub period_size: NonZeroU32,
    /// Audio sample rate to ask the board for.
    pub sample_rate: NonZeroU32,
    /// Ask libbela for CPU measurements, so the run can report its load.
    pub cpu_monitoring: Option<NonZeroU32>,
    /// Analog input gain, in dB, applied to the codec before the DSP.
    ///
    /// `None` leaves the board's default of +16 dB, **which clips a
    /// line-level source and has to be set** — see
    /// `docs/oxtt/bela/noise-floor.md`, which is about the board rather than
    /// about `oxtt` and applies unchanged here.
    pub adc_gain_db: Option<f32>,
    /// Headphone output level, in dB, applied to the codec after the DSP.
    ///
    /// On a Gem Stereo this is what sets the line output's level; libbela's
    /// line out level does not move it ([bela-rs#123](https://github.com/akiomik/bela-rs/issues/123)).
    /// `None` leaves libbela's default of -6 dB.
    pub headphone_level_db: Option<f32>,
    /// Digital channel an LED is wired to, lit while the input clips.
    pub clip_led: Option<usize>,
    /// Print [`RunDiagnostics`] after a normal exit.
    pub report_on_exit: bool,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            period_size: PERIOD_SIZE,
            sample_rate: SAMPLE_RATE_HZ,
            cpu_monitoring: None,
            adc_gain_db: None,
            headphone_level_db: None,
            clip_led: None,
            report_on_exit: false,
        }
    }
}

/// Builds the settings a run asks libbela for.
///
/// Separate from [`run`] so that it is not behind `cfg(bela_device)` and can
/// be checked on a development machine.
#[must_use]
pub const fn settings(options: &RunOptions) -> bela::Settings {
    let settings = bela::Settings::new()
        .period_size(options.period_size.get())
        // Deliberately absent: `num_analog_out_channels`. A Gem Stereo has
        // none, and a request whose counts disagree fails `Bela_initAudio`.
        .audio_sample_rate(options.sample_rate)
        .use_analog(true)
        .num_analog_in_channels(ANALOG_IN_CHANNELS)
        .use_digital(true)
        // One thread because every resonator carries its tail across frames;
        // `HyperglareApplication::validate_settings` refuses anything else.
        .thread_count(NonZeroU32::MIN)
        // So that `underrun_count` in the run's diagnostics means something.
        .detect_underruns(true);

    match options.cpu_monitoring {
        Some(measurements) => settings.cpu_monitoring(measurements),
        None => settings,
    }
}

#[cfg(bela_device)]
mod device {
    use bela::{Bela, Channel};

    use super::{ChordSource, HostError, HyperglareApplication, Processor, RunOptions, settings};
    use hyperglare_dsp::processor::HyperglareParams;

    /// Brings up the audio system, runs until stopped, and reports.
    ///
    /// The processor is built and tuned here, before any audio system exists.
    /// It is retuned in `setup` for the rate the board actually settled on,
    /// which is the only thing about it the command line cannot know.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Bela`] if the audio system cannot be built,
    /// refuses the settings, or reports callback faults.
    pub fn run(
        params: HyperglareParams,
        chord: &ChordSource,
        options: &RunOptions,
    ) -> Result<(), HostError> {
        #[expect(
            clippy::cast_precision_loss,
            reason = "a sample rate is far below f32's exact-integer limit"
        )]
        let sample_rate = options.sample_rate.get() as f32;
        // Empty under `ChordSource::Midi`, so a run that is played does not
        // also hold a chord nobody asked for.
        let notes_hz = chord.initial_notes();
        let mut processor = Processor::new(params, sample_rate);
        processor.apply_params(&params, &notes_hz);

        // Before the audio system, so a port that will not open ends the
        // program with its own message rather than failing an initialisation
        // the process cannot then retry (bela-rs#112, the same reason
        // `validate_settings` exists).
        let midi = match chord {
            ChordSource::Fixed(_) => None,
            ChordSource::Midi(port) => Some(bela::MidiInput::open(port)?),
        };

        let application = HyperglareApplication::new(
            processor,
            params,
            notes_hz,
            options.clip_led,
            options.report_on_exit,
            midi,
        );

        let mut bela = Bela::new(application, &settings(options))?;

        // Between construction and starting, so a level failure cannot happen
        // mid-run.
        if let Some(decibels) = options.adc_gain_db {
            bela.set_audio_input_gain(Channel::All, decibels)?;
        }
        if let Some(decibels) = options.headphone_level_db {
            bela.set_headphone_level(Channel::All, decibels)?;
        }

        bela.until_stopped()?;
        Ok(())
    }
}

#[cfg(bela_device)]
pub use device::run;

#[cfg(test)]
mod tests {
    use super::*;

    /// `Settings` has no getters, so this pins the whole configuration by
    /// comparing it against the chain written out by hand. A change detector
    /// on purpose: the settings are the part of this host a board *fails* on
    /// rather than complains about, so a call appearing or disappearing here
    /// should have to be written down twice.
    #[test]
    fn the_default_settings_are_exactly_this_and_touch_no_analog_outputs() {
        let expected = bela::Settings::new()
            .period_size(PERIOD_SIZE.get())
            .audio_sample_rate(SAMPLE_RATE_HZ)
            .use_analog(true)
            .num_analog_in_channels(ANALOG_IN_CHANNELS)
            .use_digital(true)
            .thread_count(NonZeroU32::MIN)
            .detect_underruns(true);
        assert_eq!(settings(&RunOptions::default()), expected);
    }

    /// A run that takes keys starts with nothing held.
    ///
    /// **The defect this type exists to make impossible**: the chord had a
    /// default, so a MIDI run used to start with five notes sounding and the
    /// keys layered over them.
    #[test]
    // A test, not a callback: `initial_notes` allocates by design and is
    // documented as belonging before the audio system
    // (`docs/effectkit/realtime.md`).
    #[allow(
        clippy::disallowed_macros,
        reason = "outside the real-time path; the chord is built before any audio system"
    )]
    fn a_midi_run_starts_with_no_chord() {
        assert!(
            ChordSource::Midi("hw:0,0,0".to_owned())
                .initial_notes()
                .is_empty()
        );
        assert_eq!(
            ChordSource::Fixed(vec![110.0, 164.8]).initial_notes(),
            vec![110.0, 164.8]
        );
    }

    /// And the command line cannot ask for both, so the choice above is a
    /// choice rather than a precedence nobody would remember.
    #[test]
    fn a_chord_and_a_port_cannot_be_combined() {
        use clap::Parser as _;

        assert!(BelaCli::try_parse_from(["hyperglare-bela"]).is_ok());
        assert!(BelaCli::try_parse_from(["hyperglare-bela", "--notes", "60"]).is_ok());
        assert!(BelaCli::try_parse_from(["hyperglare-bela", "--midi-port", "hw:0,0,0"]).is_ok());
        assert!(
            BelaCli::try_parse_from([
                "hyperglare-bela",
                "--notes",
                "60",
                "--midi-port",
                "hw:0,0,0",
            ])
            .is_err(),
            "the two should be refused together"
        );
    }

    /// The settings this host asks for are the ones `oxtt-bela` was measured
    /// with, so that a first run of a new effect fails on the effect rather
    /// than on a configuration nobody has seen this board accept.
    #[test]
    fn the_configuration_matches_the_host_that_has_run_on_this_board() {
        assert_eq!(SAMPLE_RATE_HZ.get(), 48_000);
        assert_eq!(PERIOD_SIZE.get(), 16);
        assert_eq!(ANALOG_IN_CHANNELS, 8);
    }

    #[test]
    fn cpu_monitoring_changes_the_settings_only_when_asked_for() {
        let plain = settings(&RunOptions::default());
        let monitored = settings(&RunOptions {
            cpu_monitoring: NonZeroU32::new(4),
            ..RunOptions::default()
        });
        assert_ne!(plain, monitored, "--report-cpu must reach the settings");
    }

    /// The codec levels are applied to the handle, not the settings.
    #[test]
    fn codec_levels_do_not_reach_the_settings() {
        let levelled = settings(&RunOptions {
            adc_gain_db: Some(-6.0),
            headphone_level_db: Some(-12.0),
            ..RunOptions::default()
        });
        assert_eq!(settings(&RunOptions::default()), levelled);
    }
}
