# ADR 0002: One Stereo-Linked Detector Per Band, Not Independent L/R Gain

## Status

Accepted

## Context

A multiband compressor can compute gain reduction/expansion independently per channel, or derive one shared control signal from both channels ("stereo link"). Independent L/R detection reacts more precisely to each channel's own level, but for a hard-panned or mono-adjacent source it can pull one channel's gain away from the other's on every transient, shifting the perceived stereo image.

## Decision

Each band uses exactly one detector, driven by `p[n] = max(left[n]^2, right[n]^2)` (100% peak-link), and applies the resulting `dynamic_gain` identically to both channels. There is no per-channel gain path and no partial-link ratio parameter in v0.1.

The detector's `low_env`/`high_env` state is initialized to the *threshold-derived* boundary power, not zero, so that the first sample after construction, `reset`, or a sample-rate change produces 0 dB gain rather than a transient of maximum boost/cut.

## Consequences

- Stereo image is preserved under transients that hit only one channel: driving one channel hard does not desync the gain applied to the other channel, since `DualThresholdCompressor::process` takes a single detector power `p` and returns a single `dynamic_gain` applied to both `wet_left` and `wet_right` (`src/dsp/mod.rs`, `src/dsp/compressor.rs`).
- A future "selectable stereo link" or per-channel/sidechain mode would need a new parameter and a second gain-computation path; it is not a mode switch on the existing detector.
- The threshold-snapped initial state avoids a startup artifact and is required for `set_params`/`reset` semantics to compose correctly with parameter smoothing (see `contracts.md` section 2).
- Verified by `init_state_yields_0db_gain` (`src/dsp/compressor.rs`), `init_and_reset_snap_to_threshold_powers` and `reset_after_use_snaps_back_to_threshold_powers` (`src/dsp/envelope.rs`).
