# What hyperglare costs on a Bela Gem Stereo

Measured on a PocketBeagle 2 (AM6254, Cortex-A53), Bela Debian Bookworm image
2026-03-25, at 48 kHz with a period of 16 and one render thread — the settings
`hyperglare-bela` asks for. `bela_daemon` stopped, output disconnected, music
at the input at `--adc-gain-db 0`, runs of 14 seconds with `--report-cpu 4`.

**Re-measured after [ADR 0022](../../decisions/0022-the-band-ladder-stops-at-two-kilohertz.md)
took the band count from seven to six**, and reaching further up: the first
sweep spanned 32 to 72 resonators and extrapolated badly past them, which this
one shows and corrects.

**This is the measurement ADR 0016 and ADR 0020 were accepted without.** Both
delegated a number to "the CPU decides", and neither could be checked because
`hyperglare` had never been on a board.

## It runs

Repeats of the same configuration, so the spread is visible rather than
implied:

```text
 settings                              resonators   runs   min  median   max
 defaults (5 notes, octaves, 5 kHz)            32     11  12.5    13.3  15.4
 the same at a 9 kHz ceiling                   36      1  13.1    13.1  13.1
 seven notes at 9 kHz                          50      4  14.8    16.3  19.6
 harmonics, three notes                        60      1  17.0    17.0  17.0
 octave pairs, five notes at 9 kHz             72      7  18.5    18.5  19.7
 harmonics, three low notes                   194      4  37.0    37.0  39.5
 harmonics at capacity                        256      4  44.9    47.8  47.8
```

**No configuration underran, including the one that fills the bank.** The
defaults sit around 13%, below `oxtt`'s measured 19% on the same board.

**The spread is up to 4.8 points**, and it is not proportional to the load —
72 resonators repeated to within 1.2 points while 50 varied by 4.8. Whatever
produces it is the board's, not the chord's. A single run of this rig is worth
about ±2 points, which is the resolution every number here has.

## What a resonator costs

Least squares over all seven configurations, 32 to 256 resonators:

```text
 CPU% = 7.9 + 0.15 × resonators         residual under 0.7 points
```

Fitted to the medians; fitting the minima instead moves the slope to 0.147 and
leaves the intercept where it is, so the line does not depend on which end of
the spread is taken.

- **7.9% is the fixed half**: the band split's biquads, the exciter's gates,
  the host, and everything else that does not scale with the chord.
- **0.15% each** is one normalised state-variable filter per sounding
  resonator, at 48 kHz.

Extrapolated, one core runs out at about 600 resonators, well past the bank's
capacity of 256.

**The first sweep's line was `5.8 + 0.222` and it was wrong past its own
data.** It was fitted over 32 to 72 resonators and predicted 62.7% at capacity
against 44.9 to 47.8 measured. Over a 40-resonator span the intercept and the
slope trade off almost freely, so neither was determined; extending the sweep
to 256 is what fixes it. The difference between the two lines is a difference
in the fit, not in the board.

## What this settles, and what it does not

**Settled: the band count is affordable.** Six bands are ten biquads and six
gates, and they are inside the fixed term against 0.15% for each of the
resonators they feed. ADR 0016 left the count open partly on CPU grounds; on
this board that is not the binding constraint.

**Not settled, and not settleable here: what one band costs.** This sweep was
run to see what ADR 0022's seventh band was worth, and it cannot say. Two
biquads out of a term that also carries the host is smaller than the 4.8
points the same configuration varies by between runs. Matched configurations
measured 0 to 3.3 points lower than the seven-band sweep, in the direction
removing work should go, and that range straddles the noise. **Any number
attributed to a band from these figures would be read out of the variation.**

**Settled: the ceiling is affordable.** Raising it from 5 kHz to 9 kHz adds
four resonators and 0.6 points. ADR 0020's choice was made on sound, and
nothing here argues with it.

**Settled: the load does not depend on the input.** From the seven-band sweep,
which ran first on a board with nothing plugged in, so the gate that multiplies
the noise path never opened, and then with music at the input peaking at
−14 dBFS:

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

**Repeat every configuration.** One run is worth about ±2 points here, so a
difference smaller than that is not a difference. The configurations above,
in the order of the table:

```text
 --notes 56,59,61,63,66
 --notes 56,59,61,63,66 --high-hz 9000
 --notes 50,52,54,56,59,61,63 --high-hz 9000
 --notes 56,59,63 --geometry harmonics
 --notes 56,59,61,63,66 --geometry octave-pairs --high-hz 9000
 --notes 33,40,45 --geometry harmonics
 --notes 28,33,40 --geometry harmonics
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
