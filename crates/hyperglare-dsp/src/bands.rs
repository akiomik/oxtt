//! Which band a frequency belongs to, and how a signal is cut up to match.
//!
//! [ADR 0016](../../../docs/decisions/0016-the-bank-is-excited-per-band.md)
//! makes a resonator's excitation come from the input's energy near its own
//! frequency rather than from the input as a whole. That needs two things that
//! have to agree: a map from frequency to band, and a way of splitting a
//! signal along the same lines. Both live here so they cannot drift apart.
//!
//! # This is not a crossover
//!
//! Each band is its own pair of second-order Butterworth sections — a
//! high-pass at the edge below and a low-pass at the edge above — run
//! independently of the others. **The bands do not sum back to the input**,
//! and nothing here needs them to: they feed a gate and a waveshaper, and what
//! reaches the output is the resonators' own ringing rather than any
//! reconstruction of the source. `oxtt` has the opposite requirement and uses
//! `effectkit`'s Linkwitz-Riley crossover for it.
//!
//! # Why not the cheaper ladder
//!
//! A ladder of low-passes, each subtracted from the next, sums to the input
//! exactly by algebra and costs one multiply-add per edge. It was tried first
//! and does not work, for two reasons measured rather than reasoned about:
//!
//! - **The subtraction only behaves for one-pole stages.** With second-order
//!   Butterworth sections the phase rotation makes `x − L` exceed the input:
//!   a 760 Hz tone came out loudest in the band *above* the one it belongs to,
//!   because the top band had a gain over one there.
//! - **One-pole stages do not separate anything.** Six decibels an octave
//!   leaves the band from 130 to 260 Hz passing 60 Hz at 0.205 and 180 Hz at
//!   0.334 — four decibels apart. A kick would drive the band above it nearly
//!   as hard as the content that belongs there, which is the defect ADR 0016
//!   exists to remove.
//!
//! Twelve decibels an octave on both sides is what the prototype that ADR 0016
//! measured actually used, and this is that.

use effectkit::filter::{Biquad, biquad_coeffs};

/// Where one band ends and the next begins, in hertz.
///
/// **Octaves, and that is load-bearing rather than tidy.** The gain law
/// measures a resonator's share as `BW/W_b`, and above the breakpoint `BW`
/// grows with frequency; the share only stops changing — which is what makes
/// the compensation flat there — if `W_b` grows at the same rate. A band that
/// spans two octaves breaks that, and the resonators inside it are lifted
/// against their neighbours by `(W_b/W_ref)^p`.
///
/// So the edges reach as far as the grid does. They ran to 1040 while
/// `DEFAULT_HIGH_HZ` was 1.8 kHz; at 5 kHz that left a top band four times too
/// wide, and it showed up as a step in the middle of the band rather than as
/// anything subtle. **Moving the ceiling means checking these.**
///
/// Not further than the grid, either: an edge above the ceiling splits a
/// region no resonator is ever placed in, which is arithmetic whose result is
/// thrown away.
///
/// **Fixed rather than derived from the grid.** Deriving them would make
/// [`SplitCoeffs::new`] depend on [`BankParams`], and it depends on the sample
/// rate alone today — which is what keeps
/// `HyperglareProcessor::set_sample_rate` a statement about the rate. The
/// bottom and top bands are open-ended, so a caller who widens the grid gets a
/// wider band rather than a broken one.
///
/// # The top edge is one too many
///
/// **Known defect, measured and not yet fixed.** `4160.0` sits close enough to
/// the default ceiling of 5 kHz that the band above it is 840 Hz wide against
/// the 2080 below, and the gain law reads a narrower band as a smaller share:
///
/// ```text
///  edge      130     260     520    1040    2080    4160
///  step   +0.00   +1.51   +1.51   +1.51   +1.51   -1.97   dB
/// ```
///
/// Every edge steps up by `6.02·p` dB except the last, which steps down. With
/// `4160.0` dropped the top band spans 2080 Hz to the ceiling and the last
/// step is `+2.24` dB — still off the pattern, and in the direction the rest
/// of the ladder goes.
///
/// Left in place because it changes how the effect sounds and
/// [ADR 0020](../../../docs/decisions/0020-the-grids-ceiling-is-five-kilohertz.md)
/// names both edges; see
/// [ADR 0022](../../../docs/decisions/0022-the-band-ladder-stops-at-two-kilohertz.md).
///
/// [`BankParams`]: crate::bank::BankParams
pub const EDGES: [f32; 6] = [130.0, 260.0, 520.0, 1040.0, 2080.0, 4160.0];

/// How many bands [`EDGES`] cuts the spectrum into.
pub const BANDS: usize = EDGES.len() + 1;

/// The band a frequency belongs to.
///
/// Total over every `f32`. Zero, the negatives and `NaN` land in the bottom
/// band together — none of them is a frequency, and one answer for all of them
/// beats three.
#[must_use]
pub fn band_of(hz: f32) -> usize {
    // `NaN` compares false against every edge, which would otherwise put it at
    // the top. Nothing places a resonator at one, but a frequency that is not
    // a frequency should land where the other unusable ones do.
    if hz.is_nan() || hz <= 0.0 {
        return 0;
    }
    let mut band = 0;
    while band < EDGES.len() {
        let Some(edge) = EDGES.get(band) else { break };
        if hz < *edge {
            return band;
        }
        band = band.saturating_add(1);
    }
    band
}

