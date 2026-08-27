# ADR 0019: `drive` Moves the Level, and Where the Shaper Is Normalised Is Why

## Status

Proposed

## Scope

`hyperglare`.

Concerns `Shaper` in `crates/hyperglare-dsp/src/exciter.rs` and the level
contract in [`docs/hyperglare/contracts.md`](../hyperglare/contracts.md) §5 and
§6. **No change to the bank, the grid or the band split.**

Completes what [ADR 0017](0017-the-wet-path-carries-no-time-constant-of-its-own.md)
left open. That ADR replaced a level follower with a static compensation and
said `color` would become a real crossfade. It became one against the bank; it
did not become one against `drive`, and this is why.

## Context

`Shaper::shape` is normalised so that a full-scale input comes back at full
scale.

```rust
let driven = self.gain * x;                        // gain = db_to_amp(drive * 30)
let shaped = driven / (1.0 + driven.abs()) * self.normalise;
self.amount.mul_add(shaped - x, x)
```

The curve compresses, so **a normalisation is exact at one input level and
wrong at every other one**. Holding the peak still means lifting everything
below it, and everything below it is where music lives.

Every figure below is the same measurement: `duck`, the paired recording, wet
only, taken from `--raw-output` so that loudness matching is not in it.

```text
 drive     wet RMS    against drive 0    small-signal theory
  0.0      -25.24 dB          +0.0 dB               +0.0 dB
  0.3      -21.99 dB          +3.2 dB               +5.3 dB
  0.6      -15.94 dB          +9.3 dB              +15.2 dB
  1.0       -7.84 dB         +17.4 dB              +30.3 dB
```

**The knob moves the wet by 17 dB on this source**, which is the same order as
the 18 dB the bank sits under the dry before `makeup` corrects it. So the
defect ADR 0017 named is only half shut: `color` crossfades between the dry and
a wet whose level is set by a different knob.

Three things about the number:

- **It is not uniform across the bands, and the spread cannot be compensated
  either.** The same render, per band: 24.4 / 19.5 / 15.7 / 17.2 / 17.4 dB
  from the bottom band up — 8.7 dB apart. That is not a global scalar, but it
  is not a static one either: the shaper is applied to each band separately
  now, so how much each one saturates is set by how loud *that band of that
  source* happens to be. A per-band term in the gain law would have to know the
  material, and the gain law is deliberately settings-only (§5).
- **`noise_amount` does not do this.** The same sweep moves the level by at
  most 0.7 dB, in the top band. The noise path changes what is in the
  excitation, not how much of it there is.
- **It is not derivable from the knob, because it is a function of the input's
  level.** A 110 Hz tone through the same sweep:

  ```text
   input      wet rise, drive 0 → 1
   -6 dBFS            +6.3 dB
  -26 dBFS           +24.0 dB
  -46 dBFS           +29.4 dB
  ```

  Approaching the small-signal 30.3 dB as the input gets quiet, and collapsing
  to a sixth of it when the input is loud enough to sit in the bend. **This is
  the whole of the decision below**: the number is not a property of the knob.

## Decision

**Not taken.** What the options are is settled and which one is right is not.

The diagnosis above says the choice is one-dimensional rather than a menu.
`Shaper` normalises so that the curve is unity-gain at *some* input level, and
every level is available:

```text
 normalise at ...   drive at maximum does ...        what it costs
  full scale        lifts everything below by ~17 dB  today's behaviour
  −12 dBFS          splits the difference             untried
  zero (small       crushes peaks by ~30 dB           the least like a
  signal)                                             drive knob
```

**Today's setting is one end of that line, not a neutral default.** The
question is where on the line the effect wants to be, and it is a question
about what a drive knob should sound like — turning it up making things louder
is what drive knobs do, and 17 dB is more than that idiom usually means.

Two treatments exist and are recorded as treatments rather than options:
compensating from the small-signal gain over-corrects by 13 dB at maximum on
`duck` and by 1 dB on a quiet tone, so the knob's level would move
unpredictably with the material rather than not at all; and leaving it alone
contradicts what §5 says about `color` being a crossfade. Neither addresses the
normalisation point, which is the thing that produced the number.

## Consequences

- **The implementation is cheap, wherever on the line this lands.** `Shaper`
  computes `normalise` once at control rate, so moving the normalisation point
  changes one expression in `Shaper::new`. **No gating changes**: `apply_params`
  already rebuilds `ExciterCoeffs` — and with it the `Shaper` — whenever
  `ExciterParams` moves, which arrived with the commit that took the `powf` off
  the sample path rather than with ADR 0016. The bank's retune gate is not
  involved either way, because nothing here belongs in the gain law's share.
- **Every render changes**, and the drive knob's character changes with it —
  that is the point of it and also its risk. The current curve is what M0
  listened to.
- **Until this is decided, §5 and §6 carry the exception.** They record that
  `sear` and `drive` are outside the compensation, which is honest and is not a
  design.
- **What decides it is a listening test.** The numbers above already say what
  each normalisation point does to the level. What none of them says is which
  one sounds like a drive knob, and the effect has never been heard normalised
  anywhere but at full scale.
