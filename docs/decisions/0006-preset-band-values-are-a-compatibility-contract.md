# ADR 0006: Preset Band Values Are a Compatibility Contract for `Default`

## Status

Accepted

## Context

`oxtt` ships two presets, `SafeStart` and `Default`, which currently share one per-band threshold/amount/makeup-gain table (`Preset::LOW_BAND`, `MID_BAND`, `HIGH_BAND` in `src/params.rs`) and differ only in global `depth` and `output_gain_db`. Retuning these per-band values in place would be tempting whenever the sound is improved, but doing so silently would change the behavior of every existing `Default` preset user without warning.

## Decision

The per-band values in `Preset::LOW_BAND`, `MID_BAND`, `HIGH_BAND` are a compatibility contract for the `Default` preset: once shipped, they must not change silently. A DSP or tuning change that alters these values must ship under a new preset name (a new `Preset` variant) or a major version bump — never as an in-place edit to the existing constants.

## Consequences

- `presets_share_band_values` (`src/params.rs`) documents today's starting point — both presets share one table — but does not by itself enforce the compatibility contract for future changes; that is a process rule for contributors editing `src/params.rs`, not something the test suite can catch automatically.
- A future retuned preset must be added as a new `Preset` variant rather than by mutating `LOW_BAND`/`MID_BAND`/`HIGH_BAND`.
