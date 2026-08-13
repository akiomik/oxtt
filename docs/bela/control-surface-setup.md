# Bela Gem Stereo Setup: `oxtt`'s Physical Control Surface

This is the wiring for `oxtt`'s physical control surface on a Bela Gem Stereo —
six potentiometers driving `depth`/`time`/`upward`/`downward` and the
input/output gains, plus a latching bypass switch. It is the counterpart of
[`docs/raspberry-pi/control-surface-setup.md`](../raspberry-pi/control-surface-setup.md),
and it is a much shorter document for one reason: a Gem has the converter and
the GPIO on the board already, so there is no ADC to wire, no SPI bus to
enable, and no ADC reference to get right.

For the design, see
[ADR 0010](../decisions/0010-three-layer-control-surface-and-newest-value-handoff.md)
and [ADR 0011](../decisions/0011-bela-gem-stereo-as-the-second-host.md); for the
guarantees it must satisfy, [`contracts.md` §8](../contracts.md#8-control-surface).
For building and deploying, [`cross-compile.md`](cross-compile.md).

Vendor part numbers are deliberately absent: which specific pot, switch or
resistor you buy is environment-specific.

## Target hardware

| Role | Reference configuration |
| --- | --- |
| Board | Bela Gem Stereo on PocketBeagle 2 |
| Pots | Six linear-taper (B-curve) potentiometers, 10 kΩ, on analog inputs `A0`–`A5` |
| Unused analog inputs | `A6` and `A7`, tied to ground — never left floating |
| Bypass switch | Latching (alternate-action) switch on digital `D0`, active low against an **external** 10 kΩ pull-up to 3.3 V |
| Pot reference | 3.3 V from the board, against the converter's 4.096 V full scale |
| Assembly | Breadboard with jumper wiring, not an enclosure |

The constants above are not free choices. `src/bela_host/controls.rs` holds
them and records why each is what it is; changing the channel assignment or the
switch polarity changes that module too.

## Wiring

Each pot has its two ends across 3.3 V and ground, and its wiper to an analog
input:

```text
3.3 V ──┬── pot 1 end ── wiper ──> A0   Depth
        ├── pot 2 end ── wiper ──> A1   Time
        ├── pot 3 end ── wiper ──> A2   Upward
        ├── pot 4 end ── wiper ──> A3   Downward
        ├── pot 5 end ── wiper ──> A4   Input Gain
        └── pot 6 end ── wiper ──> A5   Output Gain
                (other end of each pot to GND)

3.3 V ── 10 kΩ ──┬──> D0    bypass switch
                 └── switch ── GND

GND ──> A6, A7                     unused analog inputs
```

Three things in that diagram are the whole document.

### Linear taper is a correctness requirement

The mapping layer divides the reading by full scale with no curve fitting, so a
logarithmic pot would not be a different feel — it would be a different
parameter. The gain pots map their lower stop to −24 dB and their upper stop to
+24 dB, which puts unity gain at the **centre of rotation** only for a linear
taper.

### The switch needs an external pull-up

This is the one wiring difference from the Raspberry Pi. The Pi's GPIO has an
internal pull-up that `PiControls` enables in software; Bela's digital pins have
none, so the pull-up is a physical 10 kΩ resistor to 3.3 V with the switch
shorting the pin to ground.

The polarity is then identical to the Pi's — active low, switch closed means
bypassed — and it fails in the right direction: a broken connection or a
disconnected switch reads high, which is *not bypassed*, so the effect keeps
working rather than silently dropping out.

All of a Gem's digital pins start as inputs, so nothing has to configure `D0`
before reading it.

### Tie the unused analog inputs to ground

A floating analog input on a Gem is an antenna, not a zero: unconnected pins
were measured drifting between 0.015 and 0.147 of full scale, and moving a hand
near them moved the reading. `A6` and `A7` are not read by oxtt, so this is
tidiness rather than correctness — but the same rule applied to a *used* channel
is why a disconnected pot must read as its lower stop, which is −24 dB on the
gain pots and fully dry on depth. Everything fails quiet.

## What a reading means

`analog_read` returns 0.0 to 1.0 for 0 V to 4.096 V — the converter's own
internal reference, which is **above** the 3.3 V rail the pots are wired
across. A pot at its upper stop therefore reads about **0.806**, not 1.0.

`src/bela_host/controls.rs` scales by that ratio, so the top of the travel means
depth 1.0 and +24 dB. Scaling by full scale instead would stop the pots at 826
of 1023 steps, which is +14.6 dB on the gain pots and a depth that never reaches
fully wet — a plausible-looking result that is simply wrong, which is why it is
written down here and asserted in that module's tests.

The measured ceiling on one board was 0.8064, slightly *above* the nominal
3.3/4.096 = 0.80566. The conversion clamps for that reason.

## Running with the surface

```sh
scripts/bela-deploy.sh -- --controls --preset safe-start --report-on-exit
```

`--controls` is opt-in, so the same binary still runs on a board with nothing
wired to its headers. Without the six analog inputs and one digital input the
surface needs, the run is refused *before* the audio system is built, with a
reason — see [`contracts.md` §9](../contracts.md#9-bela-host-lifecycle).

The switch position is the bypass state from the first reading onward, so a
board started with the switch in the bypassed position comes up bypassed.
