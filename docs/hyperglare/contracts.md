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

`ResonatorBank::retune`, `resettle`, `note_on` and `note_off` are the only
ways a chord or a setting reaches the filters, and all four are control-rate:
everything they compute reaches the per-sample path through a `tan` and a
`powf`, and doing that per sample would cost two orders of magnitude more than
the filtering itself.

**Which of the four a caller uses is not a preference.** `retune` names the
whole chord and `note_on`/`note_off` name one key, so a caller mixing them
would release with one hand what it pressed with the other; `resettle` is the
settings-only path for the keyed caller, which has no chord to name.

- **A voice owns a block of slots and keeps it.** Voice `k` has the resonator
  slots `[k·stride, (k+1)·stride)`, where the stride is the most grid points
  any one note can produce. **No note can be moved by another note arriving or
  leaving**, which is what makes a keyboard playable: before this the slots
  were packed in the order the caller listed its notes, so releasing the middle
  note of a chord gave the released note's ringing state to the note above it
  and cut that note's own tail.
- **Retuning preserves the filter state of a voice whose note did not move.**
  A decay or a tilt may change under a ringing bank without a click.
- **A voice given a different note starts from rest.** One rule for a chord
  changing and for a note stealing a voice: both write new coefficients over
  state that meant another frequency, and playing that state at the new pitch
  is a note nobody pressed.
- **Filters entering use start from rest.** A resonator that was not sounding
  before begins at zero rather than at whatever a previous note left in it.
  A stolen voice is reset, which is what keeps this true of an allocator.
- **`reset_state` clears the tails and keeps the tuning.** It is the only way
  to silence a ringing bank. A caller that wants the tails to survive a change
  simply does not call it.
- **A non-positive or non-finite note is skipped**, and reads back as zero.
- **Releasing a note stops its excitation and nothing else.** The voice keeps
  its coefficients and its state and decays at its own `T60`. The decay is the
  release; nothing in the wet path gains a time constant (section 5, ADR 0017).
- **Capacity is a bound, in whole voices.** The table is `min(voices,
  capacity / stride)` blocks, so a chord that does not fit loses its *last*
  voices in the order the caller listed them. A geometry too dense for even one
  block gets one block and truncates inside it. An allocator that cares which
  notes survive orders the table itself.

  **Truncation is not charged twice.** The divisor in section 5 is capped at
  the capacity, so a bank too small for its settings does not also divide by
  resonators it does not have. What it still costs is whatever the gain law's
  own spread across the band says: the resonators that survive are the lowest,
  and since the share is measured against the band the lowest are the
  quietest.

  **`active()` is the cost and `held_voices()` is the chord.** The bank runs
  its blocks whether or not the voices have notes — that is what keeps the load
  flat when a chord arrives — so what a caller reads to see truncation is the
  voice count, not the resonator count.

## 2.0 Excitation is per band

**A resonator is excited by the input's energy near its own frequency, not by
the input as a whole** ([ADR 0016](../decisions/0016-the-bank-is-excited-per-band.md)).
`Exciter::process` returns one sample per band; `ResonatorBank::process` takes
them as a slice and each resonator reads the one its frequency maps to.

- **A band the source is silent in produces exactly zero.** The gate is per
  band now, so the silence guarantee in section 4 holds band by band rather
  than only for the whole.
- **The band a resonator draws on is decided at retune**, from its centre
  frequency, and does not change until the next one. `crate::bands::band_of`
  is the map, and it is total: zero, the negatives and `NaN` all land in the
  bottom band.
- **A slice shorter than the band count is silence, not an error.** A missing
  band reads as zero. There is nothing an audio callback could do with a
  failure here, and a bank that goes quiet is a symptom a caller can find.
- **The bands do not sum back to the input.** Each is its own band-pass, not a
  branch of a crossover. Nothing downstream reconstructs the source, so the
  split owes isolation rather than reconstruction — 12 dB per octave on each
  side, which is about 10 dB down an octave outside a band and 22 dB down two.

## 2.1 Derived values and the settings they came from

**Nothing on the per-sample path reads a parameter.** `ResonatorBank` keeps no
`BankParams`, and since the drive's `10^(x/20)` moved off the sample path
`Exciter` keeps no `ExciterParams` either: both see coefficients that were
derived at control rate and nothing else.

This is a guarantee about a failure that cannot happen rather than one about
behaviour. A path that could see both would be a path that can be handed a
coefficient and a setting disagreeing about the same knob, and the symptom —
a knob that reads back correctly and does nothing — is one a caller cannot
diagnose from the outside.

