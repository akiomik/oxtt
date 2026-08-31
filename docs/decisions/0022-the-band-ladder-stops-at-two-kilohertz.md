# ADR 0022: The Band Ladder Stops at 2 kHz, Because the Ceiling Is Not a Rung

## Status

Accepted, on the arithmetic rather than on a preference.

Judged by ear on all six sources — the five paired recordings
[ADR 0018](0018-hyperglare-is-judged-against-paired-recordings.md) adjudicates
from and `oxtt`'s drum loop — at `--color 1.0` and at the demo's 0.63, both
loudness-matched. **The listener called it source-dependent and narrow**: the
low chord loses a little and the top opens up, some sources are preferable the
old way, and at the mix setting the chord sense is hard to tell apart at all.

**So it is accepted for the reason it was proposed and not for a better one.**
A ladder whose last step reverses is wrong wherever the material happens to
land; a ladder that steps the same way throughout is the more general answer,
and the listen says the price of the change is small either way.

**One mechanism in it was found after the listen and is not what this ADR
first described.** See "It is not only the gain law" below.

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
  of `CPU% = 5.8 + 0.222 × resonators`, so **the constant moves**. Re-measured
  after this landed: the line is `7.9 + 0.15 × resonators` over 32 to 256
  resonators, and the band count's own contribution is smaller than the rig's
  run-to-run spread, so it cannot be read out of it.
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

### It is not only the gain law

This ADR was written about `band_width_hz`, which is what the gain law divides
by. `EDGES` also builds the **split**, and the top band's split has no
low-pass: it passes everything above its lower edge. So removing the 4160 edge
does two things, and the second is the larger.

**The resonators between 2080 and 4160 Hz used to be excited by a 2080 Hz
slice and are now excited by everything above 2080 Hz** — about 22 kHz of it
at 48 kHz. That is why the rendered difference is bigger than the
compensation arithmetic predicts. Measured over the six sources at
`--color 1.0`, per octave band, against a gain law that only asks for +0.74
and +2.71 dB:

```text
                 63    125    250    500     1k     2k     4k     8k    16k
 ice           +0.01  +0.01  +0.01  +0.01  +0.01  +0.05  +2.58  +3.76  +8.45
 laser         -0.26  -0.26  -0.26  -0.26  -0.25  -0.24  +2.08  +3.78  +6.36
 spoon         +0.01  +0.01  +0.01  +0.01  +0.02  -0.00  +1.73  +4.25  +1.52
 swan          -0.74  -0.74  -0.74  -0.74  -0.74  -0.68  +0.99  +4.46  +4.07
 duck          -1.13  -1.13  -1.13  -1.13  -1.13  -1.10  +0.78  +2.98  +3.31
 oxtt          -0.97  -0.97  -0.97  -0.98  -0.98  -0.92  +1.19  +3.68  +6.22
```

Nothing below 2 kHz moves; the uniform negatives there are the loudness match
taking back what the top gained. At `--color 0.63` the same columns are +0.3
to +1.4 and +1.6 to +3.0.

**This sits against [ADR 0016](0016-the-bank-is-excited-per-band.md)**, whose
whole point is that a resonator draws on its own neighbourhood. A resonator at
2500 Hz now hears up to Nyquist, which is a looser neighbourhood than it had.

Two things make it acceptable rather than a reason to stop:

- **The mismatch shrinks.** The gain law closes the top band at the grid's
  ceiling — deliberately, so a render at 96 kHz matches one at 48 — while the
  split runs it to Nyquist, so the two have never agreed. At 48 kHz the old
  top band was excited by 24 times the width it was compensated for; the new
  one by about seven. Same trade, same direction, three times smaller.
- **It is what was listened to.** The renders judged above were made with the
  change applied, so the listener's verdict covers this mechanism whether or
  not the ADR had named it.

**What it does not do is settle the split's own ceiling.** A low-pass on the
top band would make excitation and compensation agree, at the cost of putting
the sample rate or the grid's ceiling into `SplitCoeffs::new`, which is the
dependency `bands.rs` documents itself as avoiding. Not decided here.

## Consequences

- **Every render changes.** The gain law's part is `+0.74` dB for resonators
  that were in band 5 and `+2.71` dB for those in band 6, both from a band
  2080 or 840 Hz wide becoming one of 2920. The rendered difference is larger
  than that, for the reason in "It is not only the gain law" above.
- **The geometries drift apart by 0.69 dB.** `Geometry::Harmonics` measured
  1.89 dB from `Geometry::Octaves` with seven bands and 2.58 with six, because
  the two do not have the same share of their resonators above 2080 Hz. The
  level-matching contract still holds — choosing a geometry is still not
  choosing a loudness — but its test bound moves from 2.0 dB to 3.0, and 2.0
  had only 0.11 dB of room left.
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
