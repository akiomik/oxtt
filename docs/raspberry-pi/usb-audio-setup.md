# Raspberry Pi 5 Setup: `oxtt` over JACK with a USB Audio Interface

This is the reproducible setup for running `oxtt` on a Raspberry Pi 5 as a
CLI-only JACK client, with a class-compliant USB audio interface as the
full-duplex device — on real hardware, before any physical controls. It is one
of the Raspberry Pi configurations under `docs/raspberry-pi/`; an I2S HAT
configuration is planned but not yet documented (see
[ADR 0008](../decisions/0008-usb-audio-clock-slip-and-i2s-migration.md)).

For the results this setup produced, see
[`usb-audio-verification.md`](usb-audio-verification.md). For the decision it led
to, see [ADR 0008](../decisions/0008-usb-audio-clock-slip-and-i2s-migration.md).

Concrete names in this document — the host name `oxtt-pi`, the shared home server
`daphnis`, the ALSA card name `Pro73056544`, the workspace path
`~/workspaces/oxtt`, and specific port numbers — are examples from the
environment these instructions were validated on. Substitute your own values;
the commands that must use your value point it out.

## Target hardware and stack

| Role | Reference configuration |
| --- | --- |
| SBC | Raspberry Pi 5 |
| OS | 64-bit Raspberry Pi OS Lite (host and build target are both `aarch64-unknown-linux-gnu`) |
| Cooling | Active Cooler or equivalent active cooling |
| Pi power | Official 27 W USB-C supply, or an equivalent stable 5 V / 5 A supply |
| Audio I/O | Class-compliant USB interface, full-duplex, one shared hardware clock. Validated with an RME Babyface Pro FS in class-compliant mode. |
| Connection | Interface connected directly to a Pi USB port, no hub |
| Sample rate | 48 kHz baseline |
| Channels | Stereo capture / stereo playback |

The DSP core is independent of the audio host (see
[`../architecture.md`](../architecture.md) and
[ADR 0007](../decisions/0007-alsa-direct-not-cpal-for-pi-native-backend.md)), so
these instructions target JACK2 over ALSA and do not depend on any interface
beyond a full-duplex, class-compliant, single-clock-master device.

## Build vs. run split

The setup keeps two environments deliberately separate:

- **Build** runs in a Debian container (this validation used a `distrobox`
  named `oxtt` from `docker.io/library/debian:bookworm`). Rust, `pkg-config`, and
  `libjack-jackd2-dev` are installed here.
- **Run** happens on the Pi host itself, which owns `/dev/snd`, the realtime
  scheduling limits, and the JACK server.

The container and host share the workspace directory, so the repository is cloned
once and used from both. Rust toolchains, realtime limits, and audio group
membership are **not** shared between them; each is configured on the side that
needs it. If you build natively on the host instead of in a container, install
the build packages on the host and skip the container-specific steps.

## RME Babyface Pro FS in class-compliant mode

The reference interface is used in class-compliant (CC) mode. Per RME's manual,
CC mode is recognized by Linux as a standard USB Audio Class 2.0 device with no
vendor driver, exposes up to 24-bit / 192 kHz and multiple I/O channels, and
disables TotalMix FX and the internal effects that the vendor driver would
provide. Enter CC mode by holding `SELECT` and `DIM` until the level meter shows
`CC`.

The interface can run from the Pi's USB bus power, but an RME-compatible external
supply (per RME support, 9–14 V DC, at least 1 A, center-positive) is recommended
to avoid momentary droop under load (e.g. phantom power). Regardless, give the Pi
a stable 5 V / 5 A-class supply and confirm no undervoltage or USB resets occur.

Update the interface firmware from a supported PC before connecting it to the Pi.
On the Pi, set input gain and output level on the interface itself or through the
project's controls; do not rely on TotalMix.

## 1. OS image and first boot

1. Flash 64-bit Raspberry Pi OS Lite with Raspberry Pi Imager. Set the host name
   (this document uses `oxtt-pi`), user, timezone, network, and an SSH public key
   in Imager before writing. Record the image name and Imager version in your
   test notes.
2. Attach the Active Cooler and use a 5 V / 5 A-class supply. Do **not** connect
   the audio interface yet.
3. Connect over SSH (replace `<user>` with the account you created):

   ```sh
   ssh <user>@oxtt-pi.local
   ```

4. Update and reboot:

   ```sh
   sudo apt update
   sudo apt full-upgrade
   sudo reboot
   ```

