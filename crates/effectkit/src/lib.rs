//! Effect-independent building blocks for real-time audio effects.
//!
//! Everything here holds itself to the audio callback's prohibitions — no
//! allocation, no locks, no I/O, no panic — because that is where it runs.
//! Several items carry `#[no_panic]` proofs that are checked at link time by
//! `cargo test --release`.
//!
//! - [`smooth`] — sample-rate-independent parameter smoothing, linear and
//!   logarithmic.
//! - [`filter`] — RBJ cookbook coefficients, a Direct Form I biquad, and the
//!   cascaded pair that makes a 4th-order Linkwitz-Riley section.
//! - [`decibels`] — dB conversions for the real-time path, and the floor they
//!   respect.
//! - [`metering`] — what arrived at the input, and an indicator hold that
//!   makes a single clipped frame visible.
//!
//! # The `effectkit` family
//!
//! | Crate | What it holds |
//! |---|---|
//! | `effectkit` | this crate: the DSP primitives |
//! | `effectkit-controls` | a physical control surface: six pots and a latching bypass switch, conditioned |
//! | `effectkit-controls-pi` | the Raspberry Pi's reading of that surface, over SPI and GPIO |
//! | `effectkit-pi-tools` | the wiring-verification and calibration tools those measurements come from |
//!
//! Only this crate is published. The control-surface crates fix the contract
//! of one particular panel — a 10-bit pot scale, six channels, one latching
//! switch — and a second panel has not existed yet to say which of those are
//! general.
//!
//! # Not a host framework
//!
//! There is deliberately no audio-system abstraction here. Two hosts exist
//! (JACK and Bela) and they differ in more than they share, so the trait that
//! would unify them is not yet worth guessing at.

pub mod decibels;
pub mod filter;
pub mod metering;
pub mod smooth;
