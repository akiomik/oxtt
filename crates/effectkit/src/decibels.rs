//! Decibel conversions for the real-time path, and the floor they respect.
//!
//! Effect-independent, and small enough that the temptation is to inline them
//! at each use. They are here instead because the *floor* is a shared
//! decision, not a formula: every place that takes a logarithm of a level has
//! to agree on what "silent" converts to, or two of them disagree by a few
//! hundred decibels.
//!
//! `f32` throughout, because these run per sample inside the audio callback.
//! Offline code that wants the precision does its own arithmetic in `f64`
//! (`oxtt`'s offline renderer).

/// The level below which a signal is treated as silence, in dBFS
/// (docs/contracts.md §4).
///
/// Prevents `log(0)`, division by zero, and the NaN that follows either.
/// -120 dBFS is about 20 dB below the noise floor of 16-bit audio, so nothing
/// audible is inside it, and it is far enough from `f32`'s limits that
/// squaring the corresponding amplitude — which [`power_to_db`] does — stays
/// exact.
pub const FLOOR_DB: f32 = -120.0;

/// Converts a level in dB into a linear amplitude: `10^(db / 20)`.
///
/// Unfloored, unlike [`power_to_db`]. A caller asking for an amplitude has
/// said what level it means, and clamping it here would silently refuse a
/// deliberate `-140 dB`. The floor exists for the other direction, where the
/// input is a measured power that can genuinely be zero.
#[inline]
#[must_use]
pub fn db_to_amp(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// Converts a linear *power* — an amplitude squared — into dB, flooring at
/// [`FLOOR_DB`]: `10 * log10(max(power, floor))`.
///
/// Power rather than amplitude, hence `10 *` and not `20 *`: the callers are
/// envelope followers, which hold squared values so that a detector never has
/// to take a square root per sample.
///
/// The floor is applied to the input, not to the result, so the return value
/// is exactly [`FLOOR_DB`] for a silent input rather than something a few ulps
/// away from it. Anything at or below the floor is indistinguishable
/// afterwards, which is the point: this is a value being fed back into a
/// filter, not a measurement being reported. Metering deliberately does *not*
/// use this — see [`InputMeter::peak_dbfs`](crate::metering::InputMeter::peak_dbfs).
#[inline]
#[must_use]
pub fn power_to_db(power: f32) -> f32 {
    let floor_amp = db_to_amp(FLOOR_DB);
    let floor_power = floor_amp * floor_amp;
    10.0 * power.max(floor_power).log10()
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn unity_is_zero_decibels_in_both_directions() {
        assert_eq!(db_to_amp(0.0), 1.0);
        assert_eq!(power_to_db(1.0), 0.0);
    }

    #[test]
    fn the_two_are_inverses_through_the_power_domain() {
        for db in [-96.0_f32, -24.0, -6.0, 0.0, 6.0, 24.0] {
            let amp = db_to_amp(db);
            let round_tripped = power_to_db(amp * amp);
            assert!(
                (round_tripped - db).abs() < 1e-3,
                "{db} dB round-tripped to {round_tripped} dB"
            );
        }
    }

    /// Silence lands exactly on the floor rather than near it, because the
    /// floor is applied to the input.
    #[test]
    fn silence_and_anything_below_the_floor_land_on_the_floor() {
        assert_eq!(power_to_db(0.0), FLOOR_DB);
        assert_eq!(power_to_db(f32::MIN_POSITIVE), FLOOR_DB);
        let below = db_to_amp(FLOOR_DB - 20.0);
        assert_eq!(power_to_db(below * below), FLOOR_DB);
    }

    /// The other direction has no floor: a caller asking for an amplitude has
    /// already said what level it means.
    #[test]
    fn converting_to_an_amplitude_does_not_floor() {
        assert!(db_to_amp(FLOOR_DB - 40.0) < db_to_amp(FLOOR_DB));
        assert!(db_to_amp(FLOOR_DB - 40.0) > 0.0);
    }
}
