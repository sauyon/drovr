---
name: verification-before-completion
description: Use when about to claim any work is done, fixed, or passing, before reporting, committing, or handing off — requires running the verification command in this message and reading its output; evidence before assertion, always
---

# Verification Before Completion

## Overview

**Evidence produced now, or no claim.** Follow the spirit, not the letter:
running a command and pasting its output satisfies the letter, and proves
nothing if the output is about a tree you have since edited. The rule is that
the evidence describes what you are about to claim. Nobody downstream
re-checks a report: a false "done" binds the next phase to the interface you
claimed, not the one you have.

**Write for the agent who inherits this: the next phase agent is you, with your
context gone.** They cannot see your terminal — only the commands you wrote down
and the output you pasted. Your confidence does not survive the boundary.

## The Iron Law

```
NO COMPLETION CLAIM WITHOUT FRESH EVIDENCE PRODUCED IN THIS MESSAGE.
```

**Fresh** is defined here, once, and both halves bind: the command ran **after
your last edit to the tree you are reporting on**, and its output is in the
message you are writing now. Evidence from an earlier message, from an earlier
tree, or from someone else's report is not fresh, whatever it says.

No exceptions:

- **Do not report on a tree you did not run the checks against.** An earlier
  green run is evidence about the tree it ran on. If you have edited since — a
  rename, a comment — re-run the task's named verification command against the
  tree as it stands and write the report from *that* output.
- **Do not accept a diff review as a test run.** A tech lead, a reviewer, or
  your own read tells you it *looks* safe. Run the command the task names
  anyway and cite its output; approval of a diff is not a result.
- **Do not let the clock choose what you ran.** Run every check the
  *Requirements* row for your claim names, then report late if you are late. The
  deadline changes when the report lands, not which checks can catch what your
  edit disturbed.
- **Do not claim first and verify after.** There is no version of "verify after
  claiming" that is not skipping verification with extra steps. Produce the
  evidence, then write the claim, in that order and in one message.
- **Do not pass a subagent's word through as your own evidence.** A subagent
  reporting success is making a claim, not handing you one. Open the file it
  names and read it yourself — or re-run the check when it names none — then
  report what *you* saw.

## Announce

Say this out loud when you start, before you write any part of the claim:

```
Using drovr:verification-before-completion — running the checks before claiming done.
```

## The procedure

> When a skill or briefing gives you a numbered checklist, create **one tracked item per step**
> using whatever task tool this harness exposes — `TodoWrite`, or `TaskCreate`/`TaskUpdate` —
> before you start step 1. Mark each in-progress when you start it and complete when its
> evidence is in hand. If the harness exposes no task tool, write the checklist to
> `~/.local/share/drovr/runs/<run>/checklist.md` when inside a run, or `CHECKLIST.md` at the
> repo root otherwise, and tick items there. An untracked checklist decays with the context
> window; that decay is the exact failure drovr exists to fight.

Re-create those items for each claim you are about to make, not once for the
session. If you fall back to `CHECKLIST.md` at a repo root, do not commit it.

1. **Name the claim and the tree.** Write the sentence you are about to say —
   *"the task's tests pass"*, *"the bug is fixed"* — and list every edit you have
   made since your last run. If there is one, every earlier run is about a
   different tree.
2. **Look up that claim in *Requirements* below.** The row names the evidence
   the claim takes and what does not count as it. A claim with no row still
   needs all three: a named command, its fresh output, and a sentence naming
   what that output does not cover. Do not shop for the cheapest row.
3. **Run those checks now, in this message**, against the tree as it stands,
   uncommitted edits included — that is what makes the output fresh. Paste the
   command and its output. A run you describe is not a run you did.
4. **Read the output, not the exit code alone.** The tests you claim exist must
   appear in it by name, and nothing may have been filtered or skipped out from
   under you — a suite that skipped your file is not evidence about your file. A
   green line for a test that asserts nothing is not verification.
5. **If a check failed, was skipped, or cannot be run here, you have a finding,
   not a completion.** Name the exact command and what it did. *"Cannot be run
   here"* is itself a claim: cite the command you ran and the error it gave, not
   your expectation that it would fail. Do not soften either into *"mostly
   passing"* or *"an unrelated failure"*, and do not report done.
6. **Write the claim from the output.** Record the commands, their results, and
   the checks you did not run — what you leave out is what the next agent will
   assume you did. Inside a drovr phase this is also what gates
   `drovr phase done`.

## Requirements

The middle column is the **fresh** evidence that claim takes — *fresh* as
defined above. The right column is what gets accepted instead.

