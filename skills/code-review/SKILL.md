---
name: code-review
description: Use when a drovr phase has produced an artifact (code, spec, plan) and must be reviewed before it is reported done or handed forward
---

# Code Review

## Overview

**They find, you fix.** Do not declare work done on your own judgment. Launch
one or more **read-only review subagents** to adversarially review the change,
then address what they find. Read-only reviewers preserve drovr's single-writer
rule: the subagent finds, the phase agent fixes. This is in addition to the
pipeline's final review phase — catch defects here, before they cascade.

## How to run it

- Use the Agent tool, `subagent_type: general-purpose`, model `sonnet`.
- **Run them in the FOREGROUND (blocking).** Never set `run_in_background`; never
  yield or schedule a wakeup waiting on them. A backgrounded subagent parks you
  mid-turn — indistinguishable from completion — and stalls the run until a human
  nudges the pane. Block, and work straight through to your final step.
- Ask the reviewer to check whether the tests actually exercise the behavior, not
  just that they pass.

## Check, in order

1. **Spec compliance** — does the change do what the spec agreed to, no more, no
   less?
2. **Correctness** — real bugs, unhandled cases, broken invariants.
3. **Verification** — do the claimed tests exist and exercise the behavior? Run
   them if you can (see `drovr:verification-before-completion`).
4. **Quality** — reuse, simplification, consistency with surrounding code.

Review as a skeptic, not the author.

## Resolving findings

Address **every Critical and Important finding**, then re-run the tests. Record
any finding you consciously chose not to fix, with the reason. Only after this
may you report done.

## Automatic panel — `drovr code-review run <run> <task>`

The pipeline runs this per task (see `drovr:pipeline`): one read-only reviewer per
configured angle over `git diff <base>..HEAD` (`<base>` = `<task>-base.sha`, taken
at task start). Each reviewer reads the diff + working tree, may run tests, writes
`<task>-review-<angle>.json`, runs `drovr phase done`, then exits — it never edits
project source or `state.json` (the read-only trust boundary; they find, you fix).
Findings union-merge into `<task>-review.json`, each tagged with its angle. Exit
codes: 0 clean, 3 findings, 2 timeout, 1 error.
