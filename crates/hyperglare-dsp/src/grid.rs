//! Where the resonators sit: the set of frequencies a chord turns into.
//!
//! This is the first thing `hyperglare` has to get right and the last thing
//! that can be settled on paper, so it is a choice the type makes explicit
//! rather than a formula buried in the bank.
//!
//! # Why the geometry is a choice and not a constant
//!
//! The obvious construction — stack a harmonic series on each note — does not
//! reach the band the effect is *for*. Colour bass is a bass effect, so the
//! fundamental is around 60 Hz, and the glare lives up around 8.5 kHz. That is
//! a ratio of about 142, and an unstretched harmonic series reaches it at
//! partial 142. Four voices of that is around 570 filters, not the 160 a
//! stretched series would need — see [`MAX_HARMONICS`] for why the stretch
//! that would buy the smaller number is out of range here.
//!
//! Replicating each note by octaves instead reaches the same band in seven
//! steps, because the steps are geometric rather than arithmetic. Four voices
//! is 28 filters, and the resulting set — every note of the chord, in every
//! octave — is closer to what the effect is actually named for than one bell's
//! overtones would be.
//!
//! # The coincidence problem
//!
//! An octave grid at zero detune is `f0, 2·f0, 4·f0, 8·f0, …`, which is a
//! *subset of the input's own harmonic series*. The chord comes from the same
//! instrument as the audio, so `f0` is the input's fundamental, and the
//! resonators are then emphasising energy that is already there. At zero
//! detune, on a single note, this is an octave-emphasis equaliser.
//!
//! What breaks the coincidence — and therefore what makes the effect — is:
//!
//! 1. **Detune** ([`Grid::detune_cents_per_octave`]), which walks the grid off
//!    the harmonic series a little more with every octave.
//! 2. **The chord's other notes.** A third or a fifth is not in the root's
//!    harmonic series, so those partials have nothing to emphasise and must be
//!    excited by something aperiodic.
//!
//! Neither is free, and the second is why the bank needs a noise path at all.
//!
//! # It is not a binary
//!
//! Whether a grid point counts as "coincident" depends on frequency, because a
//! resonator's bandwidth does not: at a fixed decay the width is the same
//! number of hertz everywhere, while a mistuning in cents is a *proportion* of
//! the frequency. Two cents is 0.35 Hz at 300 Hz and 5.8 Hz at 5 kHz. So the
//! high octaves fall off their own harmonics from ordinary tuning drift alone,
//! and are excited by the aperiodic path whether the chord asks for it or not.

use std::f32::consts::LOG2_E;
use std::slice::IterMut;

/// Lowest frequency a grid point is generated at.
///
/// About C2, and about where a resonance stops adding a pitch to a bass line
/// and starts competing with its fundamental. It is also the bottom of the
/// range the closest published reference uses. A starting value, not a
/// measurement.
pub const DEFAULT_LOW_HZ: f32 = 65.0;

/// Highest frequency a grid point is generated at, before Nyquist.
///
/// The top of the same reference's range, and about where "bright" stops being
/// distinguishable from "hissy" on the material this is for.
pub const DEFAULT_HIGH_HZ: f32 = 9_000.0;

/// Lowest octave step generated, relative to the played note.
///
/// Negative because the grid runs *down* from the note as well as up: see
/// [`Grid::frequencies`] for why that is what keeps the point count stable.
pub const MIN_OCTAVE_STEP: i32 = -9;

/// Highest octave step generated, relative to the played note.
///
/// `DEFAULT_LOW_HZ` to `DEFAULT_HIGH_HZ` is `log2(9000/65) = 7.11` octaves, so
/// nine steps either way covers the band from any note in it; the rest is
/// clipped by the band.
pub const MAX_OCTAVE_STEP: i32 = 9;