| The claim | Required evidence | NOT sufficient |
|---|---|---|
| *"The task's tests pass"* | The task's named verification command, run after your last edit, and its output — the number that ran and zero failures | An earlier run · a subset you chose yourself · only the tests you expect this change to touch · *"nothing here could break them"* |
| *"It builds"* | The build command run to completion on this tree, its exit status zero, and its output in this message | Tests passing — they may have run against a stale artifact · the last build, from before your edit · the editor showing no errors |
| *"The linter is clean"* | The linter run over the files you changed, showing zero findings **in those files** | A run from before your edit · *"the formatter would have caught it"* · a filtered invocation whose filter you did not state in the report |
| *"The bug is fixed"* | The original reproduction re-run and now passing, **and** the task's named verification command, **and** a test that fails without your change | The reproduction alone — green proves the trigger stopped, not that the cause is gone · *"I can no longer make it happen"* · a test you added but never watched fail |
| *"The subagent reported success"* | A file, diff or log it names, opened by path and read by **you** — or, when it reported success in prose with no separate artifact, the same check re-run by you. Its own message is a summary however much output it pastes into itself | Its summary · *"reported no findings"* · a findings file you did not read · an exit code you did not see · a subagent still running |

## Red flags — STOP

Some are thoughts, some are things you have just noticed in your own draft.
Either way you are at the line, not past it.

- *"A rename and two comments cannot change behaviour."* · *"They looked at the
  diff and said I do not need to re-run it."* · *"I will post the report now and
  re-run the suite after."* — each opens a row in *Rationalizations* below,
  verbatim. Go do the thing in its row.
- *You are typing "all tests pass" and your last run predates your last edit.* →
  Re-run first. The old output is evidence about a tree that no longer exists.
- *A subagent said it passed and you are about to repeat that.* → Open the file
  it named, or re-run the check yourself when it named none. Hearsay does not
  become evidence by being forwarded.
- *A review subagent is still running and you are writing the report.* → Block
  on it first. A parked agent is not a finished one, and its silence is not a
  pass.
- *You are reaching for a hedge — "it should pass", "the change is obviously
  correct", "an unrelated failure".* → The hedge is there because the evidence is
  not. Run the check, or report plainly that you did not.
- **Any wording that implies success you have not just watched** — *done ·
  fixed · passing · clean · working · verified · ready · shipped*, and any other
  word a reader will take as "it works". If you cannot point at output **in this
  message** that shows it, you have not earned the sentence. Change what you
  ran, not how you word it.

## Rationalizations

The right-hand column is an instruction, not an argument. Do the thing in it.

| The thought | Do this instead |
|---|---|
| *"A rename and two comments cannot change behaviour."* | Run the suite and find out. *Very likely* behaviour-preserving is a judgment call, and this skill exists specifically to override judgment calls with evidence at the moment of claiming done. |
| *"I have never once seen a rename break a test suite."* | Run it anyway and report from the output. That is a claim about your sample of past renames, not about this one — and the whole point of running the suite is that you do not have to trust your own read of the diff. |
| *"They looked at the diff and said I do not need to re-run it."* | Run the command the task's verification names and cite its output. A diff review is useful signal that the change *looks* safe; it does not substitute for the verification the task specifies. |
| *"It ran green earlier in this session."* | Re-run it against the tree as it stands. That run is evidence about the older tree, not this one. |
| *"I will post the report now and re-run the suite after."* | Run first, post second. If the run fails, a broken report has already gone to the next phase, which binds to it — and nobody re-checks it. |
| *"I am already past the time I promised."* | Take the four minutes and say in the report that you are late. A late verified report costs less than an on-time unverified one. |
| *"The reviewer subagent came back with no findings."* | Open the findings file it names and read it before you repeat its verdict — or re-run the check yourself when it names no file. A subagent's summary is a claim; the artifact it names is the evidence. |

## Worked example

❌ **What this looks like when it fails:**

> Verification `./scripts/test-billing.sh` — 96 passing. Since that run: a
> rename and two comments, nothing that touches proration. Reporting done.

✅ **What it looks like when it holds:**

> Using drovr:verification-before-completion — running the checks before
> claiming done. The 18:31 run verified a different tree than the one I am about
> to report on: I renamed `total` to `lineTotal` in `src/billing/lines.ts` after
> it. A rename plus comments is very likely behaviour-preserving, but "very
> likely" is a judgment call, and this skill exists to override judgment calls
> with evidence at the moment of claiming done. The tech lead read the diff —
> that is a diff review, not a test run. So, against the tree as it stands, four
> minutes:

```
$ ./scripts/test-billing.sh
96 passing (4m 01s)
0 failing
$ echo $?
0
```

> Report: verification `./scripts/test-billing.sh`, re-run after the rename —
> 96 passing, 0 failing, exit 0. I did not run the linter; this task's
> verification does not name one. I am eleven minutes past the time I promised
> and this report says so: being eleven minutes late with a verified report is a
> much smaller cost than being on time with an unverified one.

## Cross-refs

- `drovr:tdd` — REQUIRED when the claim is that a new behaviour works. The red
  run you watched is what makes the green one evidence.
- `drovr:systematic-debugging` — REQUIRED before you claim a bug is fixed. It
  owns the reproduction; this skill owns why green is not sufficient.
- `drovr:code-review` — REQUIRED before you call a change done. Its reviewers
  run in the foreground for the reason the subagent row gives above.
- `drovr:handoff` — REQUIRED at a phase boundary. Put the commands and their
  output there; the diff does not carry them.