5. After reconnecting, confirm a 64-bit OS and a healthy power/thermal baseline.
   `uname -m` must be `aarch64` and `getconf LONG_BIT` must be `64`.

   ```sh
   uname -m
   getconf LONG_BIT
   cat /etc/os-release
   vcgencmd measure_temp
   vcgencmd get_throttled
   ```

   `vcgencmd get_throttled` should read `throttled=0x0`. Any other value records
   an undervoltage or throttling event — fix power or cooling before running any
   audio test, because it confounds every later measurement.

## 2. Host realtime privileges

Realtime scheduling and PAM limits are configured on the Pi host, not in the
build container. Installing `jackd2` in a container does not change the host's
login limits.

1. On the host, add your user to the `audio` group and set the limits:

   ```sh
   id -nG
   getent group audio
   sudo usermod --append --groups audio <user>
   sudoedit /etc/security/limits.d/audio.conf
   ```

   Put these two lines in `audio.conf`:

   ```text
   @audio - rtprio 95
   @audio - memlock unlimited
   ```

2. Reboot so the login session picks up the group and limits, then verify:

   ```sh
   sudo reboot
   ```

   ```sh
   id -nG
   ulimit -r
   ulimit -l
   ```

   Confirm `audio` is in `id -nG`, `ulimit -r` is greater than 0, and `ulimit -l`
   is `unlimited`.

## 3. Host runtime packages

The host runs JACK, `oxtt`, and — for the audio verification — the test scripts.
Two groups of packages are involved. Install both if you intend to run the
verification in [`usb-audio-verification.md`](usb-audio-verification.md); install
only the first if you just want to run `oxtt` by hand.

**To run JACK, `oxtt`, and inspect the hardware** (needed for the manual setup in
steps 5–6 and to run the effect at all):

```sh
sudo apt update
sudo apt install jackd2 jack-example-tools alsa-utils usbutils file
```

- `jackd2` — the JACK server and `libjack.so.0`.
- `jack-example-tools` — the JACK CLI clients (`jack_lsp`, `jack_connect`,
  `jack_cpu_load`, `jack_iodelay`, `jack_samplerate`, `jack_bufsize`). On
  Debian 13 (Trixie) these are **not** in `jackd2` — they moved to this separate
  package, which `jackd2` only `Recommends`. So `apt install jackd2` alone (or an
  apt setup that disables recommends) will not install them; name it explicitly.
- `alsa-utils` (`aplay`/`arecord`), `usbutils` (`lsusb`), `file` — used by the
  hardware-identification steps below.

**Additionally, to run the verification scripts**
(`scripts/pi-jack-usb-soak-test.sh` and `scripts/pi-jack-usb-latency-test.sh`):

```sh
sudo apt install git
```

The scripts run on the host and record the git revision, working-tree status, and
current branch as run provenance, so `git` must be present on the host — not only
in the build container of step 4. The scripts otherwise rely on tools that a
standard Raspberry Pi OS install already provides: `sudo`, `systemd`'s
`journalctl` (kernel-log capture), `procps` (`ps`), `mawk` (`awk`), `coreutils`
(`timeout`, `seq`, `stat`, `date`, `tee`), and `vcgencmd`. On a stripped-down
image, confirm those are installed before running the scripts.

The test signal generator, WAV recorder, and WAV analyzer are the project's own
native `oxtt-jack-tools` crate (`soak_source`, `soak_recorder`, `soak_analyze`),
built in the container in step 4 and run from the shared `target/release/` on the
host — so no external sound-file player or recorder is needed.

If `jackd2` asks `Enable realtime process priority?`, answer `Yes`. The
authoritative values are still the host `audio.conf` and login session from
step 2.

## 4. Container packages, Rust, and release build

Run these inside the build container, in the shared workspace.

1. Confirm the architecture and that the workspace is shared with the host:

   ```sh
   uname -m               # aarch64
   dpkg --print-architecture   # arm64
   cat /etc/os-release
   ```

2. Install the build packages (this changes only the container, not the host):

   ```sh
   sudo apt update
   sudo apt install build-essential pkg-config git curl file libjack-jackd2-dev
   ```

   The container is build-only. Do not install JACK runtime/test tools here;
   those go on the host (step 3).

3. Install `rustup` with no default toolchain — the first Rust command in the
   repository installs the version and components from `rust-toolchain.toml`. The
   toolchain is not shared with the host, so do not skip this even if the host
   has Rust.

   ```sh
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- --profile minimal --default-toolchain none
   source "${HOME}/.cargo/env"
   rustup show active-toolchain
   rustc -vV                # host must be aarch64-unknown-linux-gnu
   ```

