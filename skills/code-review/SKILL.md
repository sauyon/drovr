---
name: code-review
description: Use when a drovr phase has produced an artifact (code, spec, plan) and must be reviewed before it is reported done or handed forward
---

# Code Review

## Overview

**They find, you fix.** Do not declare work done on your own judgment. Launch one or
more **read-only review subagents** to adversarially review the change, then address
what they find — read-only keeps drovr's single-writer rule intact. This is in
addition to the pipeline's final review phase: catch defects here, before they
cascade.

## How to run it

- **Never write a reviewer's prompt.** Its prompt is the output of
  `drovr code-review brief <run> <task> --angle <angle> [--context "<what changed>"]`
  — pass that verbatim. drovr owns the frame (angle, scope, findings schema); you
  contribute only `--context`.
- Use the Agent tool, `subagent_type: general-purpose`, model `sonnet`.
- **Run them in the FOREGROUND (blocking).** Never `run_in_background`, never yield
  or schedule a wakeup on them: a backgrounded subagent parks you mid-turn —
  indistinguishable from completion — and stalls the run until a human nudges the
  pane. Block, and work through to your final step.

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

## Automatic panel — `drovr code-review run <run> <task> [--context …]`

The pipeline runs this per task (see `drovr:pipeline`): one read-only reviewer per
configured angle, each given the brief above. Findings union-merge into
`<task>-review.json`, tagged by angle. Exit: 0 clean, 3 findings, 2 timeout, 1 error.
The panel needs a herdr workspace to spawn panes into, so from a session drovr did
not start, use `code-review brief` and spawn the reviewer yourself.

**Exit 2 is slow, not broken.** Re-running the *same* command RESUMES: re-attaches to
the panel in flight, keeps the angles already banked, waits only on the stragglers,
respawns a dead reviewer. Loop on 2 as freely as on 3. `--fresh` throws that panel
away and pays for a new one — never use it to unstick a timeout.
