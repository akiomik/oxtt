//! Effect-independent building blocks for real-time audio effects.
//!
//! Everything here holds itself to the audio callback's prohibitions — no
//! allocation, no locks, no I/O, no panic — because that is where it runs.
//! The per-sample path carries `#[no_panic]` proofs, checked at link time by
//! `cargo test --release` (see the `proofs` test module).
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

#[cfg(test)]
mod proofs {
    //! Link-time proofs that the per-sample path cannot panic
    //! (docs/effectkit/realtime.md).
    //!
    //! `#[no_panic]` only holds under full optimisation, so these are checked
    //! by `cargo test --release` and are inert in a debug build. What the
    //! bodies compute does not matter; that they reach the functions does.
    //!
    //! They are here rather than left to a caller's proof on purpose. This is
    //! the crate a second effect depends on, and a proof that lives in the
    //! caller says nothing to the next caller.

    use crate::decibels::{db_to_amp, power_to_db};
    use crate::filter::{Biquad, Lr4, biquad_coeffs};
    use crate::metering::{ClipIndicator, InputMeter};
    use crate::smooth::{LogSmoothed, Smoothed};

    const SAMPLE_RATE: f32 = 48_000.0;

    #[test]
    fn a_biquad_cannot_panic_per_sample() {
        #[cfg_attr(all(test, not(debug_assertions)), no_panic::no_panic)]
        fn run(section: &mut Lr4, stage: &mut Biquad, x: f32) -> f32 {
            section.process(x) + stage.process(x)
        }

        let mut section = Lr4::default();
        section.set_cutoff(2_500.0, SAMPLE_RATE, false);
        let mut stage = Biquad::default();
        stage.set_coeffs(biquad_coeffs(150.0, SAMPLE_RATE, true));

        for x in [0.0, 0.5, -1.0, 1.0] {
            assert!(run(&mut section, &mut stage, x).is_finite());
        }
    }

    #[test]
    fn smoothing_cannot_panic_per_sample() {
        #[cfg_attr(all(test, not(debug_assertions)), no_panic::no_panic)]
        fn run(linear: &mut Smoothed, log: &mut LogSmoothed) -> f32 {
            linear.tick() + log.tick_hz()
        }

        let mut linear = Smoothed::new(0.0, SAMPLE_RATE);
        linear.set_target(1.0);
        let mut log = LogSmoothed::new(150.0, SAMPLE_RATE);
        log.set_target_hz(2_500.0);

        assert!(run(&mut linear, &mut log).is_finite());
    }

    #[test]
    fn the_decibel_conversions_cannot_panic() {
        #[cfg_attr(all(test, not(debug_assertions)), no_panic::no_panic)]
        fn run(db: f32, power: f32) -> f32 {
            db_to_amp(db) + power_to_db(power)
        }

        assert!(run(-6.0, 0.25).is_finite());
        assert!(run(f32::NAN, 0.0).is_nan());
    }

    #[test]
    fn metering_cannot_panic_per_frame() {
        #[cfg_attr(all(test, not(debug_assertions)), no_panic::no_panic)]
        fn run(meter: &mut InputMeter, indicator: &mut ClipIndicator, l: f32, r: f32) -> bool {
            meter.observe(l, r);
            indicator.update(meter.clipped_frames(), 1)
        }

        let mut meter = InputMeter::new();
        let mut indicator = ClipIndicator::new(64);
        assert!(!run(&mut meter, &mut indicator, 0.5, -0.5));
        assert!(run(&mut meter, &mut indicator, 2.0, 0.0));
    }
}