/// How wide band `b` is, for the purpose of asking what share of it one
/// resonator collects.
///
/// Every band but the top has two edges and so has a width. **The top band is
/// open**, and running it to Nyquist would make the gain law depend on the
/// sample rate — a render at 96 kHz would not match one at 48. It is closed at
/// the grid's own ceiling instead, which is where resonators stop existing,
/// and which lives in `BankParams` so a change to it already retunes.
///
/// The bottom band is open too and is *not* clipped. Its lower edge is zero
/// and its width is the same number at every setting, which is what keeps
/// [`REFERENCE_BAND_HZ`] a fixed reference: the compensation has to be unity
/// somewhere that does not move, or sweeping the exponent sweeps the loudness
/// with it.
///
/// A ceiling below the top band's floor leaves that band empty, so the value
/// is never read; it still returns the floor rather than zero or a negative,
/// because a width divides.
#[must_use]
pub fn band_width_hz(band: usize, high_hz: f32) -> f32 {
    let lo = EDGES.get(band.wrapping_sub(1)).copied().unwrap_or(0.0);
    EDGES.get(band).map_or_else(
        || {
            let width = high_hz - lo;
            if width > 0.0 { width } else { lo.max(1.0) }
        },
        |hi| hi - lo,
    )
}

/// The band width [`crate::bank::BankParams::compensation_exponent`] is
/// measured against.
///
/// The bottom band's width, because
/// [`REFERENCE_NOTE_HZ`](crate::bank::REFERENCE_NOTE_HZ) is 60 Hz and falls in
/// it. Like the reference decay, this only decides *where* the compensation is
/// unity — the point at which sweeping the exponent does not also sweep the
/// loudness — so what is asked of it is that it be fixed and be somewhere a
/// bass note actually is. [`band_width_hz`] leaves the bottom band unclipped
/// so that this stays true of it.
pub const REFERENCE_BAND_HZ: f32 = EDGES[0];

/// The split's coefficients, derived once per sample-rate change.
///
/// Two `sin_cos` per edge, held off the sample path for the reason every other
/// coefficient in this crate is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SplitCoeffs {
    /// High-pass at `EDGES[k]`, opening band `k + 1` from below.
    high: [[f32; 5]; EDGES.len()],
    /// Low-pass at `EDGES[k]`, closing band `k` from above.
    low: [[f32; 5]; EDGES.len()],
}

impl SplitCoeffs {
    /// Derives the sections' coefficients for a sample rate.
    #[must_use]
    pub fn new(sample_rate: f32) -> Self {
        let mut high = [[0.0; 5]; EDGES.len()];
        let mut low = [[0.0; 5]; EDGES.len()];
        for ((up, down), edge) in high.iter_mut().zip(low.iter_mut()).zip(EDGES) {
            *up = biquad_coeffs(edge, sample_rate, true);
            *down = biquad_coeffs(edge, sample_rate, false);
        }
        Self { high, low }
    }
}

/// One band-pass per band, each with its own state.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Split {
    /// The high-pass opening band `k + 1`. Band 0 has none.
    high: [Biquad; EDGES.len()],
    /// The low-pass closing band `k`. The top band has none.
    low: [Biquad; EDGES.len()],
}

impl Split {
    /// A split at rest.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Clears every section, so silence in gives silence out from the next
    /// sample rather than after the filters have run down.
    pub fn reset_state(&mut self) {
        for stage in self.high.iter_mut().chain(self.low.iter_mut()) {
            stage.reset_state();
        }
    }

    /// One sample in, one sample per band out.
    #[inline]
    pub fn process(&mut self, x: f32, coeffs: &SplitCoeffs) -> [f32; BANDS] {
        let mut out = [0.0; BANDS];

        // The bottom band is open below, so it is a low-pass and nothing else.
        // Every band above it starts from the high-pass at its own lower edge
        // and is then closed by the low-pass at its upper edge, except the top
        // band, which is open above.
        for (index, slot) in out.iter_mut().enumerate() {
            let mut value = x;
            if let (Some(stage), Some(c)) = (
                index.checked_sub(1).and_then(|k| self.high.get_mut(k)),
                index.checked_sub(1).and_then(|k| coeffs.high.get(k)),
            ) {
                stage.set_coeffs(*c);
                value = stage.process(value);
            }
            if let (Some(stage), Some(c)) = (self.low.get_mut(index), coeffs.low.get(index)) {
                stage.set_coeffs(*c);
                value = stage.process(value);
            }
            *slot = value;
        }
        out
    }
}

#[cfg(test)]
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::float_cmp
)]
mod tests {
    use core::f32::consts::PI;

    use super::*;

    const SR: f32 = 48_000.0;

