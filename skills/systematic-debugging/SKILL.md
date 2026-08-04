---
name: systematic-debugging
description: Use when encountering any bug, test failure, or unexpected behavior, before proposing or writing a fix — requires a reproduction and a mechanistic root cause before any code change
---

# Systematic Debugging

## Overview

**Find the root cause before you touch the code.** A fix aimed at a symptom you
have not reproduced and localized is a guess — it may hide the bug or add a new
one. Work the loop below in order; do not skip to the fix.

Fan-out investigation belongs to **read-only explorers** (e.g. `explore-mcp`),
never to parallel writers — dispatch them to understand code you only need to
read, and keep editing to yourself. Inside a drovr phase this is also what keeps
the single-writer rule intact.

## The loop

1. **Reproduce** — get a reliable, minimal trigger. A bug you can't reproduce on
   demand, you can't verify you fixed. Capture the exact command and output.
2. **Isolate** — narrow to the smallest input, file, or code path that still
   shows it. Bisect, add logging, or send a read-only explorer to map the
   suspect area.
3. **Root-cause** — explain *why* it happens, mechanistically. "Adding this line
   makes it go away" is not a cause. Keep going until the explanation predicts
   the observed behavior exactly.
4. **Fix** — make the minimal change that addresses the cause, not the symptom.
5. **Verify** — reproduce your original trigger and confirm it's gone, then run
   the full suite (see `drovr:verification-before-completion`). Add or fix a test
   that would have caught it.

## Red flags — STOP

- Editing code before you've reproduced the failure → reproduce first.
- "This line probably fixes it" with no explanation of why → you have a symptom
  patch, not a root cause.
- The fix works but you can't say why the bug happened → keep digging.
- Can't reproduce, so you'll "fix it and see" → not a fix; find the trigger.
