//! oxtt's hosts: the audio systems the DSP runs under, and the offline
//! renderer.
//!
//! The DSP itself is not here. `OttProcessor` and its parameters are
//! [`oxtt_dsp`], which depends on no audio API at all — that is what lets it
//! be tested without an audio system, and what made the second host an
//! adapter rather than a port (ADR 0007, docs/architecture.md).
//!
//! Two hosts, one per feature: `jack-host` (on by default) runs the DSP under
//! a JACK server, and `bela-host` runs it under Bela's `render` callback on a
//! Bela Gem Stereo (ADR 0011). [`render`] needs neither and builds always.
//!
//! [`control`] holds oxtt's end of the physical control surface: which pot
//! does what, and the JACK-only thread that carries the result into the audio
//! callback. The surface itself — the hardware read and the conditioning — is
//! `effectkit-controls`.

#[cfg(feature = "bela-host")]
pub mod bela_host;
pub mod cli;
pub mod control;
#[cfg(feature = "jack-host")]
pub mod jack_host;
pub mod render;
