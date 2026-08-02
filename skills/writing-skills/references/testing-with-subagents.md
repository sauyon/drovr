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

## Running the cycle

**RED — baseline, without the skill.** Give the subagent the development
scenario and no skill text. Transcribe its choice and its reasoning **verbatim**
into the evidence file. Paraphrase loses the thing you need: the exact sentence
is what the counter-text has to answer, and "the agent was wrong" tells you
nothing about what to write.

**GREEN — write the minimum.** Counter the excuses you actually saw. Text for
failures you imagined is text nobody has evidence for, and it costs context in
every session that loads the skill.

**Re-test on the held-out scenarios.** Feed the skill text explicitly in the
prompt — nothing in the repo reaches a live session until the plugin pin is
bumped, so a probe that assumes the skill is loaded is measuring the wrong
thing. Take the text from the arm snapshot under `docs/skill-evidence/arms/`,
and check its hash against the manifest before you use it.

**REFACTOR — close the holes.** Every new rationalization gets the four-part
closure from `SKILL.md`, all four parts. Then re-test.

**The meta-test, when the agent complies with nothing to show for it.** Ask the
subagent that just chose wrongly: *how should this skill have been written so
the correct option was unmistakable?* Three answers, three different repairs:

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
