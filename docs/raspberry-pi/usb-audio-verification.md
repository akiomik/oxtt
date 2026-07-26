# Raspberry Pi 5 Verification: `oxtt` over JACK with a USB Audio Interface

This is the audio-stability and latency verification for `oxtt` running on a
Raspberry Pi 5 as a JACK client, with a class-compliant USB audio interface, and
the results it produced. It assumes the environment from
[`usb-audio-setup.md`](usb-audio-setup.md). The decision these results led to is
[ADR 0008](../decisions/0008-usb-audio-clock-slip-and-i2s-migration.md).

Concrete card names, host names, and port numbers are examples from the
validation environment (see [`usb-audio-setup.md`](usb-audio-setup.md));
substitute your own.

JACK settings are written `frames×periods` — e.g. `128×3` is 128 frames per
period over 3 periods. At 48 kHz one callback is nominally about 1.33 ms for
64 frames, 2.67 ms for 128 frames, and 5.33 ms for 256 frames.

## Test scripts

Two scripts under `scripts/` drive the tests. Neither embeds an adopted setting,
and neither is used to start `oxtt` for normal playing — they are test-only. See
[`scripts/README.md`](../../scripts/README.md) for full argument reference.

- `scripts/pi-jack-usb-soak-test.sh` — audio-stability soak. Takes `frames`,
  `periods`, duration, mode (`direct` loopback or through `oxtt`), the
  capture/playback ports, and an output directory as explicit arguments. It
  records the git revision, binary build time, RT thread info, `get_throttled` at
  start and end, the selected physical ports, the JACK/`oxtt`/recorder/kernel
  logs, the recorded WAV, and the automated continuity verdict. It exits 0 on
  pass, non-zero on fail.
- `scripts/pi-jack-usb-latency-test.sh` — analog-loopback round-trip latency via
  `jack_iodelay`, for direct and `oxtt` modes. The numeric pass/fail is a human
  judgment; the script exits non-zero only when the measurement itself does not
  establish (JACK not started, ports unconnected, `jack_iodelay` cannot lock a
  value, throttling, or an xrun through `oxtt`).

## Audio-stability procedure

Each independent run starts from a fresh boot with the official 5 V / 5 A supply,
the Active Cooler, and the interface connected directly over USB, with the
interface's Phones L/R output looped back to the Line/Instrument 3/4 input by a
physical cable. No listening headphones are connected and phantom power is off.

1. From a fresh boot, first run a `direct` loopback at the setting under test.
2. Only if `direct` passes, run the `oxtt` path from the same boot. The script
   generates a 48 kHz stereo 997 Hz tone with `soak_source`, routes
   `soak-source → oxtt → playback (Phones) → physical loopback → capture
   (Line/Instrument) → soak-recorder`, and records a 16-bit stereo WAV.
3. For an adoption candidate, reboot and repeat the `oxtt` run twice more, into
   `audio-2` and `audio-3` output directories. To change the setting, change only
   `--frames`, `--periods`, and the output directory name.

Example `oxtt`-path invocation (128×3, 30 minutes), with the card name and ports
replaced by yours:

```sh
./scripts/pi-jack-usb-soak-test.sh \
  --card Pro73056544 --frames 128 --periods 3 --mode oxtt --duration 1800 \
  --capture-left system:capture_3 --capture-right system:capture_4 \
  --playback-left system:playback_3 --playback-right system:playback_4 \
  --output-dir <output-dir>
```

## Pass criteria

A `128 frames`-or-lower setting is an adoption candidate only if it passes three
independent 30-minute `oxtt`-path runs, each from a fresh boot. **A zero xrun
count alone does not qualify a setting.** For each run the script confirms:

- exactly one `oxtt: xrun_count=0` line, a zero JACK-log xrun count, and exactly
  one recorder `dropped_frames=0` line;
- no JACK control-plane errors, and a control-plane spot check (one `jack_lsp`
  and one `jack_cpu_load` right after the graph is established) that responds;
- the recorded WAV has no leading or trailing silence, no silent gap longer than
  40 frames (~0.83 ms) even once, and no sample reaching near full scale
  (32760/32767 or above) even once.

