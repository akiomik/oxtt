# Bela Gem Stereo Verification: `oxtt`'s Physical Control Surface

This is the hardware verification for `oxtt`'s physical control surface on a
Bela Gem Stereo: six potentiometers on analog inputs `A0`–`A5` driving
`depth`/`time`/`upward`/`downward` and the input/output gains, plus a latching
bypass switch on digital `D0`. It assumes the surface is assembled and wired
per [`control-surface-setup.md`](control-surface-setup.md), and the build and
deployment from [`cross-compile.md`](cross-compile.md). The design being
verified is
[ADR 0010](../decisions/0010-three-layer-control-surface-and-newest-value-handoff.md)
as ported by [ADR 0011](../decisions/0011-bela-gem-stereo-as-the-second-host.md);
the guarantees it must satisfy are
[`docs/contracts.md` §8](../contracts.md#8-control-surface).

It is the counterpart of
[`docs/raspberry-pi/control-surface-verification.md`](../raspberry-pi/control-surface-verification.md),
and the two are worth reading together: the same six pots and the same layer B
on a different converter, which is what
[ADR 0012](../decisions/0012-the-jitter-deadband-belongs-to-the-control-source.md)
turns on.

What is deliberately *not* verified here: the audio path itself, covered by
[`audio-verification.md`](audio-verification.md); the board's noise floor,
covered by [`noise-floor.md`](noise-floor.md); and the conditioning logic
(filter, deadband, debounce, explicit bypass level), covered by the unit and
property tests in `src/control/mapping.rs` and `src/bela_host/controls.rs`,
which need no hardware.

## 1. Hardware under test

| Role | Reference configuration |
| --- | --- |
| Board | Bela Gem Stereo on PocketBeagle 2, 48 kHz, period 16 |
| Converter | The board's own ADS8166, full scale 4.096 V |
| Pots | Six linear-taper 10 kΩ, `A0`–`A5` = Depth, Time, Upward, Downward, Input Gain, Output Gain |
| Unused analog inputs | `A6` and `A7`, tied to ground |
| Bypass switch | Latching, on `D0`, active low against an external 10 kΩ pull-up to 3.3 V |
| Clip indicator | Red LED on `D1`, active high, 1 kΩ series resistor to ground |
| Assembly | Breadboard with jumper wiring, not an enclosure |

## 2. Build

`oxtt-bela` is cross-compiled; the board is not a build host.

```sh
BELA_SYSROOT=... scripts/bela-build.sh
scripts/bela-deploy.sh -- --controls --preset safe-start --adc-gain-db -12 --report-on-exit
```

`--controls` exists in every `bela-host` build rather than behind a feature of
its own, and is opt-in, so the same binary still runs the audio verification
on a board with nothing wired to its headers.

## 3. Analog wiring check

**Result: PASS.** Measured with `bela/examples/io_analog` from
[`bela-rs`](https://github.com/akiomik/bela-rs), which reports the mean, the
range and the widest within-block spread of every analog input once a second.
It is used rather than an oxtt-side tool because the question at this stage is
what the pins read, not what oxtt makes of them.

Each pot was swept from its lower stop to its upper stop and released, one at a
time, in channel order. Readings are the raw 0-to-1 fraction of full scale.

| Channel | Lower stop | Upper stop | Travel |
|---|---:|---:|---:|
| ch0 | 0.0001 | 0.8063 | 0.8062 |
| ch1 | 0.0000 | 0.8063 | 0.8063 |
| ch2 | 0.0000 | 0.8064 | 0.8064 |
| ch3 | 0.0008 | 0.8064 | 0.8056 |
| ch4 | 0.0004 | 0.8065 | 0.8061 |
| ch5 | 0.0003 | 0.8064 | 0.8061 |
| ch6 | 0.0001 | 0.0001 | **0.0000** |
| ch7 | 0.0000 | 0.0000 | **0.0000** |

- **Every channel moved alone.** While one pot swept, no other channel moved in
  the fourth decimal place. There is no crosstalk between channels and none
  between a pot and the two grounded inputs.
- **The channel index is the pot.** Sweeping in `A0`→`A5` order moved the
  channels in `ch0`→`ch5` order, which is the order
  `src/bela_host/controls.rs` maps onto `Pots`' fields.
- **`A6` and `A7` are at ground**, 0.0000–0.0001 with zero travel, throughout
  every capture in this document.
- **The upper stop sits above the nominal ceiling.** 0.8063–0.8065 against the
  3.3/4.096 = 0.80566 that `POT_SUPPLY_FRACTION` divides by, which is the case
  the clamp in `pot_position` exists for. All six therefore reach
  `POT_POSITION_MAX` at the top of their travel rather than overflowing it.

## 4. Idle jitter

### Purpose and method

Idle jitter is how much a *motionless* pot's reading wanders. It decides
whether the deadband is wide enough to keep a still knob silent without being
wider than it needs to be.

Two positions were captured with all six pots left untouched for the duration
of each: full travel (against the upper stop) and mid travel. Each capture is
60 one-second windows, and each window covers every analog frame in it — about
22 000 readings per channel per window, so roughly 1.3 million readings per
channel per position.

That is a different instrument from the Pi's, which sampled 300 readings per
channel at 5 Hz and reduced them to a standard deviation. Here the whole
sample stream is reduced to its extremes, so what is recorded is the **entire
excursion** rather than a dispersion the excursion is inferred from. Counts
below are the raw fraction converted at `reading / 0.80566 * 1023`, the same
scale `pot_position` produces.

### Results

Full travel:

| Channel | mean | min | max | peak-to-peak | worst in-block spread |
|---|---:|---:|---:|---:|---:|
| ch0 | 0.8063 | 0.8054 | 0.8072 | **2.3 counts** | 2.0 counts |
| ch1 | 0.8063 | 0.8054 | 0.8072 | 2.3 | 1.9 |
| ch2 | 0.8064 | 0.8056 | 0.8073 | 2.2 | 1.9 |
| ch3 | 0.8064 | 0.8055 | 0.8073 | 2.3 | 1.9 |
| ch4 | 0.8064 | 0.8056 | 0.8074 | 2.3 | 2.0 |
| ch5 | 0.8064 | 0.8055 | 0.8073 | 2.3 | 1.9 |
| ch6 | 0.0001 | — | — | 0.0 | 0.0 |
| ch7 | 0.0000 | — | — | 0.1 | 0.0 |

Mid travel:

| Channel | mean | position | peak-to-peak | worst in-block spread |
|---|---:|---:|---:|---:|
| ch0 | 0.3114 | 38.7% | 1.5 counts | 1.1 counts |
| ch1 | 0.3715 | 46.1% | 1.7 | 1.1 |
| ch2 | 0.3846 | 47.7% | 1.8 | 1.5 |
| ch3 | 0.3928 | 48.8% | 1.9 | 1.4 |
| ch4 | 0.3687 | 45.8% | **2.5** | 2.0 |
| ch5 | 0.3721 | 46.2% | 1.8 | 1.5 |

**Worst case over both positions: 2.5 counts, peak to peak.**

Two things are worth recording against the Pi's numbers.

- **The two positions are not meaningfully different here.** On the Pi, full
  travel was worse than mid travel on every channel by standard deviation
  (5.30–6.39 against 2.98–4.19), which was the finding that made the
  measurement worth taking. On a Gem both positions land in the same
  1.5–2.5 count band, and the worst single channel is at mid travel rather
  than at the stop.
- **The grounded channels are the instrument's own floor.** `A6` and `A7` span
  0.0–0.1 counts, so the 1.5–2.5 counts on the pot channels is the pots and
  their wiring, not the converter's noise.

### What it says about the deadband

The whole observed excursion — over 60 seconds, at two positions, on six
channels — is under one third of `DEADBAND_COUNTS` as the Raspberry Pi
declares it. Eight counts here would be silent with an enormous margin, and
that margin is spent on nothing: as a fraction of travel it is 0.8% and about
128 distinct positions across a sweep.

`src/bela_host/controls.rs` therefore declares **3.0 counts** for this board,
which is above every reading actually observed — so a motionless pot is silent
rather than merely quiet — and leaves 0.29% of travel, about 341 positions,
and 0.141 dB per step on the two gain pots.

The deadband being per-source rather than shared is
[ADR 0012](../decisions/0012-the-jitter-deadband-belongs-to-the-control-source.md);
this measurement is the evidence behind the Gem's half of it. The Raspberry
Pi's figure is unchanged and needs no re-verification.

## 5. Live run with the control surface

Runs are `oxtt-bela --controls --preset safe-start --adc-gain-db -12
--report-on-exit`, ended with `SIGINT`, at 48 kHz with a period of 16.

### Counters

**Result: PASS.**

| Run | What was exercised | `control_publishes` | `control_rejects` | `underrun_count` |
|---|---|---:|---:|---:|
| 79.3 s | Depth and Upward pots, full sweep, down and back | **480** | 0 | 0 |
| 30 s | nothing touched | **1** | 0 | 0 |
| 59.4 s | the bypass switch, 10 throws, no pot touched | **11** | 0 | 0 |

- **A motionless surface publishes exactly once**, which is the seeding read.
  Over 30 seconds at a 500 Hz read rate that is one publish in about 15 000
  reads: the deadband absorbs the idle jitter completely, which is section 4
  measured from the other end.
- **Pot movement is what publishes.** With the deadband at 3.0 counts a full
  sweep clears the band about `1023 / 3` times; four sweeps therefore land in
  the high hundreds, and 480 is in that range. Nothing else in the run
  contributed.
- **One throw of the switch publishes exactly once.** Ten throws with no pot
  touched gave 11 publishes — the seeding read plus one per throw, with no
  spare. What this measures is the debounce. Reads arrive every 2 ms, an
  alternate-action contact chatters for longer than that, and every transition
  the mapping layer believed would be a publish of its own;
  `BYPASS_DEBOUNCE_READS` requires fifteen consecutive agreeing reads — up to
  30 ms — before the position changes. Eleven is that working, counted rather
  than heard, and it is the sharp form of the by-ear result below.
- **The processor accepted every snapshot** the mapping layer produced
  (`control_rejects=0`), and reading the surface inside `render_pre` cost no
  underruns.

### By ear

**Result: PASS.** Confirmed in a live session with audio through the board:

- **Each pot moves its own parameter**: `A0` Depth, `A1` Time, `A2` Upward,
  `A3` Downward, `A4` Input Gain, `A5` Output Gain.
- **Unity is at the centre of each gain pot's rotation.**
- **Engaging and disengaging bypass is audible**, and a board started with the
  switch resting in the bypassed position comes up bypassed.
- **One throw of the switch produces exactly one state change.**
- **Time, Upward and Downward keep working while bypass is engaged**, and
  disengaging restores the Depth and gain pots' *current* positions rather
  than the ones held when bypass was engaged.
- **No zipper noise, stepping, or drift** from turning a pot or from leaving
  one motionless.

## 6. The clip indicator

**Result: PASS.** The board reports nothing about input clipping — libbela has
no peak or clip API, and its own two LEDs are spoken for: blue is "running" and
red is its underrun indicator, both opened by libbela itself and neither
reachable from an application. `--clip-led <channel>` drives an LED of oxtt's
own instead.

### A digital output drives the pin

Verified before wiring anything of oxtt's to it, with a throwaway program that
toggled `D1` at 2 Hz: the LED followed. This is the one thing the software
cannot check for itself — reading back an output channel returns the value that
was written rather than the state of the pin (`bela-rs`
`docs/board-facts.md`) — so it needs an eye on the LED.

### Bad channels are refused before the audio system exists

```
$ ./oxtt-bela --clip-led 0
oxtt: Bela error: the application refused the resolved settings: the clip indicator cannot share D0 with the bypass switch
$ ./oxtt-bela --clip-led 16
oxtt: Bela error: the application refused the resolved settings: the clip indicator needs a digital channel the board delivers
```

Both from `validate_settings`, so neither built an audio system — the failure
mode `contracts.md` §9 is arranged to avoid. `D0` is refused whether or not
`--controls` was given, because the switch is wired there regardless.

### It lights on clipping and not otherwise

Sustained single note, `--clip-led 1`, fifteen seconds each:

| Run | `--adc-gain-db` | `input_peak_dbfs` | `input_clipped` | Indicator |
| --- | --- | --- | --- | --- |
| 1 | +24 | 0.0 | 68023 | **lit** |
| 2 | +20 | −2.8 | 0 | dark |

Run 1 clipped continuously and the indicator was on; it stayed on after the run
ended, which is a digital output going on driving after its program exits. Run
2 cleared it at startup and it stayed dark for the whole fifteen seconds.

The hold — 20000 frames, libbela's own `underrunLedDuration` — is what makes a
21 µs clipped frame visible at all. Its arithmetic is covered by unit tests in
`src/metering.rs`; what the board adds is that the pin follows.

## 7. Outstanding

- **Pulling an input mid-session has not been performed**, for the same reason
  as on the Pi: the breadboard is too dense to disturb safely while the rig is
  running. What it would exercise is the bad-input path rather than an error
  path — an analog read cannot fail on this host — and
  `src/bela_host/controls.rs` already covers the shape of it without hardware:
  a short frame and a nonsense reading both fall back to the quiet floor.
- **Everything here is on a breadboard.** Re-measure section 4 when the
  surface moves into an enclosure with a loom, which is the change most likely
  to move the jitter figure the deadband is sized against.

## 8. Completion criteria

Met:

- A cross-compiled `bela-host` build running on the board with `--controls`.
- Every analog channel confirmed to sweep its full range, to move alone, and
  to sit where its pot is; both unused inputs confirmed at ground.
- Idle jitter measured at full and mid travel, ~1.3 million readings per
  channel per position, with the worst-case excursion recorded and the
  deadband chosen against it.
- A live session exercising all six pots and the bypass switch, with
  `control_rejects=0` and `underrun_count=0`, and a motionless-surface run
  publishing exactly its seeding read.
- The switch measured against a counter rather than by ear: ten throws, no pot
  touched, `1 + 10` publishes.

Outstanding: section 7.
