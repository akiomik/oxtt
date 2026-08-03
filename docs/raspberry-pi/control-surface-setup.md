# Raspberry Pi 5 Setup: `oxtt`'s Physical Control Surface

This is the reproducible setup for `oxtt`'s physical control surface on a
Raspberry Pi 5 — four potentiometers on an MCP3008 SPI ADC driving
`depth`/`time`/`upward`/`downward`, plus a momentary bypass switch. It is one of
the Raspberry Pi configurations under `docs/raspberry-pi/`, and it assumes the
environment already built by
[`usb-audio-setup.md`](usb-audio-setup.md): the same Pi 5, the same 64-bit
Raspberry Pi OS Lite, the same workspace, and a working JACK-over-USB audio
path.

For the results this setup produced — the wiring check, the idle-jitter
measurement, and the live JACK session — see
[`control-surface-verification.md`](control-surface-verification.md). For the
design being set up, see
[ADR 0010](../decisions/0010-three-layer-control-surface-and-newest-value-handoff.md);
for the guarantees it must satisfy, `docs/contracts.md` §8.

**The one step in here that is not a formality is enabling SPI0** (steps 3 and
4). It is easy to believe SPI is already on, because a Pi 5 has a `/dev/spidev*`
node whether or not the 40-pin header's SPI0 is enabled. That belief cost real
time on this hardware. Steps 3 and 4 exist to make it impossible to hold: they
enable SPI0 explicitly, and then check for the header's bus **positively**,
rather than for the existence of "some spidev node".

Concrete names in this document — the host name `oxtt-pi`, the workspace path
`~/workspaces/oxtt`, and your user account — are examples from the environment
these instructions were validated on. Substitute your own values. Vendor part
numbers are deliberately absent for the same reason the ALSA card name is an
example in [`usb-audio-setup.md`](usb-audio-setup.md): the bill of materials
below is generic, and which specific MCP3008 breakout, pot, or switch you buy is
environment-specific.

## Target hardware and stack

| Role | Reference configuration |
| --- | --- |
| SBC | Raspberry Pi 5, set up per [`usb-audio-setup.md`](usb-audio-setup.md) |
| ADC | MCP3008, single-ended, on SPI0/CE0 at 500 kHz, SPI mode 0 |
| Pots | Four linear-taper (B-curve) potentiometers, 10 kΩ, on MCP3008 CH0–CH3 |
| Bypass switch | Momentary push switch on GPIO17 (BCM), active low against the SoC's internal pull-up |
| Reference | 3.3 V from the Pi header, so full scale is 1023 counts |
| Assembly | Breadboard with jumper wiring, not an enclosure |

The constants above are not free choices — `src/control/pi.rs` holds them and
records why each one is what it is, and `oxtt-pi-tools` reads the same hardware
the same way. If you change the bus, the clock rate, the SPI mode, or the
channel assignment, you are changing that module too.

## 1. Bill of materials and electrical rules

### Parts

Generic parts; no specific vendor part number is assumed.

- **MCP3008** in the DIP-16 package, plus a 16-pin DIP socket. The socket is not
  optional in spirit: it lets you pull the ADC out without disturbing the rest
  of the breadboard, which is exactly the manoeuvre the verification document
  records as too risky to do with a soldered-in part.
- **Four 10 kΩ potentiometers, linear taper (B-curve).** Not log/audio taper.
  This is a correctness requirement, not a preference — see below.
- **One momentary (non-latching) push switch.** Normally open, two terminals
  used.
- **0.1 µF ceramic capacitors** for supply decoupling on the MCP3008. The MCP3008
  has separate supply and reference pins even though both go to 3.3 V here, so
  buy more than one.
- **Breadboard and jumper wires**, enough for four pots plus the ADC plus the
  switch.
- **A multimeter.** Step 2 is not doable without one, and step 2 is what keeps a
  wiring mistake from becoming a damaged Pi.

### Why the pots must be linear taper

`src/control/mapping.rs` converts a raw count to a normalized parameter with a
plain `raw / 1023` and no curve fitting, and `oxtt-pi-tools` displays the same
scale. That mapping is only correct for a linear-taper pot. A log/audio-taper
pot will still read 0–1023 end to end and will still look fine in
`oxtt-pi-tools`, so the mistake does not announce itself — it just makes every
knob's travel feel wrong, with all the useful range crowded into one end.

### Electrical rules

These are the rules that decide whether a mistake is recoverable:

- **3.3 V, never 5 V.** The MCP3008's `VDD` and `VREF`, and the top of every
  potentiometer, go to the Pi's 3.3 V rail. The Pi's header also carries 5 V;
  it has no role in this circuit.
