# hyperglare Demo Audio

| File | What it is |
| --- | --- |
| `dry.wav` | A drum pattern, whole. Stereo 32-bit float, 48 kHz, 14.75 s |
| `wet-default.wav` | The defaults, on a G♯ minor pentatonic chord |
| `wet-flashy.wav` | The same, with the decay at the far end of its range |

The renders are regenerated rather than recorded, so they follow the DSP
instead of pinning it. The commands are at the bottom.

## Why this source and not a bass

**It used to be a bass, and that was wrong.** The design held that a resonator
bank wants a distorted bass where a compressor wants a musical excerpt, so no
one file could serve both. Measured, the opposite: the drum pattern colours
better than the bass did, by a wide margin and in every band.

`dry.wav` here is an excerpt of the loop `demo/oxtt/` uses. **It is a copy, not
a reference** — these become separate repositories
([ADR 0015](../../docs/decisions/0015-documentation-is-namespaced-by-project.md)),
and a file two of them shared would belong to neither.

**It is the whole loop, and shortening it was tried twice.** The pattern has no
gap in it — hits land every ten to twenty milliseconds — so a cut taken
anywhere but the ends begins inside the decay of a hit the listener never heard
begin, and it is audible as a missing attack however carefully the point is
chosen. Cutting the first six seconds instead leaves the pattern stopping dead
where the wet is still ringing.

A loop like this has one place it starts and one place it ends. Both are
already in the file, so the file is what is committed.

## What the source has to be, and why

**Whatever the effect is asked to colour has to already be there.** A bank of
band-pass filters emphasises energy the input has and cannot invent any, and
since [ADR 0016](../../docs/decisions/0016-the-bank-is-excited-per-band.md)
that is true band by band: a resonator draws on the input's energy near its own
frequency, so a band the source is silent in produces nothing at all.

**The source's reach is the effect's reach, and it can be measured before
anybody listens.** Energy per band, loudest band at 0 dB:

```text
                     125-250  250-500   500-1k    1k-2k    2k-4k    4k-8k
 this file               0.0     -9.7    -12.4    -12.1     -8.1     -5.0
 the bass it replaced    0.0     -4.6    -16.9    -33.1    -54.0    -70.0
```

**Roughly 10 dB per octave of fall is the practical bound**, on the evidence of
the sources that have worked and the ones that have not. The bass fell off a
cliff — 54 dB down at 2 kHz — so everything above its tenth harmonic was
silence, and stayed silence.

| | |
| --- | --- |
| Format | Stereo, 32-bit IEEE float WAV — what `hyperglare-render` reads and writes |
| Length | A few seconds. The renderer appends the tail, so the source need not |
| Content | Energy across the spectrum, and movement in it |
| Level | Peaks a few dB below full scale. The renderer will not write past −1 dBTP, and reports what that cost |

**Name the chord to match the source.** This loop sits in G♯ minor, so the
renders use `--notes 56,59,61,63,66` — a pentatonic set rather than a triad,
because five pitch classes is closer to what the material this effect is for
actually uses.

## What these two show

**One knob apart, deliberately.** `wet-default.wav` is the shipped settings;
`wet-flashy.wav` changes the decay to 0.6 s and nothing else, so what is heard
between them is the decay and not five things at once.
[ADR 0017](../../docs/decisions/0017-the-wet-path-carries-no-time-constant-of-its-own.md)
moved the default to 0.25 s because colour stops growing there while the
reverberation does not; 0.6 s is where a listener put "flashy, and it matches
the original concept".

The pair these replaced compared two geometries and reached for `--stretch 35`
to do it. That detune is 175 cents across five octaves, which a three-note
triad carries and a five-note chord does not: it came out dissonant. A demo
that changes one thing is a demo of that thing.

The grid's ceiling is 5 kHz here, which is the default and was chosen by ear
across six sources — see
[`docs/hyperglare/contracts.md`](../../docs/hyperglare/contracts.md) §5.0 for
what it trades. **These demonstrate the defaults**, so anything that would
flatter them is not in the commands below.

## Regenerating the renders

```sh
cargo run --release -p hyperglare-render -- \
  --input demo/hyperglare/dry.wav --output demo/hyperglare/wet-default.wav \
  --notes 56,59,61,63,66 --color 0.63

cargo run --release -p hyperglare-render -- \
  --input demo/hyperglare/dry.wav --output demo/hyperglare/wet-flashy.wav \
  --notes 56,59,61,63,66 --decay 0.6 --color 0.63
```

Read the two numbers the renderer prints. `normalization_gain_db` says how much
of a setting was a level change, which is half of what a comparison is for —
these need the amounts the commands print. `loudness_shortfall_db` says whether the peak
ceiling stopped the match from landing; both are zero here, so the two are
level-matched against the source and against each other.
