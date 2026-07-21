# ADR 0007: Use ALSA Direct, Not CPAL, for the Raspberry Pi Native Backend

## Status

Accepted

Supersedes [ADR 0005](0005-jack-adapter-with-alsa-direct-deferred.md).

## Context

`oxtt` currently uses the `jack` crate and processes capture and playback in one
JACK process callback. This is a good fit for PC development and remains the
baseline for the first Raspberry Pi 5 measurements. The DSP core is already
independent of JACK and operates on audio slices, so a second host adapter can be
added without rewriting the DSP.

A daemon-free Raspberry Pi backend may still be useful if JACK fails the measured
latency or stability targets, or if removing JACK materially simplifies operating
the finished DIY effect. The backend only needs to serve Linux on Raspberry Pi 5
with one full-duplex, class-compliant audio interface acting as the clock master.
Cross-platform low-level audio-I/O abstraction is not a project goal.

CPAL's ALSA backend was considered because it provides device/configuration
discovery, stream lifecycle management, fixed-period requests, xrun reporting,
and optional real-time thread promotion. However, CPAL exposes capture and
playback as separate streams. On ALSA they run on separate worker threads; CPAL
does not provide a full-duplex callback that presents matching input and output
periods to the effect.

Using CPAL for `oxtt` would therefore still require a bounded SPSC audio ring,
startup prefill, separate-stream synchronization, underflow/overflow policy, and
coordinated recovery after either stream xruns. That bridge is the hardest and
most latency-sensitive part of this backend. It can add at least one deliberately
buffered handoff and makes it possible for capture and playback to recover out of
alignment. CPAL's cross-platform benefits do not offset that cost for this
Linux-only appliance backend.

ALSA exposes the primitives needed to keep the full-duplex pair under one adapter's
control: capture/playback PCM parameter negotiation, linked start/stop where the
device supports it, poll/read/write access, hardware timestamps, and explicit xrun
recovery. The Rust `alsa` crate exposes these facilities without requiring a new
project-specific cross-platform audio abstraction.

## Decision

Keep the existing `jack` crate adapter as the current and reference audio host.
Do not replace it with CPAL's JACK backend, and do not add CPAL solely to obtain
ALSA support.

If Raspberry Pi measurements justify a native backend, implement a Linux-only
ALSA-direct adapter using the `alsa` crate. The adapter MUST:

- open capture and playback on the same explicitly selected physical device and
  require one shared hardware clock; multi-device aggregation and resampling are
  out of scope;
- negotiate identical sample rate and period geometry for capture and playback,
  beginning with the project's 48 kHz and 128-frames/period baseline;
- link the capture and playback PCMs when supported and otherwise fail clearly
  unless an equally deterministic synchronized-start mechanism is documented and
  tested;
- run capture -> `OttProcessor` -> playback in one real-time processing loop,
  without an inter-thread audio ring;
- allocate and prepare any interleaving, sample-format-conversion, or channel-
  selection buffers before the real-time loop, never while processing audio;
- treat an xrun or device failure as a failure of the full-duplex pair: silence
  output, recover or rebuild both directions together, reset DSP state, and expose
  non-real-time diagnostic counters;
- satisfy the real-time callback rules in `docs/contracts.md` section 6 even
  though ALSA uses a processing loop rather than a JACK callback.

Do not introduce a general `AudioBackend` trait before the ALSA adapter exists.
JACK and ALSA may initially remain separate, concrete host modules. Shared host
lifecycle code should be extracted only when both implementations demonstrate a
real common boundary.

Implementation remains gated by the Raspberry Pi JACK baseline. The ALSA adapter
is not required while JACK meets the existing 30-minute xrun, CPU-headroom, and
10 ms round-trip-latency targets and its operational cost is acceptable.

## Consequences

- The project avoids implementing CPAL's missing full-duplex semantics on top of
  two independent streams and avoids the corresponding audio handoff buffer.
- The ALSA adapter will contain more Linux-specific setup and recovery code than a
  CPAL adapter, but that code is limited to the exact full-duplex behavior the
  hardware effect needs.
- Device enumeration, sample-format conversion, real-time scheduling, xrun
  recovery, and shutdown become explicit project responsibilities and require
  hardware-backed tests. CPAL would have provided parts of these, but not their
  full-duplex composition.
- The direct backend is allowed to reject unsupported hardware configurations
  instead of hiding them behind conversion, aggregation, or resampling. The first
  supported target is the Raspberry Pi 5 with the Babyface Pro FS in class-
  compliant mode.
- JACK remains the portable development and routing environment. There is no goal
  to make JACK and ALSA interchangeable across every supported operating system.
- If future requirements expand to multiple desktop platforms or independent
  capture/playback devices, this decision must be revisited; those requirements
  could make CPAL or another higher-level abstraction valuable again.

## References

- [CPAL 0.18.1 `DeviceTrait`](https://docs.rs/cpal/0.18.1/cpal/traits/trait.DeviceTrait.html)
  exposes separate input- and output-stream constructors.
- [CPAL 0.18.1 ALSA backend source](https://docs.rs/cpal/0.18.1/src/cpal/host/alsa/mod.rs.html)
  implements those streams with separate input and output worker threads.
- [Rust `alsa` 0.11.0 `PCM` API](https://docs.rs/alsa/0.11.0/alsa/pcm/struct.PCM.html)
  exposes PCM linking, polling, parameter control, timestamps, and recovery.
- [ALSA's full-duplex latency test](https://www.alsa-project.org/wiki/Test_latency.c)
  documents linked capture/playback setup and hardware-sync verification.

## Validation

The ALSA-direct backend can become the preferred Raspberry Pi runtime only after
it is compared with JACK on the same Pi, interface, sample rate, and period size.
At minimum, record:

- negotiated capture/playback format, channel mapping, period size, and buffer
  size;
- xrun and recovery counts over 30 minutes at 128 frames/period and, if stable,
  64 frames/period;
- analog-loopback round-trip latency, with the existing 10 ms target;
- processing-loop worst-case time and CPU headroom;
- behavior on startup, clean shutdown, USB disconnect, and forced xrun.

If ALSA direct does not materially improve latency, stability, or operation over
JACK, retain JACK and remove or leave the direct adapter experimental rather than
maintaining two production backends without a demonstrated benefit.
