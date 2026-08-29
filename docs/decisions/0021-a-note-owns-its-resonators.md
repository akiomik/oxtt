# ADR 0021: A Note Owns Its Resonators for as Long as It Rings

## Status

Proposed.

Nothing here has been heard. The behaviour it changes — a chord changing
under a bank that is already ringing — is the one thing
[`docs/hyperglare/bela/audio-verification.md`](../hyperglare/bela/audio-verification.md)
lists as never having run on hardware, and it is not reachable from the
command line either, because the command line's chord is fixed for the run.
So this ADR is argued from the code and from one measurement, and it wants a
listen before it is accepted.

**The title overstates by one case, and the decision says which.** A note owns
its resonators until the voice table is full; past that it is stolen, and
stealing reintroduces the failures this ADR is written against. What the
decision buys is that they happen when a player runs out of voices instead of
whenever a key is lifted.

**The part most likely to be revised is the voice count's new default.** See
the last section of the decision.

## Scope

`hyperglare`.

Changes how `ResonatorBank` assigns slots, how `HyperglareProcessor` holds a
chord, and the meaning of `BankParams::voices`. Adds a MIDI intake to
`hyperglare-bela`. Revises [`docs/hyperglare/contracts.md`](../hyperglare/contracts.md)
§5 and the capacity paragraph in §2.

**§2's "filters entering use start from rest" is deliberately not revised.** A
voice-stealing allocator is the first thing that could break it, and the
decision below resets a stolen voice partly so that it does not.

Depends on [ADR 0016](0016-the-bank-is-excited-per-band.md), which made a
resonator's excitation a per-resonator lookup settled at retune; this ADR adds
a second thing to that lookup and needs no new mechanism because of it. Keeps
[ADR 0017](0017-the-wet-path-carries-no-time-constant-of-its-own.md) by adding
no time constant at all.

**Not decided here:** where MIDI comes from physically, control changes,
velocity curves, channel filtering, or anything to do with the control
surface. Those are additive and none of them changes what is decided below.

## Context

### Nothing has ever changed the chord while the bank was ringing

`ResonatorBank::retune` documents itself as safe for this — "what lets a chord
change under a ringing bank without a click" — and the claim has never been
exercised. The renderer retunes once. The board's binary takes `--notes` and
holds it for the run. A MIDI port is the first caller that changes a chord
while there is something to hear, and it does it several times a second.

### Releasing a note moves the other notes' resonators

Slots are handed out in the order the caller lists its notes, and each note
takes as many as the grid gives it. Measured on the default grid at 48 kHz:

```text
 chord           D          F#          A       total
 D  F#  A    slots 0-6   slots 7-12  slots 13-18   19
 D      A    slots 0-6        -      slots 7-12    13
```

The grid gives each note as many points as fit under the ceiling, so the
counts differ — seven for D, six each for F# and A. Releasing the middle note
moves the top note down six slots. Two things follow from what `retune` then
does with them, and both are audible:

- **The released note's tail is played at the wrong pitch.** Slots 7–12 held
  F#'s ringing state. They keep it — that is the no-click guarantee — and they
  are now given A's coefficients. F#'s tail glides to A. `retune`'s own
  documentation calls that a portamento, and for a chord change it is; for a
  key lift it is a note nobody played.
- **The held note's tail is cut.** Slots 13–18 held A's state and are now
  above `written`, so the loop that clears everything above the chord resets
  them. A's ringing stops at the instant F# was *released*.

### And the last key lifted silences everything at once

With no notes held, `written` is zero, so that same loop resets every filter
in the bank. **A resonator bank whose tails vanish the moment the last key
lifts is not a resonator bank.** It is the one behaviour a player would call a
bug without needing to be told what the effect does.

### The clearing is correct, for a design that had no key lift

The loop says why it exists: so that "a resonator coming into use on some
later chord starts from silence rather than from a chord that stopped
playing", and `contracts.md` §2 promises it. That is right, and this ADR does
not weaken it. What is wrong is the layout underneath — slots are positional
in a list that compacts, so a note that is still sounding can be moved out
from under its own state.

The codebase already anticipated the fix. `BankParams::drift_cents` documents
its spread as being "over slot positions, not over the notes sounding",
because the alternative "moves every other voice's tuning whenever one note is
added or released" — and names a voice-stealing allocator as the caller that
would need to know. That allocator is what this ADR builds.

## Decision

**A note is given a fixed block of resonator slots when it sounds, and keeps
that block until something else needs it.**

