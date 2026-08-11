# Architecture

This document describes the internal architecture of the `oxtt` DSP engine, its JACK host adapter, and the physical control surface that drives it: how audio data flows through the system, which component owns which state, and where the real-time / non-real-time boundary lies.

## Component Overview

```
main.rs
  -> cli::Cli::parse              CLI parsing (clap), presets, per-field validation
  -> control::ControlHandle::spawn  control thread (--controls, `pi-controls` builds only)
       -> control::PiControls        layer A: six MCP3008 pots on SPI0/CE0, latching bypass switch on GPIO17
       -> control::ControlMapping    layer B: jitter filter, deadband -> ControlSnapshot
       -> triple_buffer::Input       layer C: publishes snapshots toward the audio callback
  -> jack_host::run               JACK client lifecycle, port registration
       -> AudioProcessHandler     audio callback (real-time thread)
            -> triple_buffer::Output  newest control snapshot -> OttProcessor::set_control_snapshot
            -> dsp::OttProcessor::process
                 -> dsp::crossover::Crossover
                 -> dsp::compressor::DualThresholdCompressor  (one per band)
                      -> dsp::envelope::BandEnvelope
                 -> dsp::smooth::Smoothed / LogSmoothed
       -> Notifications           JACK notification callback (shutdown, sample-rate change)
```

`OttProcessor` (`src/dsp.rs`) has no dependency on JACK types or any other host-audio API; it operates purely on `&[f32]` slices. `jack_host.rs` only registers ports and wires callbacks — it contains no DSP logic. This separation is what lets the DSP core run and be tested (`cargo test`) without a JACK server, and what would let the Linux-only ALSA-direct adapter selected for a future Raspberry Pi native backend be added without touching `dsp/` (see `decisions/0007-alsa-direct-not-cpal-for-pi-native-backend.md`).

The control surface (`src/control.rs`) is layered on the same principle. Layer A is the hardware read behind the `ControlSource` trait — the only platform-specific piece, implemented for the Raspberry Pi by `PiControls` (`src/control/pi.rs`) behind the `pi-controls` feature. Layer B, `ControlMapping` (`src/control/mapping.rs`), turns raw ADC counts into a complete `OttParams` and is pure, allocation-free and panic-free, so it holds itself to the audio callback's own prohibitions (`contracts.md` section 6) even though only layer C calls it today. Layer C, `ControlHandle` (`src/control/thread.rs`), is the polling thread and the handoff into the callback, and is the only layer a platform that reads its controls inside its own real-time callback would drop. See `decisions/0010-three-layer-control-surface-and-newest-value-handoff.md`.

## Signal Flow

Per stereo frame, `OttProcessor::process_frame` performs:

```
input_l, input_r
  -> Crossover::process_frame            raw 3-band split, per channel
       low  = phase_comp(LP4_low(x))       phase-compensated (decisions/0001)
       mid  = LP4_high(HP4_low(x))
       high = HP4_high(HP4_low(x))
  -> bypass branch: sum(raw low, mid, high)             unity reconstruction
  -> effect branch: input gain per band
  -> per band: DualThresholdCompressor.process(detector_power(l, r))  (decisions/0002)
       wet_band    = band_input * dynamic_gain * makeup_gain
       band_output = lerp(band_input, wet_band, depth)                (decisions/0004)
  -> sum(effect low, mid, high) -> output gain
  -> linear crossfade(effect, bypass reconstruction, bypass mix)
  -> finite-value guard (non-finite -> 0.0)
  -> output_l, output_r
```

`Crossover` and the three `DualThresholdCompressor`s are the only stateful DSP components in the signal path. Every scalar parameter (gains, depth, time, thresholds, amounts, crossover frequencies) is wrapped in a `Smoothed` (or `LogSmoothed`, for crossover frequencies) and advances per sample while transitioning, so it converges toward its target with a fixed 20 ms time constant independent of host buffer size. A crossover snaps to its target within the documented 0.1-cent tolerance; its coefficients are then stable until a cutoff target or sample rate changes.

## State Ownership

`OttProcessor` owns, per instance:

- `GlobalRuntime`: smoothed input/output gain, depth, time, upward, downward.
- `Crossover`: log-smoothed low/high cutoff, plus, per channel, three independent `Lr4` pairs (low split, high split, phase compensator) — six second-order biquad cascades per channel, twelve total.
- `Bands<BandProcessor>`: smoothed per-band thresholds/amounts/makeup gain, and one `DualThresholdCompressor` (two envelope states, `low_env` and `high_env`) each. `Bands<T>` (`src/bands.rs`) fixes the arity at exactly `low`/`mid`/`high` rather than `[T; 3]`, since oxtt is architecturally a 3-band compressor — used the same way for `OttParams::bands` and for `Crossover`'s per-band filter outputs, so the "3 bands" concept has one representation from config through to the real-time core.

