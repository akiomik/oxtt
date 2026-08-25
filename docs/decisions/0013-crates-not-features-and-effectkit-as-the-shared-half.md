# ADR 0013: Crates, Not Features, and `effectkit` as the Shared Half

## Status

Accepted

Supersedes the packaging decision in
[ADR 0011](0011-bela-gem-stereo-as-the-second-host.md): the two hosts are no
longer Cargo features of one package. Everything ADR 0011 decided about *the
host itself* — that a Bela is the second one, that its callback drives the
control surface directly — stands.

Also revises the layering in
[ADR 0010](0010-three-layer-control-surface-and-newest-value-handoff.md):
layer B is split into conditioning and assignment, in two crates.

## Context

One package held the DSP, the parameters, the control surface, two hosts and
an offline renderer, with three Cargo features (`jack-host`, `bela-host`,
`pi-controls`) selecting between them. The arrangement worked, and its central
claim — that everything below the host adapters builds with no host at all —
was true and tested.

It was tested by a CI job. Seven jobs covered the feature combinations, and
each of them checked something the compiler could have been made to check
instead. `jack-host` and `bela-host` were not configuration: they were a crate
boundary written in the only notation the package had.

The trigger for changing that is a second effect. A `colorbass`-style effect
for the same Bela and the same breadboard wants the smoothing, the biquads,
the decibel conversions, the input meter, the jitter filter, the deadband and
the switch debounce — and none of the compressor. Under one package, sharing
those means either depending on `oxtt` (which drags in JACK, `clap` and a
3-band compressor) or copying them.

Copying is the outcome to avoid specifically. The control surface's constants
are justified by measurements recorded in `docs/`, and a copy detaches the
second effect's numbers from the session that produced them.

## Decision

**Split into eleven packages, on one question: does this depend on what oxtt
*is*?**

| Effect-independent | oxtt | Tools |
| --- | --- | --- |
| `effectkit` | `oxtt-dsp` | `oxtt-jack-tools` |
| `effectkit-controls` | `oxtt-controls` | |
| `effectkit-controls-pi` | `oxtt-args` | |
| `effectkit-pi-tools` | `oxtt`, `oxtt-bela`, `oxtt-render` | |

**Each binary is a package.** `jack-host` and `bela-host` are gone. The two
hosts link different audio systems that exist on different machines, which is
a dependency-graph fact, so it is now stated in the dependency graph.

**`pi-controls` survives, and is the only feature left.** It is a real
platform gate — `rppal` does not compile on macOS, where the DSP is developed
— and it switches a whole crate on rather than a module.

**Layer B splits at the seam that generalises.** Conditioning (jitter filter,
deadband, debounce, normalisation onto `PotTravel`) is
`effectkit-controls::SixPotBypassConditioner`; assignment — what each pot
*does* — is `oxtt_controls::assign`. The seam between them is a fraction of
travel, not ADC counts, so an effect can say what its knobs do without knowing
a converter's full scale.

**`oxtt-controls` is separate from `oxtt-dsp`** so that `oxtt-render`, which
has no control surface, does not acquire a six-pot API transitively.

**`effectkit` is the name, and the only crate published.** The rest carry
`publish = false`.

### Why `effectkit` and not `oxtt-core`

The shared half is not oxtt's core; it is the part that is *not* oxtt. A name
prefixed with the first effect would make the second effect's dependency read
as a dependency on the first.

### What `effectkit` does not contain, deliberately

**No host abstraction.** A `StereoProcessor` trait is the obvious next piece
and is deferred: two hosts that differ in more than they share are not two
examples of the same thing, and a trait written against one implementation is
usually wrong. The duplication between `oxtt`'s and `oxtt-bela`'s setup code
is accepted until a second effect makes the shape visible.

> If a second effect ships without that trait being written, the duplication
> becomes the permanent answer and `effectkit` is documented as a kit of
> primitives rather than a framework.

**No transport layer.** `ControlHandle` stays in `oxtt`. A shared one would
take the assignment as a type parameter or a closure, and there is no second
implementation to check that guess against — a Bela-only second effect will
not provide one, because a Bela reads its controls inside the audio callback.

**No shared value objects.** `IoGain`, `NormalizedF32` and the rest stay in
`oxtt-dsp`. Matching ranges are not evidence of a shared concept; the same
name meaning the same thing in two effects is, and that has not happened yet.
`PotTravel` is the exception, because it *is* the seam.

### Why the control crates are not published

`effectkit-controls` has a general name and a specific contract: a 10-bit pot
scale, six channels in ADC order, one latching switch, and — in module `gem` —
the ratio between a particular converter's reference and the rail its pots are
wired across. A second effect on the *same breadboard* is not a second
independent user of a general control API. Publishing waits for a panel that
differs.

## Consequences

- **The `no-host` CI job is gone**, along with `bela-host`'s. What they
  asserted is now a linkage fact: `oxtt-dsp` cannot name a host, because it
  does not depend on one. Seven jobs become five.
- **The no-panic proofs are per crate.** `effectkit`, `effectkit-controls`,
  `oxtt-dsp` and `oxtt-controls` each prove what is closed inside them, and
  their composition is deliberately proved nowhere, so a failure names the
  crate that caused it. This survived the split: `lto = true` and one codegen
  unit keep the cross-crate inlining the proofs depend on.
- **`Pots<T>`'s fields are named for the wiring** (`adc0`..`adc5`), not for
  OTT's macros, because the type is now shared. The cost is that a swapped
  pair in `assign` is silent — it still makes sound — so `assign` is
  documented as the one place that can be wrong, and each effect owes a
  channel-order test.
- **A version bump now touches eleven manifests.** `[workspace.package]` and
  `[workspace.lints]` hold the shared fields, so it is one edit; the price is
  that lint inheritance is all-or-nothing, and the two tool crates spell out
  their weaker set in full.
- **`--workspace` is a Linux and CI command.** `effectkit-controls-pi` and
  `effectkit-pi-tools` depend on Linux-only `rppal`, so `default-members`
  leaves them out of the no-selector commands.

## References

- [ADR 0010](0010-three-layer-control-surface-and-newest-value-handoff.md) —
  the three-layer split this revises.
- [ADR 0011](0011-bela-gem-stereo-as-the-second-host.md) — the second host,
  and the feature-based packaging this supersedes.
- [ADR 0012](0012-the-jitter-deadband-belongs-to-the-control-source.md) — the
  conditioning constants that now travel together as one value.
- [ADR 0014](0014-the-pot-position-scale-is-ten-bit.md) — the one contract
  the shared control crate fixes for every surface after it.
- [`docs/architecture.md`](../architecture.md) — the resulting crate map.
