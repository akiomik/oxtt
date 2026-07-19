# ADR 0001: Phase-Compensate the Low Branch Instead of Mixing Raw Dry Signal

## Status

Accepted

## Context

`oxtt` splits each channel into three bands using two cascaded 4th-order Linkwitz-Riley (LR4) splits: first at `f_low` (producing `low` and an `upper` branch), then at `f_high` on the `upper` branch (producing `mid` and `high`). Unlike `mid` and `high`, `low` only passes through one split stage, so on its own it is one full LR4 stage "ahead" in phase relative to `mid`/`high`. Summing `low + mid + high` directly, without compensation, would not reconstruct the original signal even in the unprocessed case, because `low` and the `mid + high` pair have gone through a different number of all-pass stages.

A simpler alternative would be to sidestep the phase problem entirely by mixing the *dry* input in parallel with the processed bands. This is attractive because it avoids crossover-induced phase shift altogether, but it introduces a different signal at a different phase into the sum, which risks comb filtering when blended with the crossover path.

## Decision

Route the `low` branch through a second, independent LR4 low-pass/high-pass pair at `f_high` and sum their outputs (`A_high(z) = LP4_high(z) + HP4_high(z)`, a unity-gain all-pass response), instead of mixing in the raw dry input. This phase compensator shares its coefficient-update sequence with the mid/high split — both always track the same smoothed cutoff, `f_high` — but keeps fully independent filter state.

Raw dry/wet mixing is disallowed as a consequence: any dry/wet blend operates on the *crossover-reconstructed* signal per band, never on the untouched input (see ADR 0004).

## Consequences

- The unprocessed 3-band sum (`low + mid + high`) is flat within +/-0.1 dB from 20 Hz to `0.45 * sample_rate`, independent of `depth`, because `A_low(z) * A_high(z)` is a unity-gain all-pass response (verified by `reconstruction_is_flat_*` and `phase_compensator_alone_is_unity_gain` in `src/dsp/crossover.rs`).
- `depth = 0` is well-defined and testable in isolation: it must equal "input gain -> LR4 reconstruction -> output gain" exactly, not "input gain -> raw passthrough" (verified by `depth_zero_matches_pure_crossover_reconstruction` in `src/dsp/mod.rs`).
- The low branch pays the extra CPU cost of one more `Lr4` pair (two biquad cascades) per channel, and its filter state must never be shared with the mid/high split's own `Lr4` pair — sharing would corrupt both computations while the smoothed cutoff is mid-transition. This drives the `ChannelSplitter` struct layout in `src/dsp/crossover.rs`, which keeps `low_split_*`, `high_split_*`, and `phase_comp_*` as six separate `Lr4` instances.
- Because reconstruction is amplitude-flat but not phase-identical to the raw input, `oxtt` never claims sample-accurate bypass; a literal raw bypass is treated as a separate, out-of-scope feature.
