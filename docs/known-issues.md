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
