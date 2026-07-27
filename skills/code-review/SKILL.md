---
name: code-review
description: Use when a drovr phase has produced an artifact (code, spec, plan) and must be reviewed before it is reported done or handed forward
---

# Code Review

## Overview

**They find, you fix.** Never call work done on your own judgment. Launch **read-only
review subagents** to adversarially review the change, then fix what they find —
read-only keeps the single-writer rule intact. Do it even though the pipeline reviews
again later: catch defects before they cascade.

## How to run it

- Use the Agent tool, `subagent_type: general-purpose`, model `sonnet`.
- **Scope: the diff AND the whole repo.** Prompt each reviewer with `git diff
  <base>..HEAD` plus the working tree, *and* that its checkout is full — read any file,
  follow callers and callees, run tests. A diff-only reviewer misses the caller the
  change breaks and the empty test.
- **Run them in the FOREGROUND.** Never `run_in_background`, never yield or schedule a
  wakeup: a backgrounded subagent parks you mid-turn — indistinguishable from
  completion — and stalls the run until a human nudges the pane.

## Check, in order

1. **Spec compliance** — what the spec agreed to, no more, no less.
2. **Correctness** — real bugs, unhandled cases, broken invariants.
3. **Verification** — do the claimed tests exist and exercise the behavior? Run them
   (`drovr:verification-before-completion`).
4. **Quality** — reuse, simplification, consistency with surrounding code.

Review as a skeptic, not the author.

## Resolving findings

Fix **every Critical and Important finding**, re-run the tests, and record any you
chose not to fix, with the reason. Only then report done.

## Automatic panel — `drovr code-review run <run> <task>`

The pipeline runs this per task (see `drovr:pipeline`): one read-only reviewer per
angle, each pointed at the run's full checkout — `git diff <base>..HEAD`
(`<base>` = `<task>-base.sha`), the working tree, plus any file it reads.
Findings union-merge into `<task>-review.json`, by angle. Exit: 0 clean,
3 findings, 2 timeout, 1 error.

**Exit 2 is slow, not broken.** Re-running the *same* command RESUMES: re-attaches to
the panel in flight, keeps banked angles, waits on stragglers, respawns dead
reviewers. Loop on 2 as freely as on 3. `--fresh` throws that panel away — never use it
to unstick a timeout.
