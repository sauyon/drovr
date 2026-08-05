---
name: code-review
description: Use when you have written any change, before calling it done or handing it forward — requires read-only reviewer subagents run in the foreground, with every Critical and Important finding resolved or explicitly recorded as deferred
---

# Code Review

## Overview

**They find, you fix.** Follow the spirit, not the letter: dispatching a
reviewer satisfies the letter; one that read an empty diff, or that you never
waited for, found nothing because it looked at nothing. The rule is that somebody
who did not write the change has read it and said what is wrong with it — and
you cannot be that somebody. You read the change you meant to make; the defect
is in the gap between that and what is on disk.

**Write for the agent who inherits this: the next phase agent is you, with your
context gone.** A finding you settled in your head is one they meet again as a
bug, with nothing saying anyone saw it.

## The Iron Law

```
NO CHANGE IS DONE UNTIL A READ-ONLY REVIEWER HAS SEEN IT AND
EVERY CRITICAL AND IMPORTANT FINDING IS RESOLVED OR RECORDED AS DEFERRED.
```

**Read-only** is a trust boundary, not a performance note: the reviewer may
read and run anything, and never edits project source. You stay the single
writer.

No exceptions:

- **Never run a reviewer in the background.** Do not set `run_in_background`,
  do not yield or schedule a wakeup while one is out. A backgrounded
  reviewer parks you mid-turn, indistinguishable from a finished turn, and the
  run stalls until a human nudges the pane. Dispatch it blocking and read its
  verdict in the turn you dispatched it.
- **Do not judge a change too small to review.** A one-line edit to a condition
  is the size of change this skill exists for. Dispatch a reviewer over it.
- **Do not count your own read as the review.** Re-reading your own diff tells
  you it matches what you intended — the one thing never in doubt. Dispatch
  someone who does not know your intent.
- **Do not leave it for the pipeline's review phase.** That phase reads what
  you hand it, after later work is built on it. Dispatch here, while the change
  is one commit and one context wide; that phase is the second look.
- **Do not close a finding by disagreeing with it.** When you will not fix one,
  quote it and write the reason in the report or handoff. An unrecorded
  decision reads downstream exactly like an unread finding.
- **Do not repeat a verdict you have not read.** Open the file the reviewer
  names and read it, or re-run the review when it named no file. A reviewer
  that never came back has not said "clean".

## Announce

Say this out loud when you start, before dispatching anything:

```
Using drovr:code-review — dispatching read-only reviewers before calling this done.
```

## The procedure

> When a skill or briefing gives you a numbered checklist, create **one tracked item per step**
> using whatever task tool this harness exposes — `TodoWrite`, or `TaskCreate`/`TaskUpdate` —
> before you start step 1. Mark each in-progress when you start it and complete when its
> evidence is in hand. If the harness exposes no task tool, write the checklist to
> `~/.local/share/drovr/runs/<run>/checklist.md` when inside a run, or `CHECKLIST.md` at the
> repo root otherwise, and tick items there. An untracked checklist decays with the context
> window; that decay is the exact failure drovr exists to fight.

Re-create those items for each change you review, not once for the session.

1. **Write down the change and the exact range to review.** Name the files and
   the range — in a drovr run, `<base>..HEAD` with `<base>` from
   `<task>-base.sha`, recorded before your first edit. Get this wrong and the
   reviewer reads an empty range and reports clean, having read nothing.
2. **Dispatch read-only reviewers, blocking, one per angle.** Agent tool,
   `subagent_type: general-purpose`, model `sonnet`. Tell each to review as a
   skeptic, not the author, and to say whether the tests exercise the behaviour
   rather than only that they pass. The angles:
   - **Spec compliance** — does it do what was agreed, no more, no less?
   - **Correctness** — real bugs, unhandled cases, broken invariants.
   - **Verification** — do the claimed tests exist, and would they fail if the
     behaviour regressed?
   - **Quality** — reuse, simplification, consistency with the code around it.

   In a drovr run, `drovr code-review run <run> <task>` dispatches that panel
   for you and merges its findings into `<task>-review.json`, tagged by angle.
3. **Wait for every reviewer, then read what each wrote.** Open the findings
   file by path. Exit codes: 0 clean, 3 findings, 2 timeout, 1 error — **only 0
   is clean**, and a reviewer that returned no verdict is none of the four.
   Re-dispatch it rather than reporting its silence.
4. **Check what the reviewer actually read.** The diff it names must be
   non-empty and must be your change. A clean verdict on an empty range, or on
   the tree before your last edit, is a clean verdict about a change nobody
   read.
5. **Resolve every Critical and Important finding, or record the deferral.**
   Per finding: quote it, then name the edit answering it by file or write down
   why you are not making it.
6. **Re-run the tests after your last review-driven edit, then say it is done.**
   The fixes are new, unverified code. Report the findings, the edits and the
   deferrals; inside a drovr phase this also gates `drovr phase done`.

## Requirements

The middle column is what the claim takes; the right column is what gets
accepted instead.

