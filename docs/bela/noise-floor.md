# The Noise Floor on a Bela Gem Stereo, and What It Costs `oxtt`

`oxtt` on a Bela Gem Stereo hisses. This document is the investigation: what
the hiss is, what it is not, what was measured, and what is still undecided.

It is not an ADR — nothing here is decided. It exists so that the decision,
when it is made, is made from the measurements rather than from memory.

The pass/fail records are in [`audio-verification.md`](audio-verification.md);
this is the reasoning behind them.

## The short version

An upward compressor raises quiet passages, and a noise floor is a quiet
passage. On an RME Babyface Pro FS the raised floor lands at about -85 dBFS
and nobody hears it. On a Bela Gem Stereo it lands about 30 dB higher, and
everybody does.

Nothing is wrong with the port, and after correcting the input gain nothing is
wrong with the board's gain staging either. The board's converters have about
30 dB less usable range than the interface `oxtt` was developed against, and
`oxtt`'s upward compression converts that deficit directly into audible hiss.

**`safe-start` as written does not work on this board.** How to fix that is
the open question.

## How this was measured

Source → Bela Gem Stereo → RME Babyface Pro FS → host, recording the
Babyface's channels 3/4. The Babyface's own input floor is -116.5 dBFS with
nothing connected, stable across every session, and 30 dB below anything being
measured — so it is a valid reference for the board rather than a contributor
to it.

Levels are quoted at the Babyface. The Babyface's input gain was never
established, so *absolute* dBFS figures do not describe levels inside the
Bela; ratios between two of them do, and every conclusion below rests on a
ratio.

Three pitfalls cost real time and are worth writing down, because anything
driving `oxtt-bela` over ssh will hit all three:

- **libbela renames the process** to `oxtt-bela:<pid>:`, so `pkill -x
  oxtt-bela` never matches. An unbracketed `pkill -f oxtt-bela` matches the
  ssh shell running it, which then kills itself and leaves the run alive.
  `pkill -f '[o]xtt-bela'` is the one that works.
- Sleeping for a fixed time instead of polling for the process to appear and
  disappear silently records one run's output under the next run's label.
- `avfoundation` device indices shift whenever any audio device appears or
  disappears. Resolve the interface by name, and check the captured channel
  count — a wrong device yields a file the analysis cannot use.

Two readings were wrong because of the first two and were re-measured; the
harness now verifies rather than assumes.

## What the hiss is

### It is the effect amplifying the converter's noise floor

`--depth 0` — the unity crossover reconstruction, compressors running but not
mixed in — is inaudible at every preset. The analog path is not itself audible.
Mixing the effect in is what makes it audible.

Offline, through `oxtt-render` with nothing but white noise at a known level as
input:

| Input noise floor | `safe-start` | `riot` |
| --- | --- | --- |
| −90 dBFS | +21.9 dB | +49.8 dB |
| −75 dBFS | +19.5 dB | +44.9 dB |
| −60 dBFS | +8.8 dB | +33.9 dB |

`riot` puts the floor about 25 dB higher than `safe-start` does, which is why
`riot` hisses on the Raspberry Pi host too. `riot` is out of scope here — it is
acknowledged as not yet designed. `safe-start` is the problem.

### It is not gain staging — but the gain staging was wrong anyway

This took three attempts to get right, and the wrong answers are recorded
because each looked convincing.

**Attempt 1 — "the board's +16 dB default input gain is too much."**
`--adc-gain-db 0` audibly reduces the hiss. It reduces the signal by the same
16 dB; the test was run on silence with no level compensation, so it showed
nothing.

**Attempt 2 — "so analog gain is what buys signal-to-noise; keep +16 dB."**
Level-matched — 16 dB of analog gain against 16 dB of digital gain across a
unity DSP path — the analog arrangement is quieter, so the dominant noise is
downstream of the input gain. True, and it led straight to the wrong
conclusion, because it was measured with a source 14 dB below line level.

**Attempt 3 — the source was the variable that mattered.** With an Elektron
Syntakt at half output, sweeping the input gain across a linear DSP path:

| Input gain | RMS step (per +3 dB) | Crest factor |
| --- | --- | --- |
| +1 → +4 → +7 | +2.96, +2.96 | 15.2, 15.0, 15.1 |
| +10 | +3.00 | 13.6 |
| +13 | +2.92 | 13.1 |
| **+16 (board default)** | **+2.24** | **12.1** |

