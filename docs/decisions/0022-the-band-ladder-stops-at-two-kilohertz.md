# ADR 0022: The Band Ladder Stops at 2 kHz, Because the Ceiling Is Not a Rung

## Status

Proposed.

A defect fix rather than a preference, and it still changes how the effect
sounds — the top band gains about 2.7 dB — so
[ADR 0018](0018-hyperglare-is-judged-against-paired-recordings.md) applies and
this wants a listen before it is accepted. The measurement below is arithmetic
on the gain law, not a rendering.

## Scope

`hyperglare`.

Removes one element from `crates/hyperglare-dsp/src/bands.rs`'s `EDGES`, so
`BANDS` goes from seven to six.

**No other change to the code.** The split is built from `EDGES`, `BANDS` is
`EDGES.len() + 1`, `SplitCoeffs` is sized from it and `REFERENCE_BAND_HZ` is
`EDGES[0]`; nothing counts bands by hand.

**Four documents do name the values**, and all four move with it:

- `bank.rs`'s module documentation — the seven-band spans-and-widths table,
  and the paragraph giving the step at 4160 Hz.
- `bands.rs`'s "the top edge is one too many" section, which goes from a known
  defect to a fixed one.
- [`cpu.md`](../hyperglare/bela/cpu.md) — "the band split's twelve biquads,
  the exciter's seven gates" become ten and six, and "**Seven bands are twelve
  biquads**" with them. Those biquads are itemised inside the 5.8% fixed term
  of `CPU% = 5.8 + 0.222 × resonators`, so **the constant moves** and is worth
  re-measuring rather than adjusting on paper.
- [`contracts.md`](../hyperglare/contracts.md) §5.0, for the reason in the
  decision below.

**Withdraws one number from [ADR 0020](0020-the-grids-ceiling-is-five-kilohertz.md).**
That ADR's decision reads "**`bands::EDGES` reach up with it**, to 2080 and
4160", and its reasoning is quoted and kept below — it is the reasoning that
says 4160 should not be there. ADR 0020's decision otherwise stands: the
ceiling is 5 kHz and the ladder still reaches up with it, one edge less far.

## Context

ADR 0020 raised `DEFAULT_HIGH_HZ` from 1.8 kHz to 5 kHz and extended the band
ladder to match, for a reason it states plainly:

> The gain law measures a resonator's bandwidth against its band's, so edges
> left at the old ceiling leave a top band four times too wide, and the
> resonators in it are compensated as if they were in a band four times
> too wide.

Correct, and it is why an edge at 2080 was needed. The second edge, at 4160,
overshoots the same target in the other direction.

The ceiling is 5 kHz and the ladder is octaves of 130, so the edge above 4160
would be 8320. **The ceiling lands 0.27 octaves into a band that wants to be a
whole one.** `band_width_hz` closes the top band at the grid's ceiling rather
than at Nyquist — deliberately, so that a render at 96 kHz matches one at 48 —
so the top band is 840 Hz wide where the one below it is 2080.

The gain law's compensation is `(share/share_ref)^(-p)` with
`share = BW/W_b`, so below the breakpoint, where `BW` is the same for every
resonator, the compensation goes as `W_b^p`. A band that is narrower than its
neighbour is therefore compensated *down*. Every step across the ladder, at
the defaults:

```text
 edge      130     260     520    1040    2080    4160
 step   +0.00   +1.51   +1.51   +1.51   +1.51   -1.97   dB
```

**Every edge steps up by `6.02·p` dB except the last, which steps down by
almost exactly the same amount.** The step at an edge is
`(W_above / W_below)^p` exactly: `BW` is continuous in frequency, so it
divides out of the ratio and **the steps do not depend on `q_max` at all**.
Where the breakpoint sits changes almost everything else about the shape and
does not touch this, so the figures hold under `BankParams::default` and under
the command lines' `--breakpoint-hz` of 1100 Hz alike.

The 3.47 dB reversal at 4160 Hz is not something anyone chose; it is what a
ladder of octaves does when it is asked to stop at a frequency that is not one
of its rungs.

It is a narrow defect. The band is 0.27 octaves wide, so an octave grid puts
at most one point per note inside it, and only for the roughly quarter of
notes whose octave class lands there. But it is at the top of the band, which
is where the effect is named for.

## Decision

**The ladder stops at 2080.** `EDGES` becomes
`[130.0, 260.0, 520.0, 1040.0, 2080.0]`, and the top band runs from 2080 Hz to
whatever ceiling `BankParams` carries.

At the default ceiling that band is 2920 Hz wide against the 1040 below it, so
the last step is `+2.24` dB where the pattern is `+1.51`:

