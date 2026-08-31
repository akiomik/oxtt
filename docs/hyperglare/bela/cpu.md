# What hyperglare costs on a Bela Gem Stereo

Measured on a PocketBeagle 2 (AM6254, Cortex-A53), Bela Debian Bookworm image
2026-03-25, at 48 kHz with a period of 16 and one render thread — the settings
`hyperglare-bela` asks for. Runs of 11 to 19 seconds, `--report-cpu 4`, first with
nothing connected to the input and then with music into it at `--adc-gain-db
12`, which peaked at −14 dBFS without clipping.

**This is the measurement ADR 0016 and ADR 0020 were accepted without.** Both
delegated a number to "the CPU decides", and neither could be checked because
`hyperglare` had never been on a board.

## It runs

```text
 settings                                        resonators  CPU %  underruns
 defaults (5 notes, octaves, 5 kHz)                      32   13.5          0
 the same at a 9 kHz ceiling                             36   14.1          0
 seven notes at 9 kHz                                    50   15.9          0
 harmonics, three notes                                  62   18.6          0
 octave pairs, five notes at 9 kHz                       72   23.0          0
 harmonics at capacity                                  256   51.1          0
```

Six more runs with signal at the input measured the same, and are below.

**No configuration underran, including the one that fills the bank.** The
defaults sit at 13.5%, below `oxtt`'s measured 19% on the same board.

## What a resonator costs

Least squares over the five rows below capacity:

```text
 CPU% = 5.8 + 0.222 × resonators        residual under 1.2 points
```

- **5.8% is the fixed half**: the band split's biquads, the exciter's gates,
  the host, and everything else that does not scale with the chord.
- **0.222% each** is one normalised state-variable filter per sounding
  resonator, at 48 kHz.

Extrapolated, one core runs out at about 424 resonators — which the bank's
capacity of 256 already sits under. At capacity the line predicts 62.7% and the
board measured 51.1%, so the estimate is conservative in the direction that
matters.

## What this settles, and what it does not

**Settled: the band count is affordable.** The sweep ran with seven bands,
which is twelve biquads, and they were inside the 5.8% fixed cost against
0.222% for each of the resonators they feed. ADR 0016 left the count open
partly on CPU grounds; on this board that is not the binding constraint.

**The band count has since gone to six** — ten biquads and six gates —
[ADR 0022](../../decisions/0022-the-band-ladder-stops-at-two-kilohertz.md).
Two biquads out of a fixed term that also carries the host is a small part of
it, and it can only have gone down, so the 5.8% above is an upper bound for
the current build rather than a measurement of it.

**Settled: the ceiling is affordable.** Raising it from 5 kHz to 9 kHz adds
four resonators and 0.6 points. ADR 0020's choice was made on sound, and
nothing here argues with it.

**Settled: the load does not depend on the input.** The first sweep ran on a
board with nothing plugged in, so the gate that multiplies the noise path never
opened. Repeated with music at the input, peaking at −14 dBFS:

```text
 settings                          resonators   silent    signal
 defaults                                  32     13.5      13.5
 octave pairs at 9 kHz                     72     23.0      23.2
 harmonics at capacity                    256     51.1      47.9
```

Within a third of a point, and the largest configuration measured *lower* with
signal than without. The filters run whatever arrives and the gate is a
multiplication that happens either way, so this is what the arithmetic said it
would be — but it was worth measuring rather than asserting, because a bank
that only ran cheaply into silence would be no use.

**Not settled: latency, or the audio.** The output was disconnected for these
runs. What they prove is that the DSP keeps up, not what it sounds like.

## Reproducing

```sh
export BELA_SYSROOT=/path/to/bela-sysroot
export BELA_PACKAGE=hyperglare-bela
scripts/bela-build.sh
scripts/bela-deploy.sh -- --report-cpu 4 --report-on-exit --adc-gain-db 0
```

`--adc-gain-db` is not optional with a source connected: the board's default of
+16 dB clips a line-level input (`docs/oxtt/bela/noise-floor.md`, which is
about the board rather than about `oxtt`). Measured on one source, the codec
follows the request one for one and clips nowhere in the usable range:

```text
 --adc-gain-db     -12     -6      0      6     12
 input peak      -39.2  -33.6  -27.6  -21.8  -14.8   dBFS, none clipped
```

That source never came near full scale, so the ceiling above +12 dB is
untested; find it per source with `--report-on-exit` rather than assuming this
one.
