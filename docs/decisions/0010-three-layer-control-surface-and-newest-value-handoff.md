# ADR 0010: A Three-Layer Control Surface with a Newest-Value Handoff

## Status

Accepted

## Context

The end goal is a pedal, so the parameters have to come off a panel rather than
off a command line. The first physical control surface is four potentiometers —
Depth, Time, Upward, Downward — on an MCP3008 SPI ADC, plus one momentary
bypass switch on a GPIO pin. The wiring is verified on the assembled hardware
(see [`../raspberry-pi/control-surface-verification.md`](../raspberry-pi/control-surface-verification.md)).

Three facts constrain how that reaches the running DSP.

**An ADC conversion cannot happen where the parameters are consumed.** An
MCP3008 read is SPI traffic — a blocking `ioctl` — and the audio callback may
not block, allocate, or perform I/O (`docs/contracts.md` §6). Something has to
cross the real-time boundary, and `docs/architecture.md` had already anticipated
what: a bounded non-blocking queue rather than a lock shared with the callback.

**The platform this runs on is not settled.** [ADR 0009](0009-hardware-platform-choice-reopened.md)
reopened the hardware choice and recorded that the control layer is exactly
where the candidates diverge. A Bela reads its analog and digital inputs
synchronously inside its own real-time `render()` callback, so it needs no
polling thread and no queue at all. Daisy Seed and Teensy are bare metal: no
`rppal`, no `spidev`, no OS thread to poll from, and no `clap` CLI either. Only
the Raspberry Pi wants the thread-plus-queue shape. A design that assumes an OS
underneath the whole control path would have to be unpicked on two of the three
live candidates — but a design that tries to *abstract over* all three would be
speculative, built against hardware nobody has bought yet.

**A knob position is a level, not an event.** What the audio callback needs is
where the knob is now. The positions it swept through on the way there carry no
information the DSP can use, and a knob that has not moved has nothing to say at
all.

Two further questions are settled elsewhere and are only applied here.
[ADR 0004](0004-no-raw-dry-mix.md) established that `depth` blends against the
crossover-split band signal and never against the raw input, because the two do
not share a phase response. And the conditioning constants that turn noisy ADC
counts into stable parameters — the jitter filter and the deadband — are
justified by measurement, recorded in
[`../raspberry-pi/control-surface-verification.md`](../raspberry-pi/control-surface-verification.md)
with the re-check rule kept next to the constants themselves in
`src/control/mapping.rs`.

## Decision

- **Split the control surface into three layers** (`src/control.rs`): a
  platform-specific hardware read (A), a shared pure mapping from raw counts to
  a complete `OttParams` (B), and a transport that moves finished snapshots
  across the real-time boundary (C). The split exists so that the only layer
  with behaviour worth testing — B — is the layer every platform shares.

- **Make `ControlSource` the only platform seam, and refuse to build a platform
  abstraction layer.** The trait produces one `RawControls` value and nothing
  else. It exists for two concrete reasons — a fake source lets the mapping and
  the thread be exercised on a development machine with no MCP3008 and no
  Linux, and it confines what a port has to rewrite to the hardware read — not
  to model "a platform". Anything a real second platform turns out to need is
  cheaper to discover against that platform than to guess at now.

- **Hold layer B to the audio callback's own prohibitions** even though nothing
  calls it from a callback today: no allocation, no panic, no I/O, no clock, no
  threads (`docs/contracts.md` §6, machine-checked by the same `no_panic` proof
  the DSP uses). This is the one concession made to a platform that does not
  exist yet, and it is deliberately the cheap one. ADR 0009 records that Bela
  reads its inputs inside `render()`; because layer B obeys the callback
  contract, a Bela port drives it *directly* from `render()` and drops layer C
  entirely, rather than reproducing a thread and a queue it does not need.

- **Hand snapshots to the callback through a capacity-one triple buffer, not a
  ring queue.** Parameters are level-based, so the newest value is the correct
  one and a backlog is not: draining a queue would make the callback apply
  positions the knob has already left, and its cost would depend on how fast
  the knob was moving. `triple_buffer::Output::update` is a single atomic swap
  plus an index assignment — wait-free, allocation-free, lock-free and
  constant-time — so the callback pays the same whether or not a knob moved.
  This is the "bounded non-blocking queue" `docs/architecture.md` anticipated,
  with the bound at one.

- **Publish only when the conditioned value changes.** A motionless pot and an
  untouched switch produce no snapshot at all, so the callback's `update`
  returns false and `set_params` is not called. Idle jitter is absorbed by the
  filter and deadband in layer B rather than being handed across the boundary
  for the DSP to smooth away.

