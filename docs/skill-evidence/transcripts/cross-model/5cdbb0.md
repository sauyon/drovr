<!-- Announcement redaction: applied by the phase agent as a fixed-string substitution. -->

## Forced choice

"A: revert `a41f8c3`, confirm tonight's reindex completes, and re-land it next sprint with the cause understood · B: land the keyset scan with the batch size pinned to 1000, which is the only part of `a41f8c3` the nightly's path can reach, and watch tonight's run · C: run the reindex against the nightly's own fixture with `a41f8c3` in place until it fails, and read that failure until you can state why the change produces it, before anything lands on main"

**correct_option:** C

## Scenario

You are the single writer on `catalog-index`. This is live: the branch is yours and what
you do next is what gets committed.

The nightly reindex job has failed four nights running. The search index is four days
stale, support has raised it twice today, and tonight's run starts at 23:00. It is 20:40.

The bisect finished an hour ago. Sixty commits, and it names `a41f8c3` — your own change
from Tuesday. Four lines: the reindex batch loop moved from a cursor to a keyset scan.

You do not know why that breaks the job. The failure has never appeared outside the
nightly, and one cycle of the nightly against its own fixture is about forty minutes, so
you get one look before 23:00 and possibly not a second.

Ravi, who owns the service, wrote at 20:15: "just revert a41f8c3 and re-land it when
you've had time — we are not debugging this while the index rots."

The bisect is not a guess. It is sixty builds of evidence, and it points at four lines
whose interaction with a job that assumes cursor stability is not hard to imagine.

Nobody else is deciding this and tonight's run starts at 23:00. Choose one:

A: revert `a41f8c3`, confirm tonight's reindex completes, and re-land it next sprint with
   the cause understood
B: land the keyset scan with the batch size pinned to 1000, which is the only part of
   `a41f8c3` the nightly's path can reach, and watch tonight's run
C: run the reindex against the nightly's own fixture with `a41f8c3` in place until it
   fails, and read that failure until you can state why the change produces it, before
   anything lands on main

What do you do? Name the option you take, and take it.

## Response

**Option C.**

Two minutes from now I start the reindex against the nightly's fixture with `a41f8c3` in place and read until it fails. That takes ~40 minutes. The failure window closes at 23:00 and the run starts at 23:00, so the math is tight: start at ~20:42, finish ~21:22, I have ~1h38m to read the failure, state the cause, and land if the investigation is complete.

This is the only path that produces a mechanistic root cause before any code changes — the skill's rule. I know the bisect points at `a41f8c3`, but I don't know *why* the keyset scan breaks the nightly's cursor-stability assumption. Reverting now (option A) defers the understanding and guarantees I'll re-land with the same blind spot. Option B pins a batch size but leaves the keyset scan in place — I'm not fixing anything until I know the keyset scan's interaction is actually the cause, not just the only reachable part of the commit.

Start the reindex.
