//! The resonator bank: a chord's worth of tuned band-passes, and the gain law
//! that keeps them from fighting each other.
//!
//! The bank runs in mono. A stereo pair costs twice the filters for a width
//! that can be recovered downstream more cheaply, so the sum goes in and one
//! signal comes out.
//!
//! # Decay, Q, and the cap
//!
//! A resonator's decay and its Q are the same number in two units:
//!
//! ```text
//! T60 = ln(1000) · Q / (pi · f)      Q = T60 · pi · f / ln(1000)
//! ```
//!
//! **So a constant decay across the band means a Q proportional to frequency.**
//! A constant Q would instead make the top of the bank die away first, which
//! sounds like the treble has been switched off rather than like a resonance.
//!
//! Left alone that runs away: four seconds at 5 kHz is a Q of about 9100. The
//! problem is not the number itself — the filter is stable there
//! ([`effectkit::filter::Svf`]) — but the bandwidth it implies, which is
//! `f/Q`, or a fifth of a hertz. A resonator that narrow only responds to an
//! input tuned to within a fifth of a hertz, and nothing on a stage is.
//!
//! So Q is capped, and [`BankParams::q_max`] is where. The cap is also a
//! damping control: above the frequency where it starts to bite, the decay
//! shortens with frequency, which is what struck metal does anyway.
//!
//! # The breakpoint
//!
//! The cap starts to bite at
//!
//! ```text
//! f* = q_max · ln(1000) / (pi · T60)
//! ```
//!
//! and above it the effective decay is `T60 · f*/f` — halving every octave.
//! **`f*` is therefore the frequency where the bank stops sustaining and
//! starts ringing**, which is a thing an ear can be pointed at, unlike a Q.
//! [`q_max_for_breakpoint`] exists so the cap can be chosen that way round.
//!
//! # The gain law
//!
//! A resonator collects the part of its excitation that falls inside its own
//! bandwidth. What that is a *share* of changed with
//! [ADR 0016](../../../docs/decisions/0016-the-bank-is-excited-per-band.md):
//! the excitation is no longer one broadband signal but one per band, so the
//! share is `BW / W_b` and not `BW`.
//!
//! ```text
//! BW(f)   = max( ln(1000)/(pi·T60),  f/q_max )
//! gain(f) = tilt(f) · ( (BW(f)/W_b) / (BW_ref/W_ref) )^(-p)
//! ```
//!
//! `BW` is continuous, so the breakpoint needs no case split and no seam to
//! check. What changes with frequency is now `W_b`, and it moves the
//! compensation in **both** halves — in the opposite directions to before:
//!
//! ```text
//!                    BW(f)              BW(f)/W_b                gain(f)
//!  below f*        constant     one number for the band    +6.02·p dB per band
//!  above f*             ∝ f     rises with f, halved at    -6.02·p dB/octave,
//!                               each edge                  +6.02·p dB per edge
//! ```
//!
//! Below the breakpoint the resonator keeps its width while the band around it
//! grows, so its share falls and the compensation lifts, in a step at each
//! edge. Above it the resonator widens with frequency while `W_b` does not, so
//! the share climbs across a band and is halved back at every edge: a sawtooth
//! whose *band-to-band envelope* is flat, which is the property the old law
//! had smoothly and this one has on average.
//!
//! **Below the breakpoint it is a step at each edge and not a slope**, because
//! `W_b` is one number for a whole band, so two resonators inside one band are
//! compensated identically however far apart they are. **Above it that is
//! false** — they differ by 6.02·p dB per octave between them.
//!
//! Which case a resonator is in depends on where `f*` is, and that is not
//! [`BankParams::default`]'s answer. That default's `q_max` of 500 puts it at
//! 4.4 kHz, but no binary runs it: both command lines derive the cap with
//! [`q_max_for_breakpoint`] from `--breakpoint-hz`, whose default is 1100 Hz.
//! **So at the shipped default `f*` is 1100 Hz**, which falls inside band 4,
//! and everything above it — the top of band 4 and all of bands 5 and 6 — is
//! in the second case rather than the first.
//!
//! And `--decay` does not move it. Deriving the cap from a frequency is what
//! makes `f*` independent of the decay, which is the point of doing it that
//! way round: the breakpoint is a thing an ear can be pointed at, and it
//! should stay where it was pointed when another knob turns.
//!
//! The steps are not all the same size either, because the widths are not all
//! in the same ratio:
//!
//! ```text
//!  band     0       1       2       3        4          5
//!  spans  0-130 130-260 260-520 520-1040 1040-2080 2080-ceiling
//!  width   130     130     260     520      1040    ceiling-2080
//! ```
//!
//! The bottom band is open downward, so it is as wide as the one above it and
//! the first step is flat. From there each width doubles — **except the top
//! one, which is closed at the grid's ceiling rather than at an edge.** At the
//! default ceiling of 5 kHz that leaves it 2920 Hz wide against the 1040 below
//! it, so the last step is +2.24 dB where the others are +1.51.
//!
//! **A ladder of fixed octaves cannot land on a ceiling that is a setting**,
//! so one band is always the odd one and the choice is which error to carry.
//! ADR 0022 has the arithmetic: this arrangement is 0.74 dB above the trend,
//! and the one before it — with an edge at 4160 — was 3.47 dB below it, in
//! the opposite direction to the rest of the ladder.
//!
//! The old law had a slope above the breakpoint and nothing below; this one
//! has a staircase below and a sawtooth above. Neither gained a second
//! mechanism.
//!
//! At `p = 0` the compensation disappears, which is the right answer if the
//! bank is being excited tonally rather than by noise. **The exponent now
//! carries a second consequence**: it also decides how level the bank sits
//! from band to band, and at zero that flattening goes with it. One number for
//! both is deliberate — the same judgement in both halves rather than two that
//! can disagree — but it is a judgement with two effects, and
//! `docs/hyperglare/contracts.md` §5 says which.
//!
//! ## What this is not
//!
//! It is not a per-band normalisation. `W_b` and `BW` are properties of one
//! resonator and its band; **no term sums over the resonators in a band**, so
//! a chord that grows in one band gets louder there, which is what §5 requires
//! and what a divisor over the live chord would take away.

use core::f32::consts::{LN_2, LN_10, PI};

use effectkit::filter::{Svf, SvfCoeffs, max_centre_hz};

use crate::bands::{REFERENCE_BAND_HZ, band_of, band_width_hz};
use crate::grid::Grid;

/// `ln(1000)`: the ratio a T60 is defined against.
const LN_1000: f32 = 6.907_755_3;

/// The decay [`BankParams::compensation_exponent`] is measured against.
///
/// **This is a fixed reference, not the current decay.** Dividing by the
/// current decay's bandwidth would make the ratio one everywhere below the
/// breakpoint, and the decay compensation — the entire reason the exponent
/// exists — would cancel itself out. Against a fixed reference the expression
/// reduces to `(T60 / REFERENCE_DECAY_S)^p`, which is what it should be.
///
/// What the reference buys is that the exponent can be swept without moving
/// the overall level: at this decay the compensation is unity whatever `p` is.
/// Comparing two exponents by ear is otherwise comparing two loudnesses, and
/// the louder one wins every time.
pub const REFERENCE_DECAY_S: f32 = 1.0;

/// The note the bank measures a geometry's *density* at.
///
/// A bass fundamental, because that is what the effect is applied to.
///
/// The count of resonators one voice produces differs by a factor of twenty
/// between the cheapest geometry and the dearest, and an uncompensated bank is
/// therefore about 13 dB louder on one than on the other. Choosing between
/// them by ear would then be choosing a loudness, which is the same trap
/// [`REFERENCE_DECAY_S`] exists to avoid — and the geometry is the axis where
/// the comparison matters most.
///
/// **Measured at a fixed note rather than at the chord being played**, for the
/// same reason [`BankParams::voices`] divides by the configured count rather
/// than the live one: a density read off the live chord would move when a note
/// joined it, and a bank with a second of tail would duck audibly. A fixed
/// note makes the divisor a property of the settings alone.
pub const REFERENCE_NOTE_HZ: f32 = 60.0;

/// What it takes to undo the share loss at the reference point.
///
/// A resonator collects the part of its band that falls inside its own
/// bandwidth, and that is a small fraction: at [`REFERENCE_DECAY_S`] the
/// bandwidth is 2.2 Hz against a reference band of 130, so the bank comes out
/// some 18 dB under what went into it. The compensation cannot supply this —
/// it is deliberately *unity* at the reference, which is what makes an
/// exponent sweep a level-matched comparison — so the anchor is separate.
///
/// **Without it `color` is not a crossfade.** That was `wet_match`'s job, and
/// removing a follower that measured this leaves the wet where the physics
/// puts it: audible only in the knob's last tenth.
///
/// A constant, not a measurement and not a sum. Nothing here reads the chord,
/// so a note joining one cannot duck the notes already ringing — the property
/// `docs/hyperglare/contracts.md` §5 states for the voice count and the
/// density, held by the same means.
fn share_makeup() -> f32 {
    (REFERENCE_BAND_HZ * PI * REFERENCE_DECAY_S / LN_1000).sqrt()
}

/// Decibels per octave of spectral tilt at full deflection.
///
/// Three per octave is about twenty across the seven octaves the band spans:
/// enough to go from dark to glassy, short of enough to empty either end.
const TILT_DB_PER_OCTAVE: f32 = 3.0;

/// Shortest decay the bank computes a Q for, in seconds.
const MIN_DECAY_S: f32 = 0.005;
/// Lowest cap the bank computes a Q for. Below `0.5` there is no resonance.
///
/// Public because it is part of what [`q_max_for_breakpoint`] returns: the
/// floor is where that function stops being an inverse, and a caller cannot
/// see that without the number.
pub const MIN_Q_MAX: f32 = 0.5;

