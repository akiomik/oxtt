# ADR 0017: The Wet Path Carries No Time Constant Longer Than the Source's

## Status

Proposed

## Scope

`hyperglare`.

Changes the default of `BankParams::decay_t60_s` and removes
`HyperglareParams::wet_match` in its present form
(`crates/hyperglare-dsp/src/wet_match.rs`). Revises
[`docs/hyperglare/contracts.md`](../hyperglare/contracts.md) §5.

Depends on [ADR 0016](0016-the-bank-is-excited-per-band.md) for the per-band
replacement this ADR points at, and is otherwise independent of it.

## Context

Two mechanisms in the wet path outlive the sound that started them, and a
listener heard both as the same thing: "distant", "reverberant", "the chord
does not stop when the source does".

Measured as the drop, in decibels, over the 150 ms after an onset — larger is
tighter:

```text
              source   reference   hyperglare
 duck            8.8         6.3          5.7
 spoon          11.3        11.6          6.7
 swan            6.0         7.7          3.2
```

**The reference preserves or tightens the source's transient; hyperglare always
loosens it.** `spoon` falls 11.3 dB in 150 ms and arrives at 6.7.

### The decay knob has a knee, and the default sits past it

Sweeping `decay_t60_s` on one source, with pitch-class concentration in
500–2000 Hz as the measure of how much colour is bought:

```text
 decay   transient   colour
  0.12       8.1dB    0.202
  0.25       6.6dB    0.247
  0.40       5.5dB    0.252
  0.60       4.6dB    0.247   <- today's default
```

**Colour saturates at 0.25 s. Past it the decay buys reverberation and nothing
else.** The reference's own transient figure for this source is 6.3 dB, which
0.25 s reproduces almost exactly. The default of 0.6 s was chosen as a
plausible resonator decay, never measured against material.

### `wet_match` does two harmful things to buy one useful one

`wet_match` was added for a real defect: the wet arrived roughly 20 dB below
the dry, so `color` did almost nothing until its last tenth. It measures both
signals and pushes the wet onto the dry's level, with followers of 300 ms and
500 ms.

Holding everything else fixed and toggling only `wet_match`:

```text
                        brightness (2-8k / 250-1k)
 swan  decay 0.12  on          -3.9 dB
 swan  decay 0.12  off         -8.4 dB
 swan  reference               -7.0 dB
```

**It is a brightness control that nobody asked for.** Matching the wet to the
dry as one number erases the level differences *between bands*, which after
ADR 0016 is precisely the information the effect is supposed to be following.
The 4.5 dB swing is entirely `wet_match`; the decay moves brightness by 0.2 dB.

And its followers stretch transients on their own. At decay 0.25 s, `swan`
measures 2.7 dB of transient with `wet_match` on and 5.4 dB with it off: a
gain that rises as the dry falls is a reverberator with extra steps.

## Decision

**Nothing in the wet path may have a time constant longer than the excitation
gate's release.**

Two consequences follow, and both are decided here:

1. **`decay_t60_s` defaults to 0.25 s**, the measured knee, rather than 0.6 s.
   The knob keeps its full range: 0.6 s remains reachable and a listener judged
   it "flashy, and it matches the original concept", so this is a default
   change, not a range change.

2. **`wet_match` is removed as a global follower.** The problem it solved is
   real and does not go away, so it is replaced rather than deleted: the wet's
   level is compensated **per band**, from the same band split ADR 0016
   introduces, and the compensation is a static function of the bank's
   parameters wherever the gain law can supply one.

### The alternative, and why it is not one

Compensating the *excitation* per band instead of the wet looks equivalent and
is not. It was proposed while implementing this, on the reasoning that the wet
is summed and so cannot be attributed to a band.

**That reasoning is wrong.** A band is a property of the resonator, not of the
signal — [ADR 0016](0016-the-bank-is-excited-per-band.md) assigns it by
frequency — so the sum can be grouped by band before it is taken:

```text
wet = Σᵢ gainᵢ · bpᵢ(exc_b(i))  =  Σ_b [ Σ_{i∈b} gainᵢ · bpᵢ(exc_b) ]
```

The two differ in the operation, and that is what decides it. **Compensating
the wet multiplies; normalising the excitation divides.** ADR 0016 guarantees
a band the source is silent in produces exactly zero excitation, and a finite
compensation leaves that alone: `c_b · 0 = 0`. Dividing by a small band's
level lifts an empty band back up, which is the property ADR 0016 exists to
create, removed.

So the compensation belongs on the wet, and the excitation is left as the
source made it.

**And it costs nothing per sample.** Because `c_b` is a static function of the
bank's parameters rather than a measurement, the band grouping above collapses:

```text
wet = Σᵢ (c_b(i) · gainᵢ) · bpᵢ
```

`c_b` folds into `gains[i]` at retune. No per-band wet array, no new state, no
new time constant — which is this ADR's title, met in the strongest form
available rather than by choosing a fast follower.

**What is deliberately not decided:** whether any residual per-band follower is
needed once the excitation is per band, and if so how fast. The prototype
suggests a partial match still helps brightness on some material, but a
prototype that ran `wet_match` at 0.5 across six independent instances is not
evidence about one implementation with six bands. That measurement comes after
ADR 0016 lands.

## Consequences

- **Every render changes.** Existing `demo/hyperglare/*.wav` and any figure
  quoted from them are invalidated by the decay default alone.
- **`color` may stop being a usable crossfade in the interim.** Removing the
  global match without the per-band replacement in place reopens the original
  defect — the wet sitting 20 dB down — so these two must land together, not in
  separate commits.
- **`crates/hyperglare-dsp/src/wet_match.rs` does not survive in its current
  form.** Its followers, its `DRY_FLOOR` and its ±24 dB bounds were tuned for a
  single broadband gain and none of them transfer unexamined to a per-band one.
- **The renderer's `--wet-match` flag changes meaning**, so any recorded
  command line that uses it stops reproducing its file.
- **`color` is a crossfade against the bank, and not yet against the exciter.**
  The static compensation puts the bank at the level of what excited it, which
  is what closes the defect `wet_match` was built for. It does not follow
  `drive`, and `drive` moves the wet by about 21 dB across its range —
  measured, and evenly across the bands. **So this ADR closes half of what it
  names.** The other half is a normalisation question about the waveshaper
  rather than a level question about the bank, and belongs to ADR 0019.
- **This narrows the effect's identity.** A resonator bank with a long decay is
  a reverb with a chord in it, and that is a thing somebody might want. This
  ADR says `hyperglare` is not it by default, on the evidence that the material
  the effect is for has transients worth keeping.
