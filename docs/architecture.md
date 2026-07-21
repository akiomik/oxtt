# Architecture

This document describes the internal architecture of the `oxtt` DSP engine and its JACK host adapter: how audio data flows through the system, which component owns which state, and where the real-time / non-real-time boundary lies.

## Component Overview

```
main.rs
  -> cli::Cli::parse              CLI parsing (clap), presets, per-field validation
  -> jack_host::run               JACK client lifecycle, port registration
       -> AudioProcessHandler     audio callback (real-time thread)
            -> dsp::OttProcessor::process
                 -> dsp::crossover::Crossover
                 -> dsp::compressor::DualThresholdCompressor  (one per band)
                      -> dsp::envelope::BandEnvelope
                 -> dsp::smooth::Smoothed / LogSmoothed
       -> Notifications           JACK notification callback (shutdown, sample-rate change)
```

`OttProcessor` (`src/dsp/mod.rs`) has no dependency on JACK types or any other host-audio API; it operates purely on `&[f32]` slices. `jack_host.rs` only registers ports and wires callbacks — it contains no DSP logic. This separation is what lets the DSP core run and be tested (`cargo test`) without a JACK server, and what would let the Linux-only ALSA-direct adapter selected for a future Raspberry Pi native backend be added without touching `dsp/` (see `decisions/0007-alsa-direct-not-cpal-for-pi-native-backend.md`).

## Signal Flow

Per stereo frame, `OttProcessor::process_frame` performs:

```
input_l, input_r
  -> input gain
  -> Crossover::process_frame            3-band split, per channel
       low  = phase_comp(LP4_low(x))       phase-compensated (decisions/0001)
       mid  = LP4_high(HP4_low(x))
       high = HP4_high(HP4_low(x))
  -> per band: DualThresholdCompressor.process(detector_power(l, r))  (decisions/0002)
       wet_band    = band_input * dynamic_gain * makeup_gain
       band_output = lerp(band_input, wet_band, depth)                (decisions/0004)
  -> sum(low, mid, high)
  -> output gain
  -> finite-value guard (non-finite -> 0.0)
  -> output_l, output_r
```

`Crossover` and the three `DualThresholdCompressor`s are the only stateful DSP components in the signal path. Every scalar parameter (gains, depth, time, thresholds, amounts, crossover frequencies) is wrapped in a `Smoothed` (or `LogSmoothed`, for crossover frequencies) and advances per sample while transitioning, so it converges toward its target with a fixed 20 ms time constant independent of host buffer size. A crossover snaps to its target within the documented 0.1-cent tolerance; its coefficients are then stable until a cutoff target or sample rate changes.

## State Ownership

`OttProcessor` owns, per instance:

- `GlobalRuntime`: smoothed input/output gain, depth, time, upward, downward.
- `Crossover`: log-smoothed low/high cutoff, plus, per channel, three independent `Lr4` pairs (low split, high split, phase compensator) — six second-order biquad cascades per channel, twelve total.
- `Bands<BandProcessor>`: smoothed per-band thresholds/amounts/makeup gain, and one `DualThresholdCompressor` (two envelope states, `low_env` and `high_env`) each. `Bands<T>` (`src/bands.rs`) fixes the arity at exactly `low`/`mid`/`high` rather than `[T; 3]`, since oxtt is architecturally a 3-band compressor — used the same way for `OttParams::bands` and for `Crossover`'s per-band filter outputs, so the "3 bands" concept has one representation from config through to the real-time core.

There is no intermediate buffer sized to the host's callback buffer. Processing is frame-by-frame: one stereo sample is split, processed by all three bands, summed, and written, before moving to the next sample. This is what makes `process()`'s output independent of how the caller chunks the input slices — verified by `chunking_does_not_affect_output` (`src/dsp/mod.rs`).

## Real-Time / Non-Real-Time Boundary

```
non-real-time                          |  real-time (JACK audio thread)
----------------------------------------|---------------------------------------
main.rs: Cli::parse, Client::new,       |  AudioProcessHandler::process
  OttProcessor::new, activate_async     |    - swap pending_sample_rate (Atomic)
                                         |    - OttProcessor::process
signal_hook: SIGINT/SIGTERM -> Atomic   |      (no alloc, no lock, no I/O)
main loop: poll shutdown flag, sleep    |
active_client.deactivate(), CLI report  |  Notifications::sample_rate / shutdown / xrun
                                         |  (JACK-internal thread, Atomic stores only)
```

`AudioProcessHandler` and `Notifications` (`src/jack_host.rs`) communicate only through `Arc<AtomicBool>` and `Arc<AtomicU32>`. `Notifications` also updates an `Arc<AtomicU64>` xrun diagnostic counter; the main thread reads it only after deactivation and the CLI emits it only for `--report-xruns-on-exit`. The audio callback never blocks on a lock, allocates, or performs I/O. See `contracts.md` (section 6) for the full list of operations prohibited inside the callback, and for how a future control-surface thread (GPIO/ADC on Raspberry Pi) would plug into this same boundary via a bounded non-blocking queue instead of a new lock.

## Parameter Update Path

`OttProcessor::set_params` only updates smoothing *targets*; it never snaps `current` to `target`. Only `OttProcessor::new` and `OttProcessor::reset` (invoked on a JACK sample-rate change) snap all state immediately, which avoids an audible startup fade while still guaranteeing smooth, click-free transitions for any later parameter change. See `contracts.md` (section 2) for the exact pre/postconditions of `new`, `reset`, and `set_params`.