| The claim | Required evidence | NOT sufficient |
|---|---|---|
| *"This change has been reviewed"* | A read-only reviewer dispatched over this change in the foreground, returned in this turn, and the range you handed it, written down | Your own re-read · a reviewer launched and not waited for · a review of the tree before your last edit |
| *"The reviewer found nothing"* | Its findings file opened by path and read by you, **and** the diff it read shown to be non-empty and to be your change | Its summary · exit 1 or 2 · a clean verdict whose range you did not check · a verdict that never arrived |
| *"Every Critical and Important finding is resolved"* | Each quoted, the edit answering it named by file, and the tests re-run after the last edit | A count · agreeing with one in prose · fixing the Criticals and leaving the Importants |
| *"That finding does not apply here"* | The finding quoted, and the reason written where the next agent reads it | Deciding it in your head · *"out of scope"* with no scope named · silence, which downstream reads as resolved |
| *"This change is done"* | The four rows above, satisfied for this change, in this message | Green tests, another skill's bar · the pipeline's later review phase, which reads what you hand it |

## Red flags — STOP

Some are thoughts, some are things you have just written. Either way you are at
the line, not past it.

- *"They are going to read it line by line anyway."* · *"It is a two-line
  change."* · *"I already reviewed it myself."* · *"The pipeline's review phase
  will catch it."* — each opens a row in *Rationalizations* below, verbatim. Go
  do the thing in its row.
- *You are about to launch a reviewer in the background and keep working.* →
  Dispatch it blocking. No version of that ends with you reading its verdict in
  this turn.
- *A reviewer has not come back and you are writing the report.* → Block on it.
  Silence is not a pass; a parked reviewer is not a finished one.
- *The panel came back clean in seconds on a large diff.* → Check the range it
  read before repeating the verdict. Clean and empty look identical from here.
- **Any sentence calling the change done, ready, shipped, or safe to hand off**
  while a reviewer is unread, unreturned, or holding an unresolved Critical or
  Important finding. If you cannot name the reviewer and point at what it
  wrote, you have not earned the sentence. Change what you dispatched, not how
  you word it.

## Rationalizations

The right-hand column is an instruction, not an argument. Do the thing in it.

| The thought | Do this instead |
|---|---|
| *"They are going to read it line by line anyway."* | Dispatch the reviewer first, then send it. Two different reads: theirs is for fit, the review's is for correctness, and one does not make the other redundant — reading a document aloud catches prose gaps, not retry semantics. |
| *"The lead said just send it."* | Send it after the review, and tell them the call moves. That is a scheduling reflex, not anyone weighing the technical risk — the calendar is theirs, the correctness is yours to carry. |
| *"I will send it flagged as not yet reviewed."* | Review it, then send it. The flag ships the same content and risk with a disclaimer on top; it does not change what the reader does once they have read it and moved toward signing. |
| *"It is a two-line change."* | Dispatch a reviewer over the two lines. Size is not a risk measure, and the risk you are estimating in order to skip the review is what the review would have told you. |
| *"I already reviewed it myself."* | Dispatch one that did not write it. Your read confirms the diff matches your intent; every defect worth catching is in what you did not think of. |
| *"The pipeline's review phase will catch it."* | Dispatch here, now. That phase reads what you hand it, after later tasks built on it; a finding costs more at every step it survives. |
| *"The reviewer came back with no findings."* | Open the file it named and read it, and check the diff it read was non-empty and yours. An empty range comes back clean from every angle at once. |
| *"I am out of time before the deadline."* | Take the twenty-five minutes and move the deadline. A late reviewed artifact costs less than an on-time unreviewed one. |

## Worked example

Condensed from a real decision: a 320-line integration spec, a client call in 19
minutes where three people will read it aloud, sign-off gating a $45,000
payment, the lead writing *"just send it."* Reviewer output below is
illustrative, not a transcript.

❌ **What this looks like when it fails:**

> They are going to read it out loud section by section — that *is* the review,
> and a review pass first is a second reading of something nobody has read
> once. The lead said send it. Sending.

✅ **What it looks like when it holds:**

> Using drovr:code-review — dispatching read-only reviewers before calling this
> done. The "second reading" framing conflates two reads: theirs is for fit, the
> review's is for correctness, and one does not make the other redundant. "Just
> send it" is a scheduling call, the lead's to make; the retry semantics are
> mine. Moving the call, four angles, blocking:

```
$ drovr code-review run warehouse-sync spec
reviewed 320 lines (a41c9e2..HEAD) · 4/4 angles returned
correctness  IMPORTANT  §4.2 retry window (30s) < the 45s ack timeout in §3.1
correctness  IMPORTANT  §6 reconciliation excludes the partial-day boundary
spec         nit        §2 names two ids for one endpoint
exit 3
```

> Both Importants fixed: retry window 30s → 60s, §6 reconciliation now spans the
> boundary day. The nit is deferred and recorded here: the duplicate id is in a
> section the client wrote, and renaming it mid-review would confuse the
> walkthrough. Sent 22 minutes late, call moved — the retry window would have
> shipped a contract we could not meet, and nobody was catching that by reading
> it aloud.

## Cross-refs

- `drovr:verification-before-completion` — REQUIRED before you repeat a
  reviewer's verdict. It owns why a subagent's report is a claim, not evidence.
- `drovr:systematic-debugging` — REQUIRED when a finding is a bug. A finding is
  a report, not a diagnosis; reproduce it before you fix it.
- `drovr:tdd` — REQUIRED when a finding needs new behaviour: the fix is new
  code and gets a failing test first.
- `drovr:handoff` — REQUIRED at a phase boundary: deferred findings travel in
  the handoff or not at all.
- `drovr:pipeline` — the run-level review phase: a second look, not this one.
