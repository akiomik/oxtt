# ADR 0008: Reject the 128×2 JACK Setting and Plan an I2S HAT Migration

## Status

Superseded by [ADR 0009](0009-hardware-platform-choice-reopened.md)

ADR 0009 corrects an availability premise in this ADR's HAT ranking below — all
high-quality stereo line ADC/DAC HATs are import-tier in Japan, so availability
does not differentiate them — re-ranks the HATs, and reopens the SBC /
microcontroller platform question this ADR kept out of scope. The USB clock-slip
findings, the `128×3` baseline, and the requirement that the audio interface
share its clock in hardware (not USB isochronous) are unchanged and carry
forward.

Builds on [ADR 0007](0007-alsa-direct-not-cpal-for-pi-native-backend.md), which
kept JACK as the baseline while its latency and stability targets held. This ADR
records the first Raspberry Pi 5 measurements on real hardware against that
baseline and their consequence for the hardware direction.

## Context

This evaluation asks whether a Raspberry Pi 5, JACK2, and a class-compliant USB
audio interface can reach the latency and stability a playable effect needs. The
reference interface for these measurements is an RME Babyface Pro FS in
class-compliant mode; the concrete card names, host names, and port numbers in
this ADR and its companion
[`docs/raspberry-pi/usb-audio-verification.md`](../raspberry-pi/usb-audio-verification.md)
are examples from that setup, not fixed requirements.

JACK settings are written `frames×periods` (e.g. `128×3` is 128 frames per period
over 3 periods). The soak procedure lowers the setting one step at a time and
requires each candidate to pass three independent 30-minute audio-stability runs
from a fresh boot. The full procedure, pass criteria, and raw findings are in
`docs/raspberry-pi/usb-audio-verification.md`.

The measurements produced two results that drive this decision:

1. **`128×3` passes; `128×2` fails in a way neither JACK nor `oxtt` detects.**
   At `128×2`, JACK reported no xruns, `oxtt`'s self-reported xrun counter stayed
   at zero, and the recorder dropped no frames, yet sample-level analysis of the
   physical-loopback recording found hundreds of short dropouts (over 2 ms) and
   dozens of full-scale clipping events per 30-minute run, recurring at a roughly
   regular interval. The defect sits below the JACK client callback deadline, at
   the ALSA/USB/interface boundary, so the client-level instrumentation on both
   sides misses it entirely.

2. **The dropouts survive elimination of every host-side and software cause.**
   Across short reproductions, seven candidate causes were ruled out one by one:
   background load from co-tenant services, CPU-load spikes, LAN traffic,
   `oxtt`'s own DSP and JACK-client layer (the same pattern reproduces on a
   direct `soak-source → playback` loopback with `oxtt` entirely absent),
   USB bus-power limits (an external interface supply changed nothing), host
   scheduling latency (`cyclictest` stayed well under the dropout length), and
   the USB connector type / xHCI instance. A kernel and firmware update did not
   change the pattern either.

The dropout length matches almost exactly one period (128 frames), the pattern is
absent at `periods=3` and present only at `periods=2`, and it reproduces
identically regardless of host USB topology. The most consistent explanation is a
few-ppm frequency difference between the Pi's USB host clock recovery and the
interface's internal 48 kHz crystal, which USB Audio Class adaptive/asynchronous
clocking periodically corrects as a one-period slip. A 3-period ring absorbs that
correction; a 2-period ring does not. This is a property of the USB Audio Class
transport, not something JACK, the kernel, or the DSP can tune away.

For latency, `128×3` measures roughly 11 ms analog-loopback round-trip
(host + interface direct, and with `oxtt` inserted), about 1–1.4 ms over the
10 ms round-trip target. Inserting `oxtt` adds under one period, consistent with
the contract of zero added DSP latency; the round-trip cost is dominated by the
USB host path, not the DSP.

## Decision

- **Confirm `128×3` as the JACK setting for this configuration and reject
  `128×2`.** A
  zero-xrun count alone does not qualify a setting; the audio-quality criteria in
  `docs/raspberry-pi/usb-audio-verification.md` (bounded silent-gap length and no
  full-scale clipping) apply equally. `128×2` fails those criteria, so the
  lower-latency `64×3` and `64×2` settings are not pursued over USB.
- **Stop further `128×2` root-cause work over USB.** The seven eliminated causes
  and the kernel/firmware update exhaust the cost-effective host-side and
  software-side investigation. Deeper direct observation (`perf sched record`,
  USB packet capture) is judged low return on investment.
- **Adopt an I2S HAT migration as the hardware direction.** I2S shares the audio
  clock in hardware and does not route through USB isochronous transfer or
  asynchronous clock recovery, so this class of periodic clock slip cannot arise
  by construction. The DSP core is already independent of JACK and the host
  (ADR 0007), so the migration cost is limited to buying a HAT and re-running the
  Raspberry Pi setup and tests, not rewriting the effect.
- **Keep moving to a different SBC or microcontroller out of scope.** The Pi 5's
  heat and form-factor constraints are a separate problem that a HAT migration
  does not solve and that this decision does not address.

## Consequences

