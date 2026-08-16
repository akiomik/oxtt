//! Bela audio-system setup and lifecycle (docs/architecture.md,
//! docs/contracts.md §6, §9; ADR 0011).
//!
//! The counterpart of [`jack_host`](crate::jack_host): it builds the settings,
//! brings the audio system up, sets the codec levels, waits, and reports. The
//! processing itself is in [`app`], which is deliberately free of libbela so
//! that it compiles and tests on a development machine — only [`run`] needs a
//! board, and it is the only thing here behind `cfg(bela_device)`.

pub mod app;
pub mod controls;

use core::num::NonZeroU32;

use thiserror::Error;

use crate::params::ConfigError;

pub use app::{OttApplication, OttRenderState, RunDiagnostics};
pub use controls::{
    ANALOG_CHANNELS_USED, DEADBAND_COUNTS, PollDecimator, TARGET_POLL_HZ, pot_position,
    raw_controls,
};

/// Audio sample rate oxtt asks a Bela for.
///
/// Not the board's 44.1 kHz default, for two reasons. It matches the rate the
/// Raspberry Pi host was measured at, so the two platforms are comparable
/// (ADR 0008). And it makes the control-surface read divisor exact: 48000/16
/// is 3000 blocks a second, which divides by 6 to precisely
/// [`TARGET_POLL_HZ`], so the mapping layer's constants keep the timings they
/// were calibrated with (see [`PollDecimator`]).
///
/// A Gem Stereo was measured running every rate from 8 kHz to 106 kHz, with
/// 108 kHz and above aborting the process from inside the codec (bela-rs
/// `docs/board-facts.md`), so this sits well inside what the hardware does.
pub const SAMPLE_RATE_HZ: NonZeroU32 = NonZeroU32::new(48_000).expect("48000 is not zero");

/// Audio frames per block oxtt asks for, which is also the board's default.
///
/// The smallest block the board supports is 2 frames and periods of 1 and 3
/// fail inside the PRU with eight analog inputs configured, so 16 is both the
/// default and the smallest size worth asking for. It is 0.33 ms at
/// [`SAMPLE_RATE_HZ`].
pub const PERIOD_SIZE: NonZeroU32 = NonZeroU32::new(16).expect("16 is not zero");

/// Analog input channels oxtt asks for: the board's default of eight.
///
/// Six are the control surface (`A0`-`A5`) and two are spare. The count is
/// left at the default deliberately — a Gem Stereo has no analog *outputs*,
/// and asking for a different number of inputs than outputs fails
/// `Bela_initAudio` outright (bela-rs `docs/board-facts.md`), so the safe
/// configuration is the one that was measured.
pub const ANALOG_IN_CHANNELS: u32 = 8;

