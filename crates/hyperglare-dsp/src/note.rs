//! Notes, in the unit a chord actually arrives in.
//!
//! The bank is tuned in hertz, but nothing upstream of it thinks in hertz: a
//! keyboard, a sequencer and a MIDI cable all speak note numbers. Converting
//! here rather than at each caller means the command line that stands in for
//! MIDI today and the MIDI input that replaces it later are the same path with
//! a different front end, instead of two paths that can disagree about what
//! A4 is.

/// The note number concert pitch is defined at, and the pitch it is defined as.
///
/// A440 rather than a configurable reference. A tuning knob is a real feature —
/// a resonator bank is exactly the effect that would want one, because its
/// grid has to land on the input's partials — but it belongs with the chord
/// source, once there is one that is not a fixed argument.
pub const REFERENCE_NOTE: u8 = 69;
/// The frequency of [`REFERENCE_NOTE`], in hertz.
pub const REFERENCE_HZ: f32 = 440.0;

use core::f32::consts::LN_2;

/// Semitones in an octave.
const SEMITONES_PER_OCTAVE: f32 = 12.0;

/// The frequency of a MIDI note number.
///
/// Defined over the whole `u8` range rather than over `0..=127`, because a
/// caller that has a byte has a byte; the notes above 127 are simply very
/// high, and the bank's band limit discards them the same way it discards
/// anything else out of range.
#[must_use]
pub fn note_hz(note: u8) -> f32 {
    let semitones = f32::from(note) - f32::from(REFERENCE_NOTE);
    REFERENCE_HZ * exp2(semitones / SEMITONES_PER_OCTAVE)
}

/// `2^x`, without reaching for `powf`.
#[inline]
fn exp2(x: f32) -> f32 {
    (x * LN_2).exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The definition, and the two octaves either side of it.
    #[test]
    fn concert_pitch_and_its_octaves() {
        for (note, hz) in [(69u8, 440.0f32), (57, 220.0), (81, 880.0), (45, 110.0)] {
            let got = note_hz(note);
            assert!(
                (got / hz - 1.0).abs() < 1e-5,
                "note {note} should be {hz} Hz, got {got}"
            );
        }
    }

    /// The notes the design's own examples are written in, so that a figure
    /// quoted in hertz and a note number given on a command line agree.
    #[test]
    fn the_notes_the_design_quotes_in_hertz() {
        for (note, hz) in [
            (28u8, 41.2f32), // E1
            (33, 55.0),      // A1
            (36, 65.4),      // C2, the band's floor
            (40, 82.4),      // E2
            (45, 110.0),     // A2
        ] {
            let got = note_hz(note);
            assert!(
                (got - hz).abs() < 0.1,
                "note {note} should be about {hz} Hz, got {got}"
            );
        }
    }

    /// Monotonic over the whole domain, and finite at both ends.
    #[test]
    fn every_byte_is_a_finite_increasing_frequency() {
        let mut previous = 0.0f32;
        for note in 0..=u8::MAX {
            let hz = note_hz(note);
            assert!(hz.is_finite() && hz > 0.0, "note {note} gave {hz}");
            assert!(hz > previous, "note {note} went backwards: {previous} {hz}");
            previous = hz;
        }
    }
}
