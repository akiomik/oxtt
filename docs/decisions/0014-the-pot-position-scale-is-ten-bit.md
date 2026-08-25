# ADR 0014: The Pot Position Scale is Ten Bits, For Every Surface

## Status

Accepted

Makes explicit a constant that
[ADR 0010](0010-three-layer-control-surface-and-newest-value-handoff.md)
introduced as an implementation detail. Nothing about the three-layer split
changes.

## Context

`PotPosition` is where a pot is sitting, as a step in `0..=1023`. The ceiling
came from the Raspberry Pi's MCP3008, which is a 10-bit successive-
approximation converter and produces that range directly.

The Bela Gem Stereo's converter is not 10-bit. Its layer A divides the reading
by the ratio between the ADS8166's 4.096 V reference and the 3.3 V rail the
pots are wired across, then rounds onto the same `0..=1023` scale. It maps
*onto* the Pi's scale rather than using its own.

While that was one crate's internal detail, it was a comment. Now that
`effectkit-controls` is the crate every future control surface reads through,
the ceiling is a declaration about all of them, and the reason it is worth
writing down is not the loss of resolution — it is what depends on the number:

- **The deadband is in counts.** Each surface's `deadband_counts` was measured
  as counts of *this* scale (8.0 on the Pi, 3.0 on the Gem, ADR 0012).
  Changing the scale silently rescales every measured deadband.
- **The filter's noise gain argument is in counts.** The rule
  `deadband_counts >= σ` of the raw jitter compares two quantities that are
  both counts.
- **The debounce is not.** It counts reads, so it is unaffected — which is
  worth stating, because the four values otherwise travel together.

## Decision

**`POT_POSITION_MAX = 1023` is the scale every control surface reports on,
whatever its converter.** A surface with a wider converter scales onto it in
its layer A, as the Gem does.

**A wider scale is a change to `PotPosition`, not a parameter.**
`PotTravel::from_counts` deliberately takes no full-scale argument. Passing
one would create a correspondence nothing enforces — "any scale, as long as it
is the one `PotPosition` uses" — and the type is where the scale belongs.

**Raising it means re-deriving every `deadband_counts` from its measurement**,
not multiplying the existing numbers. The measurements are the source of those
values; the numbers are derived.

### Why not normalise in layer A and skip counts entirely

Because the deadband is applied before normalisation and is meaningful only in
the converter's own units. Hysteresis against a fraction of travel would have
to be converted back before it could be compared against anything, and the
`deadband >= σ` rule would compare a fraction against a count.

The asymmetry is deliberate and is confined: the seam *out* of conditioning is
`PotTravel`, a fraction, so counts never reach an effect's assignment step.
`ConditioningConfig`'s documentation says so, because it is the one type that
holds a count and is read from outside.

### Why not make `Pots<T>` generic over the arity while we are here

Six is the arity both existing surfaces have. A third with a different count
gets its own type rather than making this one generic over length: the reason
`Pots` has named fields at all is that the assignment step can then name each
pot the way the wiring does, and an arity parameter takes that away.

## Consequences

- Ten bits is 1023 distinct positions across a sweep. Against the Gem's
  measured deadband of 3.0 counts, roughly 341 of them are reachable; against
  the Pi's 8.0, roughly 128. The deadband, not the converter, is the limit on
  both surfaces, so a wider scale would buy nothing until the noise floor
  changes.
- `PotTravel::from_counts` divides by this constant, so the rounding it
  introduces is fixed by this decision too — including that
  `1023 = 2^10 - 1` sits just below a power of two, which widens the relative
  spacing of the result by about 0.098%.
- A surface whose converter is *narrower* than ten bits would report a scale
  it cannot fill. Nothing prevents that, and nothing should: the deadband
  would absorb the gaps.

## References

- [ADR 0010](0010-three-layer-control-surface-and-newest-value-handoff.md) —
  where `PotPosition` came from.
- [ADR 0012](0012-the-jitter-deadband-belongs-to-the-control-source.md) — the
  per-surface deadbands measured in these counts.
- [ADR 0013](0013-crates-not-features-and-effectkit-as-the-shared-half.md) —
  the crate split that turned this detail into a shared contract.
- [`docs/raspberry-pi/control-surface-verification.md`](../raspberry-pi/control-surface-verification.md),
  [`docs/bela/control-surface-verification.md`](../bela/control-surface-verification.md)
  — the measurements the counts belong to.