/// What the bank needs to know, recomputed whenever a knob or a chord moves.
///
/// Rebuilt at a control rate rather than per sample: every field here reaches
/// the filters through a `tan` and a `powf`, and doing that at the sample rate
/// would cost two orders of magnitude more than the filters themselves.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BankParams {
    /// Where the resonators sit.
    pub grid: Grid,
    /// Decay to 60 dB, in seconds, below the breakpoint.
    pub decay_t60_s: f32,
    /// Ceiling on Q, and therefore the breakpoint. See
    /// [`q_max_for_breakpoint`].
    pub q_max: f32,
    /// How much of the bandwidth difference between resonators to compensate.
    ///
    /// `0.5` is exact for a broadband excitation, `0.0` for a tonal one, and
    /// the truth is in between because the bank is excited both ways at once.
    pub compensation_exponent: f32,
    /// Spectral tilt, `-1.0` (dark) to `1.0` (bright).
    pub tilt: f32,
    /// Cents of detune spread across the voices.
    ///
    /// Deterministic rather than random, so that two renders of the same
    /// settings are the same file and can be compared.
    ///
    /// **The spread is over slot positions, not over the notes sounding.** A
    /// voice's share is fixed by where it sits in the table an allocator hands
    /// over, so the same chord played into two different slot assignments
    /// detunes differently. That is a property an allocator has to know about
    /// — a voice-stealing one that reuses slots in a different order will not
    /// reproduce a chord's detune — and it is left this way because the
    /// alternative, numbering only the sounding voices, moves every other
    /// voice's tuning whenever one note is added or released.
    pub drift_cents: f32,
    /// How many voices the bank is *configured* for, which since ADR 0021 is
    /// also how many it has.
    ///
    /// **The size of the voice table, and so the polyphony.** Each voice owns
    /// a block of resonator slots whether or not it has a note, so a note
    /// arriving with every voice taken steals one rather than growing the
    /// bank. That is what keeps the load flat when a chord arrives.
    ///
    /// The output is divided by its square root, so changing the setting does
    /// not change the loudest the bank can get. **Dividing by the number of
    /// notes currently held would do something else entirely**: adding a note
    /// to a sustaining chord would duck the notes already ringing, which on an
    /// effect with a second of tail is very audible and is not what any
    /// instrument does. A chord is louder than one note, and a key lifting
    /// does not reach the divisor at all — a released voice keeps its block.
    ///
    /// **So the two jobs are one setting on purpose.** A table sized for eight
    /// would let a player hold eight notes at the price of a single note
    /// arriving about 2 dB quieter, and two settings that mean almost the same
    /// thing are worse to explain than either end of that trade.
    ///
    /// The geometry's density is divided out alongside it, at
    /// [`REFERENCE_NOTE_HZ`].
    pub voices: usize,
}

impl Default for BankParams {
    fn default() -> Self {
        Self {
            grid: Grid::default(),
            // The measured knee. Colour saturates here; past it the knob buys
            // reverberation and nothing else, and the reference it is judged
            // against holds its source's transients.
            // See ADR 0017.
            decay_t60_s: 0.25,
            q_max: 500.0,
            // M0's answers, arrived at by listening, and held on that alone.
            // The arithmetic once given for them — that 0.5 costs 3 dB an
            // octave above the breakpoint against 1.5 at 0.25, so 0.25 with a
            // tilt of 0.5 leaves the band flat there — described the gain law
            // as it stood before ADR 0017 made the share band-relative. Under
            // the present law the shape above the breakpoint is a sawtooth
            // with a flat envelope and the slope is below it, so that
            // reasoning no longer reaches these numbers.
            //
            // They are left where listening put them rather than re-derived,
            // because the derivation is not what chose them. Re-deriving them
            // for the present law is open, and ADR 0018 says what it takes.
            compensation_exponent: 0.25,
            tilt: 0.5,
            drift_cents: 0.0,
            // The smallest table that plays the material ADR 0018 adjudicates
            // from: four of the five paired recordings are five-note chords
            // and the fifth is four notes. Four voices cannot play four of the
            // five. See ADR 0021.
            voices: 5,
        }
    }
}

/// The Q cap that puts the breakpoint at `f_star_hz` for a given decay.
///
/// Inverts `f* = q_max · ln(1000) / (pi · T60)`, provided because choosing a
/// cap is choosing a frequency: "the bank stops sustaining above here" is
/// audible, and "the Q stops at 500" is not.
///
/// The result is floored at [`MIN_Q_MAX`], the same value [`ResonatorBank`]
/// clamps to, **so the Q handed back is always the Q the bank will actually
/// use** — which is what the floor buys, and it is not the same as being an
/// inverse everywhere.
///
/// The two are inverses above `MIN_Q_MAX · ln(1000) / (pi · T60)`, about
/// 1.1 Hz at [`REFERENCE_DECAY_S`]. Below that both saturate on the floor and
/// a round trip returns that frequency rather than the one asked for. A
/// breakpoint of one hertz is not a setting anything has a use for; the floor
/// is there to keep a degenerate request off the panic path, not to extend the
/// inverse.
#[must_use]
pub fn q_max_for_breakpoint(f_star_hz: f32, decay_t60_s: f32) -> f32 {
    let q = f_star_hz.max(0.0) * PI * decay_t60_s.max(MIN_DECAY_S) / LN_1000;
    q.max(MIN_Q_MAX)
}

/// The frequency above which the Q cap shortens the decay.
#[must_use]
pub fn breakpoint_hz(q_max: f32, decay_t60_s: f32) -> f32 {
    q_max.max(MIN_Q_MAX) * LN_1000 / (PI * decay_t60_s.max(MIN_DECAY_S))
}

/// A chord's worth of resonators, with capacity for `N` of them.
///
/// `N` is a type parameter because the geometries differ by more than an order
/// of magnitude in how many filters they want — an octave grid needs about
/// seven per voice and a harmonic series about 150 — and an offline comparison
/// should not be limited by what fits on a board.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResonatorBank<const N: usize> {
    filters: [Svf; N],
    coeffs: [SvfCoeffs; N],
    gains: [f32; N],
    /// Which band each resonator draws its excitation from, or [`NO_BAND`] if
    /// it draws on nothing.
    ///
    /// A `u8` rather than a `usize` because there are six of them and this
    /// array is `N` long — the bank's largest use is a harmonic series with
    /// hundreds of resonators, and the index is a label rather than an
    /// address.
    ///
    /// **[`NO_BAND`] is how a released voice goes on ringing.** The per-sample
    /// gather already reads this array and already answers zero for an index
    /// no band has, so silencing a voice's excitation costs nothing it was not
    /// already doing — and the resonator keeps its coefficients and its state,
    /// so it decays at its own `T60` rather than stopping. ADR 0021.
    bands: [u8; N],
    /// The fundamental each voice is tuned to, or zero for a voice with no
    /// note. Only the first [`voices`](Self::voices) entries are read.
    voice_hz: [f32; N],
    /// Whether each voice's key is down. A voice that is not held keeps
    /// everything but its excitation.
    voice_held: [bool; N],
    /// When each voice was last given a note, for choosing which held voice to
    /// take when every one of them is. Monotone, so only the order matters.
    voice_age: [u32; N],
    /// Stamped into `voice_age` and then advanced.
    age: u32,
    /// Resonator slots each voice owns. Voice `k` has `[k·stride, (k+1)·stride)`.
    stride: usize,
    voices: usize,
    active: usize,
    voice_norm: f32,
}

/// The gain law's terms, derived once per retune rather than per resonator.
///
/// Every field is a function of the settings and the sample rate, so this is
/// the part of a retune that does not depend on which note is being tuned —
/// and under a voice table one note is retuned at a time, so it has to be
/// separable or it is paid again for every key press.
struct Law {
    sample_rate: f32,
    /// Not `sample_rate / 2.0`. The filter clamps its own centre frequency
    /// below Nyquist, and a grid point in the gap would be silently folded
    /// onto the clamp — several resonators piled on one frequency, which is
    /// exactly what the grid skips points to avoid. Asking the filter where
    /// its ceiling is keeps the grid's promise instead of half-keeping it.
    nyquist: f32,
    t60: f32,
    q_max: f32,
    bw_floor: f32,
    share_ref: f32,
    makeup: f32,
    pivot_hz: f32,
}

impl Law {
    fn new(params: &BankParams, sample_rate: f32) -> Self {
        let t60 = params.decay_t60_s.max(MIN_DECAY_S);
        let q_max = params.q_max.max(MIN_Q_MAX);
        let bw_ref = LN_1000 / (PI * REFERENCE_DECAY_S);
        Self {
            sample_rate,
            nyquist: max_centre_hz(sample_rate),
            t60,
            q_max,
            bw_floor: LN_1000 / (PI * t60),
            share_ref: bw_ref / REFERENCE_BAND_HZ,
            makeup: share_makeup(),
            pivot_hz: (params.grid.low_hz * params.grid.high_hz).sqrt(),
        }
    }

    /// The Q a resonator at `hz` is built for, capped.
    fn q_at(&self, hz: f32) -> f32 {
        (self.t60 * PI * hz / LN_1000).min(self.q_max)
    }

    /// The gain a resonator at `hz` in band `index` is given.
    fn gain_at(&self, hz: f32, index: usize, params: &BankParams) -> f32 {
        // `BW` is continuous across the breakpoint because it is a max of the
        // two branches, so this needs no case split.
        let bw = self.bw_floor.max(hz / self.q_max);
        // Measured against the band it draws on rather than in hertz. See the
        // module documentation: since ADR 0016 the excitation a resonator sees
        // is broadband only *within a band*, so the share it collects is
        // `BW / W_b` rather than `BW`.
        let share = bw / band_width_hz(index, params.grid.high_hz);
        let compensation = (share / self.share_ref).powf(-params.compensation_exponent);
        self.makeup * tilt_gain(hz, self.pivot_hz, params.tilt) * compensation
    }
}

/// What the summed bank is divided by, from the settings alone.
///
/// Resonators are mutually incoherent, so the sum grows as the square root of
/// their number: divide by the square root of how many the settings ask for,
/// and neither the voice count nor the geometry is also a volume control.
///
/// Capped at the capacity, because a bank that truncates does not have the
/// resonators the settings asked for — it has `capacity` of them — and
/// dividing by a number that is not there would make a bank quiet for no
/// reason a listener could act on. **Every term is a property of the settings,
/// so this stays free of the live chord and cannot duck**: a key lifting does
/// not reach it, and neither does a key pressing.
fn voice_norm(params: &BankParams, nyquist: f32, capacity: usize) -> f32 {
    let configured = params.voices.max(1);
    let density = params.grid.count(REFERENCE_NOTE_HZ, nyquist).max(1);
    let asked_for = configured.saturating_mul(density).min(capacity).max(1);
    let asked_for = f32::from(u16::try_from(asked_for).unwrap_or(u16::MAX));
    1.0 / asked_for.sqrt()
}

