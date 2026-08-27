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

**How many, not how long.** Every file here is in every clone forever, so the
budget is real — 34 MB across the two directories today, six files of about
15 seconds each. What keeps that finite is the count: **one source and at most
two renders per effect**, and a render earns its place by answering a question
somebody asked rather than by showing another setting.

**Length is the material's to decide, not this file's.** This paragraph used to
say "a few seconds rather than a chorus", and that rule cost two bad edits
before it was noticed. `hyperglare`'s source is a drum pattern with no gap in
it, so every cut starts inside the decay of a hit the listener never heard
begin. There is exactly one place a loop like that starts and one place it
ends, and they are the ones the loop already has. A rule that asks for six
seconds of a fifteen-second loop is asking for an edit nobody would keep.

So: trim when the material has a place to be trimmed at, and when it does not,
commit the whole thing and spend the budget on having fewer files.
