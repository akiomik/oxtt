# ADR 0005: Depend on JACK Now, Keep the Door Open for ALSA-Direct Later

## Status

Accepted

## Context

The target deployment includes a Raspberry Pi 5 with a class-compliant USB audio interface, eventually running unattended. JACK (via JACK2 on Linux) already covers both "runs on a PC today for fast DSP iteration" and "runs on Raspberry Pi with acceptable latency," but it is an additional daemon and dependency compared to talking to ALSA directly.

## Decision

Depend on the `jack` crate (0.13.x) exclusively for audio I/O, and keep all JACK-specific code confined to `src/jack_host.rs` and `src/main.rs`. `src/dsp/` and its public type `OttProcessor` (`src/dsp/mod.rs`) have zero dependency on the `jack` crate or any other host-audio API — they operate purely on `&[f32]` slices.

## Consequences

- The DSP core is testable and runs its full test suite (`cargo test`) without a JACK server, an ALSA device, or any audio hardware present.
- Adding an ALSA-direct backend later, if JACK's overhead ever fails to meet a latency or stability target on Raspberry Pi, means writing a new adapter analogous to `jack_host.rs`; it does not require touching `dsp/`.
- Conversely, this repository does not attempt to abstract over multiple audio backends today — there is no `AudioBackend` trait and no cfg-gated backend selection. That abstraction is deferred until a second backend actually exists, to avoid speculative generality.
- `AudioProcessHandler`'s real-time constraints (`contracts.md` section 6) are specific to the shape of the `jack::ProcessHandler` callback; a future ALSA-direct backend would need to satisfy the same constraints against its own callback API, but the constraints themselves are not JACK-specific.