The crest factor collapsing from 15 dB to 12 dB is peak clipping. **The board's
default input gain clips a line-level source**, and had been clipping
throughout every measurement above. At full Syntakt output — 19 dB hotter, an
audio-taper volume control — the clean ceiling moves to about -12 dB.

Nothing reports this. It is visible only as squashed peaks.

### Gain staging cannot be improved further

With the source silent, the floor does not move with the input gain:

| Input gain | RMS | A-weighted |
| --- | --- | --- |
| −12 dB | −86.76 | −91.69 dBA |
| 0 dB | −86.74 | −91.44 dBA |
| +16 dB | −84.19 | −88.03 dBA |

Identical within 0.02 dB between −12 dB and 0 dB. The noise is downstream of
the input gain — the converter's own — so the source's volume and the split
between source level and input gain do not change it. The 2.6 dB at +16 dB is
the input stage starting to contribute at high gain.

That settles the rule: **set the input gain as high as the source allows
without clipping.** The ceiling is the same however the gain is divided: both
configurations clip at about −10 dBFS peak at the Babyface.

### Nor with the output gain, where it matters

The other end of the chain is worth ruling out too, because `safe-start`'s
output gain is −18 dB and that attenuates everything ahead of it before the
converter that follows.

With the effect out of the way (`--depth 0`), the floor barely moves:

| Output gain | A-weighted |
| --- | --- |
| −18 dB (preset) | −91.61 dBA |
| −6 dB | −91.55 dBA |
| 0 dB | −91.07 dBA |

0.5 dB across 18 dB of gain. So **the floor `--depth 0` shows is the DAC and
the output stage, not the ADC** — the ADC's contribution is attenuated below it
by the same −18 dB and never surfaces. That has a consequence for how the
figures here should be read: *nothing in this document measures the ADC.*

With the effect running it is the other way round:

| Output gain | A-weighted |
| --- | --- |
| −18 dB (preset) | −77.34 dBA |
| 0 dB | −59.08 dBA |
| +6 dB | −53.06 dBA |

1:1 with the gain. By then the noise is the ADC's, raised about 31 dB by the
upward compressor, which puts it far above the DAC's own — so it scales with
the signal and the ratio does not move. **The 24 dB of unused DAC headroom
buys nothing**, which was worth measuring because it looked like it should.

### The datasheet figures

| | Specification | Measured here |
| --- | --- | --- |
| RME Babyface Pro FS, line 3/4 | 116 dB RMS unweighted, 120 dBA | −116.47 dBFS floor, unconnected |
| Bela Gem: TI TLV320AIC3104, ADC | 92 dB SNR (typ) | never the limiting term; not measured |
| Bela Gem: TI TLV320AIC3104, DAC | 102 dBA SNR | −91.07 dBA floor at unity output gain |

The interface matches its specification almost exactly, which is the reassuring
half — it is a valid reference rather than a contributor.

The codec's figures are consistent with what was measured, and they correct one
of the readings above. A round trip at unity output gain puts the clean input
ceiling about 98 dBA above the floor, which is what a 102 dBA DAC in series
with a 92 dB ADC should give, give or take the precision available here. So the
board's converters are doing roughly what they claim.

**That means the "77 dB peak-to-noise" this section first arrived at was
measuring `safe-start`'s −18 dB output gain, not the board.** Attenuating by
18 dB before a converter with a fixed noise floor throws away 18 dB, and that
is all that figure was showing. The board is about 30 dB behind the interface,
not 40.

It changes nothing about the problem. The gain that matters is the upward
compressor's, it is applied before the output gain, and the noise it raises
scales with the output gain exactly as the signal does.

A register change is reported to buy the codec about 1.5 dB at the cost of
power. Unconfirmed, and 1.5 dB against a 12 dB shortfall does not change the
decision either way.

### It is broadband, and it is high-frequency

No hum, no tonal components — the strongest FFT bins sit within a few dB of
each other, which is a noise spectrum rather than a ground loop. A-weighted,
**about 80% of the audible energy is above 2.5 kHz**, and the low band is
below measurement resolution.