/// How a note's frequency is replicated into a set of resonator frequencies.
///
/// The three shapes exist to be compared by ear, which is the only way this
/// gets decided. They are listed cheapest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Geometry {
    /// `f0 · 2^(j·s)`, `j` over both signs. Seven steps cover the band.
    ///
    /// The set is every note of the chord in every octave, which is what a
    /// scale-following equaliser produces and what the effect is named after.
    #[default]
    Octaves,
    /// `f0 · 2^(j·s)`, each step doubled into a pair a few cents apart.
    ///
    /// Twice the filters for the same span. It exists because a single note
    /// through [`Octaves`](Self::Octaves) is one resonator per octave, which
    /// may simply be too sparse to sound like anything, and because two
    /// resonators a few cents apart beat against each other on every attack —
    /// the only motion the bank has otherwise.
    OctavePairs,
    /// `f0 · k^s`, `k = 1, 2, 3, …`. One bell's overtones per note.
    ///
    /// Reaches the top of the band only at about 150 partials per voice
    /// ([`MAX_HARMONICS`]), so it is by far the expensive one; kept because
    /// "sparse but wide" and "dense but low" are different sounds and the
    /// cheap one is not automatically right.
    Harmonics,
}

/// The set of frequencies one chord turns into, before Nyquist and the band
/// limits are applied.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grid {
    /// Which construction to replicate a note with.
    pub geometry: Geometry,
    /// How far the grid walks off the harmonic series, per octave.
    ///
    /// Zero is an exact octave (or an exact harmonic, under
    /// [`Geometry::Harmonics`]). The unit is cents *per octave* rather than a
    /// raw exponent so that the knob means the same thing however many octaves
    /// are being generated: widening the band must not change what a given
    /// position sounds like.
    pub detune_cents_per_octave: f32,
    /// Cents between the two members of a pair, under
    /// [`Geometry::OctavePairs`]. Ignored otherwise.
    pub pair_spread_cents: f32,
    /// Bottom of the band grid points are generated in.
    pub low_hz: f32,
    /// Top of the band grid points are generated in.
    pub high_hz: f32,
}

impl Default for Grid {
    fn default() -> Self {
        Self {
            geometry: Geometry::default(),
            detune_cents_per_octave: 0.0,
            pair_spread_cents: 7.0,
            low_hz: DEFAULT_LOW_HZ,
            high_hz: DEFAULT_HIGH_HZ,
        }
    }
}

/// Ratio for a displacement in cents. `2^(cents/1200)`, without a `powf`.
#[inline]
fn cents_ratio(cents: f32) -> f32 {
    (cents / 1200.0 / LOG2_E).exp()
}

impl Grid {
    /// The frequencies one note contributes, written into `out`.
    ///
    /// Returns how many were written. Points outside `[low_hz, high_hz]` and
    /// at or above `nyquist_hz` are skipped rather than clamped: a clamped
    /// point would pile several resonators onto one frequency, which is both
    /// wasted work and a peak nobody asked for.
    ///
    /// **The steps run in both directions from the note.** A grid that only
    /// went up would put fewer points on a high note than a low one, and the
    /// count, the CPU cost and the loudness would all then depend on which key
    /// was pressed. Running both ways and letting the band do the limiting is
    /// what makes those three constant.
    pub fn frequencies(&self, note_hz: f32, nyquist_hz: f32, out: &mut [f32]) -> usize {
        let high = self.high_hz.min(nyquist_hz);
        // Written as positive tests rather than negated ones so that a NaN
        // takes the early return instead of falling through a comparison that
        // is false for both directions.
        if !note_hz.is_finite() || note_hz <= 0.0 || high <= self.low_hz {
            return 0;
        }
        let mut sink = Sink::new(out);
        match self.geometry {
            Geometry::Octaves | Geometry::OctavePairs => {
                let paired = self.geometry == Geometry::OctavePairs;
                let step = cents_ratio(1200.0 + self.detune_cents_per_octave);
                let spread = cents_ratio(self.pair_spread_cents);
                // `j` runs over both signs; the band decides where it stops.
                for j in MIN_OCTAVE_STEP..=MAX_OCTAVE_STEP {
                    let f = note_hz * step.powi(j);
                    if f < self.low_hz || f >= high {
                        continue;
                    }
                    if !sink.push(f) {
                        break;
                    }
                    if paired {
                        let paired_hz = f * spread;
                        if paired_hz < high && !sink.push(paired_hz) {
                            break;
                        }
                    }
                }
            }
            Geometry::Harmonics => {
                // `k^s`, with `s` carried in the same per-octave unit the
                // octave geometries use, so the knob keeps its meaning across
                // them: one octave is a doubling of `k`.
                let exponent = 1.0 + self.detune_cents_per_octave / 1200.0;
                for k in 1..=MAX_HARMONICS {
                    let f = note_hz * (exponent * harmonic_ln(k)).exp();
                    if f >= high {
                        break;
                    }
                    if f >= self.low_hz && !sink.push(f) {
                        break;
                    }
                }
            }
        }
        sink.written()
    }
}

