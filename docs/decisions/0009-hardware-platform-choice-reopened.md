# ADR 0009: Correct the HAT Availability Premise and Reopen the Hardware Platform Choice

## Status

Accepted

Supersedes [ADR 0008](0008-usb-audio-clock-slip-and-i2s-migration.md).

ADR 0008's USB clock-slip findings, its `48 kHz` / `128×3` / ~11 ms USB baseline,
and its core requirement — that the audio interface share its clock in hardware
(I2S-class), not over USB isochronous transfer — are unchanged and carry forward.
This ADR corrects one premise ADR 0008's HAT ranking rested on, re-ranks the HATs,
and reopens the SBC / microcontroller question ADR 0008 deferred. It does **not**
choose a platform.

## Context

ADR 0008 ranked the HAT candidates Pisound first, HiFiBerry DAC2 ADC Pro second,
Raspberry Pi Codec Zero as a fallback. An interim revision during this
investigation promoted the HiFiBerry DAC2 ADC Pro to first on the belief that it
was materially easier to obtain in Japan. **That belief was wrong**, and it is the
reason this ADR exists.

- **Availability does not differentiate the high-quality HATs.** Neither a current
  HiFiBerry ADC-Pro board nor Pisound has a Japanese distributor. The specific
  DAC2 ADC Pro appears on Amazon.co.jp only through overseas importers at a markup
  (the domestically listed HiFiBerry ADC board is the older, previous-generation
  DAC+ ADC), and Pisound ships from Blokas in Lithuania. The only ADC-capable HATs
  genuinely stocked domestically — the first-party Raspberry Pi Codec Zero
  (through the official Raspberry Pi channel, KSY) and the Waveshare WM8960 — both
  compromise on audio quality (96 kHz cap; mic / consumer-oriented codecs). So for
  a high-quality stereo line-level ADC/DAC HAT, **every option is import-tier in
  Japan**; brand does not change that.
- Two facts from ADR 0008's era still hold: HiFiBerry's Pi 5 driver maturity has
  since resolved (mainline overlay, field-validated real-time full-duplex use),
  and both Pisound and HiFiBerry remove the clock slip by construction because
  both are I2S.

With availability neutralised, the question ADR 0008 explicitly set aside —
"moving to a different SBC or microcontroller" — comes back onto the table. Three
findings drive the reopening.

1. **The "just buy a HAT" convenience is weaker than assumed.** No high-quality
   HAT is domestically stocked, so the low-friction procurement that partly
   justified the HAT direction does not actually hold here.
2. **The Pi 5 is ill-suited to the intended pedal form.** Sustained DSP load needs
   active cooling — a fan, i.e. noise and bulk inside an audio pedal — and the
   Pi 5 + HAT + cooler + case stack is large. This is precisely the heat and
   form-factor problem ADR 0008 deferred, and it bites hardest in the pedal form
   the project is aiming at.
3. **The DSP core's portability cost is small, and now verified.** The real-time
   signal path in `src/dsp/` uses only `exp` and `ln` transcendentals, at
   block / parameter granularity rather than per sample; its only `std` dependence
   is `std::{f32,f64}::consts`, which exist identically in `core`; there are no
   heap allocations on the audio path. A `no_std` port is therefore a mechanical
   swap (`core::` constants plus `libm` for the transcendentals), not a rewrite,
   and the `f64` used in frequency smoothing runs in hardware on a Cortex-M7 with a
   double-precision FPU.

The dedicated-board candidates weighed (not decided) are:

- **Bela** (embedded Linux, Xenomai): stereo line I/O, roughly 1 ms round-trip
  latency, purpose-built for low-latency instruments and documented for
  guitar-pedal enclosures. Because it runs full Linux, the Rust DSP compiles
  unchanged; adopting it is a new host adapter over its render callback (via the C
  API or `bela-rs`) — the same "add an adapter" move as ADR 0007. Higher unit cost
  (roughly $80–250 by variant), import only.
- **Daisy Seed** (Electrosmith, STM32H750 Cortex-M7): stereo codec up to
  24-bit / 192 kHz, sub-1 ms latency, a strong DIY guitar-pedal ecosystem
  (PedalPCB Terrarium), the cheapest (~$30), smallest, and fanless. Rust support
  exists (`daisy`, `libdaisy-rust`, `daisy-embassy`) but is evolving, so it carries
  embedded-Rust glue plus the small `no_std` port and gives up the
  develop-on-PC / deploy-the-same-binary workflow. Import only.
- **Pi 5 + HAT** remains the incumbent: zero software change, ~2 ms with Pisound,
  but the worst on pedal thermal and form.

Their trade axis is software-migration cost versus pedal form-factor, not
availability (all three are import-tier) and not clock correctness (all three
share the audio clock in hardware).

## Decision