4. Build the release binaries and confirm the JACK build environment:

   ```sh
   cargo build --release --locked
   cargo build --release --locked -p oxtt-jack-tools
   file target/release/oxtt          # AArch64 ELF
   pkg-config --modversion jack
   pkg-config --libs jack            # must include -ljack
   ```

   The `jack` crate loads JACK dynamically at runtime, so `ldd target/release/oxtt`
   may not list `libjack.so.0`; do not judge the JACK dependency from `ldd` alone.
   Confirm it instead by build-time `pkg-config` success in the container, the
   host's `libjack.so.0`, and running the binary against a started JACK server.

   On the host (the runtime side), the absence of `pkg-config`/`jack.pc` is not an
   error as long as the library is present:

   ```sh
   ldconfig -p | grep libjack
   ```

## 5. Identify the interface and ALSA card name

1. On a supported PC, update/confirm the interface firmware and switch it to
   class-compliant mode (see above). Set phantom power off and the monitor level
   to minimum, then connect it directly to a Pi USB port (no hub).
2. On the host, confirm the USB device and `/dev/snd`:

   ```sh
   lsusb
   ls -la /dev/snd
   ```

   If `/dev/snd` is permission-denied, recheck the host `audio` group, login
   session, and the interface connection.

3. Record the ALSA card and its capture/playback devices:

   ```sh
   cat /proc/asound/cards
   aplay -l
   arecord -l
   ```

4. Use the card **name** from `/proc/asound/cards` (in square brackets), not a
   boot-order-dependent number like `hw:0`. In this environment the card name is
   `Pro73056544`; yours will differ. Confirm the same name comes back after
   unplugging and replugging.

   Which JACK port number maps to which physical channel is **not** guessable
   from the ALSA `inN`/`outN` aliases — verify it by listening (step 6). In this
   environment, direct-loopback listening confirmed, among others:

   | Direction | Physical channel | JACK port |
   | --- | --- | --- |
   | input | Line/Instrument 3/4 | `system:capture_3/4` |
   | input | XLR Mic/Line 1/2 | `system:capture_1/2` |
   | output | Phones L/R (Line 3/4) | `system:playback_3/4` |
   | output | Main L/R | `system:playback_1/2` |

   The reference signal path for the tests uses Line/Instrument 3/4 in and
   Phones L/R out.

## 6. Start JACK and verify the port mapping

Run JACK, the release binary, and all port operations on the host.

1. Start JACK at the reference baseline (48 kHz, 128 frames/period, 3 periods),
   replacing the card name with yours. Leave it running in the foreground:

   ```sh
   jackd -R -d alsa -d hw:CARD=Pro73056544 -r 48000 -p 128 -n 3
   ```

   If you see `cannot use real-time scheduling`, a device-open error, or xruns,
   stop here and recheck the group/limits (step 2), the card name, and whether
   another process holds the device.

2. In another SSH session, record the JACK state and ports:

   ```sh
   jack_samplerate
   jack_bufsize
   jack_lsp -A
   ```

3. Verify the mapping with a direct physical loopback **before** involving
   `oxtt`. Keep the monitor level low and feed a test signal into the input:

   ```sh
   jack_connect system:capture_3 system:playback_3
   jack_connect system:capture_4 system:playback_4
   jack_lsp -c -A
   ```

   When the input meter and the headphone output agree, the mapping is confirmed.
   Disconnect before testing `oxtt`:

   ```sh
   jack_disconnect system:capture_3 system:playback_3
   jack_disconnect system:capture_4 system:playback_4
   ```

   Use the same one-pair-at-a-time procedure for any other physical channels;
   never infer the analog channel from the `system:capture_N` number alone.

4. Start the release binary on the host with the safe preset:

   ```sh
   ./target/release/oxtt --preset safe-start
   ```

5. In another session, connect the recorded ports:

   ```sh
   jack_connect system:capture_3 oxtt:input_l
   jack_connect system:capture_4 oxtt:input_r
   jack_connect oxtt:output_l system:playback_3
   jack_connect oxtt:output_r system:playback_4
   jack_lsp -c
   ```

   With the monitor level low, confirm left/right, input/output levels, no
   abnormal output during silence, and no click/pop. Do not use the `default`
   preset until this check passes — it is intentionally strong and can exceed
   0 dBFS.

## Next

With the graph confirmed, run the automated soak and latency tests. The
procedure, pass criteria, scripts, and recorded results are in
[`usb-audio-verification.md`](usb-audio-verification.md).
