---
name: verification-before-completion
description: Use when about to claim a drovr task is done, fixed, or passing, before writing the report or signalling phase done
---

# Verification Before Completion

## Overview

**Evidence, not assertion.** Never declare a task done off your own judgment.
Verify before claiming done: run the task's tests and the build/linter, and
confirm they pass by reading the actual output.

This matters doubly in drovr: a phase that reports "done" on unverified work
cascades a broken interface into the next phase's briefing, which binds to it.

## Before you say "done"

1. **Run the task's tests** named in the verification — not a subset, the ones
   the task specifies.
2. **Run the build and linter.** A passing test on code that doesn't build is
   not passing.
3. **Read the output.** Confirm the tests you claim exist actually ran and
   actually exercise the behavior — a green line for a test that asserts nothing
   is not verification.
4. **State what you verified vs. what you could not check.** If a step was
   skipped, say so plainly; do not imply coverage you don't have.

## Red flags — STOP

- "It should pass" / "the change is obviously correct" → run it and read the
  output.
- "Tests passed earlier" → re-run after your last edit; earlier ≠ now.
- Claiming a test exists you have not run → run it or don't claim it.
- Reporting done while a review subagent is still running → block on it first
  (see `drovr:code-review`); a parked agent is not a finished one.

## The claim

Only after the evidence is in hand may you write the report and run
`drovr phase done`. The report records the verification output, not a summary of
your confidence.
