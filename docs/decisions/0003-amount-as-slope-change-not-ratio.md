# ADR 0003: `amount` Is a Slope Change, Not a Conventional Compression Ratio

## Status

Accepted

## Context

Conventional compressors expose a "ratio" (e.g. 4:1) describing how many dB of input change above threshold produce one dB of output change. `oxtt` needs a single `0..1` knob per direction (upward/downward) that is safe to default to a strong value without risking runaway gain, and that composes predictably with the `upward`/`downward` global multipliers.

A ratio-based parameterization requires inversion (`1/ratio`) to compute a slope, and needs special-casing at its extreme (ratio approaching infinity, i.e. a limiter) — awkward to combine with a `[0, 1]`-ranged global multiplier.

## Decision

`amount` is defined directly as the change in input/output slope beyond the threshold, not as a ratio:

- `amount = 0`: 1:1 slope, no processing.
- `amount = 0.5`: 0.5:1 slope beyond threshold (equivalent to a conventional 2:1 ratio).
- `amount = 1`: the theoretical limit where steady-state output beyond threshold is pinned to the threshold level.

Gain is computed directly from this definition: `up_gain_db = effective_up_amount * (lower_threshold_db - low_level_db)`, symmetrically for the downward side, then clamped to `[-60, +30]` dB. Composition with the global multiplier is plain multiplication: `effective_up_amount = clamp(band.up_amount * upward, 0, 1)`.

## Consequences

- Every value of `amount` in `[0, 1]` is safe by construction: no input can produce an unstable or diverging slope, and no unit conversion is needed where `amount` is combined with `upward`/`downward`.
- Anyone porting a "ratio" value from a reference compressor into `oxtt`'s preset table must convert it (`amount = 1 - 1/ratio`) rather than copying the number directly. `oxtt` does not target preset compatibility with any reference implementation, so this conversion is a one-time authoring step, not a runtime concern.
- Verified at the boundary values by `gain_is_0db_everywhere_when_amounts_are_zero` and `effective_amount_clamps_to_unit_range` (`src/dsp/compressor.rs`).