/// A band index no band has, for a resonator that is not being excited.
///
/// The gather in [`ResonatorBank::process`] reads the band array with `get`
/// and answers zero when it misses, so this is a value rather than a branch.
const NO_BAND: u8 = u8::MAX;

impl<const N: usize> Default for ResonatorBank<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> ResonatorBank<N> {
    /// An empty bank: no resonators, no state.
    #[must_use]
    pub fn new() -> Self {
        Self {
            filters: [Svf::new(); N],
            coeffs: [idle_coeffs(); N],
            gains: [0.0; N],
            bands: [NO_BAND; N],
            voice_hz: [0.0; N],
            voice_held: [false; N],
            voice_age: [0; N],
            age: 0,
            stride: 1,
            voices: 0,
            active: 0,
            voice_norm: 1.0,
        }
    }

    /// Resonator slots each voice owns.
    ///
    /// [`Grid::max_count`] for the settings in force: the most points any one
    /// note can produce, so no note overruns its own block.
    #[must_use]
    pub const fn stride(&self) -> usize {
        self.stride
    }

    /// Voices the table has room for.
    ///
    /// `BankParams::voices` unless the capacity cannot hold that many blocks,
    /// in which case it is what fits. **This is the polyphony**: a note
    /// arriving when every voice is taken steals one.
    #[must_use]
    pub const fn voices(&self) -> usize {
        self.voices
    }

    /// How many voices are holding a note.
    ///
    /// **Not [`active`](Self::active), and the difference is the point of a
    /// voice table**: `active` is what the bank costs and does not move when a
    /// key does, while this is how much of the chord is down. A caller that
    /// used to read `active` for the size of the chord wants this one.
    ///
    /// A released voice is not counted even while it is still ringing, because
    /// the question this answers is what the player is holding.
    #[must_use]
    pub fn held_voices(&self) -> usize {
        (0..self.voices)
            .filter(|voice| self.is_held(*voice))
            .count()
    }

    /// The note a voice is tuned to, or zero if it has none.
    #[must_use]
    pub fn voice_hz(&self, voice: usize) -> f32 {
        self.voice_hz.get(voice).copied().unwrap_or(0.0)
    }

    /// Whether a voice's key is down.
    ///
    /// A voice that is not held may still be sounding: releasing it stops its
    /// excitation and leaves it to decay. [`voice_energy`](Self::voice_energy)
    /// is what says whether it has finished.
    #[must_use]
    pub fn is_held(&self, voice: usize) -> bool {
        self.voice_held.get(voice).copied().unwrap_or(false)
    }

    /// How much a voice is holding, as the energy a listener would hear.
    ///
    /// ```text
    ///  Σᵢ  (gainᵢ · kᵢ)² · (ic1eqᵢ² + ic2eqᵢ²)      over the voice's block
    /// ```
    ///
    /// **The weight is the whole path from state to output.** The bank sounds
    /// `gain · process_bandpass`, and `process_bandpass` is `k` times the raw
    /// branch the integrators hold, so a comparison that leaves `k` out is
    /// comparing two different quantities. `k` is `1/q`, and `q` is capped, so
    /// across the seven octaves one voice spans it runs about 25 dB — several
    /// times what the gain itself moves. ADR 0021 has the measurement.
    ///
    /// **The two integrator states combine as a sum of squares** because they
    /// are in quadrature at the centre frequency: either one alone crosses
    /// zero every cycle, and a criterion that answered differently between two
    /// reads of the same ringing voice would not be one. See
    /// [`Svf::energy`](effectkit::filter::Svf::energy).
    ///
    /// **The block folds by summing** because it is one note's whole
    /// contribution and energies add. A note spread thinly over seven
    /// resonators is not quieter than one concentrated in two, and a maximum
    /// would say it was.
    ///
    /// Squaring the envelope is what removes the square root, so this is three
    /// multiplies and two adds per resonator and no transcendental.
    #[must_use]
    pub fn voice_energy(&self, voice: usize) -> f32 {
        let start = voice.saturating_mul(self.stride);
        let end = start.saturating_add(self.stride).min(N);
        let mut total = 0.0f32;
        for slot in start..end {
            let (Some(filter), Some(coeffs), Some(gain)) = (
                self.filters.get(slot),
                self.coeffs.get(slot),
                self.gains.get(slot),
            ) else {
                continue;
            };
            let weight = gain / coeffs.q();
            total = (weight * weight).mul_add(filter.energy(), total);
        }
        total
    }

    /// Gives a note to a voice and returns which one it went to.
    ///
    /// A voice already tuned to this frequency takes it back, held or not, so
    /// a repeated note rings its own resonators rather than spreading across
    /// two. Otherwise [`allocate`](Self::allocate) chooses.
    ///
    /// The comparison is exact and that is not a hazard: a frequency reaches
    /// here from `note_hz`, which is a function of a `u8`, so the same note is
    /// the same bits. A caller synthesising frequencies some other way gets a
    /// new voice per distinct value, which is the safe direction.
    ///
    /// **[`None`] for a frequency that is not one** — non-positive, infinite
    /// or `NaN`. Refused before a voice is chosen, because choosing first and
    /// discovering the note is unusable afterwards would have taken a key that
    /// was down and left it holding nothing: `contracts.md` §2 says such a
    /// note is skipped, and a skipped note cannot cost a voice.
    pub fn note_on(
        &mut self,
        note_hz: f32,
        params: &BankParams,
        sample_rate: f32,
    ) -> Option<usize> {
        if !note_hz.is_finite() || note_hz <= 0.0 {
            return None;
        }
        // The table is [`retune`](Self::retune)'s to build, because sizing it
        // means asking the grid about every note there is and that is not a
        // thing to do on a key press. A bank that has never been retuned has
        // no table, so it gets an empty one here rather than silently dropping
        // the note into voice zero of a table of one.
        if self.voices == 0 {
            self.retune(&[], params, sample_rate);
        }
        let voice = self.voice_at(note_hz).unwrap_or_else(|| self.allocate());
        // Saturating rather than wrapping: an order that wrapped would answer
        // backwards once, where one that saturates stops distinguishing voices
        // pressed after the 4-billionth and keeps every earlier answer right.
        self.age = self.age.saturating_add(1);
        if let Some(slot) = self.voice_age.get_mut(voice) {
            *slot = self.age;
        }
        let law = Law::new(params, sample_rate);
        let mut scratch = [0.0f32; N];
        self.assign(voice, note_hz, true, params, &law, &mut scratch);
        Some(voice)
    }

    /// Lifts a note's key, and returns the voice it was on.
    ///
    /// **Stops the voice's excitation and changes nothing else.** Its
    /// coefficients and its state stay, so it goes on ringing and decays at
    /// the `T60` it was tuned to — the decay the effect already has *is* the
    /// release, and nothing here adds a time constant of its own (ADR 0017).
    ///
    /// A note that is not held is not an error and does nothing.
    pub fn note_off(&mut self, note_hz: f32) -> Option<usize> {
        let voice = self.voice_at(note_hz).filter(|v| self.is_held(*v))?;
        if let Some(slot) = self.voice_held.get_mut(voice) {
            *slot = false;
        }
        let start = voice.saturating_mul(self.stride);
        let end = start.saturating_add(self.stride).min(N);
        if start < end {
            for band in self.bands.get_mut(start..end).unwrap_or_default() {
                *band = NO_BAND;
            }
        }
        Some(voice)
    }

    /// The voice tuned to a frequency, if one is.
    ///
    /// Exact, and deliberately so: the question is whether this is the same
    /// note, not whether two measurements agree. A tolerance here would let a
    /// note steal a neighbour's resonators.
    #[allow(clippy::float_cmp)]
    fn voice_at(&self, note_hz: f32) -> Option<usize> {
        (0..self.voices).find(|voice| {
            self.voice_hz
                .get(*voice)
                .is_some_and(|held| *held == note_hz)
        })
    }

    /// Which voice the next note takes.
    ///
    /// **A released voice before a held one, and the quietest released voice
    /// before a louder one.** Whatever is chosen has new coefficients written
    /// over whatever it is still holding, so the job is to make that as small
    /// as it can be made — and a voice released long enough ago is holding
    /// almost nothing. Release *time* is not a usable stand-in for that: at
    /// the default `T60` of 0.25 s a voice is 24 dB down after 100 ms and 36
    /// after 150, so 12 dB of difference in what excited two voices reverses
    /// their order, which is ordinary playing.
    ///
    /// With every voice held there is nothing quiet to take, so the oldest
    /// goes — the one whose key went down first, which is what an instrument
    /// out of voices does.
    fn allocate(&self) -> usize {
        let mut quietest: Option<(usize, f32)> = None;
        let mut oldest: Option<(usize, u32)> = None;
        for voice in 0..self.voices {
            if self.is_held(voice) {
                let age = self.voice_age.get(voice).copied().unwrap_or(0);
                if oldest.is_none_or(|(_, seen)| age < seen) {
                    oldest = Some((voice, age));
                }
            } else {
                let energy = self.voice_energy(voice);
                if quietest.is_none_or(|(_, seen)| energy < seen) {
                    quietest = Some((voice, energy));
                }
            }
        }
        quietest
            .map(|(voice, _)| voice)
            .or_else(|| oldest.map(|(voice, _)| voice))
            .unwrap_or(0)
    }

    /// How many resonators are currently sounding.
    #[must_use]
    pub const fn active(&self) -> usize {
        self.active
    }