- **Withdraw the availability-based HAT ranking.** A high-quality stereo line
  ADC/DAC HAT is import-tier in Japan regardless of brand; availability is not a
  basis for ordering the HATs.
- **Among I2S HATs, rank Pisound first**, with the HiFiBerry DAC2 ADC Pro as the
  cheaper, equal-quality alternative. Under shared import overhead Pisound's price
  premium is relativised, so its pedal-oriented design intent and real-time
  pedigree return as the differentiator; HiFiBerry is the pick when unit cost
  dominates. (Within the HiFiBerry line the DAC2 ADC Pro is still the right
  variant: RCA line I/O suits the unbalanced sources, the Studio DAC/ADC XLR only
  adds balanced connectors at higher cost with the same ADC, the non-Pro DAC2 ADC
  drops the low-jitter clock, and DAC-only or 8-channel boards do not fit.)
- **Reject the Raspberry Pi Codec Zero as a substitute**, rather than rank it last.
  Its domestic availability is real, but accepting its audio-quality compromise
  (96 kHz, mic / AUX-oriented codec) would defeat the reason for leaving USB;
  trading transport quality for interface quality is not a net gain.
- **Keep the Waveshare WM8960 only as a throwaway bring-up board** to confirm that
  an I2S transport removes the clock slip — not as an audio front-end.
- **Reopen the hardware platform choice that ADR 0008 deferred.** Dedicated audio
  boards — Bela and Daisy Seed — are now live candidates against Pi 5 + HAT,
  justified by the absent domestic HAT, the Pi 5 pedal-thermal problem, and the
  verified-small DSP port cost. **This ADR does not choose the platform.** It
  records that the choice is open, lists the candidates and their trade axis, and
  leaves the decision to a later ADR after hands-on evaluation.

## Consequences

- The analog input stage is line level only, unchanged from the prior revision.
  The intended sources are line-output gear — Elektron Digitakt 2 / Digitone 2 /
  Syntakt and Teenage Engineering OP-1 / OP-XY — so no DI / high-impedance
  front-end is designed for. This holds for every candidate platform.
- The DSP portability finding is now a recorded asset: every candidate keeps the
  DSP in Rust; only Daisy requires the small `no_std` port, while Bela and the Pi
  reuse it verbatim. This is what makes reopening the platform question cheap
  rather than a rewrite, and it should be confirmed by reading the `process()`
  hot loop before any port.
- Whichever platform is chosen, ADR 0008's requirement stands that the audio
  interface share its clock in hardware (I2S-class), not via USB isochronous
  transfer; all three candidates satisfy it.
- ADR 0007 still holds and nothing here forces an ALSA-direct backend. If the
  platform moves off Raspberry Pi (Bela or Daisy), ADR 0007's ALSA-direct question
  becomes moot for that path, replaced by a new host adapter (a Bela render
  callback, or a bare-metal audio callback) to be decided in the platform ADR.
- ADR 0008's `48 kHz` / `128×3` / ~11 ms USB figure remains the latency comparison
  baseline for any platform evaluation.

## References

- [ADR 0008](0008-usb-audio-clock-slip-and-i2s-migration.md) — the USB clock-slip
  findings, `128×3` baseline, and the hardware-clock-sharing requirement this ADR
  builds on.
- [ADR 0007](0007-alsa-direct-not-cpal-for-pi-native-backend.md) — the
  host-independent DSP core that keeps a platform move cheap.
- [HiFiBerry: Pi 5 compatibility](https://www.hifiberry.com/blog/pi5-compatibility-with-hifiberry-products/)
  and [DAC2 ADC Pro](https://www.hifiberry.com/shop/boards/dac2adcpro/) — the
  cheaper HAT alternative and its resolved Pi 5 support.
- [Pisound (Blokas)](https://blokas.io/store/product/pisound/) — the first-choice
  HAT, sold direct from Lithuania with no Japanese distributor.
- [Bela documentation](https://docs.bela.io/) and
  [Bela audio latency](https://learn.bela.io/using-bela/about-bela/bela-hardware/)
  — the embedded-Linux dedicated-board candidate; [`bela-rs`](https://github.com/andrewcsmith/bela-rs)
  is a Rust wrapper over its C API.
- [Daisy `daisy` crate](https://crates.io/crates/daisy) and
  [`daisy-embassy`](https://crates.io/crates/daisy-embassy), with the
  [PedalPCB Terrarium](https://www.pedalpcb.com/product/pcb351/) pedal interface —
  the bare-metal Cortex-M7 candidate and its Rust support.
- [Waveshare WM8960 Audio HAT](https://www.waveshare.com/wiki/WM8960_Audio_HAT)
  and [Raspberry Pi Codec Zero](https://www.raspberrypi.com/products/codec-zero/)
  — the domestically stocked but quality-compromised boards.
