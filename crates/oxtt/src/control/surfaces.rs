//! Measured conditioning settings for control surfaces that are not
//! [`ControlSource`](super::ControlSource) implementations.
//!
//! A [`ControlSource`](super::ControlSource) carries its own settings on the
//! trait, next to the code that reads the hardware. A layer A that is a set of
//! free functions instead — the Bela host's, which is handed its samples and
//! cannot fail (`src/bela_host/controls.rs`) — has no trait to put them on,
//! and gets a named constant here.
//!
//! **The rule for where a `ConditioningConfig` lives is "beside the layer A it
//! was measured on".** That is why this module, in a place that is otherwise
//! free of any particular board, names boards: it is a catalogue of measured
//! surfaces, so board names are exactly what belongs in it.

use super::conditioning::{
    ConditioningConfig, DeadbandCounts, DebounceReads, FilterCoefficient, PollHz,
};

/// The Bela Gem Stereo's control surface, six pots on `A0`-`A5` and a latching
/// switch on `D0` (`docs/bela/control-surface-setup.md`).
///
/// **`deadband_counts = 3.0`** — the Gem's converter is far quieter than the
/// Raspberry Pi's MCP3008 on the same pots: measured over 60 seconds at each
/// of full and mid travel, no channel's reading spanned more than **2.5
/// counts** end to end, against a raw σ of 6.39 on the Pi
/// (`docs/bela/control-surface-verification.md`).
///
/// Three counts is chosen against that measured span rather than against an
/// estimated σ, and it is the stronger statement of the two: the whole
/// excursion ever observed fits inside the band, so a motionless pot is
/// silent rather than merely quiet. [`ConditioningConfig`]'s rule —
/// `deadband >= σ` of the raw jitter — is satisfied many times over by the
/// same figure.
///
/// The Pi's eight counts would also have been silent here, which is exactly
/// why they are not used: it would spend the board's quieter converter on
/// nothing. Three counts is 0.29% of travel, roughly 341 distinct positions
/// across a sweep against the Pi's 128, and `3 / 1023 * 48` ≈ 0.141 dB on the
/// two gain pots — a quarter of the Pi's step and still far under what the
/// DSP's 20 ms smoothing lets through as an audible move (ADR 0012).
///
/// **`filter_coefficient = 0.2`, `debounce_reads = 15`, `nominal_poll_hz =
/// 500.0`** — the Raspberry Pi's figures, adopted rather than re-derived; see
/// `PiControls::CONDITIONING` for what each was measured or sized against.
/// The filter and the debounce are defined per read, so they carry over
/// exactly as long as the read rate does, which is what the nominal 500 Hz
/// here says: the Bela host divides its block rate down to land on or near it
/// rather than reading on every block (`bela_host::controls::PollDecimator`).
pub const GEM: ConditioningConfig = ConditioningConfig::new(
    FilterCoefficient::new_const(0.2),
    DeadbandCounts::new_const(3.0),
    DebounceReads::new_const(15),
    PollHz::new_const(500.0),
);