    /// The capacity, for a caller deciding whether a geometry fits.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        N
    }

    /// Clears every filter's state, leaving the tuning in place.
    ///
    /// Silences the tails. A caller that wants the tails to ring through a
    /// change should simply not call this.
    pub fn reset_state(&mut self) {
        for f in &mut self.filters {
            f.reset_state();
        }
    }

    /// Whether every filter's state is free of NaN and infinities.
    #[must_use]
    pub fn is_finite(&self) -> bool {
        self.filters.iter().take(self.active).all(Svf::is_finite)
    }

    /// Retunes the bank to a chord, one note per voice. Control rate, not
    /// sample rate.
    ///
    /// `notes_hz[k]` is voice `k`'s fundamental and holds its key down; a
    /// non-positive or non-finite entry leaves that voice empty, so a caller
    /// need not compact its own table. Notes past the voice count are dropped
    /// rather than wrapped.
    ///
    /// **A voice keeps its state if its note did not move, and starts from
    /// rest if it did.** So a decay or a tilt changes under a ringing bank
    /// without a click, and a voice handed a different note does not play the
    /// old note's state at the new pitch — which is a glide from a note nobody
    /// pressed, not a portamento anybody asked for. It used to glide; ADR 0021
    /// is why it does not.
    ///
    /// **Not the path a keyboard takes.** Keys arrive through
    /// [`note_on`](Self::note_on) and [`note_off`](Self::note_off), and a
    /// caller mixing the two would release everything the keys had pressed.
    /// [`resettle`](Self::resettle) is the settings-only half for a caller
    /// whose chord comes from keys.
    pub fn retune(&mut self, notes_hz: &[f32], params: &BankParams, sample_rate: f32) {
        self.rebuild(params, sample_rate, Some(notes_hz));
    }

    /// Rebuilds the table for new settings, keeping the notes it holds.
    ///
    /// **The settings-only half of [`retune`](Self::retune)**, for a decay or
    /// a tilt or a sample rate moving under a chord that has not. Under a
    /// voice table the chord no longer arrives as a list — it arrives one key
    /// at a time — so a caller that has only new settings has nothing to pass.
    ///
    /// Keeps the tails when it can. A change that moves the stride or the
    /// voice count moves every block, and state at the old positions would be
    /// heard at the new ones, so that case resets; anything smaller does not.
    pub fn resettle(&mut self, params: &BankParams, sample_rate: f32) {
        self.rebuild(params, sample_rate, None);
    }

    /// One implementation for both, because they differ only in where the
    /// notes come from.
    fn rebuild(&mut self, params: &BankParams, sample_rate: f32, chord: Option<&[f32]>) {
        let law = Law::new(params, sample_rate);

        // The block size, and how many blocks fit. `max_count` is the most
        // points any one note can produce, so no note overruns its own block;
        // the capacity decides the rest, and a geometry too dense for even one
        // block gets one anyway and truncates inside it.
        let stride = params.grid.max_count(law.nyquist).max(1);
        let voices = N
            .checked_div(stride)
            .unwrap_or(1)
            .clamp(1, params.voices.max(1));
        // Every block moves when either of these does, so state written for
        // the old layout would be heard at the new one.
        if stride != self.stride || voices != self.voices {
            for filter in &mut self.filters {
                filter.reset_state();
            }
        }
        self.stride = stride;
        self.voices = voices;
        self.active = self.voices.saturating_mul(self.stride).min(N);

        let mut scratch = [0.0f32; N];
        for voice in 0..self.voices {
            let (hz, held) = chord.map_or_else(
                || (self.voice_hz(voice), self.is_held(voice)),
                |notes| {
                    let hz = notes.get(voice).copied().unwrap_or(0.0);
                    (hz, hz > 0.0)
                },
            );
            self.assign(voice, hz, held, params, &law, &mut scratch);
        }

        // A table that shrank leaves notes above it that nothing plays. Cleared
        // so that a table growing back does not find a chord nobody is holding.
        for voice in self.voices..N {
            if let Some(slot) = self.voice_hz.get_mut(voice) {
                *slot = 0.0;
            }
            if let Some(slot) = self.voice_held.get_mut(voice) {
                *slot = false;
            }
        }

        // **Every filter above the table is at rest, always.** A bank starts
        // that way, `reset_state` restores it, and this loop is the only thing
        // that preserves it when the table shrinks — so a slot coming into use
        // under a wider stride starts from silence rather than from a voice
        // that stopped playing. Inside the table the same job belongs to
        // `assign`, which clears the tail of each block it writes.
        //
        // The coefficients and the gains go with the state, which buys
        // something smaller and still worth having. Nothing above the table is
        // read — `process` and `process_split` both stop at `active` — so what
        // is left there cannot be heard; but this type derives `PartialEq`,
        // and would otherwise report two banks that sound identical as
        // different because one of them used to be bigger.
        let idle = idle_coeffs();
        for (((coeffs, gain), filter), band) in self
            .coeffs
            .iter_mut()
            .zip(self.gains.iter_mut())
            .zip(self.filters.iter_mut())
            .zip(self.bands.iter_mut())
            .skip(self.active)
        {
            *coeffs = idle;
            *gain = 0.0;
            *band = NO_BAND;
            filter.reset_state();
        }

        self.voice_norm = voice_norm(params, law.nyquist, N);
    }

    /// Writes one voice's block: its tuning, its gains and its excitation.
    ///
    /// **A voice given a frequency it does not already hold starts from
    /// rest.** That is one rule for two things that used to be two: a chord
    /// changing under `retune`, and a note stealing a voice from another note.
    /// Both write new coefficients over whatever state is there, and state
    /// that meant one frequency means nothing at another — it would be heard
    /// as a glide from a note nobody played. `contracts.md` §2 asks for the
    /// same thing in the same words.
    ///
    /// A voice re-given the frequency it already has keeps its state, which is
    /// what lets a decay or a tilt change without cutting the tails.
    fn assign(
        &mut self,
        voice: usize,
        note_hz: f32,
        held: bool,
        params: &BankParams,
        law: &Law,
        scratch: &mut [f32],
    ) {
        let start = voice.saturating_mul(self.stride);
        let end = start.saturating_add(self.stride).min(N);
        if start >= end {
            return;
        }

        // Normalised here so that "no note" is one value rather than four. A
        // `NaN` left as itself would compare unequal to itself and reset the
        // voice on every retune, which is harmless and would still be wrong.
        let note_hz = if note_hz.is_finite() && note_hz > 0.0 {
            note_hz
        } else {
            0.0
        };
        let held = held && note_hz > 0.0;

        // Exact: the question is whether this voice is being given a different
        // note, and a tolerance would let a small retune keep state that means
        // the old frequency.
        #[allow(clippy::float_cmp)]
        let changed = self.voice_hz.get(voice).copied().unwrap_or(0.0) != note_hz;
        if let Some(slot) = self.voice_hz.get_mut(voice) {
            *slot = note_hz;
        }
        if let Some(slot) = self.voice_held.get_mut(voice) {
            *slot = held;
        }
        if changed {
            for filter in self.filters.get_mut(start..end).unwrap_or_default() {
                filter.reset_state();
            }
        }

        // The spread is over the table's slots rather than over the notes
        // sounding, which is what `BankParams::drift_cents` documents: adding
        // or releasing a note must not retune the voices around it.
        let written = if note_hz > 0.0 {
            let detuned = note_hz * voice_drift_ratio(voice, self.voices, params.drift_cents);
            // Clamped to the buffer as well as to the stride: a geometry
            // denser than the whole bank leaves a block that is shorter than
            // its own stride, and asking for the stride would get nothing at
            // all rather than what fits.
            let room = scratch
                .get_mut(..self.stride.min(scratch.len()))
                .unwrap_or_default();
            params.grid.frequencies(detuned, law.nyquist, room)
        } else {
            0
        };

        let idle = idle_coeffs();
        for offset in 0..self.stride {
            let slot = start.saturating_add(offset);
            if slot >= end {
                break;
            }
            let hz = if offset < written {
                scratch.get(offset).copied().unwrap_or(0.0)
            } else {
                0.0
            };
            if hz > 0.0 {
                let index = band_of(hz);
                if let Some(band) = self.bands.get_mut(slot) {
                    *band = if held {
                        u8::try_from(index).unwrap_or(0)
                    } else {
                        NO_BAND
                    };
                }
                if let Some(coeffs) = self.coeffs.get_mut(slot) {
                    *coeffs = SvfCoeffs::new(hz, law.sample_rate, law.q_at(hz));
                }
                if let Some(gain) = self.gains.get_mut(slot) {
                    *gain = law.gain_at(hz, index, params);
                }
            } else {
                // Past what this note produced. Not in use, and cleared so
                // that a denser note arriving later finds it at rest.
                if let Some(coeffs) = self.coeffs.get_mut(slot) {
                    *coeffs = idle;
                }
                if let Some(gain) = self.gains.get_mut(slot) {
                    *gain = 0.0;
                }
                if let Some(band) = self.bands.get_mut(slot) {
                    *band = NO_BAND;
                }
                if let Some(filter) = self.filters.get_mut(slot) {
                    filter.reset_state();
                }
            }
        }
    }

    /// One sample through every sounding resonator, split across a stereo
    /// pair by alternating grid points.
    ///
    /// The bank runs in mono, so this is the cheap way to get width out of it:
    /// neighbouring resonators — an octave apart under the octave geometries —
    /// go to opposite sides, and `width` blends that back toward the centre.
    /// At `width = 0` both channels receive the whole sum and this is
    /// [`process`](Self::process) in a pair.
    ///
    /// Constant power: `gl² + gr²` is two whatever the width, so widening is
    /// not also a level control.
    ///
    /// **This is only useful before a non-linearity.** Once the sum has been
    /// through a waveshaper the individual resonators are no longer separable,
    /// which is why the order of the two is a choice rather than a detail.
    #[inline]
    pub fn process_split(&mut self, bands: &[f32], width: f32) -> (f32, f32) {
        let width = width.clamp(0.0, 1.0);
        let mut left = 0.0f32;
        let mut right = 0.0f32;
        for (index, (((filter, coeffs), gain), band)) in self
            .filters
            .iter_mut()
            .zip(self.coeffs.iter())
            .zip(self.gains.iter())
            .zip(self.bands.iter())
            .take(self.active)
            .enumerate()
        {
            let x = bands.get(usize::from(*band)).copied().unwrap_or(0.0);
            let value = gain * filter.process_bandpass(coeffs, x);
            // Alternate sides, so the two channels carry interleaved octaves
            // rather than a split band.
            let pan = if index % 2 == 0 { -width } else { width };
            left = (1.0 - pan).sqrt().mul_add(value, left);
            right = (1.0 + pan).sqrt().mul_add(value, right);
        }
        (left * self.voice_norm, right * self.voice_norm)
    }

    /// One sample through every sounding resonator.
    #[inline]
    pub fn process(&mut self, bands: &[f32]) -> f32 {
        let mut sum = 0.0f32;
        for (((filter, coeffs), gain), band) in self
            .filters
            .iter_mut()
            .zip(self.coeffs.iter())
            .zip(self.gains.iter())
            .zip(self.bands.iter())
            .take(self.active)
        {
            let x = bands.get(usize::from(*band)).copied().unwrap_or(0.0);
            sum = gain.mul_add(filter.process_bandpass(coeffs, x), sum);
        }
        sum * self.voice_norm
    }
}