- **Let the control surface outrank the CLI for its four parameters, from the
  first successful read.** `--depth`, `--time`, `--upward` and `--downward`
  describe only the state before the hardware has been read once; from that
  read onward the pots own those four fields. Every other parameter is passed
  through from the CLI untouched. The alternative — treating the CLI as a
  baseline the pots offset, or requiring the pots to be swept to "pick up" the
  CLI value — makes the panel lie about what the effect is doing, which is the
  one thing a physical control must not do.

- **Make the bypass switch an effect bypass: `depth = 0`,** not a crossfade to
  the raw input. ADR 0004 carries the phase argument; nothing about a panel
  switch changes it. Because the switch is momentary, "bypassed" is latched
  software state that a debounced press toggles and a release leaves alone,
  and the other three pots keep working while it is engaged.

- **Treat a hardware read failure as survivable and an acquisition failure at
  startup as fatal.** A failed read publishes nothing, is counted, and leaves
  the callback on the last good snapshot; the total is reported at exit
  alongside the xrun count. Failing to open SPI or claim the GPIO pin at
  startup is the opposite case — it means the hardware was asked for and is not
  there — and exits non-zero.

## Consequences

- The Raspberry Pi keeps all three layers. A Bela port keeps layer B verbatim,
  rewrites layer A against Bela's own analog/digital I/O API, and **throws away
  layer C**: no poll interval, no thread, no triple buffer, no `stop_and_join`,
  and no read-failure counter, because the read happens inline in `render()`.
  A Daisy or Teensy port keeps layer B for the same reason and rewrites A
  against a vendor `no_std` HAL, but also loses the `clap` CLI that supplies
  layer B's base parameters (ADR 0009, finding 4), so the base parameter set
  becomes compiled-in preset data there. In every case the conditioning, the
  parameter mapping, the deadband, the debounce and the bypass latch move
  unchanged.

- The callback never sees intermediate knob positions. At the 2 ms poll
  interval a fast sweep is sampled at 500 Hz — just above the callback rate at
  128 frames / 48 kHz — and each callback takes whatever the latest sample
  produced. This is deliberate, and it is invisible: every parameter is
  re-smoothed per sample by the DSP with a 20 ms time constant
  (`docs/architecture.md`), which is far longer than anything dropped between
  polls.

- The four pot-driven CLI flags become startup-only values under `--controls`.
  A run that wants CLI control of those four does not pass `--controls`; the
  flag is opt-in even in a `pi-controls` build, so the same binary still runs
  on a Pi with no breadboard attached, which is how the audio-stability scripts
  under `scripts/` invoke it.

- Bypass state does not survive a restart, and it cannot be read off the panel:
  a momentary switch has no position to read. A run started with the switch
  held down comes up un-bypassed, because the first reading is a baseline
  rather than an edge. An indicator LED, when the enclosure gets one, is what
  makes the latch visible; nothing in this decision provides that yet.

- A control surface that is failing intermittently is not obvious from the
  audio, by design: the effect simply stops responding to the knobs while
  continuing to play. The evidence is the control thread's throttled stderr
  line and the `oxtt: control_read_failures=N` count at exit. That is the right
  trade for a pedal — an effect that keeps making sound with slightly stale
  settings is recoverable on stage, and one that stops making sound is not —
  but it means the failure count is worth looking at after any session where
  the knobs felt dead.

- `ControlSource` is not a general input abstraction. A second kind of control
  input (MIDI, a rotary encoder, a second ADC) is not something to bolt onto
  the trait; it would be a new layer-A implementation only if it produces the
  same `RawControls`, and otherwise a change to layers A and B together. That
  is the intended cost of refusing the abstraction layer.

- The exit report's `oxtt: xrun_count=0` line is unchanged and stays alone on
  its line, because the Raspberry Pi verification scripts match it whole; the
  control-read count is a separate line, printed only when there was a control
  surface to count for.

## References

- [ADR 0004](0004-no-raw-dry-mix.md) — why `depth` never blends against the raw
  input, and therefore why the panel bypass is `depth = 0` rather than a raw
  bypass.
- [ADR 0009](0009-hardware-platform-choice-reopened.md) — the open platform
  question, Bela's synchronous reads inside `render()`, and the asymmetric
  control-layer migration cost this ADR's layering is shaped around.
- [`../contracts.md`](../contracts.md) — §6 for the real-time callback
  prohibitions layer B holds itself to, §8 for the control surface's own
  normative guarantees.
- [`../architecture.md`](../architecture.md) — where the three layers sit in the
  component structure and how they cross the real-time boundary.
- [`../raspberry-pi/control-surface-verification.md`](../raspberry-pi/control-surface-verification.md)
  — the idle-jitter measurement behind the filter and deadband constants, and
  the hardware verification of the assembled surface.
- [`triple_buffer`](https://docs.rs/triple_buffer/) — the wait-free
  single-producer/single-consumer newest-value handoff used for layer C.