- **Never feed more than 3.3 V into a Pi GPIO pin, into an MCP3008 analog input,
  or into `VREF`.** The Pi's GPIOs are not 5 V tolerant, and there is no series
  protection anywhere in this circuit.
- **One ground.** The bottom of every pot, the MCP3008's `AGND` and `DGND`, the
  Pi, and one side of the switch all share the same ground.
- **Decouple the MCP3008.** Put a 0.1 µF ceramic across its supply, as physically
  close to the chip as the breadboard allows.
- **`VREF` sets full scale.** It is tied to the same 3.3 V as `VDD`, which is
  what makes a full-travel pot read 1023 counts and makes the `raw / 1023`
  mapping mean what it says.
- **The switch needs no external resistor.** `PiControls::new` acquires GPIO17
  with the SoC's *internal* pull-up (`into_input_pullup`), and the switch simply
  shorts the pin to ground. Adding an external pull-up is unnecessary; adding an
  external pull-*down* fights the internal pull-up and breaks the read.

## 2. Wire it, and check it with a multimeter before any software

Wire everything with the Pi **powered off and unplugged**.

### Pin map

Pins are given in BCM numbering throughout, which is what `rppal`,
`src/control/pi.rs`, and `oxtt-pi-tools` use.

| Signal | Pi 5 GPIO (BCM) | Goes to |
| --- | --- | --- |
| SPI SCLK | GPIO11 | MCP3008 `CLK` |
| SPI MOSI | GPIO10 | MCP3008 `DIN` |
| SPI MISO | GPIO9 | MCP3008 `DOUT` |
| SPI CE0 | GPIO8 | MCP3008 `CS/SHDN` |
| Bypass switch | GPIO17 | one switch terminal; the other terminal to GND |

Plus the supply and analog side:

| MCP3008 pin | Goes to |
| --- | --- |
| `VDD` | Pi 3.3 V |
| `VREF` | Pi 3.3 V |
| `AGND` | common ground |
| `DGND` | common ground |
| `CH0` | Depth pot wiper |
| `CH1` | Time pot wiper |
| `CH2` | Upward pot wiper |
| `CH3` | Downward pot wiper |

Each pot is a divider: top terminal to 3.3 V, bottom terminal to ground, wiper
to its MCP3008 channel. The channel order matches `CHANNEL_DEPTH = 0`,
`CHANNEL_TIME = 1`, `CHANNEL_UPWARD = 2`, `CHANNEL_DOWNWARD = 3` in
`src/control/pi.rs`; swapping two pots here silently swaps two knobs on the
panel, and nothing downstream can tell.

Two things this table does *not* give you, on purpose:

- **Physical header pin positions.** Confirm the physical pin for each BCM
  number, and for 3.3 V and GND, against the official pinout for your board
  before you push a jumper in. A BCM-to-physical mistake is the one wiring
  mistake that can damage the Pi rather than just failing to read.
- **MCP3008 package pin numbers.** Locate `VDD`, `VREF`, `AGND`, `DGND`, `CLK`,
  `DIN`, `DOUT`, `CS/SHDN` and `CH0`–`CH3` on the datasheet pinout for the
  DIP-16 package you actually have, and note that `DIN` on the ADC is the Pi's
  *MOSI* and `DOUT` is the Pi's *MISO* — the two names invite exactly one
  transposition.

### Why GPIO8–11 and not some other SPI mapping

SPI0's `CE0`/`MISO`/`MOSI`/`SCLK` on GPIO8–11 deliberately keeps the control
surface off GPIO18–21, which is the range a typical I2S audio HAT uses. Moving
the audio path from the USB interface to an I2S HAT is still on the table
([ADR 0008](../decisions/0008-usb-audio-clock-slip-and-i2s-migration.md), which
selected its HAT candidates partly on not colliding with this ADC and this
switch), so this is a mapping to keep rather than to re-derive.

### Multimeter checks, in this order

**With the Pi unplugged**, on continuity/resistance:

1. Every signal in the two tables above: probe from the header pin to the far
   end of the jumper, at the component, and confirm continuity. Do this per
   connection rather than by eye — a jumper one row off on a breadboard looks
   correct.
2. Between the 3.3 V rail and ground: confirm there is **no** continuity, i.e.
   no short. Do this last, after everything is inserted, and before power.
3. Across the switch: open when released, continuity when pressed. If it reads
   continuous in both states, it is a latching switch, not a momentary one, and
   the bypass logic in `src/control/mapping.rs` expects momentary.