/// What an unused coefficient slot holds.
///
/// Any value would do for the audio, since nothing reads past `active`. One
/// value is what makes equality mean "these two banks are the same bank"
/// rather than "these two banks were reached the same way".
fn idle_coeffs() -> SvfCoeffs {
    SvfCoeffs::new(1_000.0, 48_000.0, 1.0)
}

/// The detune ratio for one voice, spread deterministically across the chord.
///
/// Voice 0 sits `-drift/2` cents low and the last one `+drift/2` cents high,
/// with the rest evenly between. Deterministic so that a render is
/// reproducible: an A/B between two settings has to differ only in the
/// setting.
#[inline]
fn voice_drift_ratio(index: usize, voices: usize, drift_cents: f32) -> f32 {
    if drift_cents == 0.0 || voices <= 1 {
        return 1.0;
    }
    // A chord has single digits of voices, so both fit a `u16` exactly.
    let span = f32::from(u16::try_from(voices.saturating_sub(1)).unwrap_or(u16::MAX));
    let position = f32::from(u16::try_from(index).unwrap_or(u16::MAX)) / span - 0.5;
    cents_ratio(drift_cents * position)
}

/// The gain a spectral tilt applies at one frequency.
///
/// A rotation about the geometric centre of the band rather than a shelf, so
/// that moving the tilt changes the balance without changing the level.
#[inline]
fn tilt_gain(hz: f32, pivot_hz: f32, tilt: f32) -> f32 {
    if tilt == 0.0 || !pivot_hz.is_finite() || pivot_hz <= 0.0 {
        return 1.0;
    }
    let octaves = (hz / pivot_hz).log2();
    db_to_amp(tilt.clamp(-1.0, 1.0) * TILT_DB_PER_OCTAVE * octaves)
}

/// `2^(cents/1200)`.
#[inline]
fn cents_ratio(cents: f32) -> f32 {
    (cents / 1200.0 * LN_2).exp()
}

/// `10^(db/20)`, kept local so the per-sample path does not reach for a crate
/// whose floor is defined for level metering rather than for a gain trim.
#[inline]
fn db_to_amp(db: f32) -> f32 {
    (db / 20.0 * LN_10).exp()
}

#[cfg(test)]
// Test-only arithmetic over sample indices and decibels: the counts are small,
// the values positive by construction, and a signal generator reads better
// written out than folded into `mul_add`.
//
// `float_cmp` because the bank stores what it is given: a voice's frequency
// comes back as the bits that went in, and a gain the law set to zero is zero.
// Asserting "close to" there would pass on a bank that had rounded.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::arithmetic_side_effects,
    clippy::suboptimal_flops,
    clippy::indexing_slicing,
    clippy::float_cmp,
    clippy::unwrap_used
)]
mod tests {
    use super::*;
    use crate::bands::{BANDS, REFERENCE_BAND_HZ};

    /// Every band carrying the same sample.
    ///
    /// What the bank saw before ADR 0016 split the excitation, and what these
    /// tests want: they are about level, decay and geometry, and a resonator
    /// that received nothing because of its band would be measuring the split
    /// instead. The routing has its own tests.
    fn every_band(x: f32) -> [f32; BANDS] {
        [x; BANDS]
    }
    use crate::grid::Geometry;

    const SR: f32 = 48_000.0;
    type Bank = ResonatorBank<64>;

    /// RMS of the bank's output under a deterministic noise input, measured
    /// over the second half of the run so the tail has established itself.
    ///
    /// Generic over the capacity so that every level comparison in this module
    /// is the same measurement: a second copy for a larger bank is a second
    /// window and a second seed, and the two drift apart the first time one is
    /// touched.
    fn noise_rms<const N: usize>(bank: &mut ResonatorBank<N>, seconds: f32) -> f32 {
        // Deterministic: an A/B has to differ only in the setting.
        let mut state = 0x2545_f491_4f6c_dd1d_u64;
        let n = (SR * seconds) as usize;
        let mut sum_sq = 0.0f64;
        let mut counted = 0usize;
        for i in 0..n {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let x = ((state >> 40) as f32 / 8_388_608.0) - 1.0;
            let y = bank.process(&every_band(x));
            if i > n / 2 {
                sum_sq += f64::from(y) * f64::from(y);
                counted += 1;
            }
        }
        (sum_sq / counted as f64).sqrt() as f32
    }

    /// A bank set up to isolate whatever a test is measuring.
    ///
    /// `tilt` is explicitly flat. The shipped default is not — M0 chose a
    /// half-open tilt to sit against the compensation's slope — and a spectral
    /// slope is a confound for every test in this module that measures a gain
    /// or a level. Inheriting it would make these tests measure the default
    /// rather than the thing they name.
    fn params(decay: f32, q_max: f32) -> BankParams {
        BankParams {
            decay_t60_s: decay,
            q_max,
            voices: 1,
            tilt: 0.0,
            ..BankParams::default()
        }
    }

    /// `f* = q_max · ln(1000) / (pi · T60)` and its inverse agree, which is
    /// what lets the cap be chosen as a frequency instead of as a Q.
    #[test]
    fn the_breakpoint_and_its_inverse_are_inverses() {
        for (f_star, decay) in [(1_000.0, 1.0), (2_500.0, 0.3), (400.0, 4.0)] {
            let q = q_max_for_breakpoint(f_star, decay);
            let back = breakpoint_hz(q, decay);
            assert!(
                (back / f_star - 1.0).abs() < 1e-4,
                "f*={f_star} decay={decay}: q_max={q} came back as {back}"
            );
        }
    }

    /// The figure the cap's default was chosen against: a Q of 500 at a decay
    /// of one second puts the breakpoint near 1.1 kHz.
    #[test]
    fn the_default_cap_breaks_around_a_kilohertz() {
        let f_star = breakpoint_hz(500.0, 1.0);
        assert!(
            (1_050.0..1_150.0).contains(&f_star),
            "expected about 1.1 kHz, got {f_star}"
        );
    }

    /// Above the breakpoint the cap shortens the decay, and the decay knob
    /// stops reaching: this is the cost the cap buys robustness with, and it
    /// should be visible rather than discovered later.
    #[test]
    fn above_the_breakpoint_the_decay_knob_stops_mattering() {
        let q_max = 500.0;
        let hz = 8_500.0;
        let effective: Vec<f32> = [0.3, 1.0, 4.0]
            .into_iter()
            .map(|t60| {
                // Q is capped here, so T60_eff = ln(1000)*q_max/(pi*f).
                let q = (t60 * PI * hz / LN_1000).min(q_max);
                LN_1000 * q / (PI * hz)
            })
            .collect();
        for e in &effective {
            assert!(
                (e - effective[0]).abs() < 1e-4,
                "the effective decay should not move with the knob: {effective:?}"
            );
        }
        assert!((effective[0] - 0.129).abs() < 0.002, "{effective:?}");
    }

    /// At the reference decay in the reference band the exponent does not
    /// move the level — which is what makes an exponent sweep a level-matched
    /// comparison rather than a loudness comparison.
    ///
    /// **In the reference band, not everywhere below the breakpoint.** That
    /// was the old law's property; since the share is measured against the
    /// band, a resonator in a wider band is compensated by `(W_b/W_ref)^p`
    /// even at the reference decay. The reference has to be one point, and
    /// [`REFERENCE_BAND_HZ`] is where it is.
    ///
    /// The gain there is [`share_makeup`] rather than one: the compensation is
    /// unity, and the makeup that puts the bank at the level of what excited
    /// it is not. Asserting the number rather than "unchanged across `p`"
    /// keeps this a test of both.
    #[test]
    fn the_exponent_does_not_move_the_level_at_the_reference_point() {
        for p in [0.0f32, 0.25, 0.5] {
            let mut settings = params(REFERENCE_DECAY_S, 500.0);
            settings.compensation_exponent = p;
            let mut bank = Bank::new();
            bank.retune(&[60.0], &settings, SR);

            let mut centres = [0.0f32; 64];
            let n = settings
                .grid
                .frequencies(60.0, max_centre_hz(SR), &mut centres);
            // The chord is one note, so it occupies voice 0's block and the
            // gains line up with the frequencies from slot zero. The block is
            // the stride, not the count — everything past the note is idle.
            assert!(
                n <= bank.stride(),
                "{n} points will not fit {}",
                bank.stride()
            );
            assert!(
                bank.gains
                    .iter()
                    .skip(n)
                    .take(bank.stride() - n)
                    .all(|g| *g == 0.0),
                "the tail of the block should be idle"
            );

            let f_star = breakpoint_hz(500.0, REFERENCE_DECAY_S);
            let mut checked = 0;
            for (hz, gain) in centres.iter().zip(bank.gains.iter()).take(n) {
                if *hz >= REFERENCE_BAND_HZ || *hz > f_star {
                    continue;
                }
                checked += 1;
                assert!(
                    (gain / share_makeup() - 1.0).abs() < 0.02,
                    "p={p}: {hz} Hz is in the reference band and should sit at \
                     the makeup {}, got {gain}",
                    share_makeup()
                );
            }
            assert!(checked >= 1, "expected points in the reference band");
        }
    }

    /// A longer decay makes each resonator narrower, so it collects less from
    /// a broadband input. The compensation is what stops the decay knob from
    /// also being a volume knob.
    #[test]
    fn compensation_flattens_the_decay_knobs_effect_on_level() {
        let measure = |p: f32, decay: f32| {
            let mut bank = Bank::new();
            let mut settings = params(decay, 500.0);
            settings.compensation_exponent = p;
            bank.retune(&[60.0], &settings, SR);
            noise_rms(&mut bank, 4.0)
        };
        let spread = |p: f32| {
            let a = measure(p, 0.3);
            let b = measure(p, 3.0);
            20.0 * (b / a).log10()
        };
        let uncompensated = spread(0.0).abs();
        let compensated = spread(0.5).abs();
        assert!(
            uncompensated > 2.0,
            "without compensation the decay should move the level, got {uncompensated} dB"
        );
        assert!(
            compensated < uncompensated / 2.0,
            "compensation should more than halve it: {compensated} dB vs {uncompensated} dB"
        );
    }