The obligation this puts on a caller is exact: **`ExciterCoeffs` is derived
from the sample rate *and* the exciter's settings, so it is rebuilt when either
moves.** `HyperglareProcessor` does this in `apply_params` and
`set_sample_rate`; a caller driving the pieces directly owns it.

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

  **The voice count is also the polyphony**, since each voice owns a block of
  slots whether or not it has a note (section 2). So a key lifting does not
  reach the divisor at all — a released voice keeps its block — and neither
  does a key pressing. **Nothing a player does to a keyboard can duck this
  bank.**
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

**And the wet sits at the level of what excited it, which is the one that
makes the mix knob work.** A resonator collects the part of its band that falls
inside its own bandwidth — 2.2 Hz of 130 at the reference decay — so left
alone the bank arrives some 18 dB under the dry, and a linear crossfade between
signals that far apart is not a crossfade. Measured on a percussive source
before this was corrected, nine tenths of `color` did nothing and the whole
transition happened in the last tenth.

The correction is the `makeup` term of section 6. **It is a constant**, so
nothing here reads the chord, follows an envelope or has a time constant of its
own: those are the properties the follower this replaced could not keep, and
[ADR 0017](../decisions/0017-the-wet-path-carries-no-time-constant-of-its-own.md)
records why it was removed rather than tuned.

**What it does not cover is `drive`, and it is small rather than absent.** The
waveshaper compresses, so its level depends on where the curve is normalised
and on how loud the input is; at the chosen normalisation point the knob moves
the output by 2 to 4 dB across its range against the 10 to 14 it moved before.
So `color` is a crossfade at a fixed drive and approximately one across the
drive knob. See [ADR 0019](../decisions/0019-drive-moves-the-level-and-the-normalisation-is-why.md).

## 5.0 Where the band stops

**The grid's ceiling defaults to 5 kHz, and it has been two other numbers.**

The design took 9 kHz from a published range, on the reasoning that extending
resonance into the top of the spectrum is what makes an effect glare. Measured
against three commercial processors on one source, the top came out sparse and
a listener called it a bell, so it went to 1.8 kHz — below which the top of the
output belongs to the source rather than to the bank.

**Both measurements were right about one source, and the trade has two sides.**
Swept across six — five paired recordings and a drum loop:

```text
 ceiling                    1800    3000    5000    9000
 chord content, 2-9 kHz    0.011   0.099   0.164   0.225
 spectral density, same    0.448   0.288   0.198   0.162
 references                                0.192 .. 0.298
```

Raising it puts a chord above 2 kHz, which at 1.8 kHz is simply absent: the top
two bands measure what the source already had and nothing else. Raising it also
thins that band, and below about 0.19 the top reads as isolated tones, which is
what a bell is. **5 kHz is where a listener put it across all six**, and it is
where the chord has arrived and the density has not yet left the references'
range.

**Per-band excitation changed one half of this and not the other.** Since
[ADR 0016](../decisions/0016-the-bank-is-excited-per-band.md) a resonator rings
only on energy the source has near it, so the top follows the music rather than
being invented by broadband noise. It is still one narrow filter with nothing
beside it, and the density row above is that sparsity surviving the change. The
bell risk was reduced, not removed.

**The band edges are tied to this.** `crate::bands::EDGES` are octaves because
the gain law's share is `BW/W_b`, so a band wider than an octave lifts the
resonators in it against their neighbours. **The ladder cannot reach the
ceiling**, though, because its rungs are octaves of 130 Hz and the ceiling is a
setting: at 5 kHz the next rung would be 8320. So the top band is never an
octave, and what has to be checked when the ceiling moves is **the spread of
the compensation from the last edge to the ceiling**, not the octave count.

