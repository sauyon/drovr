# Known issues

## `drovr phase send` does not reliably submit large injections

**Severity:** high (the pipeline's own injection path is unreliable).
**Found:** 2026-07-21, dogfooding the drovr-v2 pipeline (every implement task hit it).

### Symptom

After `drovr phase send <run> <phase> "<large briefing>"`, the pasted text lands in
the target claude pane's input buffer but is **never submitted** — the agent sits idle
(`$0.00` cost, no activity) indefinitely. `drovr phase wait` then times out even though
the agent never started.

Observed with multi-KB briefings, but also reproduced with a short (~1 line) pointer
message — so it is **not strictly size-gated**; the submit carriage return is being lost.

### Root cause (hypothesis)

`SystemHerdr::agent_send` writes the text, waits `PASTE_SETTLE` (150 ms), then sends a
CR to submit. claude's TUI uses bracketed-paste; when the CR arrives before the paste
has fully "settled," it is absorbed into the paste instead of submitting. The 150 ms
constant (see `cli/src/herdr.rs`, `PASTE_SETTLE`) is too short for large / slow pastes,
and the failure is timing-dependent (racy), which is why a short message can miss too.

### Reliable workaround (used to drive the drovr-v2 run)

Follow every content send with a **second, empty submit**, with a short delay between:

```
drovr phase send <run> <phase> "<briefing>"
sleep 4
drovr phase send <run> <phase> ""      # bare CR flushes the buffered paste
```

The empty send carries no paste, so its CR submits cleanly. Sending the two
back-to-back (no delay) can still race — the empty CR can fire before the big paste
lands and submit nothing.

A lower-overhead variant that also worked: inject a **short pointer** message
("your briefing is in `<path>` — read it and execute") instead of a large paste, still
followed by the empty-submit flush.

### Fix ideas (for a future drovr change)

1. Have `agent_send` always send the submit CR as a **separate keystroke after a
   readback/confirmation** that the paste landed, rather than a fixed 150 ms sleep.
2. Make `PASTE_SETTLE` scale with payload size, or poll the pane until the buffer
   reflects the full paste before sending CR.
3. Prefer a file-pointer injection convention in the skills (write the briefing to the
   run dir, send a one-line pointer) to keep pastes tiny.
4. Add an e2e/integration test that asserts a large `agent_send` actually submits.

## Review server diff rendering is hard to read

**Severity:** medium (usability of the human spec gate — reviewers struggle to see what changed).
**Found:** 2026-07-22, reviewing a spec through `drovr serve`.

### Symptom

The review server's browser UI renders the spec change in a way that's hard to read — a
reviewer can't quickly tell what actually changed between the prior version (`prior.md`) and
the current `spec.md`.

### Desired behavior

Just show **before + after**, with only the changed bits highlighted — **green for additions,
red for removals** — a plain, familiar diff view, instead of whatever is rendered now.

### Where

`cli/src/review.rs` serves `/doc` (current `spec.md`) and `/prior` (previous `prior.md`); the
diffing/rendering lives in the `cli/web/` frontend. The fix is in the frontend rendering (a
line/word-level before→after diff with red/green highlighting), possibly with a server-side
diff if that's cleaner than diffing in the browser.

## Review server: Submit button stays greyed out after a successful submit

**Severity:** high (blocks the reviewer from submitting a later turn).
**Found:** 2026-07-22, on the second review turn of a spec gate.

### Symptom

After the reviewer requests changes once and the agent re-serves a revised spec (state →
`ready`), the browser re-shows the form but the **Submit button is greyed out** (disabled) —
the reviewer can't submit again. A full page reload works around it.

### Root cause

`cli/web/index.html` `submitDecision()` sets `btn.disabled = true` before POSTing and only
re-enables it on the failure / network-error paths — **never after a successful submit**.
`refresh()` (which re-shows the form when state returns to `ready`) doesn't reset `disabled`
either, so the button carries `disabled = true` into the next turn.

### Fix

Reset `submit-btn.disabled = false` whenever the form is (re)shown — in `refresh()`'s
"idle or ready: show form" branch (or re-enable at the end of `submitDecision` on success).
One line.