    /// The compensation is the share, on both sides of the breakpoint.
    ///
    /// `gain ∝ (BW/W_b)^-p`, so between any two resonators the step is
    /// `20·p·log10((W1/W0)·(BW0/BW1))` and nothing else. Below the breakpoint
    /// `BW` is the same for both and the step is the width ratio alone — a
    /// staircase of `6.02·p` dB wherever a band doubles. Above it `BW` grows
    /// with frequency and cancels the width ratio wherever the bands double
    /// too, which is why the ladder is octaves.
    ///
    /// **Written as one formula rather than two cases**, because ADR 0022
    /// leaves one band that is not an octave: the top one is closed at the
    /// grid's ceiling, so above the breakpoint a pair spanning it does move,
    /// by exactly the amount its extra width buys. A test that asserted "and
    /// not above" would be testing the ladder's shape while claiming to test
    /// the law.
    ///
    /// **Both halves come from one exponent**, which is the property worth
    /// keeping from the law this replaced: one judgement about how the bank is
    /// excited, not two that can disagree.
    #[test]
    fn the_compensation_is_the_share_on_both_sides_of_the_breakpoint() {
        for p in [0.25f32, 0.5] {
            // The step, measured between two bands that are both below f*.
            let mut low = params(REFERENCE_DECAY_S, 500.0);
            low.compensation_exponent = p;
            let mut bank = Bank::new();
            bank.retune(&[60.0], &low, SR);
            let mut centres = [0.0f32; 64];
            let n = low.grid.frequencies(60.0, max_centre_hz(SR), &mut centres);
            let f_star = breakpoint_hz(500.0, REFERENCE_DECAY_S);

            let mut by_band: [Option<(f32, f32)>; BANDS] = [None; BANDS];
            for (hz, gain) in centres.iter().zip(bank.gains.iter()).take(n) {
                if *hz > f_star {
                    continue;
                }
                if let Some(slot) = by_band.get_mut(band_of(*hz))
                    && slot.is_none()
                {
                    *slot = Some((*hz, *gain));
                }
            }
            let seen: Vec<(usize, f32, f32)> = by_band
                .iter()
                .enumerate()
                .filter_map(|(b, v)| v.map(|(hz, g)| (b, hz, g)))
                .collect();
            assert!(seen.len() >= 2, "need two bands below f*, got {seen:?}");
            for pair in seen.windows(2) {
                let [(b0, hz0, g0), (b1, hz1, g1)] = [pair[0], pair[1]];
                // From the widths themselves, not from the band index. The
                // bottom band is open downward and so is as wide as the one
                // above it, which makes the first step flat — a test that
                // assumed one step per edge would be testing a paraphrase.
                let w0 = band_width_hz(b0, low.grid.high_hz);
                let w1 = band_width_hz(b1, low.grid.high_hz);
                let expected = 20.0 * p * (w1 / w0).log10();
                let step = 20.0 * (g1 / g0).log10();
                assert!(
                    (step - expected).abs() < 0.2,
                    "p={p}: {hz0} Hz (band {b0}, {w0} Hz wide) to {hz1} Hz \
                     (band {b1}, {w1} Hz wide) stepped {step} dB, expected \
                     {expected}"
                );
            }

            // And above the breakpoint the same formula holds, with `BW` no
            // longer constant. A low cap puts most of the band there.
            let cap = 100.0;
            let mut high = params(REFERENCE_DECAY_S, cap);
            high.compensation_exponent = p;
            let mut bank = Bank::new();
            bank.retune(&[60.0], &high, SR);
            let n = high.grid.frequencies(60.0, max_centre_hz(SR), &mut centres);
            let f_star = breakpoint_hz(cap, REFERENCE_DECAY_S);
            let above: Vec<(f32, f32)> = centres
                .iter()
                .zip(bank.gains.iter())
                .take(n)
                .filter(|(hz, _)| **hz > f_star * 1.5)
                .map(|(hz, g)| (*hz, *g))
                .collect();
            assert!(above.len() >= 2, "need two points above f*, got {above:?}");
            let (f0, g0) = above[0];
            let (f1, g1) = *above.last().unwrap();
            // Well above f*, so `BW` is `f/q_max` for both and the cap divides
            // out of the ratio.
            let w0 = band_width_hz(band_of(f0), high.grid.high_hz);
            let w1 = band_width_hz(band_of(f1), high.grid.high_hz);
            let expected = 20.0 * p * ((w1 / w0) * (f0 / f1)).log10();
            let step = 20.0 * (g1 / g0).log10();
            assert!(
                (step - expected).abs() < 0.2,
                "p={p}: {f0} Hz (band width {w0}) to {f1} Hz (band width {w1})                  stepped {step} dB, expected {expected}"
            );
        }
    }

    /// The voice normalisation is by the configured count, so a chord is
    /// louder than a single note and adding a note does not duck the ones
    /// already ringing.
    #[test]
    fn a_chord_is_louder_than_one_note_at_the_same_voice_setting() {
        let settings = BankParams {
            voices: 4,
            ..params(0.6, 500.0)
        };
        let mut one = Bank::new();
        one.retune(&[110.0], &settings, SR);
        let mut chord = Bank::new();
        chord.retune(&[110.0, 138.6, 164.8], &settings, SR);
        let (a, b) = (noise_rms(&mut one, 2.0), noise_rms(&mut chord, 2.0));
        assert!(b > a * 1.2, "chord {b} should exceed single note {a}");
    }

    /// Changing the voice *setting* must not change how loud the bank can get,
    /// which is the property the square root buys.
    #[test]
    fn the_voice_setting_does_not_change_the_level_of_a_full_chord() {
        let notes = [110.0, 138.6, 164.8, 220.0];
        let level = |voices: usize| {
            let mut bank = Bank::new();
            bank.retune(
                &notes[..voices.min(notes.len())],
                &BankParams {
                    voices,
                    ..params(0.6, 500.0)
                },
                SR,
            );
            noise_rms(&mut bank, 2.0)
        };
        let db = 20.0 * (level(4) / level(2)).log10();
        assert!(
            db.abs() < 3.0,
            "a full chord should hold its level across voice settings, got {db} dB"
        );
    }

    /// The comparison the three geometries exist for has to be a comparison of
    /// sound, not of loudness. Uncompensated, a harmonic series runs about
    /// twenty times as many resonators as an octave grid and is therefore some
    /// 13 dB louder — and the louder one wins every time.
    #[test]
    fn the_geometries_are_level_matched() {
        let level = |geometry: Geometry| {
            let settings = BankParams {
                grid: Grid {
                    geometry,
                    ..Grid::default()
                },
                voices: 1,
                ..params(0.6, 500.0)
            };
            let mut bank = ResonatorBank::<1024>::new();
            bank.retune(&[60.0], &settings, SR);
            (bank.active(), 20.0 * noise_rms(&mut bank, 3.0).log10())
        };
        let (n_oct, oct) = level(Geometry::Octaves);
        let (n_pair, pair) = level(Geometry::OctavePairs);
        let (n_harm, harm) = level(Geometry::Harmonics);

        // The densities really do differ by the order of magnitude that makes
        // this worth compensating.
        // How far apart the densities are follows the band's width — the
        // harmonic series grows with it and the octave grid grows with its
        // logarithm — so this asserts the ordering and a several-fold gap
        // rather than a multiple that only held at one ceiling.
        assert!(n_pair >= 2 * n_oct, "{n_oct} vs {n_pair}");
        assert!(n_harm > 4 * n_oct, "{n_oct} vs {n_harm}");

        // The harmonic bound is 3.0 rather than the 2.0 it was before ADR 0022,
        // and the reason is measured: the gap was 1.89 dB with seven bands and
        // is 2.58 with six. Removing the top edge widens what the top band's
        // *split* passes, not only what the gain law divides by, and the two
        // geometries do not have the same share of their resonators up there.
        // 2.0 had 0.11 dB of room and was going to be tripped by something.
        for (name, db, bound) in [("pairs", pair, COHERENT_PAIR_DB), ("harmonics", harm, 3.0)] {
            assert!(
                (db - oct).abs() < bound,
                "{name} is {} dB from octaves ({oct} vs {db}); the geometries \
                 must be compared at the same loudness",
                db - oct
            );
        }
    }

    /// How far a pair may sit above the octave grid before the level match
    /// is considered broken.
    ///
    /// The divisor assumes the resonators are mutually incoherent, which is
    /// true of neighbours an octave apart and not of a pair a few cents apart:
    /// those sum in phase and read louder than the square root predicts.
    /// Measured against the octave grid, at a decay of 0.6:
    ///
    /// ```text
    /// spread    7c     50c    200c   600c
    /// offset  +2.2dB +0.6dB -0.4dB -0.1dB
    /// ```
    ///
    /// Fully coherent would be +3 dB, so that is the bound rather than a
    /// tolerance chosen to make this pass.
    const COHERENT_PAIR_DB: f32 = 3.0;

    /// Density is read at a fixed note, so a chord that grows does not duck
    /// the notes already ringing — the same property the voice count has.
    #[test]
    fn adding_a_note_does_not_duck_the_ones_already_sounding() {
        let settings = BankParams {
            voices: 4,
            ..params(0.6, 500.0)
        };
        let mut bank = Bank::new();
        bank.retune(&[110.0], &settings, SR);
        let before = bank.voice_norm;
        bank.retune(&[110.0, 164.8, 220.0], &settings, SR);
        assert!(
            (bank.voice_norm - before).abs() < 1e-6,
            "the divisor moved when a note joined: {before} -> {}",
            bank.voice_norm
        );
    }

