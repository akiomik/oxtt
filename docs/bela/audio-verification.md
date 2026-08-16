# Bela Gem Stereo Verification: Audio Without a Control Surface

What a Bela Gem Stereo did with `oxtt-bela`, measured on the board. This is the
first milestone of the port ([ADR 0011](../decisions/0011-bela-gem-stereo-as-the-second-host.md)):
the DSP running under Bela's callbacks with its parameters from the command
line. The control surface has its own document
([`control-surface-verification.md`](control-surface-verification.md)), as it
does on the Raspberry Pi
([`raspberry-pi/control-surface-verification.md`](../raspberry-pi/control-surface-verification.md)).

For the setup these results come from, see [`cross-compile.md`](cross-compile.md).

## Environment

| | |
| --- | --- |
| Board | Bela Gem Stereo on PocketBeagle 2, 4 cores |
| Image | Bela Debian Bookworm, 2026-03-25 |
| Kernel | `6.12.49-ti-arm64-r55-evl-2` |
| `libbela.so` | dated 2026-03-25 with the image |
| Binary | `oxtt-bela`, release, cross-compiled per [`cross-compile.md`](cross-compile.md) |
| Enclosure | none — open board on a desk |
| Source | Elektron Syntakt, main out, from the input-gain section onward; nothing connected before it |
| Monitoring | RME Babyface Pro FS, recorded on the host; its own input floor is −116.5 dBFS |

`bela_daemon` was stopped for every run. Levels are quoted at the Babyface,
whose input gain was never established — absolute figures do not describe
levels inside the Bela, but ratios between two of them do, and every conclusion
rests on a ratio. How these were measured, including three ways of getting it
wrong, is in [noise-floor.md](noise-floor.md).

## Results

### Startup and shutdown — PASS

`oxtt-bela` runs on the board, resolves `libbela.so` from `/root/Bela/lib`,
brings an audio system up, and stops cleanly on `SIGTERM` with its exit report
on stderr.

### The board runs at the requested 48 kHz — PASS

`Settings::audio_sample_rate` reaches the hardware. An 11.4-second run reported
`audio_frames_elapsed=546608`, which is 11.39 s at 48 kHz. The board's default
of 44.1 kHz is excluded arithmetically rather than by assertion: 546608 frames
at 44100 Hz would be 12.4 s, longer than the 12-second timeout the run was
given.

This matters beyond the number. The read decimator divides the block rate down
to the rate the mapping layer's constants were calibrated at, and at 48 kHz
with a 16-frame period that division is exact (`contracts.md` §8).

### No underruns at any period size tested — PASS

Twelve-second runs, `--preset safe-start`, `--report-cpu 4`:

| Period | `underrun_count` | `cpu_percentage` |
| --- | --- | --- |
| 16 (default) | 0 | 18.8 |
| 64 | 0 | 19.6 |
| 128 | 0 | 18.3 |

### One render thread is enough — PASS

About 19% of one audio thread, flat across period sizes. `thread_count` is
pinned to 1 because the DSP's filters carry state across frames
([ADR 0011](../decisions/0011-bela-gem-stereo-as-the-second-host.md)), so the
question this answers is whether that pinning costs anything. At 19% it does
not: there is roughly five times the headroom the effect uses.

`cpu_percentage` is the reading at `cleanup`, not the maximum over the run —
libbela's counter reports the current cycle only. A worst-case figure would
need per-block accumulation, which is not currently done.

### A 60-second soak stays clean — PASS

`--preset riot`, the heaviest preset, `--report-cpu 4`:

```
oxtt: underrun_count=0
oxtt: audio_frames_elapsed=2946848
oxtt: cpu_percentage=18.9
```

2946848 frames is 61.4 s at 48 kHz. SoC temperature went from 48.8 °C to
49.5 °C over the run, on an open board with no enclosure and no heatsink.

That is the same footing ADR 0009 judged the Raspberry Pi 5's thermal fit on —
published power figures and an open board, not an enclosure measurement — so it
is a like-for-like reading rather than the easy half of a harder question. Two
things it does not cover: 60 seconds is not a soak, and if the pedal ends up in
an enclosure that restricts airflow, the figure has to be taken again in that
enclosure. Neither is a gate on the board, and neither requires the enclosure
to be a sealed one; ADR 0011 records why.