/// Fills a caller's buffer without indexing it, and counts what it wrote.
///
/// The buffer is a bound rather than a hint: a grid that would overrun it is
/// truncated. Written against an iterator so that neither the bound nor the
/// count can be got wrong by arithmetic, which is what keeps this callable
/// from an audio callback.
struct Sink<'a> {
    slots: IterMut<'a, f32>,
    written: usize,
}

impl<'a> Sink<'a> {
    fn new(out: &'a mut [f32]) -> Self {
        Self {
            slots: out.iter_mut(),
            written: 0,
        }
    }

    /// Writes one frequency. Returns `false` once the buffer is full.
    fn push(&mut self, hz: f32) -> bool {
        match self.slots.next() {
            Some(slot) => {
                *slot = hz;
                self.written = self.written.saturating_add(1);
                true
            }
            None => false,
        }
    }

    const fn written(&self) -> usize {
        self.written
    }
}

/// `ln(k)` for a partial number, without a `usize`-to-`f32` cast at each site.
#[inline]
fn harmonic_ln(k: usize) -> f32 {
    // `MAX_HARMONICS` is far inside `u16`, which converts to `f32` exactly.
    f32::from(u16::try_from(k).unwrap_or(u16::MAX)).ln()
}

/// Ceiling on partials per voice under [`Geometry::Harmonics`].
///
/// **Reaching 9 kHz from a 60 Hz fundamental takes 150 partials**, not the
/// forty a stretched series would need. Forty is the figure for an exponent of
/// 1.35, which in this type's unit is 420 cents per octave — four times the
/// range [`Grid::detune_cents_per_octave`] offers, because that range was
/// chosen for octave steps, where a hundred cents per octave already reaches
/// half an octave by the top of the band.
///
/// That mismatch is the geometry's own problem, not the unit's: cents per
/// octave is the natural unit for a grid whose step *is* an octave, and an
/// unnatural one for a grid whose steps are `1, 2, 3, …`. It is left visible
/// rather than papered over with a second unit, because the comparison this
/// enum exists for is exactly what decides whether the harmonic geometry
/// survives at all — and 150 partials a voice is a large part of that answer.
pub const MAX_HARMONICS: usize = 160;

#[cfg(test)]
// Test-only arithmetic over grid indices and frequency ratios: the counts are
// small, the values are positive by construction, and readability beats
// `mul_add` when the point of the line is to restate a published figure.
#[allow(
    clippy::cast_precision_loss,
    clippy::arithmetic_side_effects,
    clippy::suboptimal_flops,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]
mod tests {
    use super::*;

    const NYQUIST: f32 = 24_000.0;
    /// A bass fundamental: what colour bass is applied to.
    const BASS_HZ: f32 = 60.0;