    /// Steady-state energy per band for a tone, past the filters' settling.
    fn energy(hz: f32) -> [f64; BANDS] {
        let coeffs = SplitCoeffs::new(SR);
        let mut split = Split::new();
        let mut sums = [0.0f64; BANDS];
        for i in 0..19_200 {
            let x = (2.0 * PI * hz * i as f32 / SR).sin();
            let bands = split.process(x, &coeffs);
            if i > 9_600 {
                for (sum, value) in sums.iter_mut().zip(bands) {
                    *sum += f64::from(value) * f64::from(value);
                }
            }
        }
        sums
    }

    /// Every frequency lands somewhere, and the order is the order of the
    /// spectrum.
    #[test]
    fn the_map_is_total_and_monotonic() {
        assert_eq!(band_of(0.0), 0);
        assert_eq!(band_of(-1.0), 0);
        assert_eq!(band_of(f32::NAN), 0);
        assert_eq!(band_of(f32::INFINITY), BANDS - 1);
        assert_eq!(band_of(f32::MAX), BANDS - 1);

        let mut previous = 0;
        for step in 0..2_000 {
            let hz = step as f32 * 10.0;
            let band = band_of(hz);
            assert!(band < BANDS, "{hz} Hz landed in band {band} of {BANDS}");
            assert!(band >= previous, "{hz} Hz went backwards");
            previous = band;
        }
    }

    /// An edge belongs to the band above it, so the two halves of a
    /// comparison never both claim it.
    #[test]
    fn an_edge_belongs_upward() {
        for (index, edge) in EDGES.iter().enumerate() {
            assert_eq!(band_of(edge - 0.01), index);
            assert_eq!(band_of(*edge), index + 1);
        }
    }

    /// **The property the split exists for: the map and the filters agree.**
    ///
    /// A resonator is handed the band its frequency maps to, so a tone at that
    /// frequency has to be loudest there. A split that disagreed with its own
    /// map would excite the wrong resonators, which is the defect ADR 0016 is
    /// about rather than a matter of degree.
    #[test]
    fn a_tone_is_loudest_in_the_band_its_frequency_maps_to() {
        for hz in [40.0f32, 90.0, 180.0, 380.0, 760.0, 1_500.0, 4_000.0] {
            let sums = energy(hz);
            let loudest = sums
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.total_cmp(b.1))
                .map(|(index, _)| index)
                .unwrap();
            assert_eq!(
                loudest,
                band_of(hz),
                "{hz} Hz was loudest in band {loudest}, not {}: {sums:?}",
                band_of(hz)
            );
        }
    }

    /// The separation the ladder could not provide, and the slope that makes
    /// it grow.
    ///
    /// Twelve decibels an octave on each side, so the arithmetic is knowable
    /// before the test runs: a second-order Butterworth is 12.3 dB down an
    /// octave outside its corner and 24.1 dB down two, against a reference
    /// inside the band that is itself about 1.9 dB off its own peak. **The
    /// point is the second column** — one figure could be a coincidence, and
    /// a slope cannot be.
    ///
    /// For contrast, the one-pole ladder this replaced managed 4 dB at one
    /// octave, which is why it was replaced.
    #[test]
    fn a_band_rejects_by_more_the_further_out_it_gets() {
        // Band 2 runs 260 to 520 Hz; 360 is inside it.
        let inside = energy(360.0)[2];
        let down_to = |hz: f32| {
            let outside = energy(hz)[2];
            10.0 * (inside / outside.max(f64::MIN_POSITIVE)).log10()
        };
        for (name, one, two) in [
            ("below", down_to(130.0), down_to(65.0)),
            ("above", down_to(1_040.0), down_to(2_080.0)),
        ] {
            assert!(
                one > 9.0,
                "an octave {name} the band is only {one:.1} dB down inside it"
            );
            assert!(
                two > one + 9.0,
                "two octaves {name} is {two:.1} dB down against {one:.1} at \
                 one, so the skirt is not falling at the order it should"
            );
        }
    }

    /// No band amplifies. A split that did would put energy into a resonator
    /// that the source never had.
    #[test]
    fn no_band_has_gain_over_one() {
        for hz in [
            30.0f32, 65.0, 130.0, 200.0, 260.0, 400.0, 520.0, 900.0, 1_040.0, 3_000.0,
        ] {
            let sums = energy(hz);
            for (index, sum) in sums.iter().enumerate() {
                // A unit sine has energy 0.5 per sample; anything above that
                // is gain over one.
                let gain = (sum / (9_599.0 * 0.5)).sqrt();
                assert!(gain < 1.01, "band {index} has gain {gain:.3} at {hz} Hz");
            }
        }
    }

    /// Silence in gives silence out once the sections have run down, and
    /// `reset_state` makes that immediate.
    #[test]
    fn a_cleared_split_passes_silence_exactly() {
        let coeffs = SplitCoeffs::new(SR);
        let mut split = Split::new();
        for i in 0..2_400 {
            split.process((i as f32 * 0.01).sin(), &coeffs);
        }
        split.reset_state();
        for _ in 0..64 {
            for band in split.process(0.0, &coeffs) {
                assert_eq!(band, 0.0, "a cleared split rang at {band}");
            }
        }
    }
}
