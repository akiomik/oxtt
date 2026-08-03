# Raspberry Pi 5 Verification: `oxtt`'s Physical Control Surface

This is the hardware verification for `oxtt`'s physical control surface — six
potentiometers on an MCP3008 SPI ADC driving `depth`/`time`/`upward`/`downward`
and the input/output gains, plus a latching bypass switch. It assumes the
environment from [`usb-audio-setup.md`](usb-audio-setup.md): the same Raspberry
Pi 5, the same native build, the same JACK server over a class-compliant USB
audio interface. It also assumes the surface itself is already assembled and
enabled per [`control-surface-setup.md`](control-surface-setup.md) — the wiring,
the electrical rules, enabling SPI0 on the 40-pin header, and confirming that
`/dev/spidev0.0` is genuinely the header's bus. The design being verified is
[ADR 0010](../decisions/0010-three-layer-control-surface-and-newest-value-handoff.md);
the guarantees it must satisfy are `docs/contracts.md` §8.

Concrete card names, host names, and port numbers are examples from the
validation environment (see [`usb-audio-setup.md`](usb-audio-setup.md));
substitute your own.

Two things are deliberately *not* verified here. The audio path itself is
covered by [`usb-audio-verification.md`](usb-audio-verification.md) and its
`128×3` baseline is reused unchanged; the pure conditioning logic (filter,
deadband, debounce, bypass override) is covered by the unit and property tests
in `src/control/mapping.rs` and needs no hardware. What this document verifies is
the part that only real hardware can show: what the assembled surface actually
reads, and whether the whole chain from a knob to the audio callback behaves as
`docs/contracts.md` §8 says it must.

## What here is still a verification of what ships

**The recorded results below were produced on the previous control surface: four
pots on CH0–CH3, and a momentary bypass switch whose bypass pinned `depth`
alone.** The panel has since changed on all three counts, so the functional
results no longer describe the shipped behaviour. They are kept rather than
deleted, because one of them is a measurement of hardware that did not change
and the rest record what was actually observed at the time.

