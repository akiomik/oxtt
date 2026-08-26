# The Real-Time Callback Contract

This is the one contract every effect in this family owes, and the only one
that is shared rather than restated per project
([ADR 0015](../decisions/0015-documentation-is-namespaced-by-project.md)).

It lives under `effectkit` because `effectkit` is the artifact the projects
already share and the only one that is published: a dependent repository
reaches this document through a dependency it already has, rather than through
a link to a sibling repository. Everything else in `docs/` is namespaced by
project, and duplicated where two projects overlap.

## The prohibitions

**Every host callback that runs while audio is flowing, and its transitive
calls, must not:**

- allocate or free heap memory;
- acquire or wait on a lock;
- use a blocking channel operation;
- perform file or standard-stream I/O;
- spawn, join, or sleep a thread;
- panic or unwind;
- or take more than time proportional to the callback's frame count.

An effect that broke any of these would be broken, not different. That is what
makes this the one document worth sharing: the list is not a house style, and
there is no version of it that a second effect could legitimately disagree
with.

**Which callbacks those are is each project's own business**, and each
project's `contracts.md` names them — under JACK and under Bela they are
different functions with different signatures. This document says what the rule
is; it does not say where it lands.

## What is not bound by it

Callbacks that run *outside* audio. A host's setup and its post-stop reporting
may allocate and may write to stderr, because no audio thread is waiting on
them. Under Bela that is `validate_settings`, `setup`, `create_render_state`
and `cleanup`; under JACK it is everything before `activate` and after
`deactivate`.

The boundary is "is audio flowing", not "is this function named like a
callback".

## How it is enforced

**Three mechanisms, and none of them covers the whole list.**

**Lints, workspace-wide.** `clippy.toml` carries `disallowed-methods`,
`disallowed-types` and `disallowed-macros` naming the specific std items that
allocate, lock, block, or write to a stream. They apply crate-wide because
`clippy.toml` has no per-module scoping, so code legitimately outside the
real-time path carries a local `#[allow(...)]` saying why. That is deliberate:
the exception is visible at the site rather than absent from the list.

**`#[no_panic]` proofs, per crate.** Lints cannot prove the absence of a panic —
an index, a slice, an unwrap, or an arithmetic overflow reaches it without
naming a disallowed item. So the per-sample path carries `#[no_panic]`
attributes, checked at link time by `cargo test --release`. They only hold
under full optimisation, so they are inert in a debug build.

**The proofs are per crate on purpose, and their composition is proved
nowhere**, so a failure names the crate that caused it rather than the
composition ([ADR 0013](../decisions/0013-crates-not-features-and-effectkit-as-the-shared-half.md)).
`lto = true` and one codegen unit keep the cross-crate inlining they depend on.

**Review, for the rest.** The fifth prohibition — time proportional to the
frame count — is not machine-checked anywhere. Nothing in the toolchain reads a
loop bound and compares it against a block length. It is the one that a control
path can break silently by iterating over events instead of frames, so it is
the one worth naming when a new control path is added.

## References

- [ADR 0015](../decisions/0015-documentation-is-namespaced-by-project.md) — why
  this document is shared when the rest are duplicated.
- [ADR 0013](../decisions/0013-crates-not-features-and-effectkit-as-the-shared-half.md)
  — why the no-panic proofs are per crate.
- [ADR 0011](../decisions/0011-bela-gem-stereo-as-the-second-host.md) — the
  second host, and why its control read sits inside the callback while the
  first host's sits outside it.
- [`docs/oxtt/contracts.md`](../oxtt/contracts.md) — the first effect's
  contracts, including which of its functions this applies to.
