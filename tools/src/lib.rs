//! Development and soak-test helpers for oxtt that talk to JACK directly.
//!
//! None of this depends on the `oxtt` DSP crate; the binaries under `src/bin`
//! only need the `jack` binding, and [`analysis`] is pure so it can be unit- and
//! property-tested without JACK or a real recording.

pub mod analysis;
