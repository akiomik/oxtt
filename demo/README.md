# Demo Audio

One directory per effect, for the same reason `docs/` has one
([ADR 0015](../docs/decisions/0015-documentation-is-namespaced-by-project.md)):
these become separate repositories, and a shared `dry.wav` would be an asset
two of them both depended on with no home in either.

Nothing is shared across the directories, and that is not an oversight: a file
two of these depended on would have no home once they are separate
repositories. Each keeps its own copy of whatever it needs, even where the two
copies would be identical.

**What each effect needs from a source is not what it was made on.** This
directory once said a resonator bank needs a distorted bass and a compressor
needs a musical excerpt, so no one file could serve both. Measurement says
otherwise — what a resonator bank needs is a source whose own energy reaches
across the spectrum, which the drum loop here has and the bass does not
([ADR 0016](../docs/decisions/0016-the-bank-is-excited-per-band.md)).

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
