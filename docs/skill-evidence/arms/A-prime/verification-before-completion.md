---
name: verification-before-completion
description: Use when about to claim any work is done, fixed, or passing, before reporting, committing, or handing off — requires running the verification command in this message and reading its output; evidence before assertion, always
---

# Verification Before Completion

## Overview

**Evidence, not assertion.** Never declare a task done off your own judgment.
Verify before claiming done: run the task's tests and the build/linter, and
confirm they pass by reading the actual output.

Unverified work reported as done does not stay contained: whoever picks it up
next binds to the interface you claimed, not the one you actually have.

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

Only after the evidence is in hand may you write it up. The write-up records the
verification output, not a summary of your confidence. If you are in a phase,
this is also what gates `drovr phase done`.
