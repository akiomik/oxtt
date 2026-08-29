# ADR 0019: `drive` Moves the Level, and Where the Shaper Is Normalised Is Why

## Status

Accepted.

Implemented as `NORMALISE_AT_DBFS` in `crates/hyperglare-dsp/src/exciter.rs`.
The parameter and the `--normalise-at` flag that made the sweep possible are
gone again, as this ADR said they would be: a normalisation point is a design
constant, not something a player reaches for.

## Scope

`hyperglare`.

Concerns `Shaper` in `crates/hyperglare-dsp/src/exciter.rs` and the level
contract in [`docs/hyperglare/contracts.md`](../hyperglare/contracts.md) §5 and
§6. **No change to the bank, the grid or the band split.**

Completes what [ADR 0017](0017-the-wet-path-carries-no-time-constant-of-its-own.md)
left open. That ADR replaced a level follower with a static compensation and
said `color` would become a real crossfade. It became one against the bank; it
did not become one against `drive`, and this is why.

**It also withdraws two things ADR 0017 says about that number.** ADR 0017
gives it as "about 21 dB" and as "evenly across the bands"; both came from the
prototype rig — band-limited input into a band-limited grid — rather than from
the effect. Measured on the effect the figure is about 19 dB and the bands are
8.7 dB apart. **ADR 0017's decision stands**; what is withdrawn is the
measurement it quoted in passing, and the reasoning that rested on the bands
being even is redone below.

`docs/hyperglare/contracts.md` carries the current figures, as the living
document; ADR 0017 keeps what it was accepted with.

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
only, taken from `--raw-output` so that loudness matching is not in it, at the
defaults — **grid ceiling 5 kHz, `bands::EDGES` six entries, seven bands**. The
ceiling has moved twice already and the band edges move with it, so a figure
here without that line is a figure nobody can check later.

```text
 drive     wet RMS    against drive 0    small-signal theory
  0.0      -27.45 dB          +0.0 dB               +0.0 dB
  0.3      -23.97 dB          +3.5 dB               +5.3 dB
  0.6      -17.57 dB          +9.9 dB              +15.2 dB
  1.0       -8.78 dB         +18.7 dB              +30.3 dB
```

**The knob moves the wet by 19 dB on this source**, which is the same order as
the 18 dB the bank sits under the dry before `makeup` corrects it. So the
defect ADR 0017 named is only half shut: `color` crossfades between the dry and
a wet whose level is set by a different knob.

Three things about the number:

- **It is not uniform across the bands, and the spread cannot be compensated
  either.** The same render, per band, from the bottom up:
  24.6 / 19.7 / 15.8 / 17.4 / 20.0 / 19.9 / 19.0 dB — 8.7 dB apart. That is not a global scalar, but it
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

**The curve passes unity gain at −12 dBFS.**

The choice is one-dimensional rather than a menu: `Shaper` normalises so that
the curve is unity-gain at *some* input level, and every level is available.
Full scale is one end of that line, not a neutral default. Swept across three
sources — `duck`, `laser` and the drum loop — **at the default `color` of 0.63
and with loudness matching switched off**, because what a listener hears the
knob do is the output rather than the wet, and because the level *is* the
question:

```text
 normalise at    drive 0 → 1 moves the output by   character vs input level
  full scale          +10.1 to +14.3 dB               4.07 dB rms
  −12 dBFS             +1.6 to  +4.2 dB               1.63 dB rms
  −24 dBFS             −1.6 to  −0.8 dB               1.21 dB rms
  small signal         −2.9 to  −2.0 dB               0.95 dB rms
```

**The wet alone moves more than the output does**, because the dry dilutes it:
on `duck` at the chosen point the wet rises 7.7 dB where the output rises 4.2.
The Context table above is the wet, this one is the output; neither is wrong
and they do not answer the same question.

The right-hand column is how far the wet's spectrum moves, level removed, when
the input is driven 12 dB harder. **At full scale the character depends on the
input's level four times as much**, because a 12 dB louder input crushes the
crest factor from 17.2 dB to 10.4 where every other point holds it near 17.8.

−12 dBFS is where the knob still behaves like a drive knob — two to four
decibels, which is what the idiom means — without becoming a mix control. Ten
to fourteen is a mix control, and contradicts what §5 says about `color`.

**Two things this measurement corrected.** The first draft said small-signal
normalisation "crushes peaks by about 30 dB" and called it the least like a
drive knob. It is the reverse: dividing by `g` alone lowers the whole curve out
of its own bend, so it distorts *less* and holds the crest factor. And the
normalisation point barely changes how much distortion there is at all —
brightness moves 1.0 to 2.6 dB across the whole line. What it changes is the
level and the level-dependence.

The alternative treatment is recorded as a treatment rather than an option:
compensating from the small-signal gain over-corrects by 12 dB at maximum on
`duck` and by 1 dB on a quiet tone, so the knob's level would move
unpredictably with the material rather than not at all. It does not address the
normalisation point, which is the thing that produced the number.

## Consequences

- **The implementation is cheap, wherever on the line this lands.** `Shaper`
  computes `normalise` once at control rate, so moving the normalisation point
  changes one expression in `Shaper::new`. **No gating changes**: `apply_params`
  already rebuilds `ExciterCoeffs` — and with it the `Shaper` — whenever
  `ExciterParams` moves, which arrived with the commit that took the `powf` off
  the sample path rather than with ADR 0016. The bank's retune gate is not
  involved either way, because nothing here belongs in the gain law's share.
- **Every render changes.** The shaper's small-signal gain drops by about
  12 dB, so the wet arrives quieter at any drive above zero, and the offline
  renderer's loudness matching hides that while a live host will not.
- **Until this is decided, §5 and §6 carry the exception.** They record that
  `sear` and `drive` are outside the compensation, which is honest and is not a
  design.
- **`sear` moves with it.** The post-drive is the same `Shaper`, and it sits
  outside the compensation, so its level behaviour changed too. At the default
  of zero it is an exact bypass, so nothing at the defaults moved.
- **What is not closed is `color` across `drive`.** Two to four decibels is
  small enough to live with and is not zero, so §5's crossfade claim holds
  approximately rather than exactly. Making it exact means compensating a
  quantity that depends on the material, which is what this ADR declined.