Three parts, and the third is the one that carries the level contract.

1. **Slots are allocated per voice at a fixed stride.** Voice `k` owns slots
   `[k·S, (k+1)·S)`, where `S` is the most grid points any one note can
   produce. A voice that produces fewer leaves the rest of its own block idle.
   **Retuning one voice touches one block**, so no note can be moved by
   another note's arrival or departure.

   `S` is derived at retune from the grid's settings rather than being a
   constant, because it depends on all of them. Measured over all 128 notes at
   48 kHz:

   ```text
    geometry        detune      S    slots at 5 voices
    Octaves              0      7                   35
    Octaves          -1200     19                   95
    OctavePairs          0     14                   70
    Harmonics            0    158                  790
   ```

2. **Note off stops the excitation of that voice's resonators and does nothing
   else.** Their coefficients stay, their state stays, and they decay at the
   `T60` they were tuned to. There is no release envelope and no timer: the
   decay the effect already has *is* the release. A voice is reclaimed when a
   new note needs it, not when a clock says its tail is spent.

   This is why [ADR 0017](0017-the-wet-path-carries-no-time-constant-of-its-own.md)
   survives intact — **nothing added here has a time constant at all.**

3. **`voices` becomes the size of the voice table.** It keeps its present job
   as the level's divisor and gains the job of bounding the polyphony.

   A note arriving with no voice free **takes the one released longest ago,
   and resets its state before retuning it**. Failing that — every voice
   held — it takes the one held longest, on the same terms.

### Stealing brings the defect back, and that is the point of the ordering

**The two failures in the Context are properties of writing new coefficients
over a ringing filter, and stealing does exactly that.** This ADR does not
remove them. What it does is confine them to one case — the table being full —
and then make that case as quiet as it can be made.

The ordering is what does the work. A voice released long ago has been
decaying at its `T60` ever since, so by the time it is the oldest candidate
its state is small and what is written over it is close to silence. The
allocator therefore spends released voices first, longest-released first, and
only reaches a sounding note when there is no released one left.

**A stolen voice is reset rather than glided**, and that decides between two
audible faults in favour of the smaller one. Keeping the state gives the
Context's first failure — a tail arriving at a pitch nobody played — and
resetting gives a discontinuity. On the voices the allocator actually reaches
first the discontinuity is inaudible, because the state being zeroed is
already near zero; on a stolen sounding note it is a click where the
alternative is a wrong note, and a click is the one a player can read as "I
ran out of voices".

It also **keeps `contracts.md` §2 exactly as written.** "Filters entering use
start from rest" would be false of a voice stolen without a reset, and the
reset is what lets that sentence stand unchanged rather than needing an
exception for the allocator.

**So the honest claim is narrower than the title.** A note owns its resonators
until the polyphony is exceeded, and past that point this effect behaves like
any other instrument with a voice limit.

### Why the excitation and not the gain

Turning a released voice's gain down would cut its tail, which is the defect
being fixed. Stopping what feeds it lets the tail finish on its own.

The cost is one predicate per resonator in the gather ADR 0016 already
introduced. That gather reads `bands[i]` to pick which band's excitation a
resonator draws on; this reads one more array to decide whether it draws at
all. **A second index on an existing lookup, not a second mechanism.**

### Key lift cannot duck, and that is not an accident

§5's invariant is that the divisor is `min(voices · density, capacity)` and
that every term is a setting rather than a measurement. Under a voice table
that becomes easier to keep rather than harder: **a released voice still
occupies its slot**, so nothing a player does to the keyboard is visible to
the divisor at all. Adding a note does not duck the notes already ringing, and
removing one does not swell them.

### CPU becomes constant, which is the right constant to have

`voices · S` resonators are processed whether or not they are sounding, since
an idle one has zero gain and contributes exactly zero. At five voices on the
default grid that is 35, which the board's measured law from
[`cpu.md`](../hyperglare/bela/cpu.md) — `CPU% = 5.8 + 0.222 · resonators` —
puts at **13.6%**.

Constant is better than proportional here. A chord arriving must not cause a
step in CPU load, because the step lands exactly when the audio matters most.
And the common retune gets *cheaper*: one voice's block of seven rather than
the whole bank, so the `powf` per resonator that `retune` pays is paid seven
times on a key press instead of thirty-five.

### The default voice count rises from four to five