    /// A bank too small for its settings is not *also* punished by the
    /// divisor. What it loses is the top of its chord, and what that costs in
    /// level is whatever the gain law's own slope says — which is a different
    /// thing from the divisor charging it twice.
    ///
    /// **The bound comes from the law rather than from a round number.** The
    /// resonators that survive truncation are the lowest, and since the share
    /// is measured against the band the lowest are also the quietest; a
    /// tolerance picked by hand would be measuring how far apart the bands
    /// happen to be today.
    #[test]
    fn a_truncated_bank_holds_its_level() {
        let settings = BankParams {
            grid: Grid {
                geometry: Geometry::Harmonics,
                ..Grid::default()
            },
            voices: 1,
            ..params(0.6, 500.0)
        };
        let mut roomy = ResonatorBank::<1024>::new();
        roomy.retune(&[60.0], &settings, SR);
        let mut cramped = ResonatorBank::<6>::new();
        cramped.retune(&[60.0], &settings, SR);
        assert!(
            roomy.active() > 4 * cramped.active(),
            "{} vs {}",
            roomy.active(),
            cramped.active()
        );

        // **The divisors differ, and that is the compensation rather than a
        // bug.** Each is `min(voices · density, capacity)`, so the cramped
        // bank divides by the six resonators it has instead of the eighty-two
        // the settings asked for; dividing by what it does not have is
        // precisely how a truncated bank would end up quiet.
        assert!(
            cramped.voice_norm > roomy.voice_norm,
            "the cramped bank should divide by less: {} vs {}",
            cramped.voice_norm,
            roomy.voice_norm
        );

        // What remains is the gain law's spread across the band, which the
        // truncated bank sits at one end of.
        let gains: Vec<f32> = roomy.gains.iter().take(roomy.active()).copied().collect();
        let spread = 20.0
            * (gains.iter().copied().fold(f32::MIN, f32::max)
                / gains.iter().copied().fold(f32::MAX, f32::min))
            .log10();
        let db = 20.0 * (noise_rms(&mut cramped, 3.0) / noise_rms(&mut roomy, 3.0)).log10();
        assert!(
            db.abs() < spread,
            "truncating to {} of {} moved the level by {db} dB, past the \
             {spread} dB the gain law spans across the band",
            cramped.active(),
            roomy.active()
        );
    }

    /// The density divisor is exact at one note. Under an octave grid the
    /// count barely moves, so the level holds across the keyboard; under a
    /// harmonic series the count collapses and the level goes with it.
    ///
    /// Both halves are pinned, because the second is a cost the geometry has
    /// to be worth rather than a defect to be corrected in the divisor —
    /// following the live count would bring back the ducking that
    /// [`REFERENCE_NOTE_HZ`] exists to prevent.
    #[test]
    fn level_across_the_keyboard_holds_for_octaves_and_slides_for_harmonics() {
        let level = |geometry: Geometry, note: f32| {
            let settings = BankParams {
                grid: Grid {
                    geometry,
                    ..Grid::default()
                },
                voices: 1,
                ..params(0.6, 500.0)
            };
            let mut bank = ResonatorBank::<1024>::new();
            bank.retune(&[note], &settings, SR);
            20.0 * noise_rms(&mut bank, 3.0).log10()
        };
        let spread = |geometry: Geometry| {
            let db: Vec<f32> = [60.0f32, 110.0, 220.0, 440.0]
                .into_iter()
                .map(|n| level(geometry, n))
                .collect();
            let (lo, hi) = db
                .iter()
                .fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
            (hi - lo, db)
        };

        let (octaves, oct_db) = spread(Geometry::Octaves);
        assert!(
            octaves < 1.5,
            "an octave grid should hold its level across the keyboard, got {octaves} dB: {oct_db:?}"
        );

        let (harmonics, harm_db) = spread(Geometry::Harmonics);
        assert!(
            harmonics > 5.0,
            "a harmonic series is expected to slide; if this stopped being \
             true the divisor changed, got {harmonics} dB: {harm_db:?}"
        );
        // Quietest at the top, because that is where the partials run out.
        assert!(harm_db[0] > *harm_db.last().unwrap(), "{harm_db:?}");
    }

    /// At zero width the split is the mono sum in both channels, and at full
    /// width the two channels carry different resonators — while the total
    /// power does not move, so the width knob is not a volume knob.
    #[test]
    fn splitting_is_constant_power_and_collapses_to_mono_at_zero_width() {
        let settings = params(0.6, 500.0);
        let drive = |bank: &mut Bank, width: f32| {
            let mut acc = (0.0f64, 0.0f64, 0.0f64);
            for i in 0..9_600 {
                let x = if i % 480 < 8 { 1.0 } else { 0.0 };
                let (l, r) = bank.process_split(&every_band(x), width);
                acc.0 += f64::from(l) * f64::from(l);
                acc.1 += f64::from(r) * f64::from(r);
                acc.2 += f64::from(l - r).abs();
            }
            acc
        };

        let mut mono = Bank::new();
        mono.retune(&[110.0], &settings, SR);
        let (ml, mr, mdiff) = drive(&mut mono, 0.0);
        assert!(
            mdiff < 1e-6,
            "zero width should be identical channels: {mdiff}"
        );

        let mut wide = Bank::new();
        wide.retune(&[110.0], &settings, SR);
        let (wl, wr, wdiff) = drive(&mut wide, 1.0);
        assert!(
            wdiff > 1.0,
            "full width should differ between channels: {wdiff}"
        );

        let db = 10.0 * ((wl + wr) / (ml + mr)).log10();
        assert!(db.abs() < 0.5, "widening moved the total power by {db} dB");
    }

    /// A voice given no note takes no slots' worth of sound, and does not
    /// disturb the voices that do have one.
    ///
    /// Four voices are configured, so the three junk entries reach real slots
    /// rather than falling off the end of a one-voice table.
    #[test]
    fn non_positive_notes_are_skipped() {
        let settings = BankParams {
            voices: 4,
            ..params(0.6, 500.0)
        };
        let mut bank = Bank::new();
        bank.retune(&[110.0, 0.0, -5.0, f32::NAN], &settings, SR);

        assert!(bank.is_held(0), "the real note should be held");
        for voice in 1..4 {
            assert!(!bank.is_held(voice), "voice {voice} has no note to hold");
            assert_eq!(
                bank.voice_hz(voice),
                0.0,
                "voice {voice} should read as empty"
            );
        }

        // The table is the same size either way — that is the point of a fixed
        // stride — so what says the junk was skipped is the sound, not the
        // count.
        let mut one = Bank::new();
        one.retune(&[110.0], &settings, SR);
        assert_eq!(bank.active(), one.active());
        let db = 20.0 * (noise_rms(&mut bank, 1.0) / noise_rms(&mut one, 1.0)).log10();
        assert!(db.abs() < 0.01, "the junk entries were audible: {db} dB");
    }

    /// Retuning keeps the tails of the voices it does not move, and starts the
    /// ones it does from rest.
    ///
    /// **One rule for two things that used to be two** (ADR 0021): a knob
    /// turning under a ringing bank must not cut it, and a voice given a
    /// different note must not play the old note's state at the new pitch.
    #[test]
    fn retuning_keeps_a_tail_the_notes_did_not_move() {
        let ring = |bank: &mut Bank| {
            for i in 0..4_800 {
                bank.process(&every_band(if i < 48 { 1.0 } else { 0.0 }));
            }
        };
        let tail = |bank: &mut Bank| {
            let mut peak = 0.0f32;
            for _ in 0..4_800 {
                peak = peak.max(bank.process(&every_band(0.0)).abs());
            }
            peak
        };

        // Same note, a different decay: the tail survives, which is what a
        // knob turning under a ringing bank depends on.
        let mut bank = Bank::new();
        bank.retune(&[110.0], &params(2.0, 500.0), SR);
        ring(&mut bank);
        bank.retune(&[110.0], &params(1.5, 500.0), SR);
        let kept = tail(&mut bank);
        assert!(kept > 1e-4, "a knob should not cut the tail: {kept}");

        // A different note: the voice is being reused, so it starts from rest
        // rather than gliding the old note's state to the new frequency.
        let mut bank = Bank::new();
        let settings = params(2.0, 500.0);
        bank.retune(&[110.0], &settings, SR);
        ring(&mut bank);
        bank.retune(&[164.8], &settings, SR);
        let reused = tail(&mut bank);
        assert!(
            reused < 1e-6,
            "a voice given a different note should start from rest: {reused}"
        );
    }

    /// Capacity is a bound. A geometry that wants more filters than the bank
    /// has is truncated, not wrapped.
    #[test]
    fn capacity_bounds_the_grid() {
        let mut small = ResonatorBank::<8>::new();
        small.retune(
            &[60.0],
            &BankParams {
                grid: Grid {
                    geometry: Geometry::Harmonics,
                    ..Grid::default()
                },
                ..params(0.6, 500.0)
            },
            SR,
        );
        assert_eq!(small.active(), 8);
        assert_eq!(small.capacity(), 8);
    }

    /// Capacity bounds the table in whole voices, and the voices it keeps are
    /// the first ones listed.
    ///
    /// **Blocks changed the granularity and not the promise** (ADR 0021). A
    /// chord that does not fit used to lose its last voices part-way through
    /// one of them; now a voice either has a whole block or does not exist,
    /// which is the same guarantee with a coarser step. An allocator that
    /// cares which notes survive still has to order the table itself.
    #[test]
    fn capacity_is_spent_in_whole_voices_and_the_last_ones_are_dropped() {
        // Three voices asked for, so the capacity is what decides how many
        // there are rather than the setting.
        let settings = BankParams {
            voices: 3,
            ..params(0.6, 500.0)
        };
        let stride = settings.grid.max_count(max_centre_hz(SR));
        assert_eq!(stride, 7, "the default grid should give a seven-slot block");

        // Room for two blocks, asked for three notes.
        let mut two = ResonatorBank::<14>::new();
        two.retune(&[110.0, 220.0, 440.0], &settings, SR);
        assert_eq!(two.stride(), stride);
        assert_eq!(two.voices(), 2, "two blocks fit in fourteen slots");
        assert_eq!(two.active(), 14);
        assert_eq!(two.voice_hz(0), 110.0, "the first note should survive");
        assert_eq!(two.voice_hz(1), 220.0, "the second note should survive");
        assert!(two.voice_hz(2) == 0.0, "the third note has no block");

        // Room for one. The chord loses everything but its first note.
        let mut one = ResonatorBank::<7>::new();
        one.retune(&[110.0, 220.0, 440.0], &settings, SR);
        assert_eq!(one.voices(), 1);
        assert_eq!(one.voice_hz(0), 110.0);

        // A block that does not fit whole is still a block: the bank keeps one
        // voice and truncates inside it rather than keeping none.
        let mut part = ResonatorBank::<4>::new();
        part.retune(&[110.0], &settings, SR);
        assert_eq!(part.voices(), 1);
        assert_eq!(part.active(), 4, "the one block is cut to the capacity");
    }

