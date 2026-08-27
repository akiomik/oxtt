# ADR 0018: `hyperglare` Is Judged Against Paired Recordings, and Does Not Chase Resynthesis

## Status

Accepted.

Nothing to implement: this is how ADR 0016 and ADR 0017 were reached, and the
method has already caught four things since. Two of them were mistakes in
the measurement rather than in the effect — a `tilt` reading that was 40 times
too small because the measuring rig masked it, and a `drive` axis that had
never been swept at all.

**The rule it adds by being accepted** is that a sound change arrives with its
paired evidence or it does not arrive. What that costs is speed, and what it
buys is that the last four changes were arguable.

## Scope

`hyperglare`.

Governs how sound changes to this effect are evaluated and what counts as
evidence. Does not change any code. [ADR 0016](0016-the-bank-is-excited-per-band.md)
and [ADR 0017](0017-the-wet-path-carries-no-time-constant-of-its-own.md) are
the first two decisions this method produced, and are the worked example.

## Context

Four sound changes were proposed and accepted on measurement before this ADR,
and three of them were wrong. The pattern is worth recording because it was the
method that failed each time, not the arithmetic.

- The grid's ceiling was lowered from 9 kHz to 1.8 kHz because the high band
  measured sparse against three commercial processors. The change was real; the
  reasoning was not transferable, because those three had been applied to a
  different source than ours, so every number compared two things at once.
- The geometry was changed to a harmonic series because the octave grid
  produces only the chord's three pitch classes. The arithmetic was correct and
  the result was worse: "denser, less chord, more metallic".
- Chroma concentration was used as the measure of "chord sense" for several
  rounds. It is maximised by a static drone on three pure tones, which is
  exactly the defect being chased.

**The common fault: comparing across sources.** With the processed and the
unprocessed audio being different material, no measurement can separate what
the effect did from what the source was.

The fix is material that carries both halves. Five recordings of the same take,
dry and then processed by one commercial effect, made every earlier question
answerable in a single afternoon — including two that had been open for the
whole project.

## Decision

**A change to how `hyperglare` sounds is argued from paired recordings: the
same take, before and after, processed by something whose result is agreed to
be the target.** Cross-source comparison is not evidence and does not appear in
a rationale.

Three measures are in use, chosen because each one separated something a
listener had already reported:

| Measure | What it caught |
| --- | --- |
| Pitch-class concentration, **per octave band** | The chord being painted uniformly over every band and every material |
| Brightness, 2–8 kHz against 250 Hz–1 kHz | `wet_match` acting as a tone control |
| Level drop over the 150 ms after an onset | The wet outliving the source |

Whole-file pitch-class concentration is **retired**: it rewards the defect.

**A measure earns its place by separating a difference a listener reported, and
loses it when it fails to.** Three measures were retired this way — spectral
flatness, mid-to-low ratio, and spectral flux, none of which could tell six
different commercial treatments of one loop apart.

### What `hyperglare` does not chase

The reference's `ice` render is **+6.1 dB** brighter than its source by the
measure above; `hyperglare` reaches −3.2 dB at its brightest. The gap is about
9 dB and it is not a tuning gap: the reference moved the whole spectrum up by
about two octaves, which a bank of band-pass filters cannot do, because a
filter can only emphasise energy the input already has.

**Differences that require resynthesis are recorded and not pursued.** The
value of the paired material is knowing which differences those are.

The same applies in the other direction. The comparison cannot settle
everything: the reference applies spatial processing to its wet, and some
traits shared by both the reference and `hyperglare` on the same source — a
thin middle on `swan` — belong to the source. **A difference is attributed to
the effect only when the pair does not also show it.**

## Consequences

- **Sound changes now need paired material, and there are five pairs.** A
  proposal about material none of them resembles is a proposal without
  evidence, and should say so rather than borrowing a number.
- **The chord in a reference render can be recovered from it.** Estimating
  pitch classes from the processed audio and checking against the chord visible
  on screen matched 18 of 23 notes across the five pairs, so a pair whose
  settings are unknown is still usable.
- **The renders in `demo/hyperglare/` are not evidence and were never meant to
  be.** They demonstrate; the pairs adjudicate. A source rejected for the demo
  is not thereby rejected as a test case, and vice versa.
- **This ADR will make some future proposal harder to argue for.** That is its
  purpose: three of the four changes it would have blocked were mistakes, and
  the fourth was accepted for a reason that did not hold.
