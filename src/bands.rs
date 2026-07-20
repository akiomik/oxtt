//! `Bands<T>`: exactly one `T` per band (docs/architecture.md, ADR 0001).
//!
//! oxtt is architecturally a 3-band compressor, not a generic N-band
//! design (docs/architecture.md), so this fixes the arity at exactly three
//! named fields rather than `[T; 3]`/`Vec<T>`: `.low`/`.mid`/`.high` access
//! can't go out of range the way an array index can. Used consistently from
//! config (`OttParams::bands`) through to the real-time DSP core
//! (`OttProcessor::bands`, `Crossover`'s per-band filter outputs), so the
//! "3 bands" concept has one representation end to end.

/// One `T` per band: low, mid, high.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bands<T> {
    /// The low band.
    pub low: T,
    /// The mid band.
    pub mid: T,
    /// The high band.
    pub high: T,
}

impl<T> Bands<T> {
    /// Returns an iterator over `&low, &mid, &high`, in that order.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        [&self.low, &self.mid, &self.high].into_iter()
    }

    /// Returns an iterator over `&mut low, &mut mid, &mut high`, in that order.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        [&mut self.low, &mut self.mid, &mut self.high].into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iter_visits_low_mid_high_in_order() {
        let bands = Bands {
            low: 1,
            mid: 2,
            high: 3,
        };
        assert_eq!(bands.iter().copied().collect::<Vec<_>>(), vec![1, 2, 3]);
    }

    #[test]
    fn iter_mut_allows_updating_each_band() {
        let mut bands = Bands {
            low: 1,
            mid: 2,
            high: 3,
        };
        for v in bands.iter_mut() {
            *v *= 10;
        }
        assert_eq!(
            bands,
            Bands {
                low: 10,
                mid: 20,
                high: 30
            }
        );
    }
}
