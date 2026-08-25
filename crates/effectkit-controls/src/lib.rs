//! A physical control surface: six potentiometers and a latching bypass switch.
//!
//! Two layers live here, and what each is for is the reason they are apart:
//!
//! | Layer | Responsibility | Scope |
//! |---|---|---|
//! | A: raw read | produce a [`RawControls`] value | platform-specific — [`gem`] for a Bela Gem, `effectkit-controls-pi` for a Raspberry Pi's SPI/GPIO |
//! | B1: conditioning | jitter filter, deadband, switch debounce, normalisation onto [`PotTravel`] → [`ConditionedControls`] | shared by every effect |
//!
//! The layer above — deciding what each pot *does* — is the effect's, and
//! deliberately not here. `oxtt-controls` is one; a second effect on the same
//! breadboard writes its own and reuses everything in this crate unchanged.
//!
//! # The seam between A and B1 is a value, not a trait
//!
//! [`RawControls`] is what crosses it. [`ControlSource`] exists as well, but it
//! is the *Raspberry Pi's* shape for producing one — hardware owned by the
//! implementation, polled from a thread, able to fail — and the Bela host
//! matches none of those, so [`gem`] builds a `RawControls` directly out of the
//! samples its callback is handed (ADR 0010).
//!
//! # Real-time
//!
//! [`SixPotBypassConditioner`] is pure: no I/O, no threads, no clock, no
//! allocation and no panic. That is what lets a Bela drive it from inside the
//! audio callback with no transport layer in between (ADR 0011,
//! `docs/contracts.md` §6). Layer A implementations are not held to that —
//! `ControlSource::read` is free to block, because on the platform that has
//! one it is polled from a thread of its own.

mod conditioning;
pub mod gem;
mod raw;
pub mod surfaces;

pub use conditioning::{
    ConditionedControls, ConditioningConfig, DeadbandCounts, DebounceReads, FilterCoefficient,
    PollHz, PotTravel, SixPotBypassConditioner,
};
pub use raw::{ControlSource, POT_POSITION_MAX, PotPosition, PotPositionError, Pots, RawControls};
