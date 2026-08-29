# ADR 0020: The Grid's Ceiling Is 5 kHz, and Both Earlier Numbers Were Right About One Source

## Status

Accepted.

Implemented as `DEFAULT_HIGH_HZ` in `crates/hyperglare-dsp/src/grid.rs`, with
`bands::EDGES` extended to match.

## Scope

`hyperglare`.

Changes a default and the band edges that depend on it. **No mechanism
changes.**

[ADR 0016](0016-the-bank-is-excited-per-band.md) delegates the band count and
edges to the implementation, and this stays inside that: the edges move only
because the ceiling does. What is not delegated is the ceiling itself, which is
a user-visible default that has now moved twice, so it is recorded here rather
than only in a doc comment on the constant.

## Context

The ceiling has been three numbers, and the first two were each measured
honestly on one source.

- **9 kHz**, taken from a published range on the reasoning that extending
  resonance into the top of the spectrum is what makes an effect glare.
- **1.8 kHz**, after the top came out sparse against three commercial
  processors and a listener called it a bell. Below about 2 kHz the top of the
  output belongs to the source rather than to the bank, and the source's own
  high band is dense because the music made it.

Both measurements were of one source. Swept across six — the five paired
recordings of [ADR 0018](0018-hyperglare-is-judged-against-paired-recordings.md)
and the drum loop — the two halves of the trade are visible together:

```text
 ceiling                    1800    3000    5000    9000
 chord content, 2-9 kHz    0.011   0.099   0.164   0.225
 spectral density, same    0.448   0.288   0.198   0.162
 references                                0.192 .. 0.298
```

At 1.8 kHz the top two bands measure what the source already had: there is no
chord above 2 kHz at all. Raising the ceiling puts one there, and thins that
band; below about 0.19 the top reads as isolated tones, which is what a bell
is.

**Per-band excitation changed one half of this and not the other.** Since
ADR 0016 a resonator rings only on energy the source has near it, so the top
follows the music rather than being invented by broadband noise. It is still
one narrow filter with nothing beside it, and the density row is that sparsity
surviving the change. A claim that ADR 0016 had removed the cause was made
before this sweep and is withdrawn by it.

## Decision

**5 kHz**, chosen by ear across all six sources. It is where the chord has
arrived and the density has not yet left the range the references occupy.

**`bands::EDGES` reach up with it**, to 2080 and 4160. The gain law measures a
resonator's share as `BW/W_b`; above the breakpoint `BW` grows with frequency,
so the share stops moving — which is what makes the compensation flat there —
only if `W_b` grows at the same rate. Edges stopping at 1040 under a 5 kHz
ceiling leave a top band four times too wide, and the resonators in it are
lifted against their neighbours. **Moving the ceiling means moving the edges**,
and the band-edge test is what catches a mismatch.

## Consequences

- **Every render changes**, and a caller relying on five bands gets seven.
- **The bell risk is reduced, not removed.** The density at 5 kHz is 0.198
  against the references' 0.192–0.298 — inside the range and at its bottom. A
  source with less of its own high content will sit below it, and the knob is
  there to be turned down.
- **This will move again if the geometry or the excitation does.** Each of the
  three numbers was correct for the mechanism it was measured on, and two of
  the three mechanisms are no longer what the effect does.
- **The evidence is a listening test over six sources**, which is what
  ADR 0018 asks for and is also the reason this is not a measurement anyone can
  rerun from the repository: the five pairs are not committed.
