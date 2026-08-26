//! The DSP core of `hyperglare`, free of any audio host.
//!
//! A resonator bank tuned to a chord, plus what it takes to decide where the
//! resonators go and how they are excited.
//!
//! Everything on the per-sample path holds itself to the audio callback's
//! prohibitions (`docs/effectkit/realtime.md`), because that is where it runs.

pub mod grid;

#[cfg(test)]
mod proofs {
    //! Link-time proofs that the paths reachable from an audio callback cannot
    //! panic (`docs/effectkit/realtime.md`).
    //!
    //! `#[no_panic]` only holds under full optimisation, so these are checked
    //! by `cargo test --release` and are inert in a debug build.

    use crate::grid::{Geometry, Grid};

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
            for note_hz in [60.0, 0.0, -1.0, f32::NAN, f32::INFINITY] {
                assert!(run(&grid, note_hz, &mut out) <= out.len());
            }
        }
    }
}
