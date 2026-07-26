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
3. **The DSP core's portability cost is small, and now verified — but this
   finding is scoped to `src/dsp/` only.** The real-time signal path uses only
   `exp` and `ln` transcendentals, at block / parameter granularity rather than
   per sample; its only `std` dependence is `std::{f32,f64}::consts`, which exist
   identically in `core`; there are no heap allocations on the audio path. A
   `no_std` port is therefore a mechanical swap (`core::` constants plus `libm`
   for the transcendentals), not a rewrite, and the `f64` used in frequency
   smoothing runs in hardware on a Cortex-M7 with a double-precision FPU. It says
   nothing about the cost of the non-DSP layers below.
4. **The control-surface layer (potentiometers, bypass switch) is not yet
   written, and its migration cost differs sharply by candidate.** The design
   sketched in `docs/architecture.md` and `docs/contracts.md` §6 — an SPI-attached
   ADC and GPIO read from a Linux userspace thread, handed to the audio callback
   through a bounded non-blocking queue — assumes an OS. On Bela, still embedded
   Linux, this mostly simplifies: Bela exposes its own analog/digital I/O read
   synchronously inside its `render()` callback, so the SPI/GPIO driver swaps to
   Bela's API and the separate thread-plus-queue plumbing may not even be needed.
   On Daisy Seed or Teensy, both bare-metal, there is no Linux under the control
   layer at all: no `rppal` / `spidev` / sysfs GPIO, and no OS thread to run one
   on. Potentiometer and switch reads move to the vendor's `no_std` HAL
   (`stm32h7xx-hal` / `libdaisy-rust` for Daisy, `imxrt-hal` / `teensy4-rs` for
   Teensy), read directly in the control loop instead of through a queue from a
   separate thread. The CLI layer (`src/cli.rs`, `src/main.rs`) is `std`-dependent
   (`clap`) and does not carry over to bare metal either, though the preset table
   (`src/params/preset.rs`) is `const` data with no file I/O and is unaffected.
   For Daisy/Teensy, then, the migration is not "port the DSP plus write a new
   audio adapter" — it is that plus a rewrite of the control-acquisition and CLI
   host layer, a materially larger scope than finding 3 alone suggests. This
   finding is scoped to the software-driver layer only; it says nothing about
   hardware reference-design availability for actually building the pedal,
   where the two candidates diverge in the opposite direction (see the
   pedal-enclosure-ecosystem addendum below).

The dedicated-board candidates weighed (not decided) are:

- **Bela** (embedded Linux, Xenomai; current product line is **Bela Gem** on
  PocketBeagle 2 — the "Bela Mini" this investigation initially assumed is
  discontinued): stereo line I/O, roughly 1 ms round-trip latency, purpose-built
  for low-latency instruments and documented for guitar-pedal enclosures. Because
  it runs full Linux, the Rust DSP compiles unchanged; adopting it is a new host
  adapter over its render callback via the C API — **not** via `bela-rs`, which
  has had no commits since 2021, predates the Gem/PocketBeagle 2 hardware, and
  was already documented as incomplete (missing `rt_printf`, unresolved
  panic-unwind safety on the render thread) at its last update. Budget a
  from-scratch `bindgen` binding rather than a ready-made wrapper. Verified
  price for the Gem Stereo Starter Kit: $149.00 + $21.00 tracked/signed shipping
  = $170.00, landing at roughly ¥28,000 in Japan after import consumption tax
  (see addendum). Import only, no Japanese distributor. It also has verified
  headroom for FFT-heavy spectral effects (phase vocoders, spectral
  bitcrushing) beyond the current IIR-only DSP profile: an official NE10-based
  phase-vocoder example already ran on far weaker prior-generation Bela
  hardware (see addendum). Its power draw is now substantiated as sharply
  lower than Pi 5's, not just architecturally plausible: TI's own benchmark of
  the same SoC family tops out around 1.5 W under combined CPU+GPU load, and
  Bela's co-founder measured the Gem Multi drawing only tens of milliwatts
  more than a first-generation Bela board at 100% 4-core load, versus
  ">8 W" for a Pi 5 at full clock — supporting the fanless, no-heatsink
  pedal-form claim, though this rests on a forum statement and a related-SoC
  benchmark rather than an independent third-party measurement of
  PocketBeagle 2 itself (see addendum). Set against that: no populate-and-go
  pedal PCB or enclosure design exists for Bela Gem — its own guitar-pedal
  precedent (a forum thread of ad-hoc wire-to-header builds, and the one
  complete "Effect Cape" design) both predate Gem and target the discontinued
  Bela Mini's different header layout and footprint, so building an actual
  pedal enclosure currently means original hardware design work from scratch
  (see addendum).
