---
name: tdd
description: Use when implementing any feature or bugfix in a drovr phase, before writing implementation code
---

# TDD

## Overview

**Test-first, always.** Write the failing test before the code it covers, watch
it fail, then write the minimal code to make it pass. A test you never saw fail
proves nothing — it may pass for the wrong reason, or not exercise the behavior
at all.

In a drovr phase you are the single writer. The test is the contract the next
phase binds to, so it must be real: keep changes scoped to this task's
interfaces so the folded-forward contracts stay accurate.

## The cycle

1. **RED** — write the failing test named in the task's verification. Run it.
   *Watch it fail*, and confirm it fails for the reason you expect (missing
   behavior, not a typo or compile error elsewhere).
2. **GREEN** — write the *minimal* code to make that test pass. No extra
   features, no speculative abstraction — just enough to go green.
3. **REFACTOR** — clean up with the test as your safety net, re-running it.

Repeat per behavior. One failing test at a time.

## Red flags — STOP

- Wrote code before the test → delete it, start with the test.
- "I'll add the test after" → tests-after check *what the code does*, not what it
  *should* do. Test first.
- Test passed the first time you ran it → you never saw RED; make sure it truly
  exercises the new behavior.
- Test is scoped wider than this task's interfaces → tighten it; drift breaks the
  contract the next phase inherits.

## Verifying the pass

Green on the one test is necessary, not sufficient. Before claiming done, hand
off to `drovr:verification-before-completion` — run the full suite and the build,
and confirm with evidence, not assertion.
