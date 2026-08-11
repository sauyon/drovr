# Item 14's transmission test — the question set, and one deviation in how it was made

`questions/<fixture>.json` holds the 60 questions `PROTOCOL-2.md` item 14 puts to every arm — 20
per fixture, on the rows item 14's `seq | awk` command samples. They were written **before any
`generated-2/` spec existed**, which is what makes them blind: the writer had never seen an arm, a
spec, `blind-map-2.json`, or a retention verdict, and could not have seen one.

`cli/tests/skills_valid.rs::spec_length_2_transmission_questions_are_the_sampled_rows` recomputes the
sample from item 14's own command and requires these files to be exactly it.
`spec_length_2_protocol_precedes_every_generation` requires this directory's last commit to precede
the first `transmission-2/<id>.json`.

## The deviation: two of the 60 were re-dispatched

**Item 14 says an independent question-writer subagent composes all 60. Two of them come from a
second such subagent, and that is a departure from the item's letter.** It is recorded here rather
than in a commit message alone, because a deviation only a `git log` reader can find is one nobody
audits.

Item 14 requires each question to carry its row's **subject** with the row's **answer** removed, and
forbids it containing "the exact name, bound or exclusion that the row supplies". Two returned
questions stated a count their own row supplies:

| row | as first returned | as re-dispatched |
|---|---|---|
| `skill-stickiness-82` | What does this spec say determines whether a device ships across **all five documents** versus the baseline being kept instead? | What does this spec say happens to a device depending on whether its variant or V0 wins the comparison by the margin? |
| `tiered-review-84` | What does this spec say about how the plan phase treats the **four** approved decisions, and how it handles anything they do not cover? | What does this spec say about how the plan phase relates to the previously approved decisions, and about how it treats anything those decisions leave unaddressed? |

A probe that had never read its spec could return "all five documents" or "four" by copying the
question. Both rows would then read as `recovered` on the strength of the question rather than the
spec.

**What was done.** Each row was re-dispatched alone to a fresh question-writer under item 14's
prompt **verbatim**, handed only that ledger row. What it returned was spliced in unaltered. No
question was authored by hand, the other 58 are untouched, and the selection of *which two* was made
against the ledger rows alone — no `generated-2/` spec exists, so there was nothing arm-shaped to
select toward.

**Why re-dispatch rather than item 14's `R6a` doubt-log.** `R6a` is for a doubt about a *judgement* —
a call that could have gone either way and is recorded rather than re-taken. This was a return that
violated its own prompt's explicit instruction, which is a malformed return, and re-running one is
what `R6` is for. It was done now because nothing has been measured against these questions yet:
the same correction after T8 had probed would be indistinguishable from tuning the instrument to a
result. That is item 12a's reasoning for calibrating before the arms exist, applied here.

**Why `PROTOCOL-2.md` was NOT amended to bless it.** `T3` sits in window 2, where a governed item may
still be clarified — so amending was available and was deliberately declined. Editing the
pre-registration to sanction a step already taken is the exact move this attempt exists to rule out,
and the run's standing constraint is that a protocol that does not fit is a finding to report, not a
file to adjust. The deviation stands as a deviation.

**What it does not do: it cannot bias the A/B.** The same 60 questions go to every arm. A leaking
question inflates `recovered` for that row on *every* arm equally, and a corrected one deflates it on
every arm equally. The correction bought the transmission test back its discriminating power on two
rows of 60; it moved no arm relative to another, in either direction.

`RESULTS-2.md` must carry this deviation in its own deviation list. This file is the record it is
written from, not a substitute for it.