The 40-frame silent-gap and full-scale checks exist because a coarser threshold
(a 50 ms maximum-gap check with no clipping check) missed a real, audible defect
at `128×2` that appears in neither the JACK xrun log nor `oxtt`'s self-reported
counter — see the `128×2` findings below.

The stepping order lowers the setting one step at a time; a lower setting is
tried only after the one above it passes:

| Order | Setting | Role | Advance when |
| ---: | --- | --- | --- |
| 1 | 128×3 | First formal candidate | Passes the 30-minute stability test |
| 2 | 128×2 | Low-latency candidate | 128×3 passed, and this passes the same |
| 3 | 64×3 | Low-latency exploration | 128×2 passed, and this passes the same |
| 4 | 64×2 | Diagnostic (provisionally rejected) | Only if 64×3 passed; any audio or control-plane anomaly rejects it |
| aux | 256×3 | Isolation fallback | Only if 128×3 fails, to check for headroom |

## Results

### 128×3 — passed

`128×3` passed the 30-minute audio-stability test (direct plus three `oxtt`
runs). The recordings contained only short silent gaps of a few to ~17 frames,
consistent with the 997 Hz zero crossings, and zero full-scale samples.

### 128×2 — rejected (defect invisible to xrun counters)

`128×2` was rejected. On the surface it passed everything the earlier, coarser
script checked: zero JACK-log xruns, `oxtt: xrun_count=0`, `dropped_frames=0`,
and a maximum silent gap under 50 ms. But sample-level analysis of the 30-minute
physical-loopback recordings found, on each run, 371–400 silent dropouts longer
than 100 frames (over 2 ms) and 35–51 full-scale clipping events, recurring at
roughly 7-second intervals.

Neither JACK's xrun notification nor `oxtt`'s self-reported counter registered
any of this. The defect occurs below the level either instrument observes — at
the hardware/driver boundary — so client-level instrumentation on both sides
misses it. This is what motivated adding the 40-frame silent-gap and full-scale
clipping checks directly to the soak script: **a zero xrun count is not
sufficient evidence of clean audio.**

### Root cause of the 128×2 defect

Short-duration reproductions (well under the 30-minute soak) reproduced the same
signature every time: 120–132-frame silent dropouts, occasional full-scale
samples, recurrence at roughly 6.5–7.7-second intervals, and frequent paired
events 0.28–0.29 s apart. Against that stable reproduction, seven candidate
causes were eliminated one at a time:

1. **Co-tenant background load.** The validation host was a shared home server
   also running k3s, Prometheus, and Grafana. Stopping all of them changed
   nothing (the rate was, if anything, slightly higher), with all cores near
   100% idle afterward.
2. **CPU-load spikes.** Polling `/proc/stat` every 0.2 s showed every core under
   15% busy in the window around each detected gap.
3. **LAN traffic.** Kernel/UFW audit-log timestamps (mDNS, DHCPv6) did not line
   up with the gaps.
4. **`oxtt`'s DSP and JACK-client layer.** A `direct` loopback with `oxtt`
   entirely absent reproduced the same rate, interval, and gap length,
   eliminating both the DSP and JACK-client scheduling.
5. **USB bus-power limits.** Driving the interface from an external
   RME-compatible 12 V / 3 A center-positive supply (instead of Pi bus power)
   left the pattern essentially unchanged, with no USB reset or voltage warning
   in `dmesg` the whole time.
