//! The DSP core of `hyperglare`, free of any audio host.
//!
//! A resonator bank tuned to a chord, plus what it takes to decide where the
//! resonators go and how they are excited.
//!
//! Everything on the per-sample path holds itself to the audio callback's
//! prohibitions (`docs/effectkit/realtime.md`), because that is where it runs.

pub mod bank;
pub mod exciter;
pub mod grid;
pub mod processor;

#[cfg(test)]
mod proofs {
    //! Link-time proofs that the paths reachable from an audio callback cannot
    //! panic (`docs/effectkit/realtime.md`).
    //!
    //! `#[no_panic]` only holds under full optimisation, so these are checked
    //! by `cargo test --release` and are inert in a debug build.

    use crate::bank::{BankParams, ResonatorBank};
    use crate::exciter::{Exciter, ExciterCoeffs, ExciterParams};
    use crate::grid::{Geometry, Grid};
    use crate::processor::{HyperglareParams, HyperglareProcessor, SearPlacement};

    /// The grid is rebuilt whenever a chord or a knob moves, which under Bela
    /// is inside `render_pre`. It is not a per-sample path, but it is on the
    /// callback, so it owes the same guarantee.
    #[test]
    fn building_a_grid_cannot_panic() {
        #[cfg_attr(all(test, not(debug_assertions)), no_panic::no_panic)]
        fn run(grid: &Grid, note_hz: f32, out: &mut [f32]) -> usize {
            grid.frequencies(note_hz, 24_000.0, out)
        }

        let mut out = [0.0f32; 8];
        for geometry in [
            Geometry::Octaves,
            Geometry::OctavePairs,
            Geometry::Harmonics,
        ] {
            let grid = Grid {
                geometry,
                detune_cents_per_octave: 100.0,
                ..Grid::default()
            };
            // Including the inputs that have to be rejected rather than
            // trusted: a silent voice, a negative frequency, and a NaN.
            // The return value is bounded by the buffer's own length, so
            // asserting that would assert nothing. What is proved here is the
            // absence of a panic on inputs that have to be rejected rather
            // than trusted.
            for note_hz in [60.0, 0.0, -1.0, f32::NAN, f32::INFINITY] {
                let _ = run(&grid, note_hz, &mut out);
            }
        }
    }

    /// The per-sample path: this runs inside the audio callback for every
    /// frame, so it owes the strongest form of the guarantee.
    #[test]
    fn the_banks_sample_path_cannot_panic() {
        #[cfg_attr(all(test, not(debug_assertions)), no_panic::no_panic)]
        fn run(bank: &mut ResonatorBank<32>, x: f32) -> f32 {
            bank.process(x)
        }

        let mut bank = ResonatorBank::<32>::new();
        // Before any retune, and then tuned to a chord with every knob at an
        // extreme.
        assert!(run(&mut bank, 0.5).is_finite());
        bank.retune(
            &[55.0, 82.4, 110.0],
            &BankParams {
                decay_t60_s: 8.0,
                q_max: 20_000.0,
                tilt: 1.0,
                drift_cents: 40.0,
                ..BankParams::default()
            },
            48_000.0,
        );
        // `f32::MAX` is included to prove the path survives it, not to claim
        // the state does: it will not. What is being proved here is the
        // absence of a panic, so the return values are deliberately unused.
        for x in [0.0, 1.0, -1.0, f32::MAX] {
            let _ = run(&mut bank, x);
        }
    }

    /// Retuning runs on the callback too, under Bela: a chord change arrives
    /// in `render_pre`.
    #[test]
    fn retuning_cannot_panic() {
        #[cfg_attr(all(test, not(debug_assertions)), no_panic::no_panic)]
        fn run(bank: &mut ResonatorBank<32>, notes: &[f32], params: &BankParams) {
            bank.retune(notes, params, 48_000.0);
        }

        let mut bank = ResonatorBank::<32>::new();
        for notes in [
            &[][..],
            &[60.0][..],
            &[0.0, -1.0, f32::NAN, f32::INFINITY][..],
            &[55.0, 65.4, 82.4, 110.0, 130.8, 164.8, 220.0][..],
        ] {
            run(&mut bank, notes, &BankParams::default());
        }
    }

    /// The exciter is the first thing every sample meets, so it is on the
    /// per-sample path with the bank.
    #[test]
    fn the_exciters_sample_path_cannot_panic() {
        #[cfg_attr(all(test, not(debug_assertions)), no_panic::no_panic)]
        fn run(exciter: &mut Exciter, params: ExciterParams, coeffs: ExciterCoeffs, x: f32) -> f32 {
            exciter.process(x, &params, &coeffs)
        }

        let coeffs = ExciterCoeffs::new(48_000.0);
        let mut exciter = Exciter::new();
        for params in [
            ExciterParams {
                drive: 0.0,
                noise_amount: 0.0,
            },
            ExciterParams {
                drive: 1.0,
                noise_amount: 1.0,
            },
            // Out of range on purpose: the shaper clamps rather than trusting.
            ExciterParams {
                drive: 9.0,
                noise_amount: -1.0,
            },
        ] {
            for x in [0.0, 0.5, -1.0, f32::MAX, f32::MIN, f32::NAN] {
                let _ = run(&mut exciter, params, coeffs, x);
            }
            exciter.reset();
        }
    }

    /// The whole chain, which is what a host actually calls per frame.
    #[test]
    fn the_processors_frame_path_cannot_panic() {
        #[cfg_attr(all(test, not(debug_assertions)), no_panic::no_panic)]
        fn run(processor: &mut HyperglareProcessor<64>, l: f32, r: f32) -> (f32, f32) {
            processor.process_frame(l, r)
        }

        for placement in [SearPlacement::AfterSum, SearPlacement::BeforeSplit] {
            let params = HyperglareParams {
                sear_placement: placement,
                sear: 1.0,
                width: 1.0,
                color: 2.0,
                input_gain_db: 24.0,
                output_gain_db: 24.0,
                ..HyperglareParams::default()
            };
            let mut processor = HyperglareProcessor::<64>::new(params, 48_000.0);
            processor.apply_params(&params, &[55.0, 82.4, 110.0]);
            for (l, r) in [
                (0.0, 0.0),
                (1.0, -1.0),
                (f32::MAX, f32::MIN),
                (f32::NAN, f32::INFINITY),
            ] {
                let (out_l, out_r) = run(&mut processor, l, r);
                assert!(out_l.is_finite() && out_r.is_finite());
            }
        }
    }
}