### Refusals are typed, and do not cost the process — PASS

Two configurations were refused before any audio system existed, each exiting 1
with its own message rather than a generic initialisation failure:

```
$ ./oxtt-bela --period 256 --report-cpu 4
oxtt: Bela error: CPU monitoring needs a period size of at most 128 frames, not 256: above that libbela renders on a separate thread from the one it measures

$ ./oxtt-bela --low-crossover 1000 --high-crossover 1500
oxtt: high_crossover_hz (1500) must be at least one octave above low_crossover_hz (1000)
```

The first is libbela's pre-init check reached through `bela`'s wrapper; the
second is oxtt's own, at parse time. Neither built an audio system, so neither
left the process unable to build one afterwards — the failure mode
`contracts.md` §9 is arranged to avoid.

### The control surface's code path runs — PASS (as a smoke test only)

With `--controls` and **nothing wired to the headers**, a 11.4-second run
reported:

```
oxtt: underrun_count=0
oxtt: control_publishes=1076
oxtt: control_rejects=0
```

Floating analog inputs are an antenna, so the readings move and the mapping
layer publishes; that is what makes this useful without hardware. It exercises
layer A, layer B and the handoff into the processors, under a real audio
callback, at the real read rate.

`control_rejects=0` is the result worth keeping: every snapshot the mapping
layer produced from essentially garbage input was accepted by the processor's
own validation. The mapping cannot produce an out-of-range parameter, whatever
it reads.

It says nothing about whether the right pot moves the right parameter, which
needs the wiring in [`control-surface-setup.md`](control-surface-setup.md) and
a document of its own.

### Audio passes through — PASS

Input reaches output and the effect is recognisably doing its job.

### The board's default input gain clips a line-level source — FAIL

`Bela_defaultSettings` sets `audioInputGains` to +16 dB and `oxtt-bela` leaves
it alone unless `--adc-gain-db` says otherwise. With an Elektron Syntakt at
half output, sweeping the input gain across a unity DSP path:

| Input gain | RMS step (per +3 dB) | Crest factor |
| --- | --- | --- |
| +1 → +4 → +7 | +2.96, +2.96 | 15.2, 15.0, 15.1 |
| +10 | +3.00 | 13.6 |
| +13 | +2.92 | 13.1 |
| +16 (default) | +2.24 | 12.1 |

A crest factor falling from 15 dB to 12 dB while the RMS step falls short of
linear is peak clipping. The clean ceiling is +7 dB at half output, and about
−12 dB at full output.

`--adc-gain-db` must therefore be set for the source. What the default should
be is undecided ([noise-floor.md](noise-floor.md)). At the time this was
measured nothing reported the clipping and it was visible only as squashed
peaks; the section below is what changed that.

### The input meter tracks the input and pins at full scale — PASS

`oxtt: input_peak_dbfs` and `oxtt: input_clipped` exist so that
`--adc-gain-db` can be set from a number instead of inferred from a crest
factor. Twelve-second runs, `--preset safe-start`, source an Elektron Syntakt
at full main output playing a single-note loop — chosen over a pattern so that
the peak is the same from one run to the next and the runs can be compared.

| `--adc-gain-db` | `input_peak_dbfs` | Step | `input_clipped` |
| ---: | ---: | ---: | ---: |
| −12 | −18.4 | — | 0 |
| −9 | −15.3 | +3.1 | 0 |
| −6 | −12.4 | +2.9 | 0 |
| −3 | −9.3 | +3.1 | 0 |
| 0 | −6.3 | +3.0 | 0 |
| +3 | −3.3 | +3.0 | 0 |
| +6 | −0.3 | +3.0 | 0 |
| +9 | 0.0 | — | 140099 |
| +12 | 0.0 | — | 266526 |
| +16 | 0.0 | — | 324508 |

- **The peak tracks the input one for one.** Eighteen decibels in 3 dB steps,
  every step within 0.1 dB of the gain that produced it. A second ladder on a
  quieter source tracked the same way from −12 dB to +16 dB.