    fn collect(grid: &Grid, note_hz: f32) -> Vec<f32> {
        let mut buf = [0.0f32; 256];
        let n = grid.frequencies(note_hz, NYQUIST, &mut buf);
        buf[..n].to_vec()
    }

    /// The count that made the octave geometry the default: seven or eight
    /// points per voice covers 65 Hz to 9 kHz, against about forty for a
    /// harmonic series reaching the same top.
    #[test]
    fn an_octave_grid_spans_the_band_in_single_digits() {
        let f = collect(&Grid::default(), BASS_HZ);
        assert!(
            (7..=8).contains(&f.len()),
            "expected the band to take 7-8 octave steps, got {}: {f:?}",
            f.len()
        );
        assert!(*f.first().unwrap() >= DEFAULT_LOW_HZ);
        assert!(*f.last().unwrap() < DEFAULT_HIGH_HZ);
        // The top point is within an octave of the ceiling: the band is
        // covered, not merely entered.
        assert!(*f.last().unwrap() > DEFAULT_HIGH_HZ / 2.0);
    }

    /// The arithmetic that disqualified a harmonic series as the default: it
    /// needs about forty partials to reach the same ceiling, which is where
    /// "four voices, 160 filters" comes from.
    #[test]
    fn a_harmonic_grid_needs_dozens_of_partials_for_the_same_band() {
        let grid = Grid {
            geometry: Geometry::Harmonics,
            ..Grid::default()
        };
        let f = collect(&grid, BASS_HZ);
        assert!(
            f.len() > 100,
            "an unstretched harmonic series needs about 150 partials to cover \
             the band, got {}",
            f.len()
        );
        assert!(*f.last().unwrap() > DEFAULT_HIGH_HZ / 2.0);
        // Twenty times the octave grid's count, for the same band. This is the
        // number the geometry has to justify by sounding better.
        assert!(f.len() > 15 * collect(&Grid::default(), BASS_HZ).len());
    }

    /// Both directions from the note, so the count does not depend on which
    /// key was pressed. This is what makes the CPU cost and the loudness
    /// constant across the keyboard.
    #[test]
    fn the_count_is_stable_across_octaves_of_the_played_note() {
        let grid = Grid::default();
        let counts: Vec<usize> = [32.7, 65.4, 130.8, 261.6, 523.3]
            .into_iter()
            .map(|hz| collect(&grid, hz).len())
            .collect();
        let (min, max) = (*counts.iter().min().unwrap(), *counts.iter().max().unwrap());
        assert!(
            max - min <= 1,
            "count should not swing with the played octave, got {counts:?}"
        );
        // A note well above the floor only stays covered because `j` goes
        // negative: C4 with upward-only steps would start at 261 Hz.
        let c4 = collect(&grid, 261.6);
        assert!(
            *c4.first().unwrap() < 100.0,
            "the grid should extend below the played note, got {c4:?}"
        );
    }

    /// The detune's unit is per octave, so widening the band must not change
    /// what a given knob position does to the octaves already in it.
    #[test]
    fn detune_is_per_octave_and_independent_of_the_bands_width() {
        let narrow = Grid {
            detune_cents_per_octave: 50.0,
            high_hz: 2_000.0,
            ..Grid::default()
        };
        let wide = Grid {
            high_hz: DEFAULT_HIGH_HZ,
            ..narrow
        };
        let (a, b) = (collect(&narrow, BASS_HZ), collect(&wide, BASS_HZ));
        assert!(a.len() < b.len(), "the wide band should have more points");
        for (x, y) in a.iter().zip(b.iter()) {
            assert!(
                (x - y).abs() < 0.01,
                "shared points should be identical: {x} vs {y}"
            );
        }
    }

