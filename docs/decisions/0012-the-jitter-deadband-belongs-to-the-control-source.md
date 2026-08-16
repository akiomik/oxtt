# ADR 0012: The Jitter Deadband Belongs to the Control Source, Not to Layer B

## Status

Accepted

Revises one decision in
[ADR 0010](0010-three-layer-control-surface-and-newest-value-handoff.md) and
the sentence in
[ADR 0011](0011-bela-gem-stereo-as-the-second-host.md) that restated it: layer
B's deadband is no longer a shared constant. Everything else about the
three-layer split stands, including the duty ADR 0011 attached to it.

## Context

ADR 0010 put the jitter filter and the hysteresis deadband in layer B, shared
by every platform, and ADR 0011 confirmed the arrangement when the Bela host
arrived: layer B was reused "with *no source change at all* — not one
constant". The alternative it named and rejected was "Bela-specific constants
in layer B", on the grounds that this would make the shared layer
platform-dependent and put its calibration out of reach of the measurement
that justified it.

Both boards' surfaces have now been measured, and the premise underneath that
choice does not hold. `DEADBAND_COUNTS = 8.0` was derived from one converter,
and the two converters are not in the same class:

| | Raspberry Pi (MCP3008) | Bela Gem Stereo (ADS8166) |
|---|---|---|
| Idle jitter, full travel | σ 5.30–6.39 counts | 2.2–2.3 counts peak-to-peak |
| Idle jitter, mid travel | σ 2.98–4.19 counts | 1.5–2.5 counts peak-to-peak |
| Sampling behind the figure | 300 readings per channel | ~1.3 million readings per channel |
| Worst case | σ ≈ 6.4 | whole excursion ≤ 2.5 counts |

The measurements are in
[`docs/raspberry-pi/control-surface-verification.md`](../raspberry-pi/control-surface-verification.md)
and
[`docs/bela/control-surface-verification.md`](../bela/control-surface-verification.md).

Eight counts on a Gem is not wrong — it is silent there too, by a wide
margin — it is simply eight counts spent on jitter that measures under three.
The cost is resolution: 8/1023 is 0.8% of travel and about 128 distinct
positions across a sweep, where 3/1023 is 0.29% and about 341. On the two gain
pots the same ratio is 0.375 dB against 0.141 dB per step.

The reverse substitution is worse and is what the shared constant was really
protecting against: three counts on a Pi is half its measured σ, so a
motionless pot there would publish continuously.

## Decision

**Layer B stops owning a deadband value. It keeps the rule.**

`ControlMapping::new` takes `deadband_counts` from its caller.
`src/control/mapping.rs` still documents what the value has to satisfy —
`deadband_counts >= σ` of the raw idle jitter, which is what
`FILTER_COEFFICIENT`'s exact 1/3 noise gain reduces three sigma of margin
to — and still owns the hysteresis behaviour, the travel fraction and the dB
arithmetic. What it no longer contains is a number derived from one board.

**Each layer A declares its own measured figure.**

- `ControlSource::DEADBAND_COUNTS` is an associated constant with **no
  default**, so adding a source is also being asked what its idle jitter is.
  `PiControls` declares 8.0. `ControlHandle::spawn` passes `S::DEADBAND_COUNTS`
  through, so layer C never chooses.
- The Bela host does not implement `ControlSource` — ADR 0010's reasoning for
  that is untouched — so `bela_host::controls::DEADBAND_COUNTS` sits beside the
  board's other measured constants and `OttApplication::new` passes it in.
  It is 3.0.

**Three counts is chosen against the measured span, not an estimated σ.** No
channel's reading spanned more than 2.5 counts end to end in 60 seconds at
either position, so the whole excursion ever observed fits inside the band.
That is a stronger statement than the Pi's 3.8σ, which admits an occasional
publish from a motionless pot by construction.

## Consequences

- **Layer B is more platform-independent than before, not less.** It named a
  board's converter in a constant; now it names none. The objection ADR 0011
  raised was to *Bela-specific constants in layer B*, and this is the opposite
  arrangement: the value moves out to the hardware that was measured, and the
  rule stays in the layer that enforces it.
- **The Raspberry Pi is unaffected, and needs no re-verification.** Its value,
  its rule and its measurement are unchanged; only the constant's address
  moved, from `src/control/mapping.rs` to `src/control/pi.rs`. ADR 0010's
  re-check discipline applies to a *changed* deadband, and the Pi's has not
  changed.
- **A Gem's knobs resolve about 2.7× finer**, which is audible only in the
  sense that it removes a step the deadband could previously force: 0.141 dB
  on the gain pots against 0.375 dB. Neither was above what the DSP's 20 ms
  smoothing lets through as a jump.
- **A third host has one more question to answer before it can be added**, and
  no default to answer it with by accident. That is the intended cost: the
  wrong deadband fails in one of two ways — a knob that chatters, or a knob
  that is needlessly coarse — and neither is visible in code review.
- **Layer B's other two constants stay shared.** `FILTER_COEFFICIENT` and
  `BYPASS_DEBOUNCE_READS` are not measurements of a converter:
  the filter coefficient is a noise-gain-versus-lag trade that both boards
  make identically, and the debounce is a property of the switch part, which
  is the same class on both. `PollDecimator` already exists to give the
  debounce the read rate it was calibrated for (ADR 0011), and that duty is
  unchanged.

## References

- [ADR 0010](0010-three-layer-control-surface-and-newest-value-handoff.md) —
  the three-layer split, and where the deadband was put.
- [ADR 0011](0011-bela-gem-stereo-as-the-second-host.md) — the port that
  reused layer B verbatim, and the rejected alternative this ADR revisits.
- [`docs/raspberry-pi/control-surface-verification.md`](../raspberry-pi/control-surface-verification.md)
  — the MCP3008 measurement behind 8.0.
- [`docs/bela/control-surface-verification.md`](../bela/control-surface-verification.md)
  — the Gem measurement behind 3.0.
- [`docs/contracts.md` §8](../contracts.md#8-control-surface) — the guarantees
  the deadband is one half of.
