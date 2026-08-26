# hyperglare Demo Audio

**Empty, and waiting on one recording.** Everything M0 has left to decide is
decided by ear, and there is nothing to listen to yet.

## What the source has to be

**A distorted bass.** Not a stylistic preference — a requirement of the
mechanism. A bank of band-pass filters is a filter and not an oscillator, so it
can only emphasise energy the input already has at the frequencies it is tuned
to. A clean sine has one partial and the bank has almost nothing to ring on; a
saturated bass has a dense series, and the resonators sitting on those partials
sound.

The effect has a waveshaper of its own (`--drive`) for exactly this reason, but
it can only work with what arrives. A source that is already dense is a fairer
test of the resonators than one that has to be manufactured by the drive stage
first.

| | |
| --- | --- |
| Format | Stereo, 32-bit IEEE float WAV — what `hyperglare-render` reads and writes |
| Length | 8–15 seconds. About 1.4 MB at 48 kHz mono-into-stereo |
| Content | A bass line with obvious saturation. A held note and a couple of moves is plenty |
| Level | Peaks a few dB below full scale. The renderer matches loudness, so absolute level does not matter, but headroom does |
| Pitch | Somewhere around A1–A2 (MIDI 33–45), because that is where the design's figures are quoted and where the chord defaults sit |

Suggested name: `dry.wav`, matching [`../oxtt/`](../oxtt/).

## Why a pattern is not what to record first

A phrase that moves across the keyboard cannot settle the first question.

The bank divides out a geometry's density at a fixed reference note, which is
what stops a growing chord from ducking the notes already ringing. A fixed
reference is exact at one note only, and under `Geometry::Harmonics` the error
reaches about 9 dB across three octaves of played note. So on a bass line the
harmonic geometry sounds thin at the top — and whether that is timbre or level
cannot be told apart by ear.

**A sustained note near the reference is a fair comparison between the
geometries. A line is not.** Record the held note first; a pattern is worth
having later, for the questions that are about movement rather than about which
geometry to keep.

## What goes here after it

Renders from `hyperglare-render`, named for what they demonstrate, the way
[`../oxtt/`](../oxtt/) names its presets. They are regenerated rather than
recorded, so they follow the DSP instead of pinning it.

```sh
cargo run --release -p hyperglare-render -- \
  --input demo/hyperglare/dry.wav \
  --output demo/hyperglare/wet-octaves.wav \
  --notes 33,40,45
```

Read the reported `normalization_gain_db`: a setting that needed a large
correction was mostly a level change, which is half of what a comparison is
for.
