# Testing with subagents

Load this when you are about to run scenarios — the baseline, a re-test, or a
scorer.

## The foreground rule

**Every probe subagent and every scorer subagent runs in the FOREGROUND.**

- Dispatch with the `Agent` tool, `subagent_type: general-purpose`,
  `model: sonnet`.
- Never set `run_in_background`. Never schedule a wake-up and yield while one is
  outstanding.
- The two samples of one scenario **may** be issued as two parallel tool-use
  blocks in a single message. That is still foreground and still blocking, and
  it is the only concurrency permitted here.

This is not a style preference. A backgrounded subagent leaves you parked
mid-turn; drovr cannot tell that state apart from a finished phase, so the run
stalls until a human notices. You get the same measurement eventually, with a
person pulled into the loop who did not need to be.

Some harnesses dispatch asynchronously even when you did not ask them to. If
that happens, it is not permission to yield: keep working and fold the results
in when they land.

## The single-writer rule

**You are the only writer.** Subagents run scenarios and score transcripts;
they never edit a skill, a scenario, or an evidence file. Two writers on the
same text produce a measurement nobody can attribute, and skill text is the
thing under test.

Read-only explorers are fine for anything you only need to understand.

## The transcript protocol

**The probe writes its own transcript file. You never relay it.**

Give each probe its output path in the prompt. It writes the file itself and
returns one line:

```json
{"transcript_id":"<id>","wrote":"<path>","ok":true}
```

The scorer works the same way: it writes its verdicts and returns a
confirmation.

The reason is the reason drovr exists. A phase that pulled twenty full agent
responses back through its own context as tool output would fill that context
and start degrading exactly where the evidence matters most. When you need a
verbatim quote, read the transcript file — do not ask for the text back.

If your own context fills anyway, stop rather than push through: author a
handoff (the seven-section shape, git pointers included), record which runs are
already in the ledger, and continue in a fresh phase. Records on disk are
resumable; a half-finished stage held only in context is not.

## Which skills need testing

Test the ones that cost something to obey: discipline rules, rules that lose you
time, rules that can be argued away for one special case, rules that contradict
what you want to do right now. Those are the ones an agent has a motive to talk
itself out of, and a motive is what a pressure scenario needs to work on.

A reference sheet — an API listing, a syntax table, a glossary — has no rule to
violate and no incentive to bypass. Running scenarios against it measures
nothing, and the runs come out of the same budget as the skills that need them.

## Which text goes in the prompt

**Get this wrong and the run measures a document nobody is editing.** The probe
never has the skill loaded — nothing in this repo reaches a live session until
the plugin pin is bumped — so the text is whatever *you paste*, and there are
two different sources depending on why you are running.

| You are… | Paste | Why |
|---|---|---|
| running RED / the baseline | **nothing** — no skill text at all | the whole point is what the agent does unaided |
| iterating: GREEN, or a REFACTOR re-test | **the working file**, `skills/<skill>/SKILL.md`, as it is on disk right now | you are testing the edit you just made. Anything else grades text you are not changing |
| measuring a frozen arm (A, A′, B, B-r*i*) | `docs/skill-evidence/arms/<arm>/<skill>.md`, **hash-checked against `arms/MANIFEST.md` first** | an arm is a fixed artifact. A′ no longer exists on disk once later fixes land, which is the only reason the snapshots exist |

The last two are not interchangeable. **A snapshot is a photograph of a decision
already made**; the working file is what you are still arguing with. Serving a
snapshot to a REFACTOR re-test means the iteration reports on the pre-rewrite
text — it can come back green because the old text passed, or red because the
old text failed, and neither result is about the change you made.

An arm gets snapshotted **when it is frozen**, at the end of the loop, not
during it. Record the arm's hash in the manifest at that moment; from then on,
that file is the arm and the working file has moved on.

**Only arm A's snapshots are machine-checked.**
`cli/tests/skills_valid.rs::arm_a_snapshots_match_manifest` re-hashes them on
every `cargo test`. Every other arm — A′, B, B-r*i*, voice — is checked by *you*,
by running `git hash-object --no-filters` against the row before you paste the
text. Nothing does it for you, and a drifted snapshot produces a perfectly
plausible result for a document you did not mean to measure.

## Running the cycle

**RED — baseline, without the skill.** Give the subagent the development
scenario and no skill text. Transcribe its choice and its reasoning **verbatim**
into the evidence file. Paraphrase loses the thing you need: the exact sentence
is what the counter-text has to answer, and "the agent was wrong" tells you
nothing about what to write.

**GREEN — write the minimum.** Counter the excuses you actually saw. Text for
failures you imagined is text nobody has evidence for, and it costs context in
every session that loads the skill.

**Re-test on the held-out scenarios.** Paste **the working file** — see the
table above — and run the meta-test on every probe, below.

**REFACTOR — close the holes.** Every new rationalization gets the four-part
closure from `SKILL.md`, all four parts. Then re-test.

**Leaving the loop takes both halves**: no rationalization you have not
countered, **and** all four pass criteria on every held-out run — or the §7.3
ceiling, whichever comes first. See `SKILL.md` → "The loop".

## The meta-test — run it on EVERY held-out probe

Pass criterion 4 is scored per run, and `meta_test_clear` is **false when the
answer is not in the transcript**. So this is not a repair tool you reach for
after a failure; it is part of every held-out probe, including the ones that
complied. A run where the agent chose correctly and was never asked cannot score
better than a run that failed the criterion outright.

After the probe has answered the scenario, ask it — in the same session, as a
follow-up turn:

> How should this skill have been written so that the correct option was
> unmistakable?

Append the question and its answer to the transcript as a `## Meta-test` block
(see `scoring-rubric.md`). Redact announcements there exactly as you do in
`## Response`.

**When the agent complied**, the answer you are looking for is that the skill
was already clear. Anything else is a wording or organisation defect the run
just handed you for free — act on it even though the run passed.

**When the agent chose wrongly**, the same question sorts the failure into one
of three repairs:

| The answer | What it means | What to do |
|---|---|---|
| "It was clear; I chose otherwise" | Not a wording problem | The foundational principle is missing or too weak — letter-versus-spirit |
| "It should have said X" | A wording problem | Add X, in their words, then re-test |
| "I did not see section Y" | An organisation problem | Move it earlier and make it harder to skim past |

## No fabricated measurements

`spec.md` §2.1 exception 1, and it binds everything you write here.

Do not state a number, a rate, a duration, or a comparative as measured unless
drovr measured it and `docs/skill-evidence/` holds the record, or a citation
supports it. This includes the tempting ones: how many iterations a skill took
to harden, how much faster a practice is, how often agents skip a step.

Rhetorical emphasis is fine — "every time" is a register, not a statistic. An
invented frequency is not, and neither is a real number from someone else's
study reported as if it were drovr's.

Null and negative results are recorded next to the positive ones. A skill that
failed its bar is evidence; a stage that hit its ceiling is evidence. Both get
written down.
