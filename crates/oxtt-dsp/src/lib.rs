//! oxtt's DSP core: a 3-band upward/downward multiband compressor.
//!
//! No dependency on JACK, on libbela, or on any other host-audio API — it
//! operates on `f32` samples and nothing else, which is what lets it be tested
//! without an audio system and what made the second host an adapter rather
//! than a port (ADR 0007, docs/architecture.md).
//!
//! It knows nothing about a control surface either. [`OttProcessorUpdate`] is
//! the one atomic update [`OttProcessor`] accepts, and both the control
//! surface and the command line build one; assigning potentiometers to
//! parameters happens in `oxtt-controls`, on the far side of that type.
//!
//! The effect-independent primitives underneath — parameter smoothing, the
//! biquad and Linkwitz-Riley sections, the decibel conversions — come from
//! [`effectkit`].

pub mod bands;
pub mod dsp;
pub mod params;

pub use dsp::OttProcessor;
pub use params::{OttParams, OttProcessorUpdate};