Five is the smallest table that can play the material this effect is judged
against. Of the five paired recordings
[ADR 0018](0018-hyperglare-is-judged-against-paired-recordings.md) adjudicates
from, four are five-note chords and one — `swan` — is four. Four voices cannot
play four of the five; five can play all of them and nothing in the set asks
for a sixth. Four was chosen before those recordings existed.

**This is the part of the decision most likely to be wrong**, and it is worth
being plain about how thin the argument is: it rests on one set of five
chords, chosen by whoever made those recordings, for a keyboard nobody has
played into this effect yet. It is a floor with evidence, not a value with
evidence.

Coupling the polyphony to the divisor is also a real trade and this picks one
end of it: a table sized for eight would let a player hold eight notes, at the
price of a single note arriving `sqrt(8/5)` — about 2 dB — quieter than it
does now. The alternative is two settings that mean almost the same thing,
which is worse to explain than either end of the trade.

## Consequences

- **Every render changes.** The divisor moves with the voice count, so
  `demo/hyperglare/*.wav` and any figure quoted from them are invalidated by
  the default alone, exactly as in ADR 0017.
- **`voices`'s documentation stops being true.** "Not the number of notes" is
  precisely what it becomes. The invariant it was protecting is unaffected, and
  the doc comment has to say why.
- **Capacity stops being a free pool, and it takes `Geometry::Harmonics` off
  the board.** `voices · S` must fit, so a denser geometry now buys its density
  with polyphony rather than with headroom. `Geometry::OctavePairs` doubles
  `S` to 14 and so halves the polyphony that fits. `Geometry::Harmonics` puts
  `S` at 158, which is 790 slots at five voices against the board's
  `CAPACITY` of 256 — **one voice fits** — and which the CPU law would price
  at `5.8 + 0.222 · 790`, about **181%**. It was already the geometry that
  truncates; under a fixed stride it stops being playable on a board at all,
  and that is a real narrowing rather than a detail.
- **`--notes` starts truncating silently.** `hyperglare-render` accepts up to
  `MAX_NOTES = 8` and today's `voices` is only a divisor, so eight notes all
  sound. Against a table of five, three of them are stolen — and the
  workaround, raising `--voices`, also divides the level, so a caller cannot
  ask for the polyphony without asking for the quieter single note. The two
  were separable before this ADR and are not after it.
- **`Grid` needs to report its maximum count**, since `S` is a property of the
  grid rather than of any note. It has `count(note_hz, nyquist)` and nothing
  that maximises over notes — and the maximum has to be taken over the detune
  as well, because a negative `detune_cents_per_octave` shrinks the octave and
  fits more steps into the band. Measured, `Geometry::Octaves` goes from 7
  points to 19 at `-1200`, which is
  `MAX_OCTAVE_STEP - MIN_OCTAVE_STEP + 1` — the compile-time bound, reached.
- **`drift_cents` becomes what its documentation already claims.** Today the
  spread's span is the live note count; under a voice table it is the table
  size, which is what "over slot positions" means. Inert at the default of
  zero, and a behaviour change wherever it is not.
- **`--notes` and MIDI are alternatives, not layers.** Given a MIDI port the
  table starts empty, so the effect is silent at `--color 1.0` until a key is
  pressed — correct, and worth saying out loud because it looks like a fault.
- **Running status is a per-transport hazard, and only one transport has been
  checked.** Bela's MIDI parser discards running-status continuations rather
  than reading them as the message they continue, so a stream that arrives as
  `90 3C 64`, `40 6E`, `43 71` delivers **one note and no report of the rest**
  — and a chord is exactly where running status appears.

  Measured on the path in use: a three-note chord sent to the board arrived at
  its rawmidi device as `903C64`, `904064`, `904364` and their note offs —
  three complete messages, each with its own status byte. **On this transport
  the hazard does not arise.**

  What that does not settle is any other transport, and writing full status
  bytes is not what makes it safe: bela-rs records the opposite case, where a
  sender wrote `90 3C 64` and then `90 40 6E` and an intervening sequencer
  re-encoded the pair into running status, so Bela delivered one message. **A
  keyboard through the board's USB host port, and DIN when it exists, are both
  unchecked, and DIN is the wire format where keyboards actually do this.**
- **Velocity is ignored and channels are not filtered.** A note on with
  velocity zero is a note off, per the specification. Both are additive and
  neither interacts with the voice table.
- **The tail through a key lift has never been heard**, which is the whole
  point of the change and the reason this ADR is proposed rather than
  accepted.