At the default that spread is 1.90 dB across one band. Stopping the ladder a
rung later, at 4160, makes it 3.87 dB across two bands and a reversed edge —
which is how the octave count came to be the wrong thing to check. See
[ADR 0022](../decisions/0022-the-band-ladder-stops-at-two-kilohertz.md).

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
gain(f) = makeup · tilt(f) · ( (BW(f)/W_b) / (BW_ref/W_ref) )^(-p)
```

- **`BW` is continuous across the breakpoint**, because it is a maximum of the
  two branches. There is no case split and therefore no seam that can be got
  wrong.
- **`BW_ref` is a fixed reference decay, not the current one.** Against the
  current decay the ratio would be one everywhere below the breakpoint and the
  decay compensation would cancel itself out entirely.
- **The share is measured against the band, not in hertz.** Since section 2.0
  a resonator's excitation is broadband only *within* its band, so what it
  collects is `BW/W_b`. `W_b` is the band's own width, with the open top band
  closed at the grid's ceiling so the law does not depend on the sample rate.
- **The exponent is one judgement, and it now has two effects.** It answers
  the tonal-versus-broadband question, and because `W_b` grows with frequency
  it also decides how level the bank sits from band to band. **At `p = 0` both
  go**: the compensation vanishes and so does the flattening, which leaves
  about 10 dB of tilt across the bands. That is the right answer for a bank
  excited tonally and it is not a free setting, so it is stated here rather
  than found.
- **Sweeping the exponent does not move the level at the reference decay in
  the reference band**, which is what makes choosing it by ear a comparison of
  spectrum rather than of loudness. In the reference band only: a resonator in
  a wider band moves by `(W_b/W_ref)^p` as the exponent sweeps.
- **`makeup` is a constant, and `color` needs it.** A resonator collects a
  small fraction of its band — 2.2 Hz of 130 at the reference decay — so
  without it the wet arrives some 18 dB under the dry and `color` does nothing
  until its last tenth. The compensation cannot supply this, because it is
  deliberately unity at the reference. Nothing in it reads the chord, so it
  cannot duck.
- **The two normalisations compose in one place and in this order**: the
  divisor of section 5 is applied to the summed bank, and everything above is
  folded into each resonator's own gain at retune. Neither is a follower and
  neither reads the live chord.
- **`sear` is outside all of it.** The post-drive runs on the bank's output,
  after these gains, so its level change is not compensated. At the default of
  zero the shaper is an exact bypass, so this costs the default nothing — but
  a `sear` above zero moves the wet's level against the dry and `color` is not
  a crossfade there.
- **Nor is `drive`.** It moves the output by 2 to 4 dB across its range, which
  is uncompensated and is what a drive knob is expected to do. ADR 0019 chose
  the normalisation point that makes it that rather than the 10 to 14 dB it
  was.

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

What lands where, in this crate: `ResonatorBank::process` and
`Exciter::process` are the per-sample path, and `retune`, `note_on`,
`note_off` and `Grid::frequencies` are on the callback too — under Bela a
chord change and every key arrive inside `render_pre`.

**The MIDI drain is bounded per block, and that is the fifth prohibition
rather than a nicety.** A key press is not cheap — it scans the table for the
quietest voice and retunes a block — so draining a ring would make a block's
work depend on how long the ring had been left, which is time proportional to
events instead of frames. `hyperglare-bela` takes at most
`MIDI_MESSAGES_PER_BLOCK` and leaves the rest for the next block, so a burst
arrives late rather than partly. That prohibition is the one nothing in the
toolchain checks, which is why it is named here.

**What the bound does not buy is a proof that nothing was lost.** `bela`'s
ring overwrites when it wraps and its count is a difference modulo the ring's
size, so it does not saturate. `midi_backlog` is a high-water mark: a large
number says the run fell behind the device, and **a small one is not evidence
that it did not** — a run that wrapped can report one, and a lost note off
would leave a key down with nothing in the report to say why. Detecting the
drop itself would need a sequence number the parser does not carry.

**There is a host, and it has run.** `hyperglare-bela` puts this crate under
Bela's render callback on a Gem Stereo: the defaults cost about 13% of one core
at 48 kHz with no underruns, and a bank filled to its capacity of 256
resonators costs 45 to 48%. The per-sample cost is 7.9% fixed plus 0.15% per
resonator *slot the bank runs* — which a voice reserves whether or not a key is
down, so the load does not step when a chord arrives. The host reports
`active_resonators` beside the load because a figure without the count cannot
be acted on, and `held_voices` beside it because the slot count no longer says
how much of the chord is down.
The load does not depend on the input: measured with music at the input it is
within a third of a point of the same run into silence.
[`bela/cpu.md`](bela/cpu.md) has both sweeps.

**And it sounds like itself.** Recorded off the board and measured against the
offline renderer on the same settings, the chord content matches to 0.016 and
the band profiles have the same shape
([`bela/audio-verification.md`](bela/audio-verification.md)). What that
document adds by ear is the limit this section's neighbours state by
measurement: colour is unmistakable on sparse material and inaudible inside a
full mix, because the source's reach is the effect's reach.

**Seven functions carry `#[no_panic]` proofs**, checked at link time by
`cargo test --release`: the bank's `process`, `retune`, `note_on` and
`note_off`, `Grid::frequencies`, the exciter, and the processor's own frame.
The list lives in `crates/hyperglare-dsp/src/lib.rs`; this paragraph is a
summary of it and the tests are what enforce it.
