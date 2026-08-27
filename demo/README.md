# Demo Audio

One directory per effect, for the same reason `docs/` has one
([ADR 0015](../docs/decisions/0015-documentation-is-namespaced-by-project.md)):
these become separate repositories, and a shared `dry.wav` would be an asset
two of them both depended on with no home in either.

Nothing is shared across the directories, and that is not an oversight. What
demonstrates a multiband compressor is a full-band musical excerpt; what
demonstrates a resonator bank is a distorted bass, because a bank of band-pass
filters can only emphasise energy the input already has
(`docs/hyperglare/contracts.md`). One file cannot be both.

| Directory | Effect | State |
| --- | --- | --- |
| [`oxtt/`](oxtt/) | The 3-band upward/downward compressor | A source and two renders |
| [`hyperglare/`](hyperglare/) | The resonator bank | A source and two renders |

## What belongs here

A source, and the renders that show what the effect does to it. Renders are
produced by that effect's offline renderer rather than recorded, so they can be
regenerated when the DSP changes; the source cannot, which is why it is
committed.

Keep them small. Every file here is in every clone forever, and the point is to
demonstrate rather than to master: mono where the effect does not need stereo,
a few seconds rather than a chorus, and no more of them than answer a question
somebody actually asked.
