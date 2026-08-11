# ADR 0004: Dry/Wet Blending Never Uses the Raw Input

## Status

Accepted

## Context

The most direct way to implement a `depth` (dry/wet) control is `lerp(raw_input, wet_output, depth)`. In `oxtt`, the raw input and the crossover-reconstructed signal share the same amplitude response but not the same phase response (see ADR 0001) — the reconstructed signal has gone through the crossover's all-pass phase rotation, and the raw input has not. Blending two differently-phased copies of the same signal risks comb filtering.

## Decision

`depth` always blends between the input-gained crossover-split, per-band signal and that same signal after dynamics processing:

```
raw_band    = crossover(raw_input)
band_input  = raw_band * input_gain
wet_band    = band_input * dynamic_gain * band_makeup_gain
band_output = lerp(band_input, wet_band, depth)
```

`raw_band` is the output of the crossover splitter (already phase-shifted by the LR4 stages), never the original `input_l`/`input_r` samples. `band_input` applies the current input gain after that split. For a static gain this is linearly equivalent to gain before the split, but a moving gain intentionally leaves the crossover filter history driven by raw input.

## Consequences

- Summing all three bands after this blend, even at `depth = 0`, reconstructs the same phase-flat signal described in ADR 0001 — there is no comb-filtering risk from mixing differently-phased copies of the signal.
- `depth_zero_matches_pure_crossover_reconstruction` (`src/dsp.rs`) compares against "raw LR4 reconstruction -> per-band input gain -> sum -> output gain", not against the unmodified input signal — this is the exact reference for any future test of `depth`'s boundary behavior.
- A true, phase-identical "raw bypass" is out of scope for the DSP core as specified. If one is ever wanted, it must be a separate signal path (e.g. a switch upstream of the crossover), not an extension of `depth`.
