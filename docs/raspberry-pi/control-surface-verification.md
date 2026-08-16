# Raspberry Pi 5 Verification: `oxtt`'s Physical Control Surface

This is the hardware verification for `oxtt`'s physical control surface: six
potentiometers on an MCP3008 SPI ADC driving `depth`/`time`/`upward`/`downward`
and the input/output gains, plus a latching bypass switch. It assumes the
environment from [`usb-audio-setup.md`](usb-audio-setup.md) — the same
Raspberry Pi 5, the same native build, the same JACK server over a
class-compliant USB audio interface — and it assumes the surface itself is
already assembled and enabled per
[`control-surface-setup.md`](control-surface-setup.md): the wiring, the
electrical rules, enabling SPI0 on the 40-pin header, and confirming that
`/dev/spidev0.0` is genuinely the header's bus. The design being verified is
[ADR 0010](../decisions/0010-three-layer-control-surface-and-newest-value-handoff.md);
the guarantees it must satisfy are `docs/contracts.md` §8.

Concrete card names, host names, and port numbers are examples from the
validation environment (see [`usb-audio-setup.md`](usb-audio-setup.md));
substitute your own.

Two things are deliberately *not* verified here. The audio path itself is
covered by [`usb-audio-verification.md`](usb-audio-verification.md) and its
`128×3` baseline is reused unchanged; the pure conditioning logic (filter,
deadband, debounce, explicit bypass-level transport) is covered by the unit and property tests
in `src/control/mapping.rs` and needs no hardware. What this document verifies
is the part that only real hardware can show: what the assembled surface
actually reads, and whether the whole chain from a knob to the audio callback
behaves as `docs/contracts.md` §8 says it must.

## 1. Hardware under test

| Role | Reference configuration |
| --- | --- |
| ADC | MCP3008, single-ended, on SPI0/CE0 at 500 kHz, SPI mode 0 |
| Pots | Six linear-taper (B-curve) potentiometers, MCP3008 CH0–CH5 = Depth, Time, Upward, Downward, Input Gain, Output Gain |
| Unused ADC inputs | CH6 and CH7, tied to ground |
| Bypass switch | Latching (alternate-action) switch on GPIO17 (BCM), active low against the SoC's internal pull-up, no external resistor |
| Reference | 3.3 V from the Pi header, so full scale is 1023 counts |
| Assembly | Breadboard with jumper wiring, not an enclosure |

SPI0 must be enabled on the Pi (`dtparam=spi=on`, `/dev/spidev0.0` present)
before any of this runs; `PiControls::new` fails at startup if it is not.

## 2. Build

The control surface is behind the `pi-controls` Cargo feature, which is off by
default. On the Pi, in the repository:

```sh
cargo build --release --locked --features pi-controls
./target/release/oxtt --help
```

`--help` must list `--controls`. The flag does not exist without the feature,
so its presence is what confirms the right binary was built. `--controls` is
opt-in even in a `pi-controls` build, so this same binary still runs the
audio-stability scripts under `scripts/` exactly as before.

## 3. Wiring check

**Result: PASS.** `oxtt-pi-tools` is the standalone wiring-verification
binary. It depends only on `rppal`, not on `oxtt`, so it runs on a Pi with
nothing else working, and `src/control/pi.rs` reproduces its read byte for
byte — same bus, same mode, same clock, same three-byte conversation. Run it
before building `oxtt` and before involving JACK:

```sh
cargo run --release -p oxtt-pi-tools
```

It prints one line every 200 ms:

```
Depth=991 (0.969) Time=1023 (1.000) Upward=1017 (0.994) Downward=1002 (0.980) InputGain=512 (+0.0 dB) OutputGain=300 (-9.9 dB) Bypass=disengaged
```

All six channels tracked their own pot across the full `0..=1023` range end to
end; no channel moved when a different pot was turned; both gain pots reached
their extremes, -24.0 dB and +24.0 dB, at their stops; and the latching switch
held whichever position it was left in (`engaged`/`disengaged`) without
spontaneously reverting.

## 4. Idle jitter

### Purpose

Idle jitter is how much a *motionless* pot's reading wanders. It decides
whether the deadband is wide enough to keep a still knob silent without being
so wide that a deliberate turn feels coarse.

It is measured rather than assumed because the obvious prediction is wrong in
a checkable way. A potentiometer used as a divider has its highest source
impedance at mid travel, so mid travel is where a sampling ADC should be
noisiest — and that is not what the hardware does.

### Procedure

Two positions were captured, with all six pots left completely untouched for
the duration of each: full travel (against the upper end stop) and mid travel
(centred). Each capture is 300 readings per channel, taken together across all
six channels rather than one pot at a time; at `oxtt-pi-tools`'s 200 ms poll
interval that is 60 seconds per capture.