    /// A hundred cents per octave is six semitones by the seventh step: the
    /// figure that fixed the knob's range, and the reason the raw exponent was
    /// a bad unit — at `s = 1.05` the top is already 3.6 semitones out.
    #[test]
    fn a_hundred_cents_per_octave_reaches_half_an_octave_at_the_top() {
        let grid = Grid {
            detune_cents_per_octave: 100.0,
            ..Grid::default()
        };
        let plain = collect(&Grid::default(), BASS_HZ);
        let detuned = collect(&grid, BASS_HZ);
        // A wider step reaches the ceiling sooner, so the detuned grid has
        // *fewer* points; compare at the last index both of them have.
        let last = plain.len().min(detuned.len()) - 1;
        let steps_up = (last + 1) as f32;
        let semitones = 12.0 * (detuned[last] / plain[last]).log2();
        assert!(
            (semitones - steps_up).abs() < 0.05,
            "a hundred cents per octave should be {steps_up} semitones out \
             after {steps_up} octaves, got {semitones}"
        );
        // Six semitones by the top of the band is the figure the knob's range
        // was chosen for.
        assert!(semitones >= 5.9, "top of the band is only {semitones} out");
    }

    /// Zero detune puts the grid on the input's own harmonics — the
    /// coincidence the effect has to be steered away from.
    #[test]
    fn zero_detune_lands_on_the_inputs_own_harmonics() {
        for f in collect(&Grid::default(), BASS_HZ) {
            let harmonic = f / BASS_HZ;
            assert!(
                (harmonic - harmonic.round()).abs() < 1e-3,
                "{f} Hz is harmonic {harmonic} of the fundamental, not an integer"
            );
        }
    }

    /// Pairs double the density for the same span, which is the whole trade.
    #[test]
    fn pairs_double_the_points_without_widening_the_band() {
        let plain = collect(&Grid::default(), BASS_HZ);
        let paired = collect(
            &Grid {
                geometry: Geometry::OctavePairs,
                ..Grid::default()
            },
            BASS_HZ,
        );
        assert!(paired.len() >= 2 * plain.len() - 1);
        assert!(*paired.last().unwrap() < DEFAULT_HIGH_HZ);
        let spread = 12.0 * (paired[1] / paired[0]).log2() * 100.0;
        assert!(
            (spread - 7.0).abs() < 0.1,
            "a pair should sit at the configured spread, got {spread} cents"
        );
    }

    /// Nothing above Nyquist, whatever the geometry or the band setting.
    #[test]
    fn nothing_is_generated_at_or_above_nyquist() {
        for geometry in [
            Geometry::Octaves,
            Geometry::OctavePairs,
            Geometry::Harmonics,
        ] {
            let grid = Grid {
                geometry,
                high_hz: 100_000.0,
                ..Grid::default()
            };
            let mut buf = [0.0f32; 256];
            let n = grid.frequencies(BASS_HZ, 8_000.0, &mut buf);
            assert!(n > 0, "{geometry:?} produced nothing");
            for f in &buf[..n] {
                assert!(*f < 8_000.0, "{geometry:?} produced {f} above Nyquist");
            }
        }
    }

    /// A degenerate note or band returns nothing rather than misbehaving.
    #[test]
    fn degenerate_inputs_produce_an_empty_grid() {
        let mut buf = [0.0f32; 64];
        let grid = Grid::default();
        assert_eq!(grid.frequencies(0.0, NYQUIST, &mut buf), 0);
        assert_eq!(grid.frequencies(-100.0, NYQUIST, &mut buf), 0);
        assert_eq!(grid.frequencies(f32::NAN, NYQUIST, &mut buf), 0);
        assert_eq!(grid.frequencies(BASS_HZ, 10.0, &mut buf), 0);
    }

    /// The output buffer is a bound, not a hint.
    #[test]
    fn a_short_buffer_truncates_instead_of_overflowing() {
        let mut buf = [0.0f32; 3];
        let n = Grid::default().frequencies(BASS_HZ, NYQUIST, &mut buf);
        assert_eq!(n, 3);
    }
}