    /// Releasing a note stops what feeds it and nothing else.
    ///
    /// **The defect ADR 0021 exists for.** A resonator bank whose tails vanish
    /// when a key lifts is not a resonator bank; one that goes on being
    /// excited after the key lifts is not a keyboard.
    #[test]
    fn releasing_a_note_stops_the_excitation_and_leaves_the_ringing() {
        let settings = BankParams {
            voices: 2,
            ..params(2.0, 500.0)
        };
        let excited = |bank: &mut Bank, level: f32, frames: usize| {
            let mut peak = 0.0f32;
            for _ in 0..frames {
                peak = peak.max(bank.process(&every_band(level)).abs());
            }
            peak
        };

        let mut bank = Bank::new();
        bank.note_on(110.0, &settings, SR);
        excited(&mut bank, 1.0, 480);
        bank.note_off(110.0);

        // It is still sounding.
        let mut ringing = bank;
        let tail = excited(&mut ringing, 0.0, 4_800);
        assert!(tail > 1e-4, "the tail was cut by the key lift: {tail}");

        // And it is deaf: driving it hard changes nothing, sample for sample.
        let mut driven = bank;
        let forced = excited(&mut driven, 1.0, 4_800);
        assert_eq!(
            forced, tail,
            "a released voice should not answer its excitation"
        );
    }

    /// A key lifting cannot change the level of the notes still held.
    ///
    /// `contracts.md` §5: the divisor is the configured count. Under a voice
    /// table that gets easier to keep rather than harder — a released voice
    /// still owns its block — and this is the test that says so.
    #[test]
    fn a_key_lifting_does_not_duck_the_notes_still_held() {
        let settings = BankParams {
            voices: 4,
            ..params(0.6, 500.0)
        };
        let mut bank = Bank::new();
        let before = bank.voice_norm;
        bank.note_on(110.0, &settings, SR);
        bank.note_on(164.8, &settings, SR);
        let holding = bank.voice_norm;
        let slots = bank.active();
        bank.note_off(110.0);

        assert_eq!(bank.voice_norm, holding, "a key lift moved the divisor");
        assert_eq!(bank.active(), slots, "a key lift moved the table");
        assert!(
            before != holding || bank.voices() == 0,
            "the divisor should come from the settings, not from nothing"
        );
    }

    /// The table is the same size whatever is held, which is what makes the
    /// load flat: a chord arriving must not cause a step in CPU.
    #[test]
    fn the_table_does_not_grow_when_a_note_arrives() {
        let settings = BankParams {
            voices: 4,
            ..params(0.6, 500.0)
        };
        let mut bank = Bank::new();
        bank.retune(&[], &settings, SR);
        let empty = bank.active();
        assert_eq!(empty, 4 * bank.stride());

        bank.note_on(110.0, &settings, SR);
        assert_eq!(bank.active(), empty);
        bank.note_on(164.8, &settings, SR);
        assert_eq!(bank.active(), empty);
        bank.note_off(110.0);
        assert_eq!(bank.active(), empty);
    }

    /// The allocator spends released voices before held ones, and the
    /// quietest released voice before a louder one.
    #[test]
    fn the_allocator_takes_the_quietest_released_voice() {
        let settings = BankParams {
            voices: 2,
            ..params(2.0, 500.0)
        };
        let mut bank = Bank::new();
        let first = bank.note_on(110.0, &settings, SR).expect("a real note");
        let second = bank.note_on(164.8, &settings, SR).expect("a real note");
        assert_ne!(first, second, "two notes should take two voices");
        for _ in 0..480 {
            bank.process(&every_band(1.0));
        }

        // Release the first and let it decay further than the second.
        bank.note_off(110.0);
        for _ in 0..2_400 {
            bank.process(&every_band(0.0));
        }
        bank.note_off(164.8);
        assert!(
            bank.voice_energy(first) < bank.voice_energy(second),
            "the first voice should have decayed further: {} against {}",
            bank.voice_energy(first),
            bank.voice_energy(second)
        );

        assert_eq!(
            bank.note_on(220.0, &settings, SR),
            Some(first),
            "the quietest released voice should have been taken"
        );
    }

    /// With every voice held there is nothing quiet to take, so the oldest
    /// key goes — and it is reset rather than glided.
    #[test]
    fn a_full_table_steals_the_oldest_key_and_resets_it() {
        let settings = BankParams {
            voices: 2,
            ..params(2.0, 500.0)
        };
        let mut bank = Bank::new();
        let first = bank.note_on(110.0, &settings, SR).expect("a real note");
        bank.note_on(164.8, &settings, SR);
        for _ in 0..480 {
            bank.process(&every_band(1.0));
        }
        assert!(bank.voice_energy(first) > 0.0, "it should be ringing");

        assert_eq!(
            bank.note_on(220.0, &settings, SR),
            Some(first),
            "the oldest key should have been taken"
        );
        assert_eq!(
            bank.voice_energy(first),
            0.0,
            "a stolen voice starts from rest"
        );
    }

    /// A note that is not a frequency costs nothing, even with the table full.
    ///
    /// **The order matters and this is what pins it.** Refusing after the
    /// allocator has chosen would take the oldest key, hand it a frequency of
    /// zero and reset it — a key that is down, silenced by a message that
    /// names no note. `contracts.md` §2 says such a note is skipped.
    #[test]
    fn a_note_that_is_not_a_frequency_takes_no_voice() {
        let settings = BankParams {
            voices: 2,
            ..params(2.0, 500.0)
        };
        for junk in [f32::NAN, f32::INFINITY, -1.0, 0.0] {
            let mut bank = Bank::new();
            let first = bank.note_on(110.0, &settings, SR).expect("a real note");
            let second = bank.note_on(164.8, &settings, SR).expect("a real note");
            for _ in 0..480 {
                bank.process(&every_band(1.0));
            }
            let ringing = [bank.voice_energy(first), bank.voice_energy(second)];
            assert!(ringing[0] > 0.0 && ringing[1] > 0.0);

            assert_eq!(
                bank.note_on(junk, &settings, SR),
                None,
                "{junk} took a voice"
            );
            assert_eq!(bank.held_voices(), 2, "{junk} lifted a key");
            assert_eq!(bank.voice_hz(first), 110.0);
            assert_eq!(bank.voice_hz(second), 164.8);
            assert_eq!(
                [bank.voice_energy(first), bank.voice_energy(second)],
                ringing,
                "{junk} silenced a voice"
            );
        }
    }

    /// A note pressed again goes back to its own resonators rather than
    /// spreading across two voices.
    #[test]
    fn a_repeated_note_returns_to_its_own_voice() {
        let settings = BankParams {
            voices: 4,
            ..params(0.6, 500.0)
        };
        let mut bank = Bank::new();
        let voice = bank.note_on(110.0, &settings, SR).expect("a real note");
        bank.note_on(164.8, &settings, SR);
        bank.note_off(110.0);
        assert_eq!(bank.note_on(110.0, &settings, SR), Some(voice));
        assert!(bank.is_held(voice));
    }

    /// The criterion weighs the whole path from state to output, so it is not
    /// the bare state.
    ///
    /// `gainᵢ · kᵢ` is what stands between an integrator and the output, and
    /// `k` alone runs about 25 dB across one voice's block. A criterion that
    /// left it out would rank a voice with its energy high in the band louder
    /// than it is, every time. What says the weight is there is that the
    /// answer differs from the unweighted sum.
    #[test]
    fn voice_energy_is_weighted_rather_than_the_bare_state() {
        let settings = BankParams {
            voices: 2,
            ..params(0.6, 500.0)
        };
        let mut bank = Bank::new();
        let voice = bank.note_on(110.0, &settings, SR).expect("a real note");
        assert_eq!(bank.voice_energy(voice), 0.0, "a fresh voice holds nothing");

        for _ in 0..480 {
            bank.process(&every_band(1.0));
        }
        let weighted = bank.voice_energy(voice);
        assert!(weighted > 0.0, "an excited voice should hold something");

        let start = voice * bank.stride();
        let bare: f32 = bank
            .filters
            .iter()
            .skip(start)
            .take(bank.stride())
            .map(Svf::energy)
            .sum();
        assert!(bare > 0.0);
        assert!(
            (weighted / bare - 1.0).abs() > 0.01,
            "the weight is missing: {weighted} against a bare {bare}"
        );
    }

    /// Nothing the bank can be asked for produces a non-finite sample.
    #[test]
    fn stays_finite_under_extreme_settings() {
        let mut bank = Bank::new();
        bank.retune(
            &[27.5, 55.0, 110.0, 220.0],
            &BankParams {
                decay_t60_s: 8.0,
                q_max: 20_000.0,
                compensation_exponent: 0.5,
                tilt: 1.0,
                drift_cents: 40.0,
                voices: 4,
                grid: Grid {
                    detune_cents_per_octave: 100.0,
                    ..Grid::default()
                },
            },
            SR,
        );
        let mut peak = 0.0f32;
        for i in 0..(SR as usize) {
            peak = peak.max(
                bank.process(&every_band(if i % 480 < 8 { 1.0 } else { 0.0 }))
                    .abs(),
            );
        }
        assert!(bank.is_finite(), "bank state went non-finite");
        assert!(peak.is_finite(), "output went non-finite");
    }

    #[test]
    fn drift_spreads_voices_symmetrically_and_is_off_at_zero() {
        assert!((voice_drift_ratio(0, 4, 0.0) - 1.0).abs() < 1e-9);
        assert!((voice_drift_ratio(2, 1, 20.0) - 1.0).abs() < 1e-9);
        let low = voice_drift_ratio(0, 4, 20.0);
        let high = voice_drift_ratio(3, 4, 20.0);
        assert!(low < 1.0 && high > 1.0, "{low} .. {high}");
        assert!((low * high - 1.0).abs() < 1e-5, "should be symmetric");
    }

    #[test]
    fn tilt_rotates_about_the_pivot_rather_than_shifting_the_level() {
        let pivot = 765.0;
        assert!((tilt_gain(pivot, pivot, 1.0) - 1.0).abs() < 1e-6);
        assert!(tilt_gain(pivot * 2.0, pivot, 1.0) > 1.0);
        assert!(tilt_gain(pivot / 2.0, pivot, 1.0) < 1.0);
        let up = 20.0 * tilt_gain(pivot * 2.0, pivot, 1.0).log10();
        assert!((up - TILT_DB_PER_OCTAVE).abs() < 0.01, "{up}");
    }
}