- **It pins at full scale rather than running past it.** From +9 dB upward the
  peak is 0.0 dBFS and stays there however much more gain is asked for, while
  the clipped count keeps rising — 324508 of the run's 576000 frames at the
  board's default, which is 56% of it.
- **The clean ceiling for this source is +6 dB**, 0.3 dB below full scale.
  The board's default of +16 dB overdrives it by 10 dB.
- **Below −12 dB the codec stops responding.** −24, −18 and −12 all produce
  the same level, and −12 → −9 is the first step that moves. That is a lower
  bound on what `--adc-gain-db` can usefully be asked for; `bela-rs`'s
  `board-facts.md` records libbela's own note that its decibel-to-register
  conversion is approximate below −18 dB, and this is where it stops mattering
  in practice.

**The clean ceiling is a property of the material, not only of the gear.** The
section above measures about −12 dB for the same instrument at the same output
setting playing a pattern; this one measures +6 dB for a single note. Eighteen
decibels apart, from one source, which is the case against any fixed default
and the reason this figure is reported rather than chosen once.

### The noise floor does not move with the input gain — CONFIRMED

Source connected and silent:

| Input gain | RMS | A-weighted |
| --- | --- | --- |
| −12 dB | −86.76 | −91.69 dBA |
| 0 dB | −86.74 | −91.44 dBA |
| +16 dB | −84.19 | −88.03 dBA |

Identical within 0.02 dB between −12 dB and 0 dB, so the dominant noise is
downstream of the input gain. The rule that follows: set the input gain as high
as the source allows without clipping.

The output gain was ruled out the same way. With the effect out of the way it
buys nothing — the floor moves 0.5 dB across 18 dB of output gain, because what
`--depth 0` shows is the DAC rather than the ADC. With the effect running the
floor scales 1:1 with it, because by then the noise is the ADC's raised about
31 dB by the upward compressor. Either way the ratio does not move.

The converters measure roughly what TI claims for them: about 98 dBA from the
clean input ceiling to the floor at unity output gain, against a 92 dB ADC and
a 102 dBA DAC. The board is about 30 dB behind the interface it is measured
with, and that gap is the whole of the problem
([noise-floor.md](noise-floor.md)).

### The effect raises the noise floor into audibility — CONFIRMED

At the corrected operating point (Syntakt at full output, `--adc-gain-db -12`),
source stopped, fixed monitor level:

| `--upward` | A-weighted floor | Listening verdict |
| --- | --- | --- |
| `--depth 0` | −91.42 dBA | inaudible |
| 0 | −91.57 dBA | inaudible |
| 0.2 | −90.29 dBA | acceptable |
| 0.3 | −88.14 dBA | acceptable; tails stay clean |
| 0.4 | −85.95 dBA | borderline; HF tails merge with the noise |
| 1.0 (`safe-start`) | −76.28 dBA | unacceptable |

**`safe-start` as written does not work on this board**, missing the roughly
−88 dBA threshold by 12 dB. The mechanism, the band decomposition and the
options are in [noise-floor.md](noise-floor.md).

## Not verified

- **Round-trip latency.** The reason for choosing this board over a Raspberry Pi
  5 is roughly 1 ms against the Pi's measured 11.4 ms (ADR 0008, ADR 0009), and
  that claim is still the vendor's rather than this project's.
- **The per-band upward allocation.** [noise-floor.md](noise-floor.md) predicts
  that spending the same noise budget per band rather than globally buys back
  most of the low and mid upward compression. The per-band amounts are preset
  data and are not reachable from the command line, so the prediction has not
  been listened to.
- **`OttApplication::validate_settings`'s own refusals.** All three — more than
  one render thread, too few analog or digital channels, a crossover pair above
  the Nyquist-relative limit — are unreachable from the shipped command line.
  `oxtt-bela` does not pass arguments to libbela, so nothing can override the
  thread count or the channel counts; and `OttProcessor::new` validates the
  same parameters against the same sample rate before the audio system is
  built, so the crossover check always fires there first. The checks are
  defensive against a board that does not deliver what was asked for, and
  against a future that passes arguments through. They have not been observed
  firing.
- **Thermal behaviour over a long run, and inside whatever enclosure the pedal
  gets**, as above. The open-board reading stands; what is missing is duration,
  and a re-measurement once the mechanical form is decided.