- The baseline for this configuration is `48 kHz`, `128×3`, over USB, with a
  recorded round-trip latency of about 11 ms. This is the comparison baseline for
  any HAT evaluation.
- The audio-stability test harness must judge recorded audio quality directly
  (silent-gap length and clipping), because the transport-level defect is
  invisible to xrun counters. This criterion is now part of the soak script.
- HAT candidates were narrowed to three, in priority order: **Blokas Pisound**
  (first choice — designed for real-time performance use on the Pi, officially
  supports the Pi 5 with the Active Cooler, has user-reported round-trip latency
  near 2 ms, and uses GPIO ranges that do not collide with the planned SPI0 ADC
  and bypass switch, but has no domestic distributor and must be imported);
  **HiFiBerry DAC2 ADC Pro** (second choice — cheaper and 192 kHz capable, but
  with less certain Pi 5 driver maturity); and **Raspberry Pi Codec Zero** (a
  first-party fallback prioritizing availability and official support, capped at
  96 kHz and needing extra AUX wiring). DAC-only or amplifier-only HATs without an
  ADC do not meet the stereo-capture requirement and are excluded.
- The user-reported ~2 ms round-trip for the first-choice HAT, against the ~11 ms
  USB baseline, indicates the migration also has substantial latency headroom, on
  top of removing the clock-slip defect.
- ADR 0007 remains in force: an ALSA-direct backend is still gated behind a
  measured need. This ADR does not require it. The clock slip is a transport
  property, not a JACK-operation problem, so it is not the "JACK operational cost"
  trigger that ADR 0007 reserves for reconsidering ALSA direct.

## Addendum: search for prior reports of this pattern (2026-07-26)

A search across raspberrypi/linux GitHub issues, the Raspberry Pi Forums, RME's
user forum, linuxmusicians.com, the Blokas/Pisound community, Reddit, and Linux
kernel mailing lists found no report matching this ADR's specific pattern: a
periodic one-period slip at `128×2` that neither JACK nor the client-side xrun
counter detects, visible only in sample-level analysis of the recorded audio,
on the Pi 5's xHCI (RP1) controller. This is recorded here so a future
investigation does not repeat the same search from scratch.

The closest related reports, and why none of them match:

- [raspberrypi/linux#3795](https://github.com/raspberrypi/linux/issues/3795)
  (already cited below) — the known `dwc_otg.fiq_fsm_enable=0` fix targets the
  pre-Pi-5 `dwc_otg` controller and its ARMv8 FIQ limitation; it does not apply
  to the Pi 5's `xhci-hcd`.
- [raspberrypi/linux#5743](https://github.com/raspberrypi/linux/issues/5743)
  ("Pi 5 and soundcards") — the main open Pi 5 audio issue, but it concerns I2S
  clock producer/consumer overlay selection, not USB clock slip.
- [raspberrypi/linux#5759](https://github.com/raspberrypi/linux/issues/5759) —
  a Pi 5 USB DAC sizzling sound at 176.4/352.8 kHz only; a Raspberry Pi engineer
  measured Pi 5 SOF packet intervals at 125 µs ± 0.0625 µs on that report,
  finding no gross timing fault. Different symptom (audible only at high sample
  rates, not a `128×2`-specific slip invisible to xrun counters).
- [Pi 5 `xhci_hcd` "disabled endpoint" errors during
  playback](https://forums.raspberrypi.com/viewtopic.php?t=375580) — the
  reporter states playback itself sounds unaffected; the thread ends
  unresolved and locked.
- An xHCI isochronous ring xrun-handling kernel patch ([LKML, Feb
  2025](https://lkml.iu.edu/hypermail/linux/kernel/2502.3/04994.html)) — fixes
  a race in explicit xrun event handling (data loss / early TD completion), a
  different failure mode from a slip that produces no xrun event at all.

No public report describes the few-ppm clock-recovery/crystal-difference
mechanism this ADR attributes the `128×2` failure to, specific to the Pi 5's
RP1 xHCI controller. The soak-test findings in
`docs/raspberry-pi/usb-audio-verification.md` may be the first documented
isolation of this pattern; if reporting it upstream, that document's
cause-elimination list and period-length match are the primary evidence to
cite.

## References

- `docs/raspberry-pi/usb-audio-verification.md` — the full procedure, pass
  criteria, `128×3`/`128×2` results, the seven-cause elimination, and the
  round-trip latency measurements this ADR summarizes.
- [Raspberry Pi periodic USB-audio dropout reports](https://github.com/raspberrypi/linux/issues/3795)
  — a long-standing pattern of periodic USB-audio dropouts on Raspberry Pi; the
  known fix (`dwc_otg.fiq_fsm_enable=0`) targets the older `dwc_otg` controller,
  not the Pi 5's RP1 `xhci-hcd`.
- [Pisound specifications](https://blokas.io/pisound/docs/general-specifications/)
  — Raspberry Pi 5 real-time stereo ADC/DAC HAT (first-choice migration target).
- [HiFiBerry DAC2 ADC Pro](https://www.hifiberry.com/shop/boards/dac2adcpro/)
  and [Raspberry Pi Codec Zero](https://www.raspberrypi.com/products/codec-zero/)
  — the second-choice and fallback HAT candidates.
