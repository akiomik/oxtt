# Contracts and Invariants

Normative for `hyperglare-dsp`. What a caller may rely on, and what the crate
guarantees in return.

**This document grows with the crate.** Excitation, resonance, post-drive and
the mix exist; bypass does not, so section 7 says so rather than describing
something that is not there. A section that is absent is absent, not implied.

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

`HyperglareProcessor::process` is a loop over `process_frame` and nothing else,
so the two cannot disagree about anything.

**Output is independent of how the input is partitioned across calls.** No
stage carries block-scoped state, so a caller that chunks a buffer differently
gets the same samples — tested against block sizes of 1, 37, 64 and the whole
buffer.

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
  shape. `hyperglare` has an internal noise source in `Exciter`, and the gate
  on it is the *only* thing holding this invariant up.

  For the exciter the guarantee is exact and has a stated arrival: **once the
  gate's envelope crosses `effectkit`'s silence floor, the output is exactly
  zero**, with the drive and the noise at maximum. Not "after the release" —
  a one-pole release reaches 120 dB down after about fourteen of its own time
  constants, a bit under half a second — and not "small", because a tolerance
  would pass for a gate that had been deleted. Both the exciter's test and the
  processor's have been checked against a build with the gate removed: they are
  the only tests that fail there.

  **For the whole processor the arrival is two stages in series, and the second
  is a setting.** The gate goes on feeding the bank while it closes, and only
  then do the resonators start decaying toward the filter's own floor — which
  is what they are for. Reaching exactly zero therefore takes about half a
  second of gate plus seven decay times, so at a four-second decay it is
  measured in tens of seconds. That is correct behaviour, not a leak: a
  resonator asked to ring for four seconds rings for four seconds.

- **The output is bounded to full scale**, by a soft knee one decibel below it.
  Bounding every input into `[-1, 1]` while leaving some region untouched
  requires bending somewhere below one, so "transparent" means "transparent for
  material that is gain-staged" — which is what the input gain is for.
- **A non-finite sample never leaves the processor.** The frame path ends in a
  guard, so a host receives silence rather than a NaN. State poisoned by a
  non-finite *input* is cleared by `reset_state`, not by the guard.

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

**And the wet is matched to the dry, which is the one that makes the mix knob
work.** How loud the bank comes out depends on how much of the input's spectrum
lands on the grid, and that varied by about 20 dB across real material. A
linear crossfade between signals 20 dB apart is not a crossfade: measured on a
percussive source, nine tenths of `color` did nothing and the whole transition
happened in its last tenth. Matched, the same sweep is monotonic across the
range. `wet_match` at zero restores the raw behaviour for a caller who wants
the resonators at whatever level they were excited to.

The correction is measured on the mid and applied to both channels as a ratio.
A matcher per channel would read the dry's own left/right balance and print it
onto a wet that has no image of its own, which is a width manufactured from a
level rather than one that is there.

It holds while the input is silent. A ratio of two decaying envelopes says
nothing, and sweeping the gain across a tail would reshape the one part of the
output that is the effect's own.

## 5.1 Stereo

**The wet path is mono under `SearPlacement::AfterSum`.** Once the resonators
have been summed they cannot be separated again, so a waveshaper placed after
the sum has one signal to work on and produces one.

**What that costs depends on `color`, and at the top of its range it is the
whole stereo image.** Below one the dry survives and carries the width it
arrived with; at one and above there is no dry, so the output is mono however
wide the input was. Measured on a stereo source: 0.56 in, 0.00 out.

`SearPlacement::BeforeSplit` keeps the image, because it splits before it
shapes. A decorrelator would let `AfterSum` keep it too, and is deliberately
not built: it would change the width of one arm of a comparison whose subject
is the waveshaper's position rather than the width.

**Which of those is worth it is an M0 question**, and the cost above is one of
the things it has to weigh. It is recorded here rather than left to be found on
a stereo source, which is how it was found.

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

**Still not a contract**, and it will not be `oxtt`'s when it is one.

Deferred deliberately rather than forgotten: the choice below is between two
implementations that feel different under the hands, and there is no control
surface yet to feel them with. Nothing in the offline path needs a bypass.

`oxtt` stops when its input stops, so a 20 ms crossfade into bypass is
inaudible. `hyperglare` rings for as long as its decay, and the same crossfade
would cut a tail off mid-ring. What a pedal wants is to disconnect the input
and let the tail finish, which is a different implementation, not a different
constant — and it brings a question about whether the bank keeps running while
bypassed.

`reset` differs for the same reason: `oxtt`'s means "as if newly constructed",
and doing that to `hyperglare` deletes a sounding chord.

Both are decided when there is a host with a switch on it.

## 8. Real-time callback

See [`docs/effectkit/realtime.md`](../effectkit/realtime.md).

What lands where, in this crate: `ResonatorBank::process` is the per-sample
path, and `retune` and `Grid::frequencies` are on the callback too — under Bela
a chord change arrives inside `render_pre`. All three carry `#[no_panic]`
proofs, checked at link time by `cargo test --release`.
