# Contracts and Invariants

Normative for `hyperglare-dsp`. What a caller may rely on, and what the crate
guarantees in return.

**This document grows with the crate.** The resonator bank exists; the
processor around it — excitation, post-drive, mix, bypass — does not yet, so
the sections that belong to it say so rather than describing something that is
not there. A section that is absent is absent, not implied.

The real-time prohibitions are not restated here. They are the one contract
shared across every effect in this family rather than duplicated per project,
and they live in [`docs/effectkit/realtime.md`](../effectkit/realtime.md)
([ADR 0015](../decisions/0015-documentation-is-namespaced-by-project.md) §3).
Everything below is `hyperglare`'s own.

## 1. Parameter validation

**Not yet a contract.** `BankParams` and `Grid` are plain `pub` fields with no
invariant of their own; the arithmetic that consumes them floors what it must
(the decay, the Q cap, the tilt) so that no setting can produce a non-finite
coefficient. Validation moves into the types when there is a command line to
reject a bad value *at*, and this section will then say what each range is and
which are compatibility contracts.

Until then, the guarantee is the weaker one in section 4: any settings at all
produce finite output.

## 2. Bank lifecycle and retuning

`ResonatorBank::retune` is the only way a chord or a setting reaches the
filters. It is a control-rate operation: everything it computes reaches the
per-sample path through a `tan` and a `powf`, and doing that per sample would
cost two orders of magnitude more than the filtering itself.

- **Retuning preserves filter state.** A chord may change under a ringing bank
  without a click. What that state *means* changes with the coefficients, so a
  large retune glides rather than jumps — deliberately: a jump is a click and a
  glide is a portamento, and only one of those can be tuned into something
  musical afterwards.
- **Filters entering use start from rest.** A resonator that was not sounding
  before a retune begins at zero rather than at whatever a previous chord left
  in it.
- **`reset_state` clears the tails and keeps the tuning.** It is the only way
  to silence a ringing bank. A caller that wants the tails to survive a change
  simply does not call it.
- **A non-positive or non-finite note is skipped.** A caller need not compact
  its own voice table.
- **Capacity is a bound, not a hint.** A chord that does not fit loses its
  *last* voices, in the order the caller listed them; it does not thin all of
  them evenly and does not wrap. An allocator that cares which notes survive
  orders the table itself.

  **Truncation costs resonators, not level.** The divisor in section 5 is
  capped at the capacity, so a bank too small for its settings is not also a
  quiet one. What a caller loses is the top of its chord, which is a thing it
  can see in `active()` and act on; a level drop it could only hear would tell
  it nothing about what to change.

## 3. Buffer processing

`ResonatorBank::process` takes one sample and returns one sample. There is no
block entry point, and therefore no way for the two to disagree.

**Output is independent of how the input is partitioned across calls**, because
there is nothing to partition: the bank carries no block-scoped state. A caller
that splits a buffer differently gets the same samples.

When the processor arrives, its block entry point will be a loop over the
per-frame one and this section will say so explicitly, the way `oxtt`'s does.

## 4. Signal invariants

These hold for every sample, at any settings:

- **Finite in, finite out.** No combination of decay, Q cap, tilt, detune,
  drift or geometry produces a NaN or an infinity from finite input. A
  *non-finite* input is not defended against here — it poisons filter state,
  and the guard for that belongs at the processor's edge, where `oxtt` puts it.
- **Silence in, silence out.** A bank driven with zeroes decays toward zero and
  never grows.

  **This is not structural, and that is a difference from `oxtt`.** `oxtt` only
  ever scales what arrived, so silence out of silence is a property of its
  shape. `hyperglare` will have an internal noise source in its excitation
  stage, and once it does, the gate on that source is the *only* thing holding
  this invariant up. The test for it is therefore not "silence produces
  silence" but "silence produces silence with the excitation and the decay at
  maximum".
- **The output is bounded.** Q reaches into the thousands, so the bound is
  worth stating separately from finiteness. It is currently a consequence of
  the per-filter normalisation and the density divisor rather than of a
  limiter; a limiter belongs at the processor's output and is not here yet.

## 5. Level

Two settings that ought to change the sound without changing the loudness, and
are made not to:

- **The voice count divides by its own square root**, so changing the setting
  does not change how loud the bank can get. It is the *configured* count, not
  the number of notes held: dividing by the live count would duck a sustaining
  chord the moment another note joined it. A chord is louder than one note.
- **The geometry's density divides out the same way**, measured at a fixed
  reference note. Uncompensated, a harmonic series runs about twenty times the
  resonators an octave grid does and is some 13 dB louder — and a comparison
  between two loudnesses is decided by the louder one, whatever it sounds like.

  The divisor is `min(voices · density, capacity)`: every term is a property of
  the settings, so it never moves with the chord being played, and it never
  counts resonators a truncated bank does not have (section 2).

  **A fixed reference is exact at one note only.** Under an octave grid that
  costs nothing, because the point count barely moves across the keyboard.
  Under a harmonic series it is about 9 dB over three octaves of played note,
  quietest at the top, because a harmonic series only goes up and the band's
  ceiling does not move.

  This is not corrected in the divisor: following the live count would
  reintroduce the ducking the fixed reference exists to prevent. It is a cost
  charged to `Geometry::Harmonics`, and it means **a comparison between the
  geometries is level-matched on a sustained note near the reference and is not
  level-matched on a bass line** — the top of the line will sound thin, and
  timbre and level cannot be told apart by ear.

The decay is a third: see section 6.

## 6. The gain law

```text
BW(f)   = max( ln(1000)/(pi·T60),  f/q_max )
gain(f) = tilt(f) · (BW(f) / BW_ref)^(-p)
```

- **`BW` is continuous across the breakpoint**, because it is a maximum of the
  two branches. There is no case split and therefore no seam that can be got
  wrong.
- **`BW_ref` is a fixed reference decay, not the current one.** Against the
  current decay the ratio would be one everywhere below the breakpoint and the
  decay compensation would cancel itself out entirely.
- **The exponent is one judgement, not two.** Below the breakpoint it flattens
  the decay's effect on level; above it, it sets a slope of `-6.02·p` dB per
  octave. Both are the same number, so the tonal-versus-broadband question is
  answered once.
- **Sweeping the exponent does not move the level at the reference decay**,
  which is what makes choosing it by ear a comparison of spectrum rather than
  of loudness.

Above the breakpoint the effective decay is `T60 · f*/f`, and **the decay
setting stops reaching**. That is the cost the Q cap buys robustness with, and
it is stated here rather than left to be discovered.

## 7. Bypass and reset

**Not yet a contract**, and it will not be `oxtt`'s when it is one.

`oxtt` stops when its input stops, so a 20 ms crossfade into bypass is
inaudible. `hyperglare` rings for as long as its decay, and the same crossfade
would cut a tail off mid-ring. What a pedal wants is to disconnect the input
and let the tail finish, which is a different implementation, not a different
constant — and it brings a question about whether the bank keeps running while
bypassed.

`reset` differs for the same reason: `oxtt`'s means "as if newly constructed",
and doing that to `hyperglare` deletes a sounding chord.

Both are decided with the processor.

## 8. Real-time callback

See [`docs/effectkit/realtime.md`](../effectkit/realtime.md).

What lands where, in this crate: `ResonatorBank::process` is the per-sample
path, and `retune` and `Grid::frequencies` are on the callback too — under Bela
a chord change arrives inside `render_pre`. All three carry `#[no_panic]`
proofs, checked at link time by `cargo test --release`.
