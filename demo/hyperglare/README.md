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

**It is a poor source for this effect, and worth keeping as the example of
why.** Measured against three commercial colour-bass processors and their
shared source loop, what a source needs is energy *between* its partials and
movement over time. This one has neither: its spectral flatness is 0.00006
against about 0.5 for the reference material, and its frame-to-frame movement
is 0.15 against 0.48. A resonator bank fed a clean harmonic series can only
return a clean harmonic series, which is an organ.

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
`--notes 33,40,45` is an A minor triad rooted on it. A chord that has nothing
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
