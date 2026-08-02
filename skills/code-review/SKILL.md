---
name: code-review
description: Use when a drovr phase has produced an artifact (code, spec, plan) and must be reviewed before it is reported done or handed forward
---

# Code Review

## Overview

**Two roles, one gate.** Anyone may RUN the panel, as often as they like —
it is a test suite. Only the **driver's** run is the **gate**: a clean
verdict on a panel you ran yourself is evidence, never permission to
report done.

**They find, you fix.** Launch **read-only review subagents** to adversarially review
the change, then address what they find — read-only keeps drovr's single-writer rule
intact. In addition to the pipeline's final review phase, not instead of it.

## How to run it

- **Never write a reviewer's prompt.** It is the output of
  `drovr code-review brief <run> <task> --angle <angle> [--context "<what changed>"]`
  — pass that verbatim. drovr owns the frame (angle, scope, schema); you contribute
  only `--context`.
- Use the Agent tool, `subagent_type: general-purpose`, model `sonnet`.
- **Run them in the FOREGROUND (blocking).** Never `run_in_background`, never yield
  or schedule a wakeup on them: that parks you mid-turn — indistinguishable from
  completion — and stalls the run until a human nudges the pane.

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
any you chose not to fix, with the reason. That clears review; it is not what
makes you done — an empty finding list clears it vacuously.

## Automatic panel — `drovr code-review run <run> <task> [--context …]`

Yours to run anytime; the driver runs it per task as the gate (see `drovr:pipeline`).
One read-only reviewer per configured angle, each given the brief above. Findings
union-merge into `<task>-review.json`, tagged by angle. Exit: 0 clean, 3 findings,
2 timeout, 1 error.

It needs a herdr workspace (`drovr new` records one) and a review agent with a herdr
integration. If the panel is unavailable or wedged, use `code-review brief` and spawn
the reviewer yourself — same brief either way.

**Exit 2 is slow, not broken.** Re-running the *same* command RESUMES: it keeps the
angles already banked and waits only on stragglers. Loop on 2 as freely as on 3.
`--fresh` throws it away and pays for a new one — never use it to unstick a timeout.