`scripts/pi-idle-jitter.sh` captures one position and reduces it to
n/min/max/spread/sd per channel. Position all six pots, leave them untouched,
then run it (redirect or `tee` to save the output):

```sh
scripts/pi-idle-jitter.sh
```

Run it once per position.

### Results

Counts are raw MCP3008 counts out of 1023.

Full travel:

| Channel | n | min | max | spread | sd |
|---|---:|---:|---:|---:|---:|
| Depth | 300 | 991 | 1023 | 32 | 6.39 |
| Time | 300 | 991 | 1023 | 32 | 5.30 |
| Upward | 300 | 991 | 1023 | 32 | 5.35 |
| Downward | 300 | 991 | 1023 | 32 | 6.23 |
| InputGain | 300 | 991 | 1023 | 32 | 5.65 |
| OutputGain | 300 | 991 | 1023 | 32 | 5.79 |

Mid travel:

| Channel | n | min | max | spread | sd |
|---|---:|---:|---:|---:|---:|
| Depth | 300 | 367 | 383 | 16 | 3.21 |
| Time | 300 | 380 | 400 | 20 | 2.98 |
| Upward | 300 | 439 | 462 | 23 | 3.43 |
| Downward | 300 | 508 | 533 | 25 | 4.19 |
| InputGain | 300 | 446 | 465 | 19 | 3.21 |
| OutputGain | 300 | 447 | 473 | 26 | 3.25 |

**The worst case is at the end stop, not at mid travel** — the opposite of the
source-impedance prediction, and the reason the measurement was worth taking.
Full travel is worse than mid travel on every channel by standard deviation
(5.30–6.39 against 2.98–4.19). A peak-to-peak spread figure is one excursion
out of 300 readings and says little about how often the reading is actually
out there, which is why the deadband is judged against σ and not against the
spread.

Worst-case σ ≈ 6.39 counts (Depth, full travel), which is what the constants
are checked against.

### What it says about the shipped constants

- `FILTER_COEFFICIENT = 0.2` has a noise gain of exactly 1/3, so a raw σ of
  6.39 arrives at the deadband as σ ≈ 2.13 counts.
- `PiControls::DEADBAND_COUNTS = 8.0` is therefore roughly 3.8σ of what
  actually reaches it — enough margin that a motionless pot is quiet, while 8
  counts is only ≈ 0.8% of travel and leaves roughly 128 distinct positions
  across a full sweep.

Because the noise gain is exactly 1/3, keeping three sigma of margin reduces
to `deadband >= σ` of the *raw* jitter. `src/control/mapping.rs` records that
as the form to re-check against if the pots, the wiring, or the ADC change,
and carries the derivation; it is not repeated here. Against this measurement,
8.0 ≥ 6.39 holds with room to spare.

The value itself lives in `src/control/pi.rs`, next to the hardware it was
measured on, rather than in the mapping layer: it is a property of this
converter and not of the conditioning, and a second host measured an order of
magnitude quieter
([ADR 0012](../decisions/0012-the-jitter-deadband-belongs-to-the-control-source.md),
[`docs/bela/control-surface-verification.md`](../bela/control-surface-verification.md)).
Nothing about this measurement or the figure it justifies changed with that
move.

All six channels are the same part in the same divider on the same converter,
so this measurement judges all of them, gain pots included. On the two gain
pots the deadband also lands on a dB figure: `src/control/mapping.rs` works it
out as `8 / 1023 * 48` ≈ 0.375 dB across the pots' 48 dB span, which is well
under the roughly 1 dB step a listener picks out on programme material.

## 5. Live JACK session

### Procedure

1. Start JACK at the `128×3` baseline established by
   [`usb-audio-verification.md`](usb-audio-verification.md), replacing the
   card name with yours:

   ```sh
   jackd -R -d alsa -d hw:CARD=Pro73056544 -r 48000 -p 128 -n 3
   ```

2. In another session, start `oxtt` with the control surface and the exit
   report:

   ```sh
   ./target/release/oxtt --preset safe-start --controls --report-xruns-on-exit
   ```

3. Connect the ports (`jack_connect`, or the `oxtt-jack-tools` helpers) and
   feed real audio through the effect, with a level meter on the input and the
   output.

4. Play a sustained session: sweep all six pots end to end, slowly and
   quickly, individually and together; throw the bypass switch repeatedly and
   leave it in each position; turn the Depth and both gain pots while bypassed
   and then disengage; leave all six pots motionless for extended stretches.
   Restart the process at least once with the switch resting in each of its
   two positions.

5. Stop with `SIGINT` (Ctrl-C) and read the exit report.

### Results

The original control-surface checks passed. The bypass-level observations below
describe the superseded gain/depth-transition implementation; do not treat
them as verification of the current phase-coherent crossfade.