```text
 edge      130     260     520    1040    2080
 step   +0.00   +1.51   +1.51   +1.51   +2.24   dB
```

**Still off the pattern, and that is accepted.** A fixed ladder cannot land on
a ceiling that is a runtime setting, so some band is always the odd one; the
choice is which error to carry. `+0.74` dB above the trend is smaller than
`-3.47` below it, and it points the same way the rest of the ladder does,
which is the part that matters — a listener hears a reversal and does not hear
a rung that is slightly tall.

### It breaks §5.0's letter and serves what §5.0 is for

`contracts.md` §5.0 says the edges are octaves for a reason, and names the
condition this decision creates:

> `crate::bands::EDGES` are octaves because the gain law's share is `BW/W_b`,
> and they have to reach as far as the grid does **or the top band spans more
> than an octave and the resonators in it are lifted against their
> neighbours.**

The top band this decision makes is 2080 Hz to the ceiling, which at 5 kHz is
**1.27 octaves**. So the letter is broken, plainly and on purpose.

What the sentence is protecting is the second half: resonators inside an
over-wide band being lifted against their neighbours. Measured, this decision
**halves that**. Above the breakpoint the compensation falls `6.02·p` dB per
octave *inside* a band, so the spread from 2080 Hz to the ceiling is:

```text
                        band 5    edge at 4160   band 6    total
 today (7 bands)         -1.51          -1.97     -0.40    -3.87
 this decision           -1.90              -         -    -1.90
```

**Three and nine-tenths of a decibel of spread becomes one and nine-tenths.**
The rung that the letter forbids is worth less than the reversal the letter's
own arithmetic produces at 4160.

So §5.0's sentence has to be revised: not to drop the reason, which is sound,
but because "reach as far as the grid does" is unachievable when the grid's
ceiling is a runtime setting and the ladder is fixed octaves. What is true
instead is that the edges are octaves and the top band absorbs whatever the
ceiling leaves — and that the check when the ceiling moves is on the *spread*,
which is measurable, rather than on the octave count, which is a proxy that
has now been wrong once.

**This is where `q_max` comes back.** The steps at the edges do not depend on
it, but the within-band slope is the breakpoint's own doing, so the table
above is the command lines' default of `f*` = 1100 Hz, where all of
2080–5000 Hz is above the breakpoint. Under `BankParams::default`'s 4.4 kHz
most of that range is below it and flat, and the same two figures are
**-2.25 dB and -0.28 dB** — the same direction, a smaller effect. The decision
is unchanged either way; the size of what it buys is not.

### Why not derive the edges from the ceiling instead

That is the fix that removes the error rather than shrinking it, and it is
refused for the reason `EDGES` already gives: deriving them would make
`SplitCoeffs::new` depend on `BankParams` where it depends on the sample rate
alone, and that dependency is what keeps
`HyperglareProcessor::set_sample_rate` a statement about the rate. Buying
0.74 dB with that is not a trade worth making.

### Why not move the ceiling to 4160 or 8320

Because the ceiling was chosen by listening. ADR 0020 settled 5 kHz against a
sweep a listener judged, calling 5 kHz the best-natured of the set, and a
band ladder is not a reason to reopen it. **The ladder serves the ceiling, not the
other way round.**

## Consequences

- **Every render changes**, and by more than the step arithmetic suggests:
  resonators between 2080 Hz and the ceiling move from a band 2080 or 840 Hz
  wide to one 2920 Hz wide, which is `+0.74` dB for those that were in band 5
  and `+2.71` dB for those that were in band 6.
- **`BANDS` goes from seven to six**, so `Exciter::process` returns one fewer
  entry and the split runs one fewer pair of sections. Slightly cheaper, and
  not why this is being done.
- **A caller relying on seven bands gets six**, which is the same consequence
  ADR 0020 recorded in the other direction and has the same answer: `BANDS` is
  public and derived from `EDGES`, so nothing has to be counted by hand.
- **The defect this fixes has been in every render since ADR 0020**, including
  the ones in `demo/hyperglare/` and the board recordings in
  `docs/hyperglare/bela/audio-verification.md`. Nothing in either was judged on
  the top band, and the effect is under 3 dB on a sliver of it, so neither is
  invalidated — but a figure quoted from them above 4160 Hz carries it.
- **It does not close the general question.** A ladder of fixed octaves and a
  ceiling that is a runtime setting will disagree at whatever frequency the
  ceiling lands on. This ADR picks the arrangement where the disagreement is
  smallest at the default and points the right way; a ceiling set somewhere
  else by a caller gets whatever that arrangement gives it.