| Section | Standing |
| --- | --- |
| [1. Wiring re-check](#1-wiring-re-check-with-oxtt-pi-tools) | **Superseded.** The recorded pass covers CH0–CH3 and a momentary switch. Re-run; the tool now reads CH0–CH5. |
| [2. Idle jitter](#2-idle-jitter) | **Stands.** Same pots, same wiring, same ADC; the conditioning constants still rest on these numbers. |
| [3. Live session](#3-live-session-with-jack-previous-control-surface) | **Superseded.** Every bypass pass criterion was written against behaviour that has been deliberately reversed. Re-run. |
| [4. Re-verification](#4-re-verification-required-for-the-current-surface) | **Outstanding.** The checks the current surface needs, none of them yet performed. |

The two gain pots have never been on a bench at all. `src/control/pi.rs` is
explicit about this: CH4 and CH5 are the same part, wired the same way, read by
the same code path as the four channels `oxtt-pi-tools` verified, but they
postdate that verification and nothing has confirmed them on hardware.

## Hardware under test

What the current surface is:

| Role | Reference configuration |
| --- | --- |
| ADC | MCP3008, single-ended, on SPI0/CE0 at 500 kHz, SPI mode 0 |
| Pots | Six linear-taper (B-curve) potentiometers, MCP3008 CH0–CH5 = Depth, Time, Upward, Downward, Input Gain, Output Gain |
| Unused ADC inputs | CH6 and CH7, tied to ground |
| Bypass switch | Latching (alternate-action) switch on GPIO17 (BCM), active low against the SoC's internal pull-up, no external resistor |
| Reference | 3.3 V from the Pi header, so full scale is 1023 counts |
| Assembly | Breadboard with jumper wiring, not an enclosure |

Sections 1 and 3 were recorded on the same rig with CH4–CH7 unwired and a
momentary push switch on GPIO17. Section 2 is unaffected by either difference.

SPI0 must be enabled on the Pi (`dtparam=spi=on`, `/dev/spidev0.0` present)
before any of this runs; `PiControls::new` fails at startup if it is not.

## Build

The control surface is behind the `pi-controls` Cargo feature, which is off by
default. On the Pi, in the repository:

```sh
cargo build --release --locked --features pi-controls
./target/release/oxtt --help
```

`--help` must list `--controls`. The flag does not exist without the feature, so
its presence is what confirms the right binary was built. `--controls` is opt-in
even in a `pi-controls` build, so this same binary still runs the audio-stability
scripts under `scripts/` exactly as before.

## 1. Wiring re-check with `oxtt-pi-tools`

**Superseded; re-run.** The pass recorded here covers CH0–CH3 and a momentary
switch, and the switch check below is worded for a part that springs back, which
the current one does not. `oxtt-pi-tools` now reads CH0–CH5 and reports the two
gain channels in dB, so a re-run does cover the gain pots — but no re-run has
happened yet.

`oxtt-pi-tools` is the standalone wiring-verification binary. It depends only on
`rppal`, not on `oxtt`, so it runs on a Pi with nothing else working, and
`src/control/pi.rs` reproduces its read byte for byte — same bus, same mode, same
clock, same three-byte conversation. Run it first, before involving JACK:

```sh
cargo run --release -p oxtt-pi-tools
```

What it printed at the time, one line every 200 ms:

```
Depth=991 (0.969) Time=1023 (1.000) Upward=1017 (0.994) Downward=1002 (0.980) Bypass=released
```

The tool has since been extended to CH0–CH5 and reworded for the latching
switch, so a re-run prints two more channels and says `engaged`/`disengaged`.
[`control-surface-setup.md`](control-surface-setup.md) step 6 carries the
current output and the current by-hand checks; run those rather than the
paragraph below, which is kept only to say what was checked here.

Confirm, by hand, that each pot moves its own channel across the full `0..=1023`
range end to end, that no channel moves when a different pot is turned, and that
`Bypass` follows the switch.

**Result on the previous surface: passed.** All four wired channels tracked
their own pot across the full range, and the switch read correctly in both
states.

That result carries over to CH0–CH3 unchanged — nothing about those four
channels was touched. What it does not cover is CH4, CH5, and a switch that
holds its position; see [section 4](#4-re-verification-required-for-the-current-surface).

## 2. Idle jitter

**Stands.** This measurement is unaffected by the panel change: the same pots,
in the same wiring, on the same ADC, read by the same code. It remains the
measurement the conditioning constants in `src/control/mapping.rs` rest on.

It is how much a *motionless* pot's reading wanders. It decides whether the
deadband is wide enough to keep a still knob silent without being so wide that a
deliberate turn feels coarse.

It was measured rather than assumed because the obvious prediction is wrong in a
checkable way. A potentiometer used as a divider has its highest source
impedance at mid travel, so mid travel is where a sampling ADC should be
noisiest — and that is not what the hardware does.

### Procedure

Two positions were captured, with the pots left completely untouched for the
duration of each: all four at **full travel** (against their upper end stop), and
all four at **mid travel**. Each capture is 300 readings per channel; at
`oxtt-pi-tools`' 200 ms poll interval that is 60 seconds per capture.

Take 300 readings and reduce them per channel — the first line of output is the
tool's banner, so it is dropped:

```sh
cargo run --release -p oxtt-pi-tools \
  | head -n 301 | tail -n 300 \
  | awk '
      {
        for (i = 1; i <= NF; i++) {
          if (split($i, kv, "=") == 2 && kv[1] ~ /^(Depth|Time|Upward|Downward)$/) {
            c = kv[1]; v = kv[2] + 0
            n[c]++; sum[c] += v; sq[c] += v * v
            if (n[c] == 1 || v < min[c]) min[c] = v
            if (n[c] == 1 || v > max[c]) max[c] = v
          }
        }
      }
      END {
        split("Depth Time Upward Downward", order, " ")
        for (i = 1; i <= 4; i++) {
          c = order[i]; mean = sum[c] / n[c]
          printf "%-9s n=%3d min=%4d max=%4d spread=%3d sd=%.2f\n",
                 c, n[c], min[c], max[c], max[c] - min[c], sqrt(sq[c] / n[c] - mean * mean)
        }
      }'
```

### Results

Counts are raw MCP3008 counts out of 1023.

| Position | Channel | n | min | max | spread | sd |
|---|---|---:|---:|---:|---:|---:|
| full travel | Depth | 300 | 991 | 1023 | 32 | 5.23 |
| full travel | Time | 300 | 991 | 1023 | 32 | 6.48 |
| full travel | Upward | 300 | 991 | 1023 | 32 | 5.74 |
| full travel | Downward | 300 | 991 | 1023 | 32 | 5.91 |
| mid travel | Depth | 300 | 508 | 529 | 21 | 3.33 |
| mid travel | Time | 300 | 495 | 528 | 33 | 4.09 |
| mid travel | Upward | 300 | 508 | 528 | 20 | 3.14 |
| mid travel | Downward | 300 | 495 | 528 | 33 | 4.03 |

**The worst case is at the end stop, not at mid travel** — the opposite of the
source-impedance prediction, and the reason the measurement was worth taking.
Full travel is worse on every channel by standard deviation (5.23–6.48 against
3.14–4.09), even though two mid-travel channels show a marginally *larger*
peak-to-peak spread (33 against 32). A peak-to-peak figure is one excursion out
of 300 and says little about how often the reading is out there, which is why
the deadband is judged against σ and not against the spread.

Worst-case σ ≈ 6.5 counts, which is what the constants are checked against.

### What it says about the shipped constants

Both constants were **kept unchanged**; the measurement changed only their
justification, from an assumption to a number.

- `FILTER_COEFFICIENT = 0.2` has a noise gain of exactly 1/3, so a raw σ of 6.5
  arrives at the deadband as σ ≈ 2.2 counts.
- `DEADBAND_COUNTS = 8.0` is therefore 3.7σ of what actually reaches it — enough
  margin that a motionless pot is quiet, while 8 counts is only ≈ 0.8% of travel
  and leaves roughly 128 distinct positions across a full sweep.

Because the noise gain is exactly 1/3, keeping three sigma of margin reduces to
`DEADBAND_COUNTS >= σ` of the *raw* jitter. `src/control/mapping.rs` records
that as the form to re-check against if the pots, the wiring, or the ADC change,
and carries the derivation; it is not repeated here. Against this measurement,
8.0 ≥ 6.5 holds with room to spare.

The two gain pots inherit that margin rather than establishing their own: they
are the same part in the same divider on the same converter, so the σ measured
above is the number they are judged against too, by construction rather than by
measurement. What the gain map adds is a second reading of the same deadband —
`src/control/mapping.rs` works `DEADBAND_COUNTS` out as `8 / 1023 * 48` ≈
0.375 dB across the pots' 48 dB span, which is well under the roughly 1 dB step
a listener picks out on programme material. If the gain channels turn out to be
noisier than CH0–CH3 when they are finally measured, this section is what has to
be re-taken, and `DEADBAND_COUNTS` re-checked in both its forms.

## 3. Live session with JACK (previous control surface)

**Superseded; re-run.** This session was played on four pots and a momentary
switch, and three of its pass criteria have since been deliberately reversed or
widened: the bypass now pins both gains as well as `depth`, a switch resting
bypassed at startup now comes up bypassed rather than un-bypassed, and there is
no press to toggle anything. The procedure and results are kept verbatim below
as the record of what was observed on that surface; what the current surface
needs is [section 4](#4-re-verification-required-for-the-current-surface).

The parts of it that do carry over — no zipper noise, no drift from a motionless
pot, `oxtt: xrun_count=0` and `oxtt: control_read_failures=0` on exit — are
re-listed in section 4 rather than counted as already passed, because they were
observed with two fewer channels being converted per poll.

### Procedure

1. Start JACK at the `128×3` baseline established by
   [`usb-audio-verification.md`](usb-audio-verification.md), replacing the card
   name with yours:

   ```sh
   jackd -R -d alsa -d hw:CARD=Pro73056544 -r 48000 -p 128 -n 3
   ```

2. In another session, start `oxtt` with the control surface and the exit
   report:

   ```sh
   ./target/release/oxtt --preset safe-start --controls --report-xruns-on-exit
   ```

3. Connect the ports (`jack_connect`, or the `oxtt-jack-tools` helpers) and feed
   real audio through the effect.

4. Play a sustained session: sweep all four pots end to end, slowly and quickly,
   individually and together; work the bypass switch repeatedly, including
   holding it down and releasing it; turn the Depth pot while bypassed and then
   disengage; leave all four pots motionless for extended stretches.

5. Stop with `SIGINT` (Ctrl-C) and read the exit report.

### Pass criteria as they then stood

The bypass criteria below no longer describe the shipped behaviour — there are
no press and release edges to distinguish, the bypass now pins both gains as
well as `depth`, and a switch resting bypassed at startup now comes up
bypassed. They are the criteria this session was actually judged against.

- `oxtt: xrun_count=0` and `oxtt: control_read_failures=0` on exit.
- No zipper noise, stepping, or other artefact while a pot is turning.
- No audible drift, chatter, or parameter movement from a motionless pot.
- One bypass toggle per press; release edges ignored.
- Time, Upward and Downward keep working while the bypass is engaged.
- Disengaging the bypass restores the Depth pot's *current* position, not the
  position it held when the bypass was engaged.
- A switch held down as the process starts leaves the run un-bypassed.

### Results

**All checks passed, on the surface as it then was.**

- The exit report was exactly `oxtt: xrun_count=0` and
  `oxtt: control_read_failures=0`. Every hardware read over the whole session
  succeeded, and the control thread never disturbed the audio callback's
  deadline.
- Pot sweeps were smooth and continuous, with no zipper noise at any sweep
  speed — consistent with the DSP re-smoothing every published parameter over
  20 ms on top of the conditioning in layer B.
- Motionless pots produced no audible movement at all, which is the deadband
  doing its job at the σ margin measured above.
- The bypass toggled exactly once per press. Releasing the switch never toggled
  it, so a press-and-release is one event rather than two.
- With the bypass engaged, the Time, Upward and Downward pots kept changing the
  effect, so releasing the bypass brought back an effect set up meanwhile rather
  than a stale one.
- Turning Depth while bypassed stayed silent, and disengaging the bypass
  brought the Depth pot's position *at that moment* — not its position when the
  bypass was engaged.
- Starting the process with the switch already held down came up un-bypassed,
  and holding it down further never toggled: the first reading is a baseline,
  not an edge.

### Regression check on the audio-stability harness

`scripts/pi-jack-usb-soak-test.sh` was re-run unchanged and still passes. This
is the check that the control-surface work did not disturb the exit report the
verification scripts depend on: the scripts match `oxtt: xrun_count=0` whole and
require exactly one such line, and the new `oxtt: control_read_failures=N` line
is emitted only when a control surface was attached — which the soak script,
running without `--controls`, never is.

## 4. Re-verification required for the current surface

**Not yet performed.** These are the checks the six-pot, latching-switch surface
needs before this document is again a verification of what ships. They subsume
sections 1 and 3: run them in one sitting, on the same `128×3` JACK baseline and
with the same exit report.

### Procedure

Sections 1 and 3's setup is unchanged — `oxtt-pi-tools` first, then JACK at
`128×3`, then:

```sh
./target/release/oxtt --preset safe-start --controls --report-xruns-on-exit
```

Feed real audio through the effect, and have a level meter on the input and the
output — the bypass checks below are level comparisons, not just listening.

Then play a sustained session: sweep all six pots end to end, slowly and
quickly, individually and together; throw the bypass switch repeatedly and leave
it in each position; turn the Depth and both gain pots while bypassed and then
disengage; leave all six pots motionless for extended stretches. Restart the
process at least once with the switch resting in each of its two positions.

### Checks

New, and none of them covered by anything recorded above:

- **Both gain pots sweep, and each drives its own gain.** CH4 moves the
  pre-split input gain and CH5 the post-sum output gain, each across its full
  travel, and neither moves any other parameter. `oxtt-pi-tools` confirms the
  channels move independently; that they reach the DSP is confirmed here by
  level.
- **Unity is at the centre of each gain pot's rotation.** With both gain pots
  centred, the effect's output level matches its input; the map is linear across
  `[-24, 24]` dB, so centre is 0 dB and both stops are the parameter's own
  limits.
- **Bypass pins all three fields.** Engaging the bypass produces the same output
  whatever the Depth, Input Gain and Output Gain pots are set to: `depth = 0`
  and both gains at unity. Set the three to extremes, engage, and confirm the
  output does not change with them.
- **The bypassed output does not exceed the input.** With both gain pots at full
  travel, engaging the bypass must bring the output down to the input's level,
  never above it. This is the guarantee in `docs/contracts.md` §8 that makes
  bypass a safe thing to reach for, and it is the one check here worth doing on
  a meter rather than by ear.
- **A switch resting bypassed at startup comes up bypassed.** Start the process
  with the switch already in the bypassed position and confirm the run is
  bypassed from the first snapshot — the reverse of what the previous surface
  did, and the reason the part was changed.
- **One throw produces exactly one state change.** A single throw of the switch
  must not bypass and un-bypass, or the reverse, as the contact makes and breaks
  through the movement. `BYPASS_DEBOUNCE_READS = 15` is 28 ms at the 2 ms poll
  interval, with up to 30 ms between the throw and the parameters following, and
  `src/control/mapping.rs` marks that number **provisional**: it is sized from
  the switch class, not measured on this part. This check is what re-derives it.
  Too small and a single throw publishes twice; the failure is visible in use.

Carried over from section 3, and re-listed because they were observed with two
fewer channels converted per poll:

- `oxtt: xrun_count=0` and `oxtt: control_read_failures=0` on exit.
- No zipper noise, stepping, or other artefact while a pot is turning, on all
  six pots.
- No audible drift, chatter, or parameter movement from a motionless pot.
- Time, Upward and Downward keep working while the bypass is engaged.
- Disengaging the bypass restores the Depth and both gain pots' *current*
  positions, not the positions they held when the bypass was engaged.
- `scripts/pi-jack-usb-soak-test.sh` still passes unchanged.

## Outstanding

**Pulling the MCP3008 mid-session was not performed.** The intended check is to
remove the ADC while `oxtt` is running and confirm the control surface survives
it: audio keeps flowing, the control thread keeps polling, and the parameters
hold their last good values.

It was deferred because the cabling around the ADC on the breadboard is too
dense to disturb safely while the rig is running — the realistic outcome of
attempting it is shorting something, not a clean observation.

Two things are worth recording about what this check would and would not
demonstrate:

- It exercises the **bad-input** path, not the error path. `rppal`'s `transfer`
  returns garbage bytes rather than an error when the device is absent, so a
  pulled MCP3008 produces plausible-looking counts, not `Err`. What it would
  show is that garbage readings cannot crash, hang, or silence the effect — the
  conditioning simply follows the garbage.
- The **error** path is already covered without hardware, by the unit tests in
  `src/control/thread.rs`: a source that fails some reads and then recovers is
  counted and survived, and a source that never succeeds neither panics nor
  publishes, leaving the callback on its last good snapshot.

Re-run this check when the surface moves off the breadboard into an enclosure
with a connector that can be unplugged safely.

## Completion criteria

Met, and unaffected by the panel change:

- A `pi-controls` release build on the Pi, with `--controls` present in
  `--help`.
- Idle jitter measured at both end stop and mid travel, 300 readings per channel
  per position, with worst-case σ recorded and checked against
  `DEADBAND_COUNTS`.
- `scripts/pi-jack-usb-soak-test.sh` still passing unchanged.

Met on the previous surface only, and not yet met for the current one:

- Wiring confirmed channel by channel with `oxtt-pi-tools`, including the switch
  in both states. Holds for CH0–CH3; CH4 and CH5 are not covered by the tool at
  all, and the switch check was written for a momentary part.
- A sustained live JACK session at 48 kHz `128×3` exercising every pot and the
  bypass switch, ending in `oxtt: xrun_count=0` and
  `oxtt: control_read_failures=0`, with every pass criterion met. Passed on four
  pots and a momentary switch; section 4 lists what the six-pot, latching-switch
  surface has to show instead.

Outstanding for both:

- The ADC-removal check, with its reason and its limited scope recorded above.