There is no intermediate buffer sized to the host's callback buffer. Processing is frame-by-frame: one stereo sample is split, processed by all three bands, summed, and written, before moving to the next sample. This is what makes `process()`'s output independent of how the caller chunks the input slices — verified by `chunking_does_not_affect_output` (`src/dsp.rs`).

With a control surface attached, two more owners exist, both outside the DSP:

- `ControlMapping` (`src/control/mapping.rs`), owned by the control thread, holds the CLI-supplied base `OttParams` plus, per potentiometer, the low-pass filter state and the deadband reference — both in ADC counts — and, once per mapping rather than per pot, the last published `ControlSnapshot` and the debounced switch position. The snapshot contains the complete current pot parameters and an explicit bypass level; it does not replace a coincidental parameter triple with bypass values. `Pots<T>` (`src/control/raw.rs`) fixes the arity at exactly `depth`/`time`/`upward`/`downward`/`input_gain`/`output_gain` for the same reason `Bands<T>` fixes it at three, so one representation carries the concept from the ADC channel order through to the mapped parameters.
- `ControlHandle` (`src/control/thread.rs`) owns the thread itself, its stop flag, its read-failure counter, and the writing end of the `triple_buffer`; the audio callback owns the reading end, which `ControlHandle::take_output` can hand out exactly once. The buffer's three slots are allocated when it is built and `ControlSnapshot` is `Copy` with no `Drop`, so publishing a snapshot allocates and frees nothing on either side.

Neither of those owns any DSP state. A control snapshot reaches `OttProcessor` through its explicit `set_control_snapshot` seam; the CLI remains on the ordinary `set_params` path.

## Real-Time / Non-Real-Time Boundary

```
non-real-time                            |  real-time (JACK audio thread)
-----------------------------------------|---------------------------------------
main.rs: Cli::parse, Client::new,        |  AudioProcessHandler::process
  OttProcessor::new, activate_async      |    - swap pending_sample_rate (Atomic)
                                         |    - take newest control snapshot
control thread: ControlSource::read      |      (triple_buffer::Output::update)
  (blocking SPI/GPIO), ControlMapping,   |    - OttProcessor::process
  publish (triple_buffer::Input::write)  |      (no alloc, no lock, no I/O)
signal_hook: SIGINT/SIGTERM -> Atomic    |
main loop: poll shutdown flag, sleep     |  Notifications::sample_rate / shutdown / xrun
deactivate(), stop_and_join, CLI report  |  (JACK-internal thread, Atomic stores only)
```

`AudioProcessHandler` and `Notifications` (`src/jack_host.rs`) communicate only through `Arc<AtomicBool>` and `Arc<AtomicU32>`. `Notifications` also updates an `Arc<AtomicU64>` xrun diagnostic counter; the main thread reads it only after deactivation and the CLI emits it only for `--report-xruns-on-exit`. The audio callback never blocks on a lock, allocates, or performs I/O. See `contracts.md` (section 6) for the full list of operations prohibited inside the callback.

The control surface crosses this boundary the same way. An MCP3008 conversion is a blocking `ioctl`, so it belongs on the control thread, which polls the hardware every 2 ms, conditions the reading, and publishes a finished `OttParams` only when the conditioned value actually moved. The callback's end of that handoff — `triple_buffer::Output::update`, a single atomic swap plus an index assignment — is wait-free, allocation-free and constant-time, so a knob turn costs the callback the same as a knob at rest. This is the "bounded non-blocking queue instead of a new lock" earlier revisions of this document anticipated, with the bound at one: parameters are level-based, so the callback wants the knob's newest position and never a backlog of the positions it passed through. `contracts.md` section 8 states the guarantees; `decisions/0010-three-layer-control-surface-and-newest-value-handoff.md` records why the capacity is one.

## Parameter Update Path

`OttProcessor::set_params` only updates smoothing *targets*; it never snaps `current` to `target`. Only `OttProcessor::new` and `OttProcessor::reset` (invoked on a JACK sample-rate change) snap all state immediately, which avoids an audible startup fade while still guaranteeing smooth, click-free transitions for any later parameter change. `set_control_snapshot` applies every parameter target to the latent effect branch and independently retargets one 20 ms bypass-mix smoother. The effect and bypass branches are built from one raw-input crossover split: the bypass branch is the unscaled three-band sum, while the effect branch applies input gain after that split and output gain after dynamics. Thus both crossfade endpoints share crossover phase history; the DSP never crossfades the reconstruction against raw input. A reversal simply follows the newest mix target. Reset preserves the latest explicit bypass level and snaps the mix to its corresponding endpoint. See `contracts.md` (section 2) for the exact pre/postconditions.

A control snapshot enters through `set_control_snapshot`: the callback applies its complete parameter payload and explicit bypass level strictly after any pending sample-rate reset in the same cycle. `ControlMapping` seeds itself from its first reading rather than fading in from zero, for the same reason `OttProcessor::new` snaps its smoothers to their targets: there is no earlier state to have moved away from.