- **Both gain pots sweep independently.** CH4 and CH5 each moved only their
  own parameter — confirmed both via `oxtt-pi-tools` (independent channel
  movement) and via the audio interface's output level meter.
- **Unity is at the centre of each gain pot's rotation.** With both gain pots
  centred, the output level matched the input level on the audio interface's
  level meter.
- **Superseded: bypass pinned all three fields.** The former gain/depth
  implementation forced Depth, Input Gain, and Output Gain to its bypass
  endpoint. This is no longer its design; the current bypass is the unity
  crossover reconstruction and leaves the latent effect targets current.
- **A switch resting bypassed at startup comes up bypassed.** Confirmed.
- **One throw of the switch produces exactly one state change.** Confirmed;
  `BYPASS_DEBOUNCE_READS = 15` needed no adjustment.
- **No zipper noise, stepping, or other artefact** while turning any of the
  six pots — confirmed by ear.
- **No audible drift, chatter, or parameter movement** from a motionless pot —
  confirmed by ear.
- **Time, Upward, and Downward keep working while bypass is engaged.**
  Confirmed.
- **Disengaging bypass restores the Depth and both gain pots' *current*
  position**, not the position held when bypass was engaged. Confirmed.
- **Exit report.** `oxtt: xrun_count=0` and `oxtt: control_read_failures=0`.

### Required bypass-transition follow-up

The Pi check of the superseded gain/depth transition found a severe level dip,
up to near-silence, in both effect-to-bypass and bypass-to-effect throws. The
new phase-coherent crossfade has not yet been rechecked on hardware: feed a
sustained signal, set non-unity input and output gain positions, and observe
the audio-interface level meter through both directions. Record the measured
endpoint, peak, and trough levels before marking this follow-up passed.

### Regression check

`scripts/pi-jack-usb-soak-test.sh` was run at `--duration 60` — not the
1800-second baseline [`usb-audio-verification.md`](usb-audio-verification.md)
established — and passed:

```
PASS mode=oxtt frames=128 periods=3 duration=60
```

This script invokes `oxtt` without `--controls`, so none of the
control-surface code executes during this check at all: no control thread, no
SPI or GPIO access. The question this check answers for this document is
narrowly whether adding the (dormant, unactivated) control-surface code to the
binary disturbed the audio-stability guarantee `usb-audio-verification.md`
established — and that question does not depend on run length, because
control-surface code does not run in this configuration regardless of
duration. The 30-minute duration `usb-audio-verification.md` uses exists to
catch slow-onset audio-hardware phenomena (thermal drift, USB clock slip) that
are unrelated to whether control-surface code is present in the binary, so a
short run is adequate evidence here.

## 6. Outstanding

**Pulling the MCP3008 mid-session has not been performed.** The intended
check is to remove the ADC while `oxtt` is running and confirm the control
surface survives it: audio keeps flowing, the control thread keeps polling,
and the parameters hold their last good values.

It is deferred because the cabling around the ADC on the breadboard is too
dense to disturb safely while the rig is running — the realistic outcome of
attempting it is shorting something, not a clean observation.

Two things are worth recording about what this check would and would not
demonstrate:

- It exercises the **bad-input** path, not the error path. `rppal`'s
  `transfer` returns garbage bytes rather than an error when the device is
  absent, so a pulled MCP3008 produces plausible-looking counts, not `Err`.
  What it would show is that garbage readings cannot crash, hang, or silence
  the effect — the conditioning simply follows the garbage.
- The **error** path is already covered without hardware, by the unit tests
  in `src/control/thread.rs`: a source that fails some reads and then
  recovers is counted and survived, and a source that never succeeds neither
  panics nor publishes, leaving the callback on its last good snapshot.

Re-run this check when the surface moves off the breadboard into an enclosure
with a connector that can be unplugged safely.

## 7. Completion criteria

Met:

- A `pi-controls` release build on the Pi, with `--controls` present in
  `--help`.
- Wiring confirmed channel by channel with `oxtt-pi-tools`, including both
  switch positions.
- Idle jitter measured at both full travel and mid travel, 300 readings per
  channel per position, with worst-case σ recorded and checked against
  `DEADBAND_COUNTS`.
- A sustained live JACK session at 48 kHz `128×3` exercising every pot and the
  bypass switch, ending in `oxtt: xrun_count=0` and
  `oxtt: control_read_failures=0`. Its bypass observations apply only to the
  superseded gain/depth-transition implementation described in section 5.
- `scripts/pi-jack-usb-soak-test.sh` passing against the current binary.

Outstanding:

- The phase-coherent crossfade's bilateral meter recheck: sustained signal,
  non-unity input and output gains, and recorded endpoint, peak, and trough
  levels for both effect-to-bypass and bypass-to-effect throws (section 5).
- The ADC-removal check, with its reason and its limited scope recorded above.
