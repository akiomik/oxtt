# ADR 0016: The Bank Is Excited per Band, Not Broadband

## Status

Proposed

## Scope

`hyperglare`.

Revises the excitation described in `crates/hyperglare-dsp/src/exciter.rs` and
the level contract in [`docs/hyperglare/contracts.md`](../hyperglare/contracts.md)
§5. **No change to the grid, the geometries, or the gain law's treatment of
voice count, density and decay.**

Also withdraws one sentence of
[ADR 0015](0015-documentation-is-namespaced-by-project.md)'s `demo/`
amendment: that a resonator bank needs a distorted bass where a compressor
needs a musical excerpt, so no one file could serve both. **ADR 0015's decision
stands** — `demo/` stays namespaced, and nothing is shared across it — but the
example it argued from is measured false below, and the drum loop committed for
the compressor turns out to be the better source of the two for this effect.

## Context

The exciter produces one signal and every resonator receives it:

```text
excite(x) = shape(x, drive) + noise_amount · gate(x) · noise()
```

`gate` follows the input's broadband amplitude. So whenever the input makes any
sound at all, every resonator in the bank is fed the same white noise, and the
bank's audible shape is decided by its own gain law rather than by the
material.

**Measured against five paired before/after recordings** — the same take, dry
and then processed by a commercial colour-bass effect — this is the single
largest difference between the two. Per-octave-band pitch-class concentration,
which reads high when a band's energy sits on few pitch classes:

```text
                  125-250  250-500   500-1k    1k-2k    2k-4k    4k-8k
 laser reference    0.005    0.055    0.305    0.326    0.295    0.015
 laser hyperglare   0.249    0.165    0.135    0.141    0.145    0.158
 swan  reference    0.018    0.119    0.161    0.089    0.089    0.020
 swan  hyperglare   0.171    0.169    0.143    0.137    0.162    0.145
```

**The hyperglare rows are flat, and they are flat in the same way for
materials that have nothing in common.** The reference's rows have a shape,
and where the source carries nothing the reference adds nothing — 0.005 at the
bottom of `laser` and 0.015 at the top.

A listener described this without seeing the numbers: the chord is "sitting on
top", "a different instrument playing two or three notes", present in the
middle whatever the source is doing. That is what a fixed layer sounds like.

The same five recordings show the reference keeping the source's octave-band
envelope nearly unchanged apart from the bottom two bands. It is not adding a
layer; it is retuning what is already there.

### The prototype

Six instances of the effect, each fed one octave of the input and tuned to
grid points inside that octave only, summed:

```text
                  125-250  250-500   500-1k    1k-2k    2k-4k    4k-8k
 duck reference     0.092    0.253    0.119    0.092    0.061    0.162
 duck broadband     0.238    0.177    0.135    0.123    0.122    0.171
 duck per band      0.060    0.139    0.161    0.181    0.151    0.135
```

The bottom band's over-colouring — 0.238 where the reference has 0.092 —
resolves to 0.060. Four sources behaved the same way, and the listener
confirmed the improvement by ear on all four.

## Decision

**A resonator's excitation comes from the input's energy in its own
neighbourhood, not from the input as a whole.**

Concretely, the exciter gains a filter bank, and both of its paths become
per band:

- the gate that multiplies the noise follows the band's amplitude, not the
  broadband amplitude, so a band the source is silent in stays silent;
- the shaped path is taken from the band rather than from the full input.

The band count and their edges are an implementation choice this ADR does not
fix, because the prototype's six octaves were chosen for convenience rather
than measured against alternatives. What this ADR fixes is that the number is
greater than one and that the mapping from resonator to band is by frequency.

**The silence guarantee is unchanged and its proof gets easier.** Today
"silence in, silence out" rests on one multiplication by one gate. It will
rest on N of them, each with the same property, and a band that never opens
contributes exactly zero rather than approximately zero.

## Consequences

- **This is the only change measured to move the shape rather than the level.**
  Every earlier attempt — the grid's ceiling, the geometry, the chord's
  register — moved numbers without moving the flatness.
- **Cost is a filter bank in the audio path**, sized by the band count. The
  prototype's six bands are six pairs of second-order sections on the mono sum,
  which is small next to the bank itself; a per-resonator follower would not be.
  The band count is therefore a CPU decision as well as a sound one, and
  `hyperglare` has not yet been run on a Bela at all.
- **The excitation stops being one signal**, so `Exciter::process` cannot keep
  its shape. The gate's state becomes per band, and `Exciter::gate()` — today a
  single number used by tests — has to become per band or be replaced by an
  "any band open" predicate.
- **`ExciterParams` keeps its two knobs.** `drive` and `noise_amount` stay
  global; nothing measured suggests they should differ per band, and a
  per-band knob is not a knob a player can reach.
- **What the effect needs from a source changes from a matter of degree to a
  requirement.** Broadband excitation put a chord into every band whether or
  not the source had anything there — which is where the isolated high partials
  a listener called bells came from. Per band, a band the source is empty in
  produces nothing, so the source's own reach becomes the effect's reach. On
  the two committed demo sources, with everything else equal:

  ```text
  source's energy per band, loudest band at 0 dB
                   125-250  250-500   500-1k    1k-2k    2k-4k    4k-8k
   drum loop           0.0     -9.7    -12.4    -12.1     -8.1     -5.0
   FM bass             0.0     -4.6    -16.9    -33.1    -54.0    -70.0

  resulting chord content in the same bands
   drum loop         0.151    0.221    0.262    0.533    0.271    0.275
   FM bass           0.675    0.638    0.540    0.331    0.013    0.012
  ```

  The bass is 54 to 70 dB down above 2 kHz and gets 0.013 there; the drum loop
  is 5 to 8 dB down and gets 0.275. **A source's suitability is now measurable
  before anybody listens**, and the measure is how far its own energy reaches
  rather than how it was made.

- **The design's justification for two excitation paths is not supported and is
  not settled here.** Rendering with `--drive 0` and with `--noise 0` produces
  waveforms that correlate at 0.94–0.96 and differ by about 10 dB, on both a
  tonal and a percussive source. The design argued the shaped path feeds the
  root-coincident grid points and the noise path feeds the rest; measurement
  says the bank cannot tell them apart. Whether per-band excitation changes
  that is worth measuring after this lands, and is deliberately not decided
  now.
