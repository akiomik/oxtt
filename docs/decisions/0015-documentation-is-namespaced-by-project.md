# ADR 0015: Documentation Is Namespaced by Project; Decisions Stay Flat

## Status

Accepted

## Scope

`effectkit`, `oxtt`, `hyperglare`, workspace.

This is the first ADR to carry a `Scope` line. The decision below makes it a
convention for every ADR after it, and deliberately does not backfill the
fourteen before it — see "Why the existing decisions are not classified".

Revises the documentation layout that
[ADR 0013](0013-crates-not-features-and-effectkit-as-the-shared-half.md)'s
crate table implies. **No crate boundary changes.** ADR 0013's question — does
this depend on what oxtt *is*? — is the same question this ADR applies to
prose.

## Context

`effectkit`, `oxtt` and `hyperglare` share one workspace today and are expected
to become three repositories. That was not a premise when `docs/` took its
current shape, and it invalidates the shape.

**`docs/` is organised by topic and by board, and neither survives a split.**

```
docs/architecture.md        oxtt's architecture, under a generic name
docs/contracts.md           oxtt's contracts, under a generic name
docs/bela/                  four files about oxtt, two about the control surface
docs/raspberry-pi/          two files about oxtt, two about the control surface
```

`contracts.md` names `OttProcessor`, states "six potentiometers and a latching
bypass switch" in §8, and carries oxtt's parameter table in §1 and its
three-band assumptions in §4. Its name says otherwise. A second effect reading
it cannot tell which sections are addressed to it.

**The obvious fix is the wrong one.** The plan this ADR replaces was to split
`contracts.md` into a shared half and an effect-specific half. That produces a
document two future repositories both depend on. A shared document has no home
after the split: it belongs to none of the three, and whichever repository
keeps it becomes an upstream the other two must track for prose.

The same objection applies to any `docs/shared/`. **Splitting a document to
avoid duplication is the complecting move here**, not the duplication it
avoids.

## Decision

**Namespace the living documents by project. Keep the decisions flat.**

Those are different artifacts with different failure modes, and they get
different rules.

### 1. Living documents are namespaced by project; the board is a level below

```
docs/
  decisions/                       flat, append-only (see 4)
  development.md                   workspace (see 5)
  cross-compile.md                 workspace (see 5)
  effectkit/
    realtime.md                    the real-time callback contract
    bela/                          the Gem panel's wiring and verification
    raspberry-pi/                  the Pi panel's wiring and verification
  oxtt/
    architecture.md
    contracts.md
    bela/                          what oxtt measured on a Gem
    raspberry-pi/                  what oxtt measured on a Pi
  hyperglare/                      created when its first crate is
```

The top level is the axis that has to survive, so the top level is the split.
The board becomes a directory *under* a project, because a board is not a
namespace: it is a fact each project verifies for itself. `docs/bela/` was
already two projects' documents in one directory.

### 2. `contracts.md` is not split. Each project gets its own

`docs/oxtt/contracts.md` is today's file, moved and otherwise unchanged.
`docs/hyperglare/contracts.md` is written when `hyperglare-dsp` is.

Where they overlap — a non-finite guard, silence in giving silence out,
determinism across block partitioning — **each states it in full.**

**The duplication is deliberate, and the rule that licenses it is narrow:**

> Duplicate what may legitimately diverge. Share only what must not.

Two effects' contracts are independently owned. If hyperglare later defines its
bypass crossfade differently from oxtt's — and it has reason to, because it
rings for a second after its input stops — that is a decision, not drift. A
shared document would have made a legitimate divergence look like a defect and
would have needed an owner to arbitrate it.

This is the same trade ADR 0013 already took when it accepted the duplication
between `oxtt`'s and `oxtt-bela`'s setup code rather than guessing at a trait
with one implementation to check against.

### 3. Shared documentation is legitimate only where a shared artifact already exists

Some of the contract genuinely must not diverge: no allocation, no locks, no
I/O, no panic, and no more time than the callback's frame count. Every effect
in this family owes it, and an effect that broke it would be broken.

**That text goes to `docs/effectkit/realtime.md`, because `effectkit` is the
one artifact the three already share and the only one that is published.** Its
`lib.rs` already asserts the prohibitions in prose and proves them per crate
with `#[no_panic]`. The documentation travels with the crate to crates.io, so a
dependent repository reaches it through a dependency it already has, rather
than through a link to a sibling repository.

**Where no shared crate exists, there is no shared document.** Duplicate
instead.

### 4. Decisions stay flat, and gain a `Scope` header

`docs/decisions/` is not namespaced, is append-only, and is the record of *this
repository's* history.

**Every ADR from this one on declares `## Scope`** below `## Status`, using the
project names above plus `platform` for decisions about which board the pedal
is built on ([ADR 0008](0008-usb-audio-clock-slip-and-i2s-migration.md),
[0009](0009-hardware-platform-choice-reopened.md),
[0011](0011-bela-gem-stereo-as-the-second-host.md)). Multiple values are
expected; this ADR carries four.

