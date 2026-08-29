# What hyperglare costs on a Bela Gem Stereo

Measured on a PocketBeagle 2 (AM6254, Cortex-A53), Bela Debian Bookworm image
2026-03-25, at 48 kHz with a period of 16 and one render thread — the settings
`hyperglare-bela` asks for. Runs of 11 to 19 seconds, `--report-cpu 4`, nothing
connected to the input.

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

**No configuration underran, including the one that fills the bank.** The
defaults sit at 13.5%, below `oxtt`'s measured 19% on the same board.

## What a resonator costs

Least squares over the five rows below capacity:

```text
 CPU% = 5.8 + 0.222 × resonators        residual under 1.2 points
```

- **5.8% is the fixed half**: the band split's twelve biquads, the exciter's
  seven gates, the host, and everything else that does not scale with the
  chord.
- **0.222% each** is one normalised state-variable filter per sounding
  resonator, at 48 kHz.

Extrapolated, one core runs out at about 424 resonators — which the bank's
capacity of 256 already sits under. At capacity the line predicts 62.7% and the
board measured 51.1%, so the estimate is conservative in the direction that
matters.

## What this settles, and what it does not

**Settled: the band count is affordable.** Seven bands are twelve biquads and
they are inside the 5.8% fixed cost, against 0.222% for each of the resonators
they feed. ADR 0016 left the count open partly on CPU grounds; on this board
that is not the binding constraint.

**Settled: the ceiling is affordable.** Raising it from 5 kHz to 9 kHz adds
four resonators and 0.6 points. ADR 0020's choice was made on sound, and
nothing here argues with it.

**Not settled: what it sounds like on this board.** The input peaked at
−70 dBFS in every run, which is a board with nothing plugged into it. The
resonator loop runs regardless — the filters are not gated — but the noise
path is multiplied by a gate that never opened, so what these numbers do *not*
include is whatever that path costs when it is doing something. It should be
nothing, since the multiplication happens either way, but it has not been
measured.

**Not settled: latency, or the audio.** No signal has been through this. What
the run proves is that the DSP keeps up.

## Reproducing

```sh
export BELA_SYSROOT=/path/to/bela-sysroot
export BELA_PACKAGE=hyperglare-bela
scripts/bela-build.sh
scripts/bela-deploy.sh -- --report-cpu 4 --report-on-exit --adc-gain-db 0
```

`--adc-gain-db` is not optional with a source connected: the board's default of
+16 dB clips a line-level input (`docs/oxtt/bela/noise-floor.md`, which is
about the board rather than about `oxtt`).
