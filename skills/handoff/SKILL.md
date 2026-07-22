---
name: handoff
description: Use when finished work in one agent context must be carried across a phase boundary to a fresh clean-context agent
---

# Handoff

## Overview

A **handoff** is the atomic relay boundary: run one phase agent, compress its whole
transcript into a `<phase>-HANDOFF.md`, then seed the next fresh agent with that doc. This
is the building block `relay:pipeline` loops over; use it directly for a single boundary.

**The load-bearing move is compression by a dedicated fresh reader — never self-summary.**
`relay phase compress` spawns a one-shot read-only `claude -p` that reads the finished
phase's *entire* transcript and writes the briefing. The finishing agent does NOT summarize
itself (end-of-phase context rot produces bad briefings). Preserve decisions + interfaces,
drop narration.

## The five steps

Assume a run exists (`relay new <run> --task "..."` — see `relay:using-relay` for setup).
Run name is `<run>`; the phase you are running is `<phase>`.

1. **Start the phase.** Seed = the PRIOR phase's handoff (omit `--seed` for the first phase).
   ```
   relay phase start <run> <phase> --seed ~/.local/share/relay/runs/<run>/<prev>-HANDOFF.md
   ```
   This spawns a **plain `claude`** and records the seed path in `state.json`. It does
   **not** inject anything into the agent yet.

2. **Inject the briefing — THIS IS REQUIRED.** The CLI does not seed the agent; you do.
   Compose the first message = the phase's instructions + the prior handoff content, and
   send it:
   ```
   relay collect <run> <prev>                          # prints <prev>-HANDOFF.md
   relay phase send <run> <phase> "<instructions>\n\n<pasted handoff content>"
   ```
   `relay phase send` writes the text and submits it (adds the carriage return). For the
   first phase, the briefing is the task itself, not a prior handoff.

   The instructions you inject MUST end with the completion contract: tell the agent that
   its **final action** is to run `relay phase done <run> <phase>`, and that any review
   subagents it launches must run in the **foreground** (never `run_in_background`, never
   yield waiting on them). This is what lets step 3 detect completion; without it the agent
   finishes its work but never signals, and `wait` times out.

3. **Wait for done.**
   ```
   relay phase wait <run> <phase> --timeout-ms 600000
   ```
   This POLLS the filesystem for the marker the agent drops via `relay phase done` (step 2's
   injected final action). It deliberately does NOT watch herdr's agent status: `idle` is not
   a completion signal — it also fires while the agent is parked awaiting its own subagent, so
   watching it returns a false "done" mid-phase. Exit `0` = done · exit `2` = still running
   (timed out; wait again or investigate) · exit `1` = an I/O error. Use a generous timeout —
   real phases take minutes.

4. **Compress → `<phase>-HANDOFF.md`.**
   ```
   relay phase compress <run> <phase>
   ```
   Reads the transcript via `herdr agent read`, runs the compressor, writes exactly
   `~/.local/share/relay/runs/<run>/<phase>-HANDOFF.md`. That exact filename is the
   contract `relay collect` reads.

5. **Collect + hand forward.**
   ```
   relay collect <run> <phase>
   ```
   Prints the handoff. It becomes the `--seed` for the next phase's step 1, and the pasted
   content for its step 2.

## Quick reference

| Step | Command | Note |
|---|---|---|
| start | `relay phase start <run> <phase> [--seed <path>]` | plain claude; records seed path only |
| **inject** | `relay phase send <run> <phase> "<text>"` | **you must do this — CLI won't**; end the text with the `phase done` contract |
| wait | `relay phase wait <run> <phase> --timeout-ms <ms>` | polls for the `done` marker (not herdr idle); 0=done 2=timeout 1=io-error |
| done | `relay phase done <run> <phase>` | run by the AGENT as its final action; drops the marker `wait` polls |
| compress | `relay phase compress <run> <phase>` | writes `<phase>-HANDOFF.md` |
| collect | `relay collect <run> <phase>` | reads `<phase>-HANDOFF.md` |

## The HANDOFF doc shape

The compressor emits a fixed 7-section document (objective · state · decisions+rationale ·
interfaces/contracts · open questions · next step · artifact pointers). See
`HANDOFF-template.md` in this directory for the contract and what each section is for.
**Artifact pointers are paths, not pasted content** — the next agent re-reads source on
demand (that is what keeps each phase's context small).

## Self-review before you compress

If the phase produced an artifact (code, a spec, a plan), the phase agent should have
**reviewed its own work with read-only review subagents** and addressed their
Critical/Important findings *before* you compress and hand forward. Read-only reviewers
preserve the single-writer rule — they find, the phase agent fixes. Compressing an
unreviewed artifact just carries its defects into the next phase's briefing. (The
`relay:pipeline` phase-prompts encode this per phase.)

## Stop conditions — a bad handoff cascades

If any of these happen, **STOP and surface diagnostics** rather than seeding the next phase
with a broken briefing:

- `relay phase wait` exits `1` (failed) or keeps exiting `2` (never completes).
- `relay phase compress` errors, or `relay collect` returns empty / missing the fixed
  sections.

Diagnose with `herdr agent read <pane>` (raw transcript), or `relay attach <run>` to inspect
the pane live.
Seeding a fresh agent from a garbage handoff wastes the whole downstream chain — a broken
briefing is worse than a stopped run.

## Common mistakes

| Mistake | Fix |
|---|---|
| Skipping step 2 ("start seeds it") | It doesn't. The fresh agent sits idle until you `phase send`. |
| Letting the finishing agent write its own summary | Use `relay phase compress` — a fresh reader, not self-summary. |
| Pasting file contents into the handoff | Use artifact pointers (paths); the successor re-reads. |
| Hardcoding `<phase>-HANDOFF.md` elsewhere | Read it via `relay collect`; write it only via `relay phase compress`. |
