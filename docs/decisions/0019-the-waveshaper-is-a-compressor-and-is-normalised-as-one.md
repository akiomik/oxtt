# ADR 0019: `drive` Moves the Level by 21 dB, and the Normalisation Is the Reason

## Status

Proposed

## Scope

`hyperglare`.

Concerns `Shaper` in `crates/hyperglare-dsp/src/exciter.rs` and the level
contract in [`docs/hyperglare/contracts.md`](../hyperglare/contracts.md) §5 and
§6. **No change to the bank, the grid or the band split.**

Completes what [ADR 0017](0017-the-wet-path-carries-no-time-constant-of-its-own.md)
left open: that ADR replaced a level follower with a static compensation and
said `color` would become a real crossfade. It became one against the bank. It
did not become one against the drive, and this is why.

## Context

`Shaper::shape` is normalised so that a full-scale input comes back at full
scale. That is one of two normalisations available and it decides the level of
everything downstream.

```rust
let driven = self.gain * x;                        // gain = db_to_amp(drive * 30)
let shaped = driven / (1.0 + driven.abs()) * self.normalise;
self.amount.mul_add(shaped - x, x)
```

The curve compresses, so a normalisation is exact at one input level and wrong
everywhere else. Holding the peak still means lifting everything below it:

```text
 drive    small-signal gain     measured effect on the wet
  0.0            0.0 dB                    0 dB
  0.3           +5.3 dB                 -4.2 dB
  0.6          +15.2 dB                -11.5 dB
  1.0          +30.3 dB                -21.3 dB
```

**The knob moves the wet by 21 dB.** That is more than the 20 dB gap
`wet_match` was built to close, so the defect ADR 0017 named is only half
shut: `color` crossfades between the dry and a wet whose level is set by a
different knob.

Two things about the number are settled and one is not:

- **It is the same in every band.** Measured across the five bands on two
  sources: 1.9 dB of spread on one, 3.8 dB on the other. So it is a global
  scalar, not a per-band term, and it does not belong in the gain law's share.
- **`noise_amount` does not do this.** The same sweep moves the level by at
  most 0.7 dB, in the top band. The noise path changes what is in the
  excitation, not how much.
- **It is not derivable from the knob alone.** Small-signal theory says
  30.3 dB and the measurement says 21.3. The 9 dB gap is the saturation, which
  depends on how the input's level is distributed — a property of the material,
  not of the setting.

## Decision

**Not taken yet.** Three options, and the choice between them is a judgement
about what a drive knob is, not a measurement:

1. **Compensate from the small-signal gain.** Exact at low drive; over-corrects
   by 9 dB at maximum, so the knob would get *quieter* as it is turned up, in a
   way nothing predicts. Trading a known error for an unpredictable one.
2. **Leave it, and say so.** A drive knob that raises the level is what drive
   knobs do. The cost is that `color` is not a crossfade at high drive, and
   this contradicts what §5 says about the mix.
3. **Renormalise the shaper.** Unity small-signal slope instead of unity
   full-scale, which makes low drive level-neutral and crushes peaks by about
   30 dB at maximum. This is the option that treats the cause rather than the
   symptom, and it changes what the drive knob sounds like, not only how loud
   it is.

**What is decided is that 1 and 2 are treatments and 3 is the question.** The
shaper is a compressor with a peak normalisation; whether that is what the
effect wants is a sound decision, and it has never been listened to the other
way round.

## Consequences

- **Every render changes under option 3**, and the drive knob's character
  changes with it. That is the point of it and also its risk: the current
  curve is what M0 listened to.
- **Options 1 and 3 put `drive` back into the retune gate.** `apply_params`
  deliberately excludes `ExciterParams` today, and its documentation lists the
  knobs that cost nothing; both would need revising. Option 2 changes nothing.
- **Until this is decided, §5 carries the exception.** It says `sear` and
  `drive` are outside the compensation, which is honest and is not a design.
- **The measurement that would decide it is a listening test**, not another
  sweep. The numbers above already say what each option does to the level; what
  none of them says is which one sounds like a drive knob.