**Then power the Pi on**, with nothing running, and measure voltages:

4. MCP3008 `VDD` to ground, and `VREF` to ground: both **3.3 V**. Anything near
   5 V means the supply jumper is on the wrong header pin — power off
   immediately.
5. Each pot's top terminal to ground: 3.3 V. Each pot's bottom terminal to
   ground: 0 V.
6. Each pot's wiper to ground, while turning that pot end to end: a smooth
   sweep from 0 V to 3.3 V. A wiper that jumps, sticks, or never reaches an end
   is a wiring or pot fault, and it is far cheaper to find here than to
   misdiagnose later as ADC jitter.

Only after all six checks pass should any software touch the hardware.

## 3. Enable SPI0 on the 40-pin header

**Do not skip this, and do not assume it is already done.** On a stock Raspberry
Pi OS image `dtparam=spi=on` ships **commented out** in
`/boot/firmware/config.txt`, so the 40-pin header's SPI0 is off until you
uncomment it. That is true even though the running system already has a
`/dev/spidev*` node — see step 4 for what that node actually is.

1. Look at what the image currently has:

   ```sh
   grep -n 'dtparam=spi' /boot/firmware/config.txt
   ```

   On a stock image this shows the line present but commented (`#dtparam=spi=on`).

2. Edit the file and **uncomment the existing line** rather than appending a new
   one, so it stays in whatever section the image put it in:

   ```sh
   sudoedit /boot/firmware/config.txt
   ```

   The line must end up as exactly:

   ```text
   dtparam=spi=on
   ```

   `raspi-config`'s interface-enabling option is the equivalent route and edits
   this same line; this document does not walk through its menus, because the
   line in `config.txt` is the thing that matters and is the thing you can
   check.

3. Confirm the edit took, then reboot — the parameter is read at boot, so
   nothing changes until you do:

   ```sh
   grep -n '^dtparam=spi=on' /boot/firmware/config.txt
   sudo reboot
   ```

   The `^` in that pattern is the point of it: it matches only an uncommented
   line.

## 4. Verify that SPI0 is genuinely the header's bus

This is the step that prevents the failure described at the top. Run **both**
checks. Neither is redundant: the first confirms the right device node exists,
the second confirms the SPI driver actually owns the header pins.

### 4a. `/dev/spidev0.0` specifically — not `/dev/spidev*`

```sh
ls -l /dev/spidev0.0
```

This must succeed. Note carefully what is being asked for: `spidev0.0`, by
name.

**A Pi 5 has a `/dev/spidev10.0` regardless of anything you put in
`config.txt`.** That node is the SoC's internal boot-flash SPI controller — the
one behind `gpiochip10`, whose pins are labelled `2712_BOOT_CS_N` and similar.
It has nothing to do with the 40-pin header, it is present on an untouched
image, and talking to it will not read your MCP3008. On this hardware, its
presence was read as "SPI is enabled" and work proceeded against the boot-flash
controller for a while before the mistake was found. So:

- **Never** check with a glob like `ls /dev/spidev*` and conclude from a
  non-empty result. That check passes on a machine where header SPI is off.
- If you see `/dev/spidev10.0` and no `/dev/spidev0.0`, SPI0 is **not** enabled.
  Go back to step 3, confirm the line is uncommented, and confirm you actually
  rebooted.

### 4b. `sudo cat /sys/kernel/debug/gpio`

```sh
sudo cat /sys/kernel/debug/gpio
```

This lists every GPIO controller on the machine and, for each line, who is
holding it. It is the check that distinguishes "the pin is free" from "a driver
owns the pin", which a device-node listing cannot tell you.

With header SPI0 enabled, **GPIO8 carries a dedicated label naming the SPI
driver — `spi0 CS0`.** That label is the positive evidence: the SPI0 driver has
claimed the header pin your MCP3008's `CS/SHDN` is wired to. Without SPI0
enabled, GPIO8 shows no such consumer.

The same output also shows the boot-flash controller from 4a as a separate
`gpiochip10` with `2712_BOOT_CS_N`-style pin names, which is a useful thing to
see once: it makes concrete that the two SPI controllers on this machine are
different hardware serving different purposes.

Keep this command. It generalises past this document: whenever a header
peripheral on a Pi behaves strangely, "which driver, if any, currently holds
this pin" is the first question worth answering, and this is how you answer it.

## 5. Confirm your user can reach the SPI and GPIO devices

