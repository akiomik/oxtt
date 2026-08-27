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
below it, and everything below it is where music lives. Measured on `duck`,
wet only and before loudness matching:

```text
 drive     wet RMS    against drive 0    small-signal theory
  0.0      -25.24 dB          +0.0 dB               +0.0 dB
  0.3      -21.99 dB          +3.2 dB               +5.3 dB
  0.6      -15.94 dB          +9.3 dB              +15.2 dB
  1.0       -7.84 dB         +17.4 dB              +30.3 dB
```

**The knob moves the wet by 17 dB on this source**, which is the same order as
the 20 dB gap `wet_match` was built to close. So the defect ADR 0017 named is
only half shut: `color` crossfades between the dry and a wet whose level is set
by a different knob.

Three things about the number:

- **It is within a couple of decibels of uniform across the bands.** Measured
  on `duck` and on the `oxtt` drum loop, the rise from `drive 0` to `drive 1`
  spans 20.1–22.0 dB across the five bands on the first and 20.5–24.4 dB on the
  second. Uniform enough to treat as a global scalar, which is why it does not
  belong in the gain law's per-band share.
- **`noise_amount` does not do this.** The same sweep moves the level by at
  most 0.7 dB, in the top band. The noise path changes what is in the
  excitation, not how much of it there is.
- **It is not derivable from the knob.** Small-signal theory says +30.3 dB and
  the measurement says +17.4. The gap is the saturation, and it depends on how
  the input's level is distributed rather than on the setting: the same sweep
  on a pure tone — low crest factor, so deep into the curve — moves by about
  +7.6 dB.

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
compensating from the small-signal gain over-corrects by 13 dB at maximum, so
the knob would get *quieter* as it is turned up in a way nothing predicts; and
leaving it alone contradicts what §6 says about the mix. Neither addresses the
normalisation point, which is the thing that produced the number.

## Consequences

- **The implementation is cheap, wherever on the line this lands.** `Shaper`
  computes `normalise` once at control rate, so moving the normalisation point
  changes one expression in `Shaper::new`. **No gating changes**: since
  [ADR 0016](0016-the-bank-is-excited-per-band.md), `apply_params` already
  rebuilds `ExciterCoeffs` — and with it the `Shaper` — whenever
  `ExciterParams` moves. The bank's retune gate is not involved either way,
  because this is a global scalar and not a term in the gain law's share.
- **Every render changes**, and the drive knob's character changes with it —
  that is the point of it and also its risk. The current curve is what M0
  listened to.
- **Until this is decided, §6 carries the exception.** It records that `sear`
  and `drive` are outside the compensation, which is honest and is not a
  design.
- **What decides it is a listening test.** The numbers above already say what
  each normalisation point does to the level. What none of them says is which
  one sounds like a drive knob, and the effect has never been heard normalised
  anywhere but at full scale.