That matches how it is heard. At the point where it becomes objectionable, the
complaint is not "hiss" but that the high-frequency tail of a percussion hit
merges with the noise and pulls attention to it.

## What it costs

At the corrected operating point (Syntakt at full output, input gain −12 dB),
listening at a fixed monitor level, with the source stopped:

| `--upward` | A-weighted floor | Verdict |
| --- | --- | --- |
| `--depth 0` | −91.42 dBA | inaudible |
| 0 | −91.57 dBA | inaudible |
| 0.2 | −90.29 dBA | acceptable |
| 0.3 | **−88.14 dBA** | **acceptable; tails stay clean** |
| 0.4 | **−85.95 dBA** | **borderline; HF tails merge with the noise** |
| 1.0 (preset) | −76.28 dBA | unacceptable |

**The threshold is about −88 dBA**, and `safe-start`'s upward setting misses it
by 12 dB.

Correcting the input gain moved this a long way in the right direction. Before
it — with the input clipping — `--upward 0` was already borderline and 0.1 was
clearly audible. Clean, 0.3 is comfortable. Most of that came from removing
4 dB of peak clipping and 2.6 dB of input-stage noise.

## Where the budget goes

Decomposing the A-weighted floor by band at the corrected operating point:

| Setting | Low | Mid | High | Total |
| --- | --- | --- | --- | --- |
| upward 0 | −118.6 | −96.4 | −93.3 | −91.57 |
| upward 0.3 | −118.1 | −94.4 | −89.3 | −88.14 |
| upward 1.0 | below resolution | −85.9 | −76.8 | −76.28 |

**The low band produces no measurable noise even at full upward compression.**
The mid band consumes about a sixth of what the high band does. Turning the
global `upward` down is therefore the wrong instrument: it pays the high band's
bill out of all three bands' budgets.

`effective_up_amount = clamp(band.up_amount * upward, 0, 1)`, and `safe-start`
sets every band to 0.800, so a global 0.3 puts all three at 0.240.

Spending the same −88 dBA budget per band instead:

| Band | Global 0.3 | Per-band | Change |
| --- | --- | --- | --- |
| Low | 0.24 | **0.80** (full) | 3.3× |
| Mid | 0.24 | **0.45** | 1.9× |
| High | 0.24 | **0.15** | 0.6× |

Predicted floor −88.2 dBA, against −88.14 dBA for the global setting. Same
noise, substantially more upward compression where it is not the thing being
heard. This is a prediction from the band decomposition and has not been
listened to — the per-band amounts are preset data and are not reachable from
the command line.

## Views

These are opinions, not results.

**This is not a Bela defect, and it is not specific to Bela.** The comparison is
against an RME Babyface Pro FS, which is a considerably better converter than
anything embedded. An I2S HAT on a Raspberry Pi 5 would run into the same wall;
how far away that wall is depends on the HAT, and the ones ADR 0009 ranked
highest use better converters than the Gem's, so possibly not as close. No I2S
HAT was measured.

**The presets are calibrated for the development environment, not for a
board.** Nothing was wrong with that until now — there was one host. That the
same numbers do not survive a 30 dB drop in converter range is not a surprise
in hindsight.

**Only processing that raises the low-level region is affected.** Output gain
amplifies signal and noise alike and costs nothing. Distortion, delay, reverb,
filtering and downward compression are all unaffected. The board is a perfectly
good effects platform; it is upward compression specifically that it cannot
afford at these settings.

**"The same effect and the same signal-to-noise as the JACK host" is not
achievable.** The board is short by about 30 dB of converter range and no
amount of DSP recovers it. Something has to give, and the measurements say the
cheapest thing to give is high-band upward compression, because that is where
the noise is and it is not where most of the effect is.

## Undecided

- **Whether to add a preset with per-band upward amounts**, and what to call
  it. `bela` is concrete but misleading — the preset is for a converter with a
  high noise floor, not for a board. `ADR 0006` fixes the band values of
  `Default` as a compatibility contract, so a new preset is free to differ.
- **What the default input gain should be.** Inheriting the board's +16 dB
  clips line level and cannot stand. The right value depends on the source, and
  lowering it costs signal-to-noise one-for-one, so a conservative default is
  not free.
- **Whether the predicted per-band allocation actually sounds better** than the
  global 0.3 it is derived from.
- **Whether `riot` is worth designing for this board at all.**