**At the split, each repository copies the decisions it needs.** Copying, not
moving: an ADR is a frozen record, so a copy cannot drift from its original,
and a live cross-repository link to a frozen document buys nothing.

### 5. `development.md` and `cross-compile.md` describe the workspace, not a project

They tell you how to work *here* — the toolchain, the sysroot, the lint and
test commands, the deploy script. At the split they are rewritten per
repository, because that is when the workspace they describe stops existing.
`cross-compile.md` moves up out of `docs/bela/` to say so.

## Why the existing decisions are not classified

Two reasons, and the second is the one that matters.

**The cross-references are the artifact.** ADR 0010 and 0011 each name six
others; 0013 names five; 0009 and 0014 name four. ADR 0013 supersedes 0011's
packaging and revises 0010's layering; 0011 amends 0009 and revises 0010 in
place. Moving these into per-project directories breaks every relative link in
that web for no gain — nothing reads an ADR by walking a directory.

**Classifying them retroactively would rewrite history to be tidier than it
was.** ADR 0013 is genuinely about all three projects at once. ADR 0011 decides
a host and revises the control surface. ADR 0010's three layers span
`effectkit-controls` and `oxtt` by construction. Assigning each a single owner
now would assert a separation that did not exist when the decision was made,
and the whole value of the record is that it says what was actually thought.

The `Scope` header is for decisions taken *after* the separation is a premise.
Applying it backwards would be a claim about the past, not a label.

## Consequences

- **The same workspace is a staging area with a stated exit, not a permanent
  home.** ADR 0013 chose one workspace because `effectkit-controls` is
  unpublished and `hyperglare` reaches it by path. That reasoning stands and
  now has a horizon. A "we will split later" that is not written down becomes
  "we never split".

- **Publishing `effectkit-controls` has acquired a deadline.** ADR 0013 said
  publishing waits for a panel that differs. Two forcing functions now point
  the same way and arrive at about the same time: the seven-pot panel supplies
  the panel that differs, and the repository split removes path dependency as
  an option. The decision is no longer a judgement call about generality; it is
  a requirement with a date. It stays where it was assigned, in the ADR that
  handles the seven-pot panel.

- **Every documentation path changes.** The README's index, its inline links,
  and the cross-references inside `docs/` all move. The ADRs' links into
  `docs/` move with them; the ADRs themselves do not.

- **`docs/effectkit/`'s control-surface files still speak oxtt's vocabulary**,
  because the panel they describe is assigned by oxtt: `depth`, `time`,
  `upward`, `downward`. The wiring, the deadband and the verification are
  effect-independent and belong there; the sentence that says what each knob
  *means* does not. That sentence is left in place and pointed at
  `docs/oxtt/`, rather than rewritten now, because a second assignment does not
  exist yet to say what the general form is — the same reason ADR 0013 gave for
  not generalising the crate.

- **The shared contract moved; the rest of `effectkit`'s citations did not.**
  `effectkit` and `effectkit-controls` cited `docs/contracts.md` §6 in
  twenty-one places; those now cite `docs/effectkit/realtime.md`, and so does
  `clippy.toml`, which is workspace-wide. What is left behind is
  `effectkit`'s handful of citations of oxtt's §1, §2 and §4 — a crossover
  limit, a smoothing precondition, a decibel floor. Those are an
  effect-independent crate reasoning about one effect's validated ranges, and
  they are a real wart. This ADR does not fix them: the shared *contract* has a
  home now, and shared *values* do not, which is the same line ADR 0013 drew
  when it kept `IoGain` and `NormalizedF32` in `oxtt-dsp`.

- **`effectkit-controls-pi`'s channel constants are the same wart in code**
  (`CHANNEL_DEPTH`, `CHANNEL_TIME`, `CHANNEL_UPWARD`, `CHANNEL_DOWNWARD`,
  `CHANNEL_INPUT_GAIN`, `CHANNEL_OUTPUT_GAIN`, all private). This ADR does not
  fix it. It is named here so that the documentation move does not read as
  having settled it.

- **A fourth project would add a fourth directory and nothing else.** That is
  the property being bought.

## References

- [ADR 0013](0013-crates-not-features-and-effectkit-as-the-shared-half.md) —
  the crate split whose question this applies to prose, and the source of the
  duplication trade taken in decision 2.
- [ADR 0010](0010-three-layer-control-surface-and-newest-value-handoff.md) —
  the three-layer split that makes the control-surface documents
  effect-independent at layers A and B1, and oxtt's at B2.
- [ADR 0014](0014-the-pot-position-scale-is-ten-bit.md) — the contract
  `effectkit-controls` fixes for every surface, and the reason its documents
  belong to `effectkit` rather than to a board.