- **Daisy Seed** (Electrosmith, STM32H750 Cortex-M7; current revision is
  **Seed3**, 32-bit / 192 kHz codec, same price and pin-compatible with the
  older Seed2 DFM): stereo codec up to 24-bit / 192 kHz (32-bit on Seed3),
  sub-1 ms latency. The module itself is stereo in hardware — its 40-pin
  header breaks out dedicated `AUDIO_IN_L`/`AUDIO_IN_R`/`AUDIO_OUT_L`/
  `AUDIO_OUT_R` pads (pins 16–19), and libDaisy's SAI init is 2-channel — so
  "Terrarium is mono" is a carrier-board limitation, not a Daisy Seed one
  (see addendum). PedalPCB Terrarium remains mono-stock and Electrosmith's
  Petal has no confirmed purchase path, but open-source stereo carrier
  boards for Daisy Seed already exist and are actively maintained —
  [GuitarML/FunBox](https://github.com/GuitarML/FunBox) and
  [bkshepherd/DaisySeedProjects](https://github.com/bkshepherd/DaisySeedProjects)
  — giving a concrete path to stereo without a from-scratch board or a
  Terrarium mod. This populate-and-go pedal ecosystem (Terrarium, FunBox,
  DaisySeedProjects) is a maturity gap Bela Gem doesn't currently have an
  answer for (see addendum). Cheapest candidate, smallest, and fanless. Rust
  support has consolidated:
  `daisy-embassy` (Embassy async) is the actively maintained option, while
  `libdaisy-rust` has had no release since 2021; either way it carries
  embedded-Rust glue plus the small `no_std` port and gives up the
  develop-on-PC / deploy-the-same-binary workflow. Verified price: $29.99 +
  $12.36 standard shipping = $42.35 for either Seed3 or Seed2 DFM, landing
  under Japan's ¥10,000 duty/tax-free threshold at roughly ¥6,600 (see
  addendum). Import only. Its single Cortex-M7 core is comfortable for the
  current IIR-only DSP and for additional comb-filter / multi-band-filter
  effects, but is a tight fit for FFT-heavy spectral effects (phase vocoders):
  community attempts report high CPU cost and at least one abandoned in favor
  of a delay-based approach (see addendum).
- **Pi 5 + HAT** remains the incumbent: zero software change, ~2 ms with Pisound,
  but the worst on pedal thermal and form.

Their trade axis is software-migration cost versus pedal form-factor, not
availability (all three are import-tier) and not clock correctness (all three
share the audio clock in hardware). A third axis applies specifically between
Bela and Daisy if FFT-heavy spectral effects (phase vocoders, spectral
processing) are pursued later: Daisy's single Cortex-M7 core is a tight fit
for that workload, while Bela's multi-core Linux SoC has proven headroom
(see addendum). Pi 5's quad Cortex-A76 has still more headroom than either,
so this axis does not distinguish it from Bela.

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
  DSP in Rust; only Daisy (and Teensy) require the small `no_std` port, while
  Bela and the Pi reuse it verbatim. It should be confirmed by reading the
  `process()` hot loop before any port.
- That DSP finding does not extend to the not-yet-written control-surface layer
  (potentiometers, bypass switch) or the CLI/host shell. Bela's own analog/
  digital I/O API is expected to simplify that layer (synchronous reads inside
  `render()`, possibly no separate thread/queue at all). Daisy and Teensy instead
  require rewriting it against a vendor `no_std` HAL and dropping the `clap`-based
  CLI, since there is no Linux underneath to run the Pi-era SPI/GPIO/thread
  design on. This asymmetry — small DSP cost everywhere, but a real
  control-layer/CLI rewrite specific to Daisy/Teensy — is part of the software-
  migration side of the trade-off any later platform ADR must weigh, alongside
  pedal form-factor.
- Whichever platform is chosen, ADR 0008's requirement stands that the audio
  interface share its clock in hardware (I2S-class), not via USB isochronous
  transfer; all three candidates satisfy it.
- ADR 0007 still holds and nothing here forces an ALSA-direct backend. If the
  platform moves off Raspberry Pi (Bela or Daisy), ADR 0007's ALSA-direct question
  becomes moot for that path, replaced by a new host adapter (a Bela render
  callback, or a bare-metal audio callback) to be decided in the platform ADR.
- ADR 0008's `48 kHz` / `128×3` / ~11 ms USB figure remains the latency comparison
  baseline for any platform evaluation.

## Addendum: verified landed pricing for Bela and Daisy (2026-07-26)

The unit-cost figures in the Decision/Context above were checked against real
checkout totals (item + shipping to Japan) rather than list price alone, since
list price understates what actually lands. Recorded here so the estimate
isn't redone from scratch later; assumes $1 ≈ ¥155, the goods-plus-shipping
total is the customs base, and Japan's simplified personal-import consumption
tax applies at 10% of 60% of that base (audio/computing boards are duty-free;
see ADR 0008's sibling investigation notes on HS classification). Amounts
below are the total charged at checkout, not list price only.

| Platform | Checkout total (USD) | Landed estimate (JPY) | Notes |
|---|---|---|---|
| Bela Gem Stereo Starter Kit | $149.00 + $21.00 shipping = $170.00 | ≈ ¥28,000 | Tracked/signed shipping from shop.bela.io; above the ¥10,000 tax-free threshold, so import consumption tax applies. |
| Daisy Seed3 | $29.99 + $12.36 shipping = $42.35 | ≈ ¥6,600 | Standard shipping from daisy.audio; under the ¥10,000 threshold, so likely tax/duty-free under current rules. |
| Daisy Seed2 DFM | $29.99 + $12.36 shipping = $42.35 | ≈ ¥6,600 | Same checkout total as Seed3 (older codec revision, same price point). |
| Electrosmith Petal | — | — | `daisy.audio/daisy/petal` redirects to a 404 and Petal is absent from the current product collection; the Legacy page carries no price, purchase button, or explicit "discontinued" label, so treat as unavailable rather than confirmed-discontinued. Not found for sale on Reverb or Perfect Circuit either (tool-access-limited, not confirmed absent). No longer the blocker it looked like — stereo doesn't require Petal or a Terrarium mod; see the stereo-I/O addendum below. |

This changes two things from the unverified figures the Decision previously
carried: Bela's real entry cost is close to the top of the old "$80–250"
range once a working kit (not a bare board) and shipping are counted, while
Daisy's stays close to the bottom and likely clears Japan's import tax
threshold entirely — the cost gap between the two candidates is larger in
practice than the old range implied. Japan's small-import tax exemption is
scheduled for repeal per the FY2026 tax reform outline
(<https://www.mof.go.jp/tax_policy/tax_reform/outline/fy2026/08taikou_gaiyou.pdf>),
decided 2025-12-26 with no enforcement date set yet; the Daisy figure above
assumes current rules and should be rechecked if ordering after that repeal
takes effect.

## Addendum: resolving Daisy Seed's stereo I/O gap (2026-07-26)

Stereo I/O is a hard requirement for this project — the intended sources
(Elektron Digitakt 2 / Digitone 2 / Syntakt, Teenage Engineering OP-1 / OP-XY;
see Consequences) are all stereo line-output gear. The prior framing —
"Terrarium is mono, Petal is unavailable" — made Daisy Seed look unable to
meet that requirement. That framing conflated the module with the carrier
board; it does not survive closer inspection.

- **The Daisy Seed module is stereo in hardware, not just in the codec spec.**
  Its 40-pin header breaks out dedicated `AUDIO_IN_L` (pin 16), `AUDIO_IN_R`
  (pin 17), `AUDIO_OUT_L` (pin 18), and `AUDIO_OUT_R` (pin 19) pads — the same
  pin numbers on Seed3 — and libDaisy initialises the STM32's SAI peripheral
  for 2 channels regardless of carrier board. Any carrier board that wires
  those four pads gets stereo; Terrarium's mono-only wiring is that board's
  own design choice, not a Daisy Seed constraint (inferred from
  GuitarML/DaisyEffects only ever indexing `in[0]`/`out[0]` for Terrarium —
  Terrarium's schematic itself isn't open-sourced, only `terrarium.h` pin
  definitions are, so this is inferred rather than confirmed against the PCB
  pattern).
- **Open-source stereo carrier boards for Daisy Seed already exist and are
  actively maintained**, avoiding both a from-scratch PCB design and reliance
  on the unavailable Petal:
  - [GuitarML/FunBox](https://github.com/GuitarML/FunBox) — built by the same
    author as the Terrarium-oriented DaisyEffects project, explicitly a
    "stereo guitar pedal platform using Daisy Seed", 125B enclosure, full
    KiCad schematic/PCB/BOM/Gerbers in-repo, 253 stars, last updated
    2026-07-23. Community build notes (an op-amp swap, TL074→MCP6024, to fix
    noise/phase issues) are documented on the
    [PedalPCB forum](https://forum.pedalpcb.com/threads/developing-a-custom-pcb-for-daisy-seed-funbox.22152/)
    and in a [build guide](https://keyth72.medium.com/funbox-build-guide-afbd8046121e).
  - [bkshepherd/DaisySeedProjects](https://github.com/bkshepherd/DaisySeedProjects)
    — stereo I/O, multiple enclosure sizes (125B/1590B), MIT-licensed PCB
    files; a second independent option to compare against FunBox.
- **Daisy Pod** ($68, in stock direct from Electrosmith) has stereo 3.5 mm
  line I/O and is a viable bench-prototyping stopgap, but it is a bare board
  on a 3.5 mm connector, not a pedal enclosure — it does not replace a
  FunBox-style carrier for the final build.
- **Terrarium plus a hand-wired stereo mod** was considered and set aside: the
  Daisy-Seed-side pins are accessible, but Terrarium's schematic isn't
  open-sourced, no established community mod procedure was found, and it is
  strictly worse than starting from an already-stereo design like FunBox.

**Net effect on the Decision**: Daisy Seed's stereo gap is resolved as a
carrier-board choice — use FunBox or DaisySeedProjects instead of Terrarium —
not a hardware limitation of the module. This removes what had been the
sharpest argument against Daisy Seed for this project; the remaining
trade-offs (the control-surface/CLI rewrite cost and `no_std` port in finding
4, Rust tooling maturity) are unchanged.

## Addendum: compute headroom for future FFT-heavy spectral effects (2026-07-26)

This project's current DSP (`src/dsp/`) is IIR-only (biquads, envelope
followers), which is the workload finding 3 verified as cheap to port. Future
effects under consideration go beyond that profile: a "pitchmap/chroma"-style
effect combining a harmonic resonator, multi-stage bandpass filter, comb
filter, and phase vocoder; and a spectral bitcrusher that bit-crushes only a
target frequency band. The comb-filter / multi-BPF / harmonic-resonator part
of that is still IIR (biquad cascades), so it carries the same low cost
finding 3 already established. The phase-vocoder part is a different
workload — FFT/IFFT, phase accumulation, overlap-add — and is where Bela and
Daisy diverge sharply.

- **Bela**: its official examples repository ships a working phase vocoder
  (`examples/Audio/FFT-phase-vocoder/render.cpp`), built on the NE10
  NEON-optimised FFT library, using a 2048-point FFT at a 512-sample hop
  (4× overlap), with the FFT/IFFT computation offloaded from the audio ISR to
  a lower-priority Xenomai `AuxiliaryTask` so it can't threaten the audio
  deadline. This example already ran acceptably on the older, single-core
  Cortex-A8 @ 1 GHz Bela hardware; the current Bela Gem's PocketBeagle 2 host
  has 2–4 Cortex-A53 cores at up to 1.4 GHz plus a generation of
  microarchitecture improvement, so materially more headroom is expected,
  though this has not been benchmarked on Gem hardware itself. Bela also
  ships a general-purpose `Fft`/`Convolver` library, i.e. FFT-based processing
  is a supported, ordinary use case there, not a special-case struggle.
- **Daisy Seed**: DaisySP's stock `PitchShifter` is delay-line/cross-fade
  based (SOLA-style), not FFT — there is no first-party phase-vocoder
  implementation. Community attempts exist (`shy_fft.h`-based phase vocoders
  at FFT sizes 1024–4096), but the developer of one such project reported
  struggling with CPU cost enough to abandon the FFT/PSOLA approach and
  revert to the delay-based method. A phase-vocoder pitch shifter on Teensy 4
  (Cortex-M7 @ 600 MHz — faster than Daisy's 480 MHz) is reported to use
  roughly 75% CPU on its own. No precise CPU-percentage or latency figure for
  Daisy itself was obtained, but the pattern across same-class Cortex-M7 chips
  points the same way: a phase vocoder is achievable as a standalone effect,
  but running it concurrently, full-tilt, alongside the comb-filter/multi-BPF
  chain on Daisy's single core is unlikely to fit comfortably. Realistic
  mitigations if Daisy is chosen and this effect is pursued: a smaller FFT
  size, a larger hop (trading latency for headroom), or making the
  phase-vocoder effect and the heavier IIR chains mutually exclusive
  (one active mode at a time) rather than layering them.
- **Spectral bitcrusher**: surveying existing plugins (Digital-Hell, Hilofi
  Multiband Bitcrusher, MeldaProduction MBitFunMB) and one embedded example
  (`thesquaregroot/uncertainty-dffb`, an 8-band elliptic-IIR multiband
  bitcrusher on an RP2040) shows the dominant real-world implementation is
  multiband IIR filtering plus per-band bit reduction, not FFT-bin
  quantisation. That keeps this effect cheap on either platform; an FFT-bin
  variant is also possible and would simply reuse whatever phase-vocoder FFT
  infrastructure already exists.

**Net effect on the Decision**: this doesn't change today's decision (the
platform still isn't chosen), but it adds a real consideration favoring Bela
over Daisy specifically if FFT-heavy spectral effects are a serious future
direction, alongside the existing pedal-form-factor and control-surface/CLI
migration-cost axes. It should be weighed, not treated as settled — no direct
CPU-load or latency benchmark was obtained for either the Bela Gem or
PocketBeagle 2's NEON FFT throughput specifically; the Bela headroom claim
rests on the phase-vocoder example having run on much weaker prior-generation
hardware; and the Daisy tightness claim rests on cross-project community
reports (shy_fft.h, Teensy 4) rather than a first-party benchmark on this
project's own effect chain.

## Addendum: Bela/PocketBeagle 2 power draw versus Raspberry Pi 5 (2026-07-26)

Finding 2 above blames Pi 5's active-cooling requirement for its poor fit in
a pedal enclosure, and Bela's candidacy leans on not having that problem. That
only holds if PocketBeagle 2 (Cortex-A53, a weaker core family than Pi 5's
Cortex-A76) actually draws meaningfully less power — a slower core does not
automatically mean a cooler one. This was checked rather than assumed.

- **TI's own benchmark** (application note SPRADG1, Feb 2024, on-board
  current-monitor measurements on SK-AM62B-class hardware — the AM625/AM6254
  die family used in Bela Gem/PocketBeagle 2's Rev A1): OS idle 316–443 mW
  across 200 MHz–1.4 GHz, deep sleep 14.6–32.5 mW, and combined CPU+GPU
  maximum load (4-core stress + glmark2 at 1.4 GHz) **1.54 W**. This is
  SoC+DDR power only, not a full board, and TI does not publish a single TDP
  figure — only junction-temperature limits (Commercial 0–95 °C) and a
  θJA figure TI itself says not to use for thermal design.
- **Bela's co-founder (giuliomoro) posted measured figures** on the Bela
  forum ([thread](https://forum.bela.io/d/6483-bela-gem-two-new-boards)):
  "with the four cores spinning at 100% and the Bela program running, I
  measured only 60 mW more on the Gem Multi than on a Bela board running a
  Bela program... That's great also in comparison to the >8 W you get when
  running a Pi 5 at full clock. For the Gem and the PB2: no heatsink needed,
  no fan needed, don't be afraid of enclosures." The
  [Crowd Supply campaign page](https://www.crowdsupply.com/bela/bela-gem-stereo-and-multi)
  publishes matching board-level figures: 0.10 W (Stereo) / 0.59 W (Multi)
  board-only, 1.90 W (Stereo) / 2.39 W (Multi) with the SBC, against ">8 W"
  for Pi5-based comparison products.
- **These two independent sources land in the same order of magnitude**
  (TI's SoC-level ~1.5 W max versus Bela's board-level 1.9–2.4 W with SBC),
  which cross-checks each other reasonably well and is consistent with
  Pi 5's already-established 2.7–3.6 W idle / up to 15.9–16.8 W combined-load
  figures being roughly an order of magnitude higher.
- **Caveats, stated plainly**: the "no heatsink, no fan" claim is a forum
  statement by a co-founder, not a datasheet or independent review — no
  third-party PocketBeagle 2-specific power or thermal benchmark was found.
  TI's own documentation explicitly disclaims using θJA as a design
  parameter, so thermal margin inside a sealed pedal enclosure specifically
  is unverified for either platform. The "old-generation Bela has years of
  pedal-enclosure use" precedent is a mechanical-fit precedent only — no
  thermal data accompanies it, and the SoC changed from a single-core
  Cortex-A8 to a multi-core Cortex-A53 between generations, so it doesn't
  carry over as thermal evidence. One red herring surfaced during this
  research and is worth flagging so it isn't repeated: warnings about the
  **original** PocketBeagle (2017, OSD3358 chip) overheating are for a
  different, unrelated chip and do not apply to PocketBeagle 2.

**Net effect on the Decision**: the concern that PocketBeagle 2 might not
meaningfully differentiate from Pi 5 on heat appears unfounded — two
independent-ish sources agree it draws roughly an order of magnitude less
power under load. This substantiates finding 2's argument for Bela over Pi 5
rather than undermining it. It is not yet fully verified, though:
independent, third-party measurement of Bela Gem/PocketBeagle 2 power draw
and temperature inside a sealed pedal enclosure remains an open item for
hands-on evaluation before the platform ADR.

## Addendum: pedal-enclosure reference-design gap, Bela Gem vs Daisy Seed (2026-07-26)

Finding 4 frames Bela's control-surface story favorably: its `render()` API
"mostly simplifies" the software side. That's a software-driver-layer claim
only. Whether it makes the pedal easier to actually *build* is a separate
question, and Daisy's ecosystem (Terrarium, FunBox, DaisySeedProjects — a
buy-or-download-and-populate path covering the PCB, pot/switch/LED wiring,
and the enclosure) sets a high bar. This was checked against Bela Gem
specifically, not assumed by analogy to old-generation Bela.

- **No populate-and-go pedal PCB or enclosure design exists for Bela Gem.**
  The `BelaPlatform` GitHub org has no such repository. The clearest evidence
  is a Bela forum thread from this month
  ([STL/3D Models for Bela Gem/Gem Multi](https://forum.bela.io/d/8248-stl-3d-models-for-bela-gemgem-multi),
  2026-07-08): a user asked for an enclosure model, and Bela's own answer was
  to convert the board's KiCad 3D-preview STEP file yourself — over a year
  after Gem's launch, still no ready enclosure design, official or
  community. The one community breakout board found,
  [yannseznec/belaGemWorkshopPCB](https://github.com/yannseznec/belaGemWorkshopPCB),
  only exposes header pins to connectors; it has no pot/switch/LED/enclosure
  design and the author states it hasn't been tested yet.
- **Old-generation Bela's pedal precedent was itself ad-hoc DIY, not a
  manufactured-board ecosystem.** In the
  [Embed Bela in a guitar pedal? thread](https://forum.bela.io/d/111-embed-bela-in-a-guitar-pedal)
  (53 posts), Bela's co-founder recommends wiring panel-mount pots/switches
  directly to Bela's analog inputs by hand rather than designing a dedicated
  expansion board, and a contributor's build repurposes a generic Hammond/
  Farnell diecast enclosure, not a Bela-specific one. The one complete
  one-stop design that does exist —
  [leheltorok/effect_cape_for_bela_mini](https://github.com/leheltorok/effect_cape_for_bela_mini)
  (KiCad schematic/PCB, BOM, laser-cut acrylic enclosure, CC BY-NC-SA) —
  targets **Bela Mini**, whose PocketBeagle (first-generation, now
  discontinued) has a different header layout and footprint than the Gem's
  PocketBeagle 2, so it needs re-design, not a straight port.
- **Bela Gem's physical connectors are bare 2.54 mm pin headers**
  (BeagleBone/PocketBeagle cape-compatible stacking headers), not locking
  connectors. Direct wiring to panel-mount pots/switches is confirmed viable
  via forum examples (e.g.
  [Bela Gem Stereo digital I/O help!](https://forum.bela.io/d/7747-bela-gem-stereo-digital-io-help)),
  so a breakout board isn't strictly required. But some GPIO pins are shared
  with the PRU real-time coprocessor and need register-level `devmem2` pokes
  or a custom device-tree overlay to use (see
  [Bela Gem Neopixel Pd](https://forum.bela.io/d/7627-bela-gem-neopixel-pd)) —
  a layer of complexity beyond Daisy's vendor-HAL-mediated GPIO access.
- **No Gem-specific pedal/stompbox community project was found post-launch**
  (Gem shipped 2026-02-04). A 2026-04-08 guitar-pedal-adjacent forum thread
  ([~2.7 kHz noise when Bela is connected to guitar pedal
  ground](https://forum.bela.io/d/7808-27khz-noise-when-bela-is-connected-to-guitar-pedal-ground))
  still references "Bela Rev C" — the old generation — suggesting the
  pedal-building side of the Bela community hasn't migrated to Gem yet. This
  is an absence-of-evidence finding, not a confirmed absence: Reddit and
  Crowd Supply's comment section could not be searched (search-quota
  exhaustion during this research), so an undiscovered Gem pedal project
  can't be ruled out.
- **Bela's official hardware design files are CC BY-NC** (non-commercial),
  an additional licensing constraint that doesn't apply to Daisy's
  MIT-licensed FunBox/DaisySeedProjects designs.

**Net effect on the Decision**: finding 4's claim that Bela "mostly
simplifies" the control-surface layer is accurate at the software-API level,
but doesn't extend to hardware. At the level of actually building a pedal —
pots, switches, LEDs, enclosure — the ecosystem gap runs the opposite
direction: Daisy has multiple mature, populate-and-go reference designs;
Bela Gem currently has none, and building one means original schematic/PCB/
enclosure design work. This nuances finding 4 rather than reversing it — the
audio-callback-side software win for Bela stands on its own — but the
platform ADR should weigh Daisy's hardware-ecosystem maturity as a concrete
point in Daisy's favor, alongside the points already recorded in Bela's
favor (FFT-heavy spectral effect headroom, substantiated lower power draw).

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
  is a Rust wrapper over its C API but has had no commits since 2021 and
  predates the current Gem/PocketBeagle 2 hardware, so treat it as
  unmaintained. [shop.bela.io](https://shop.bela.io/) is the current storefront
  (Bela Gem Stereo/Multi on PocketBeagle 2; "Bela Mini" is discontinued).
- [`daisy-embassy`](https://github.com/daisy-embassy/daisy-embassy) — the
  actively maintained Rust HAL for Daisy Seed (Embassy async runtime);
  [`libdaisy-rust`](https://github.com/mtthw-meyer/libdaisy-rust) is the older,
  now-stalled alternative (no release since 2021). [PedalPCB Terrarium](https://www.pedalpcb.com/product/pcb351/)
  is the bare-metal Cortex-M7 candidate's DIY pedal interface (mono stock I/O);
  [daisy.audio](https://daisy.audio/) is the current storefront (Seed3 is the
  current revision). [GuitarML/FunBox](https://github.com/GuitarML/FunBox) and
  [bkshepherd/DaisySeedProjects](https://github.com/bkshepherd/DaisySeedProjects)
  are open-source stereo carrier boards for Daisy Seed that resolve
  Terrarium's mono limitation.
- [Waveshare WM8960 Audio HAT](https://www.waveshare.com/wiki/WM8960_Audio_HAT)
  and [Raspberry Pi Codec Zero](https://www.raspberrypi.com/products/codec-zero/)
  — the domestically stocked but quality-compromised boards.
- [Bela FFT-phase-vocoder example](https://github.com/BelaPlatform/Bela/blob/master/examples/Audio/FFT-phase-vocoder/render.cpp)
  and [Fft](https://github.com/BelaPlatform/Bela/blob/master/libraries/Fft/Fft.h) /
  [Convolver](https://github.com/BelaPlatform/Bela/blob/master/libraries/Convolver/Convolver.h)
  libraries — Bela's proven, standard-supported FFT-based spectral processing;
  [NE10](https://github.com/projectNe10/Ne10) is the NEON FFT library they use.
- [DaisySP `PitchShifter`](https://github.com/electro-smith/DaisySP/blob/master/Source/Effects/pitchshifter.h)
  — the stock delay-line/cross-fade pitch shifter (not FFT-based);
  [community discussion of `shy_fft.h` phase-vocoder attempts](https://community.daisy.audio/t/my-battle-with-shy-fft-h-and-what-it-taught-me-shyfft-quick-guide/8455)
  and [DD4WH/BirdSongPitchShifter](https://github.com/DD4WH/BirdSongPitchShifter)
  (a Teensy 4 phase-vocoder pitch shifter reported at ~75% CPU) — evidence for
  the Cortex-M7-class compute ceiling on FFT-heavy spectral effects.
- [jazamatronic/ParametricChorus](https://github.com/jazamatronic/ParametricChorus)
  and [jazamatronic/ModalResonators](https://github.com/jazamatronic/ModalResonators)
  — existing Daisy Pod projects running 8–20 parallel biquads in real time,
  evidence that comb-filter / multi-BPF / harmonic-resonator work is cheap on
  Daisy. [thesquaregroot/uncertainty-dffb](https://github.com/thesquaregroot/uncertainty-dffb)
  — an embedded (RP2040) multiband IIR bitcrusher, evidence for the
  filter-bank (non-FFT) approach to a spectral bitcrusher.
- [TI AM625 power benchmark (SPRADG1)](https://www.ti.com/lit/pdf/spradg1) and
  [AM625 datasheet](https://www.ti.com/lit/ds/symlink/am625.pdf) — official
  SoC-family power figures (idle ~0.3–0.4 W, max CPU+GPU load ~1.5 W) for the
  AM6254 used in Bela Gem/PocketBeagle 2 Rev A1. [Bela Gem: two new boards
  (forum)](https://forum.bela.io/d/6483-bela-gem-two-new-boards) — Bela
  co-founder's measured power-draw comparison against Pi 5 (">8 W") and the
  "no heatsink, no fan needed" claim; the
  [Crowd Supply campaign](https://www.crowdsupply.com/bela/bela-gem-stereo-and-multi)
  publishes matching board-level figures (0.10–0.59 W board only, 1.90–2.39 W
  with the SBC).
- [STL/3D Models for Bela Gem/Gem Multi (forum, 2026-07-08)](https://forum.bela.io/d/8248-stl-3d-models-for-bela-gemgem-multi)
  — confirms no ready Gem enclosure design exists over a year after launch;
  [yannseznec/belaGemWorkshopPCB](https://github.com/yannseznec/belaGemWorkshopPCB)
  is the one (untested) community Gem breakout board, with no pot/switch/
  enclosure design. [Embed Bela in a guitar pedal?](https://forum.bela.io/d/111-embed-bela-in-a-guitar-pedal)
  and [leheltorok/effect_cape_for_bela_mini](https://github.com/leheltorok/effect_cape_for_bela_mini)
  are the old-generation Bela Mini pedal precedents (ad-hoc wiring and the
  one complete KiCad/enclosure design, respectively) that don't carry over to
  Gem's different header layout. [Bela Gem Stereo digital I/O
  help!](https://forum.bela.io/d/7747-bela-gem-stereo-digital-io-help) and
  [Bela Gem Neopixel Pd](https://forum.bela.io/d/7627-bela-gem-neopixel-pd)
  — confirm direct header wiring works but some GPIO needs PRU-level
  workarounds. [~2.7 kHz noise when Bela is connected to guitar pedal
  ground](https://forum.bela.io/d/7808-27khz-noise-when-bela-is-connected-to-guitar-pedal-ground)
  — a 2026-04-08 guitar-pedal thread still on old-generation "Bela Rev C",
  evidence the pedal-building community hasn't visibly migrated to Gem yet.