6. **Host scheduling latency.** Running `cyclictest` (four threads, priority
   below jackd's RT thread) alongside the reproduction measured a worst-case
   latency of 520 µs with zero histogram overflows — over five times smaller than
   the ~2.6–2.75 ms dropout length, so host scheduling/IRQ delay cannot explain
   it.
7. **USB connector type / xHCI instance.** Moving the interface to a different
   physical USB port on a different xHCI root reproduced the pattern identically.

A kernel and firmware update (`sudo apt full-upgrade`, kernel confirmed
`6.12.93+rpt-rpi-2712`) also did not change it.

The dropout length matches almost exactly one period (128 frames), the pattern is
present only at `periods=2` and absent at `periods=3`, and it reproduces
identically regardless of host USB topology. The most consistent explanation is a
few-ppm frequency difference between the Pi's USB host clock recovery and the
interface's internal 48 kHz crystal, which USB Audio Class adaptive/asynchronous
clocking periodically corrects as a one-period slip. A 3-period ring
(384 frames = 8 ms) absorbs the correction; a 2-period ring (256 frames =
5.33 ms) has no headroom and surfaces it as an audible dropout, below the client
callback deadline that JACK and `oxtt` instrument. This is a property of the USB
Audio Class transport; further USB-side root-cause work was judged low return on
investment and stopped (see [ADR 0008](../decisions/0008-usb-audio-clock-slip-and-i2s-migration.md)).

Web search confirmed periodic USB-audio dropouts as a long-standing pattern on
Raspberry Pi generally; the known fix (`dwc_otg.fiq_fsm_enable=0`) targets the
older `dwc_otg` controller, not the Pi 5's RP1 `xhci-hcd`.

### Control-plane spot check

The soak script folds a single control-plane spot check into the stability run,
right after the graph is established: one `jack_lsp` and one `jack_cpu_load`,
judged only on whether they respond (no CPU-load threshold). This replaced an
earlier separate operability pass that probed every 10 seconds. The 30-minute
stability run (real-load data plane plus continuous log grep for control-plane
failure patterns) already detects sustained control-plane collapse, and the only
thing the periodic probe added was whether a fresh JACK client can open a control
socket and get a response — which the single post-graph check confirms actively.
The graph-establishment window is deliberately chosen: it is when a broken lower
setting (`64×2`) produced `Cannot create new client` and socket-read failures.

## Round-trip latency

Round-trip latency is measured with `scripts/pi-jack-usb-latency-test.sh` and
`jack_iodelay` over a single-channel analog loopback (Phones L output to
Line/Instrument 3 input). Because JACK2's default asynchronous mode adds an
implicit period to the JACK buffer, do **not** treat `frames/period × periods`
as the round-trip latency; measure it.

Example (128×3, direct), with the card name and ports replaced by yours:

```sh
./scripts/pi-jack-usb-latency-test.sh \
  --card Pro73056544 --frames 128 --periods 3 --mode direct --duration 30 \
  --playback system:playback_3 --capture system:capture_3 \
  --output-dir <output-dir>
```

### 128×3 measured latency

Only the confirmed candidate `128×3` was measured (the rejected `128×2` was not),
direct then `oxtt`, both with `throttled=0x0`:

| Mode | Round-trip (frames) | Round-trip (ms) |
| --- | ---: | ---: |
| direct (host + interface only) | 537.525 | 11.198 |
| through `oxtt` (`--depth 0` / 0 dB) | 546.560 | 11.387 |

- Inserting `oxtt` adds about 9 frames (~0.19 ms), under one period. The DSP
  contributes zero added latency by contract (see
  [`../architecture.md`](../architecture.md)); this small difference is the cost
  of adding one JACK client to the graph. The USB round-trip is dominated by the
  host + interface path, not by `oxtt`.
- Both measurements are well above `128 × 3 = 384 frames` (8.0 ms), consistent
  with JACK2's asynchronous mode adding at least one implicit period.
- Direct 11.198 ms and `oxtt` 11.387 ms both exceed the 10 ms round-trip target
  (the goal set for the physical-controls milestone) by about 1.2–1.4 ms.
  Adoption is a human judgment on both the number and the
  playing feel; the target overshoot alone does not reject the setting. Playing
  feel is not yet recorded.
- As a baseline for the hardware direction (ADR 0008), the USB round-trip is
  about 11 ms. Against the ~2 ms user-reported round-trip of the first-choice I2S
  HAT, this is substantial latency headroom for a migration, on top of removing
  the clock-slip defect.

## Completion criteria

- Three fresh-boot 30-minute `oxtt`-path audio-stability runs pass at 48 kHz and
  128 frames or lower on current `main`, each recording a `PASS` result, exactly
  one `oxtt: xrun_count=0` line, a zero JACK-log xrun count, and a successful
  automated continuity verdict. A run with a missing or duplicated summary does
  not pass.
- Each run's post-graph control-plane spot check responds, with no JACK
  control-plane timeout.
- Release-build JACK DSP load during normal playing is roughly under 50%.
- Latency is acceptable for playing; the 10 ms target is a goal, and the measured
  value is recorded.
- No USB reset, undervoltage, or thermal throttling during normal use, with
  `vcgencmd get_throttled` reading `throttled=0x0` at the start and end of each
  run.