`PiControls::new` opens `/dev/spidev0.0` and the GPIO character device as the
invoking user, and treats a failure as fatal at startup — `PiControlError::Spi`
points at wiring or a disabled SPI0, `PiControlError::Gpio` at the pin or at
permissions. Check access before you build anything, so that a permission
problem does not get misread as a wiring problem.

1. Ask the shell whether the current user can actually open the SPI device:

   ```sh
   test -r /dev/spidev0.0 && test -w /dev/spidev0.0 \
     && echo "spidev0.0 is readable and writable by $(id -un)" \
     || echo "spidev0.0 is NOT accessible by $(id -un)"
   ```

2. If that reports no access, look at who *does* own the node, and at what
   groups you are in:

   ```sh
   ls -l /dev/spidev0.0 /dev/gpiochip*
   id -nG
   ```

   **Which group owns these devices, and which file mode they carry, is
   environment-dependent** — it is set by your distribution's udev rules, not by
   anything in this repository, and it differs between images and releases.
   Read the owning group out of the `ls -l` output above and add your user to
   *that* group (then start a new login session so the membership takes effect).
   Do not assume a group name from another guide.

3. There is no equivalent one-line test for the GPIO character device, because
   which `gpiochip` carries the 40-pin header is firmware-dependent on a Pi 5
   and `rppal` selects it itself. The practical check is step 6: if
   `oxtt-pi-tools` starts and prints readings, both devices opened.

Running the tool under `sudo` is a legitimate *diagnostic* — if it works as root
and fails as your user, the problem is permissions and not wiring — but it is
not the way to run the control surface. Fix the group membership instead.

## 6. Confirm the wiring end to end with `oxtt-pi-tools`

`oxtt-pi-tools` is the standalone wiring-verification binary. It depends only on
`rppal` and not on `oxtt` at all, so it runs on a Pi with nothing else working,
and `src/control/pi.rs` reproduces its read byte for byte — same bus, same mode,
same clock, same three-byte conversation. Run it before building `oxtt` and
before involving JACK, so that anything it finds is unambiguously hardware.

In the repository on the Pi:

```sh
cargo run --release -p oxtt-pi-tools
```

It needs no Cargo feature flag: the `oxtt-pi-tools` package depends on `rppal`
unconditionally, and `pi-controls` gates only `oxtt`'s own hardware layer.

`cargo run` builds *and* runs in one command, so run it somewhere the SPI and
GPIO devices are actually reachable. If you use the container build split from
[`usb-audio-setup.md`](usb-audio-setup.md), whether the container sees
`/dev/spidev0.0` and the GPIO character device depends on how it was created;
the step 5 checks are what tell you, and they are worth re-running on whichever
side you invoke this from.

It prints one line every 200 ms:

```
Depth=991 (0.969) Time=1023 (1.000) Upward=1017 (0.994) Downward=1002 (0.980) Bypass=released
```

If it fails to start, the error says which device: an SPI error sends you back
to steps 3–5, a GPIO error to step 5 or to the switch wiring.

Then confirm, by hand:

- each pot moves **its own** channel across the full `0..=1023` range, end to
  end;
- no channel moves when a *different* pot is turned — a channel that follows the
  wrong knob is a swapped wiper, and the channel order in step 2 is what fixes
  it;
- `Bypass` reads `pressed` only while the switch is held down, and returns to
  `released` when you let go. A `Bypass` that never changes points at the switch
  wiring or at a latching switch; one that reads `pressed` with nothing touched
  points at a permanent short to ground.

The full pass record for this check, and the idle-jitter measurement that comes
next, are in
[`control-surface-verification.md`](control-surface-verification.md).

## 7. Build `oxtt` with the control surface compiled in

The control surface lives behind the `pi-controls` Cargo feature, which is off
by default (`docs/development.md`, "The `pi-controls` feature"). On the Pi, in
the repository:

```sh
cargo build --release --locked --features pi-controls
./target/release/oxtt --help
```

`--help` must list `--controls`. That flag does not exist without the feature,
so its presence is what confirms you built the right binary.

The feature only compiles the hardware layer in; it does not turn it on.
`--controls` is what starts the control thread, and it stays opt-in even in a
`pi-controls` build — so this same binary still runs the audio-stability scripts
under `scripts/` exactly as before, on a Pi with no breadboard attached.

## Next

With SPI0 confirmed as the header's bus, the wiring confirmed channel by
channel, and a `pi-controls` binary built, the hardware is ready to be verified:
idle jitter against the conditioning constants, and a sustained live JACK
session driving the effect from the panel. The procedure, pass criteria, and
recorded results are in
[`control-surface-verification.md`](control-surface-verification.md).
