//! oxtt under a JACK server.
//!
//! The DSP itself is not here. `OttProcessor` and its parameters are
//! [`oxtt_dsp`], which depends on no audio API at all — that is what lets it
//! be tested without an audio system, and what made the second host an adapter
//! rather than a port (ADR 0007, docs/architecture.md). The Bela host is
//! `oxtt-bela` and the offline renderer is `oxtt-render`; they are separate
//! packages because they link different audio systems that exist on different
//! machines.
//!
//! [`control`] holds this host's end of the physical control surface: the
//! thread that polls it and the lock-free handoff into the audio callback.
//! Which pot does what is [`oxtt_controls`], and the surface itself — the
//! hardware read and the conditioning — is `effectkit-controls`.

pub mod cli;
pub mod control;
pub mod jack_host;