/// Errors that can occur bringing up or running under a Bela audio system.
#[derive(Debug, Error)]
pub enum HostError {
    /// The audio system could not be built, started, or run.
    #[error("Bela error: {0}")]
    Bela(#[from] bela::Error),
    /// The supplied parameters failed validation.
    #[error("invalid parameters: {0}")]
    Config(#[from] ConfigError),
}

/// How a run was configured, for the caller to choose and the host to apply.
///
/// Every field has a command-line flag on `oxtt-bela` (`src/bin/oxtt-bela.rs`)
/// rather than being reached through Bela's own argument parser: oxtt does not
/// pass a command line on to [`bela::Bela`], so that there is one `--help` and
/// so that libbela's own options — several of which abort the process on bad
/// input — are never exposed to a user of this binary.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RunOptions {
    /// Audio frames per block.
    pub period_size: NonZeroU32,
    /// Audio sample rate to ask the board for.
    pub sample_rate: NonZeroU32,
    /// Drive the six pots and the bypass switch from the physical surface.
    pub controls: bool,
    /// Ask libbela for CPU measurements, so the run can report its load.
    pub cpu_monitoring: Option<NonZeroU32>,
    /// Analog input gain, in dB, applied to the codec before the DSP.
    ///
    /// `None` leaves the board's default of +16 dB, **which clips a
    /// line-level source and has to be set** (docs/bela/noise-floor.md).
    /// [`RunDiagnostics::input`] is what makes the ceiling findable: nothing
    /// on this board reports clipping otherwise.
    ///
    /// **Set it as high as the source allows without clipping**, and check
    /// whether the last few decibels bought anything: analog gain stops
    /// paying once the noise ahead of it dominates, which measured at +6 dB
    /// with an Elektron Syntakt. Whether that point belongs to the board's
    /// input stage or to the source is not established — the two look
    /// identical from here — so it is worth finding per source rather than
    /// assuming (docs/bela/noise-floor.md).
    ///
    /// The clipping ceiling is the source's, and moves a long way with what
    /// the source plays: a metered +6 dB for one note against roughly -12 dB
    /// inferred for a pattern, on the same instrument. The lower bound is not
    /// the source's: below -12 dB the codec stops responding altogether
    /// ([bela-rs#124](https://github.com/akiomik/bela-rs/issues/124)).
    pub adc_gain_db: Option<f32>,
    /// Headphone output level, in dB, applied to the codec after the DSP.
    ///
    /// On a Gem Stereo this is what sets the line output's level: measured,
    /// it moves the output one for one while libbela's line out level moves
    /// it 0.00 dB over 24 dB of request
    /// ([bela-rs#123](https://github.com/akiomik/bela-rs/issues/123)).
    ///
    /// `None` leaves libbela's default of -6 dB, and the range runs to +9 dB.
    ///
    /// Set it for the level the next device wants. Trading it against
    /// `output_gain` the way `adc_gain_db` is traded against `input_gain`
    /// does *not* work: the effect's own amplified noise follows the two in
    /// opposite directions and returns to where it started, which measures as
    /// 4.8 dB against the output stage alone and 0.5 dB — nothing — against a
    /// usable preset's hiss (docs/bela/noise-floor.md).
    pub headphone_level_db: Option<f32>,
    /// Digital channel an LED is wired to, lit while the input clips.
    ///
    /// `None` on a board with nothing wired to its digital pins, which is
    /// every run that is not a pedal.
    pub clip_led: Option<usize>,
    /// Print [`RunDiagnostics`] after a normal exit.
    pub report_on_exit: bool,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            period_size: PERIOD_SIZE,
            sample_rate: SAMPLE_RATE_HZ,
            controls: false,
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
///
/// What is deliberately *not* set: `num_analog_out_channels`. A Gem Stereo
/// has none, and a request whose input and output counts disagree fails
/// `Bela_initAudio` (bela-rs `docs/board-facts.md`), so the defaults are left
/// exactly as measured.
#[must_use]
pub const fn settings(options: &RunOptions) -> bela::Settings {
    let settings = bela::Settings::new()
        .period_size(options.period_size.get())
        // Deliberately absent: `num_analog_out_channels`. Pinned by
        // `the_settings_leave_the_analog_output_count_alone` below.
        .audio_sample_rate(options.sample_rate)
        .use_analog(true)
        .num_analog_in_channels(ANALOG_IN_CHANNELS)
        .use_digital(true)
        // One thread because the DSP's filters carry state across frames and
        // Bela divides a block by frame range; `OttApplication::validate_settings`
        // refuses anything else (ADR 0011).
        .thread_count(NonZeroU32::MIN)
        // So that `underrun_count` in the run's diagnostics means something.
        .detect_underruns(true);

    // Off unless asked for: CPU monitoring refuses a period above
    // `MAX_MONITORED_PERIOD_SIZE`, and failing to start over a diagnostic
    // nobody asked for would be the wrong trade.
    match options.cpu_monitoring {
        Some(measurements) => settings.cpu_monitoring(measurements),
        None => settings,
    }
}

#[cfg(bela_device)]
mod device {
    use bela::{Bela, Channel};

    use super::{HostError, OttApplication, RunOptions, settings};
    use crate::dsp::OttProcessor;
    use crate::params::OttParams;

    /// Brings up the audio system, runs until stopped, and reports.
    ///
    /// The processor is built here, before any audio system exists, so that
    /// an invalid parameter set is reported as itself rather than as a failed
    /// initialisation. Everything the *settings* can be wrong about is
    /// refused by `OttApplication::validate_settings`, which libbela's
    /// wrapper calls before `Bela_initAudio` — so a refusal leaves the
    /// process able to do something else, which refusing from `setup` would
    /// not (bela-rs#112).
    ///
    /// Returns once `SIGINT`, `SIGTERM`, `SIGHUP`, the board's stop button or
    /// `bela::request_stop` has ended the run.
    ///
    /// The run's diagnostics are reported by `OttApplication::cleanup` rather
    /// than returned here: `until_stopped` consumes the audio system and does
    /// not hand the application back, so the counters do not outlive it.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Config`] if the parameters are invalid at the
    /// configured sample rate, and [`HostError::Bela`] if the audio system
    /// cannot be built, refuses the settings, or reports callback faults.
    pub fn run(params: OttParams, options: &RunOptions) -> Result<(), HostError> {
        #[expect(
            clippy::cast_precision_loss,
            reason = "a sample rate is far below f32's exact-integer limit"
        )]
        let sample_rate = options.sample_rate.get() as f32;
        let processor = OttProcessor::new(sample_rate, params)?;
        let application = OttApplication::new(
            processor,
            params,
            options.controls,
            options.clip_led,
            options.report_on_exit,
        );

        let mut bela = Bela::new(application, &settings(options))?;

        // Between construction and starting, which is where the codec levels
        // are applied: the hardware ends up in the same state either way, and
        // doing it here keeps a level failure from happening mid-run.
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

    /// The reason 48 kHz is not the board's 44.1 kHz default: at the default
    /// period it divides exactly onto the rate the mapping layer's constants
    /// were calibrated for (ADR 0011).
    #[test]
    fn the_default_configuration_divides_onto_the_calibrated_poll_rate() {
        let options = RunOptions::default();
        #[expect(
            clippy::cast_precision_loss,
            reason = "a sample rate is far below f32's exact-integer limit"
        )]
        let sample_rate = options.sample_rate.get() as f32;
        let frames = options.period_size.get() as usize;
        let decimator = PollDecimator::for_block_rate(sample_rate, frames);
        #[expect(
            clippy::float_cmp,
            reason = "the point of the test is that it is exact"
        )]
        {
            assert_eq!(decimator.effective_hz(sample_rate, frames), TARGET_POLL_HZ);
        }
    }

    /// `Settings` has no getters, so this pins the whole configuration by
    /// comparing it against the chain written out by hand. That makes it a
    /// change detector on purpose: the settings are the part of this host
    /// that a board fails on rather than complains about — a Gem Stereo has
    /// no analog outputs, and asking for a different number of analog inputs
    /// than outputs fails `Bela_initAudio` outright and leaves the process
    /// unable to build another audio system — so a call appearing or
    /// disappearing here should have to be written down twice.
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

    #[test]
    fn cpu_monitoring_changes_the_settings_only_when_asked_for() {
        let plain = settings(&RunOptions::default());
        let monitored = settings(&RunOptions {
            cpu_monitoring: NonZeroU32::new(4),
            ..RunOptions::default()
        });
        assert_ne!(plain, monitored, "--report-cpu must reach the settings");
    }

    /// The codec levels are applied to the handle, not the settings, so
    /// asking for them must not change what libbela is initialised with.
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
