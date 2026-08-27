# hyperglare Demo Audio

| File | What it is |
| --- | --- |
| `dry.wav` | An FM bass from a Syntakt, growling. A1 (55 Hz), 2.5 s, stereo 32-bit float |
| `wet-octaves.wav` | The default geometry on an A minor chord, at seven tenths colour |
| `wet-stretched.wav` | Octave pairs, detuned 35 cents an octave, post-drive after the split |

The renders are regenerated rather than recorded, so they follow the DSP
instead of pinning it. The commands are at the bottom.

## What the source has to be, and why

**A distorted bass.** Not a stylistic preference — a requirement of the
mechanism. A bank of band-pass filters is a filter and not an oscillator, so it
can only emphasise energy the input already has at the frequencies it is tuned
to. A clean sine has one partial and the bank has almost nothing to ring on.

`dry.wav` measures 55 Hz with partials to about 550 Hz, which is a tenth
harmonic.

**It is a poor source for this effect, and there is now a number for it.**
[ADR 0016](../../docs/decisions/0016-the-bank-is-excited-per-band.md) is
*proposed*, not implemented: today's exciter is still broadband, and the
figures below come from a prototype rather than from this crate. Under that
proposal a resonator is excited by the input's energy in its own
neighbourhood, so a band the source is empty in would produce nothing at all —
the source's own reach becomes the effect's reach, and that can be measured
before anybody listens:

```text
source's energy per band, loudest band at 0 dB
                 125-250  250-500   500-1k    1k-2k    2k-4k    4k-8k
 this file           0.0     -4.6    -16.9    -33.1    -54.0    -70.0
 a source that works 0.0     -9.7    -12.4    -12.1     -8.1     -5.0
```

**Roughly 10 dB per octave of fall is the practical bound**, on the evidence of
the sources that have worked and the ones that have not. This file falls off a
cliff: 54 dB down at 2 kHz, 70 at 4. Everything above its tenth harmonic is
silence, so everything above its tenth harmonic stays silent.

A distorted bass with noise in it, moving under an LFO, is what the effect is
for. This file is a single sustained FM tone.

| | |
| --- | --- |
| Format | Stereo, 32-bit IEEE float WAV — what `hyperglare-render` reads and writes |
| Length | A few seconds. The renderer appends the tail, so the source does not have to |
| Content | A bass with obvious saturation. A held note and a couple of moves is plenty |
| Level | Peaks a few dB below full scale. Loudness is matched, but headroom is not recoverable |
| Pitch | Around A1–A2 (MIDI 33–45), where the design's figures are quoted |

**Name the chord to match the source.** `dry.wav` is A1, so the default
`--notes 33,40,45` is rooted on it — A1, E2, A2, which is a root, a fifth and
an octave rather than a triad. There is no third in it, deliberately: the
third is the note that decides major from minor, and a grid that states one
argues with a bass line that meant the other. A chord that has nothing
to do with the source is a legitimate thing to try — it is what the effect is
for — but it is not the first thing to listen to.

## Why a pattern is not what to record first

A phrase that moves across the keyboard cannot settle the first question.

The bank divides out a geometry's density at a fixed reference note, which is
what stops a growing chord from ducking the notes already ringing. A fixed
reference is exact at one note only, and under `Geometry::Harmonics` the error
reaches about 9 dB across three octaves of played note. So on a bass line the
harmonic geometry sounds thin at the top — and whether that is timbre or level
cannot be told apart by ear.

**A sustained note near the reference is a fair comparison between the
geometries. A line is not.**

## Regenerating the renders

```sh
cargo run --release -p hyperglare-render -- \
  --input demo/hyperglare/dry.wav --output demo/hyperglare/wet-octaves.wav \
  --notes 33,40,45 --geometry octaves --decay 0.8 --color 0.7

cargo run --release -p hyperglare-render -- \
  --input demo/hyperglare/dry.wav --output demo/hyperglare/wet-stretched.wav \
  --notes 33,40,45 --geometry octave-pairs --stretch 35 --drift 12 \
  --decay 1.2 --sear 0.5 --sear-placement before-split --width 1.0 --color 0.9
```

Read the reported `normalization_gain_db`: a setting that needed a large
correction was mostly a level change, which is half of what a comparison is
for. These two need −3.0 dB and −5.1 dB, which is a change from the 12.4 and 4.6
they needed before the wet was matched to the dry — the wet is now the same
size as the thing it is mixed against, so the render arrives close to the level
it should be.
