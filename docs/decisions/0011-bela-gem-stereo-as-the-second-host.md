# ADR 0011: Add Bela Gem Stereo as a Second Host, Alongside JACK

## Status

Accepted

Amends [ADR 0009](0009-hardware-platform-choice-reopened.md), which reopened
the hardware platform question and explicitly declined to answer it. This ADR
answers it, and corrects the one premise in ADR 0009 that turned out to be
about a different piece of software than its name suggested.

Revises two decisions in
[ADR 0010](0010-three-layer-control-surface-and-newest-value-handoff.md) in
place: what the platform seam is, and what "layer B is platform-independent"
obliges a host to do.

## Context

ADR 0009 left three candidates on the table and refused to rank them, because
the deciding evidence was hands-on rather than documentary. A Bela Gem Stereo
has since arrived, which settles the part of the question that needed hardware
to settle.

Two things had changed since ADR 0009 was written, both in the direction of
Bela.

**The software cost is lower than ADR 0009 budgeted.** ADR 0009 assessed
`bela-rs` as unmaintained since 2021, predating Gem hardware, missing
`rt_printf`, and unresolved on panic-unwind safety across the render thread —
and concluded that adopting Bela meant writing a `bindgen` binding from
scratch. That assessment was accurate, and it was about
[`andrewcsmith/bela-rs`](https://github.com/andrewcsmith/bela-rs). A second,
unrelated project publishes the `bela` and `bela-sys` crates from
[`akiomik/bela-rs`](https://github.com/akiomik/bela-rs): written against Gem
Stereo, measured on one, with `rt_println!` and with `panic = "abort"`
documented as the premise for crossing the callback boundary. Every specific
objection ADR 0009 raised is answered by the crate oxtt can actually use, and
the binding it budgeted for does not need writing.

**The port is smaller than a port.** ADR 0010 split the control surface into
three layers specifically so that a platform which reads its controls inside
the audio callback could keep the middle one and discard the transport. That
prediction was made against no hardware. It held exactly.

The DSP was never the question: `OttProcessor` is `Copy`, allocates nothing,
holds all of its state inline, and depends on no audio API. What remained was a
host adapter and a way to read six pots and a switch.

## Decision

**Add Bela Gem Stereo as a second host, and keep JACK.** The two coexist behind
Cargo features (`jack-host`, on by default, and `bela-host`) rather than one
replacing the other. Nothing below the host adapters is duplicated: the DSP,
the parameters, the presets and the control surface's mapping layer are shared,
and build with neither feature enabled — which the `no-host` CI job asserts.

Keeping JACK is not indecision about the hardware. It is what keeps the DSP
runnable on a development machine, and it is the only configuration in which
the offline renderer, the soak scripts and the existing Raspberry Pi
verification still mean anything. Retiring it is a separate decision, to be
made from measurements this ADR does not have.

The rest of this section is the decisions the port itself forced.

**Use `akiomik/bela-rs` (`bela` 0.6), not a hand-written binding.** Correcting
ADR 0009 as above. Six gaps were identified against it during planning and
raised upstream; four became API — `Settings::audio_sample_rate`, `PairedIo`,
the `links`-metadata relay of the device link arguments, and
`BelaApplication::validate_settings` — and two were closed as convenience
rather than capability and absorbed on this side. None was a blocker at the
time it was raised.

**Ask for 48 kHz, not the board's 44.1 kHz default.** It is the rate ADR 0008
measured the Raspberry Pi host at, so the two platforms stay comparable. It
also makes the control-surface read divisor exact — see below — which 44.1 kHz
does not. A Gem Stereo was measured running every rate from 8 kHz to 106 kHz,
so this is well inside what the hardware does.

**Render on one thread.** Bela divides a block across render threads by frame
range, and every filter and envelope oxtt has carries state from one frame to
the next; the crossover alone is twelve biquads per channel (ADR 0001). A
second render thread would start mid-signal from a state that never saw the
frames before it. The contract in `docs/contracts.md` §3 — that output is
bit-identical however the input is partitioned — is a property of one state
being carried through one stream, not a licence to split the stream.

**Refuse a bad configuration before the audio system exists, not from
`setup`.** `setup` runs inside `Bela_initAudio` with the hardware already up,
so a refusal from there fails initialisation and leaves the process unable to
build another audio system — measured, not inferred. `validate_settings` runs
before initialisation and returns a typed reason, so that is where the thread
count, the analog and digital channel counts, and the Nyquist-relative
crossover limit are checked. The audio channel counts cannot move there,
because libbela ignores the requested counts and the delivered ones do not
exist until initialisation has run; they stay in `setup` as the one case that
cannot be served earlier.

**Do not pass a command line on to libbela.** `Bela::new`, not
`Bela::new_with_args`. Every setting `oxtt-bela` needs has a flag of its own.
The reason is not safety — `validate_settings` catches an overridden thread
count — but that there should be one `--help`, and that several of libbela's
own options end the process rather than report an error.

**Promote `OttProcessor::process_frame` to the public API.** Bela hands over a
block as interleaved frames, and `PairedIo::frames` pairs each frame's input
with its output. Calling the existing block API through it would mean
de-interleaving into scratch buffers and copying every sample twice for
nothing. `process` was already a loop over `process_frame`, so this exposes
what was there rather than adding a second implementation, and the same
panic-free proof now covers both.

**Read the control surface in `render_pre`, and drop layer C entirely.** ADR
0010 predicted the shape; the callback that makes it work is `render_pre`,
which holds the mapping layer (`&mut self`) and the render states
(`&mut [RenderState]`) at the same time. A snapshot goes straight from one to
the other — no queue, no atomic, no thread. Layer C is compiled only under
`jack-host`, so the dependency graph says so too: `triple_buffer` is a
`jack-host` dependency.

**Decimate the control reads to ~500 Hz rather than retuning layer B.** This is
the one thing ADR 0010 did not anticipate. Layer B has no clock: its filter
coefficient is per read and its debounce counts reads, so the caller's rate is
what turns those constants into times — and they were calibrated against the
Pi's 500 Hz polling and jitter measured at it. Bela's callback runs at 3000
blocks a second, which would cut the bypass debounce from 28 ms to 5 ms, below
the make/break time of the latching switch it exists to ride out. Reading on
every sixth block restores it exactly. The alternative — Bela-specific
constants in layer B — would make the shared layer platform-dependent and put
its calibration out of reach of the measurement that justified it.

**Revised by [ADR 0012](0012-the-jitter-deadband-belongs-to-the-control-source.md)
for the deadband alone.** Once the Gem's control surface was measured, the
deadband proved to be a property of the converter rather than of layer B, and
the two boards' figures differ by more than an order of magnitude. It is now
supplied by layer A on both hosts — which is not the rejected alternative
above but its inverse: layer B ends up naming no board at all, and keeps only
the rule the value has to satisfy. The filter coefficient and the debounce
count stay shared, and `PollDecimator` stays the reason they can be.

**Rename `AdcCount` to `PotPosition`.** The type documented itself as "one
MCP3008 conversion result", and Bela has no MCP3008. What is actually shared is
a pot's position quantised to a fixed scale; that the Pi's converter produces
that scale directly is a fact about the Pi. The range stays 0..=1023, because
layer B's constants — the deadband above all — are defined in those steps.

**Give each platform its build settings through the environment, not
`.cargo/config.toml`.** Both boards compile for `aarch64-unknown-linux-gnu` and
want opposite things from it: a Pi 5 is a Cortex-A76 built natively, a Gem is a
Cortex-A53 cross-compiled with a linker of its own. A `[target.<triple>]`
section cannot tell them apart, and whichever it described would silently
mis-build the other. `scripts/pi/build.sh` and `scripts/bela/build.sh` each
export what they need. A bare `cargo check --target aarch64-unknown-linux-gnu`
is then a configuration that needs no linker and no sysroot, which is what CI
type-checks the device half with.

## Consequences

- oxtt has two hosts and one DSP. `src/bela_host.rs` is the counterpart of
  `src/jack_host.rs` and contains no DSP, as that one contains none.
- **Almost all of the Bela host is testable on a development machine.**
  `bela`'s device code is behind a `bela_device` cfg its build script sets only
  for aarch64 Linux, so the application type, the control conversion and their
  tests compile and run on macOS and on an ordinary CI runner, with no sysroot
  and no cross toolchain. Only `bela_host::run` needs a board. This is a
  sharper split than `pi-controls` has — `rppal` cannot compile on macOS at all
  — and it is why the Bela host costs the development loop nothing.
- **CI gains three jobs and still never builds a device binary.** `bela-host`
  lints and tests the portable part; `bela-device` type-checks the
  `cfg(bela_device)` half by cross-*checking*, which does not link and so needs
  neither `libbela` nor the sysroot; `no-host` asserts the shared code still
  builds with no host at all. Producing a runnable binary remains a
  cross-compile from a development machine against a sysroot synced off the
  board, which no runner has.
- **The panic-free proof still only runs on x86-64.** It needs
  `cargo test --release`, which needs to execute the tests, which needs the
  target. This is the position the Raspberry Pi is already in, and the same
  reasoning applies: the proof is a property of the source and the optimiser.
- **Layer B is untouched — and now has a documented duty attached.** Not one
  constant changed. In exchange, "a host may drive layer B directly" now comes
  with "and owes it reads at the rate its constants were calibrated for"
  (`docs/contracts.md` §8).
- The offline renderer keeps using the block API and is unaffected. Two entry
  points into the DSP now exist; §3 states that they agree.
- **What is not decided.** Whether JACK and `pi-controls` are eventually
  retired. Whether a Gem Stereo stays cool in a sealed pedal enclosure — the
  open item ADR 0009 recorded, still open, and now the main thing standing
  between this board and the pedal. Both need measurements that need the
  hardware, which is now on hand.

## References

- [`akiomik/bela-rs`](https://github.com/akiomik/bela-rs) — the `bela` and
  `bela-sys` crates, and `docs/board-facts.md`, which is where every measured
  figure quoted here comes from: the analog full scale, the sample-rate
  ceiling, the analog output count, the digital I/O latency, and the behaviour
  of a failed initialisation.
- Upstream issues raised from this port:
  [#109](https://github.com/akiomik/bela-rs/issues/109) sample rate,
  [#110](https://github.com/akiomik/bela-rs/issues/110) paired input/output,
  [#111](https://github.com/akiomik/bela-rs/issues/111) link-argument relay,
  [#112](https://github.com/akiomik/bela-rs/issues/112) pre-init validation,
  [#113](https://github.com/akiomik/bela-rs/issues/113) host-side contexts and
  [#115](https://github.com/akiomik/bela-rs/issues/115) frame views (both
  closed as convenience rather than capability),
  [#114](https://github.com/akiomik/bela-rs/issues/114) non-panicking accessors
  (closed; the slice accessors made it unnecessary here).
- [ADR 0009](0009-hardware-platform-choice-reopened.md) — the candidates, the
  power and thermal evidence, and the enclosure question this ADR inherits.
- [ADR 0010](0010-three-layer-control-surface-and-newest-value-handoff.md) —
  the three-layer split this port is the test of.
- [ADR 0008](0008-usb-audio-clock-slip-and-i2s-migration.md) — where 48 kHz
  comes from.
- [`docs/bela/`](../bela/) — the cross-compilation setup and the control
  surface's wiring.
