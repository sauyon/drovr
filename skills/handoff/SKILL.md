---
name: handoff
description: Use when finished work in one agent context must be carried across a phase boundary to a fresh clean-context agent
---

# Handoff

## Overview

A **handoff** is the atomic drovr boundary: run one phase agent, compress its whole
transcript into a `<phase>-HANDOFF.md`, then seed the next fresh agent with that doc. This
is the building block `drovr:pipeline` loops over; use it directly for a single boundary.

**The load-bearing move is compression by a dedicated fresh reader — never self-summary.**
`drovr phase compress` spawns a one-shot read-only `claude -p` that reads the finished
phase's *entire* transcript and writes the briefing. The finishing agent does NOT summarize
itself: a summary written from a full end-of-phase context suffers *context rot* (Chroma's
finding that output degrades as the window fills — trychroma.com/research/context-rot), so a
fresh reader produces the better briefing. Preserve decisions + interfaces, drop narration.
Compaction is one of drovr's three context-engineering levers (with note-taking/git and
sub-agents — see `drovr:using-drovr`); this skill is the compaction lever.

## The five steps

Assume a run exists (`drovr new <run> --task "..."` — see `drovr:using-drovr` for setup).
Run name is `<run>`; the phase you are running is `<phase>`.

1. **Start the phase.** Seed = the PRIOR phase's handoff (omit `--seed` for the first phase).
   ```
   drovr phase start <run> <phase> --seed ~/.local/share/drovr/runs/<run>/<prev>-HANDOFF.md
   ```
   This spawns a **plain `claude`** and records the seed path in `state.json`. It does
   **not** inject anything into the agent yet.

2. **Inject the briefing — THIS IS REQUIRED.** The CLI does not seed the agent; you do.
   Compose the first message = the phase's instructions + the prior handoff content, and
   send it:
   ```
   drovr collect <run> <prev>                          # prints <prev>-HANDOFF.md
   drovr phase send <run> <phase> "<instructions>\n\n<pasted handoff content>"
   ```
   `drovr phase send` writes the text and submits it (adds the carriage return). For the
   first phase, the briefing is the task itself, not a prior handoff.

   The instructions you inject MUST end with the completion contract: tell the agent that
   its **final action** is to run `drovr phase done <run> <phase>`, and that any review
   subagents it launches must run in the **foreground** (never `run_in_background`, never
   yield waiting on them). This is what lets step 3 detect completion; without it the agent
   finishes its work but never signals, and `wait` times out.

3. **Wait for done.**
   ```
   drovr phase wait <run> <phase> --timeout-ms 600000
   ```
   **Block on this in the foreground — do NOT background it.** The phase agent is a separate
   `claude` running in a herdr pane, invisible to your own harness's task tracking, so nothing
   auto-blocks you on its work; `drovr phase wait` IS the synchronization primitive that makes
   the pane agent's progress something you can wait on. Backgrounding the wait defeats its
   entire purpose — it turns the one blocking call async and lets you wander off while the
   phase is still running. Run it foreground and let it hold the turn until the phase finishes.

   This POLLS the filesystem for the marker the agent drops via `drovr phase done` (step 2's
   injected final action). It deliberately does NOT watch herdr's agent status: `idle` is not
   a completion signal — it also fires while the agent is parked awaiting its own subagent, so
   watching it returns a false "done" mid-phase. Exit `0` = done · exit `2` = still running
   (timed out; wait again or investigate) · exit `1` = an I/O error. Use a generous timeout —
   real phases take minutes.

4. **Compress → `<phase>-HANDOFF.md`.**
   ```
   drovr phase compress <run> <phase>
   ```
   Reads the transcript via `herdr agent read`, runs the compressor, writes exactly
   `~/.local/share/drovr/runs/<run>/<phase>-HANDOFF.md`. That exact filename is the
   contract `drovr collect` reads.

5. **Collect + hand forward.**
   ```
   drovr collect <run> <phase>
   ```
   Prints the handoff. It becomes the `--seed` for the next phase's step 1, and the pasted
   content for its step 2.

## Quick reference

| Step | Command | Note |
|---|---|---|
| start | `drovr phase start <run> <phase> [--seed <path>]` | plain claude; records seed path only |
| **inject** | `drovr phase send <run> <phase> "<text>"` | **you must do this — CLI won't**; end the text with the `phase done` contract |
| wait | `drovr phase wait <run> <phase> --timeout-ms <ms>` | polls for the `done` marker (not herdr idle); 0=done 2=timeout 1=io-error |
| done | `drovr phase done <run> <phase>` | run by the AGENT as its final action; drops the marker `wait` polls |
| compress | `drovr phase compress <run> <phase>` | writes `<phase>-HANDOFF.md` |
| collect | `drovr collect <run> <phase>` | reads `<phase>-HANDOFF.md` |

## The HANDOFF doc shape

The compressor emits a fixed 7-section document (objective · state · decisions+rationale ·
interfaces/contracts · open questions · next step · artifact pointers). See
`HANDOFF-template.md` in this directory for the contract and what each section is for.
**Artifact pointers are paths, not pasted content** — the next agent re-reads source on
demand (that is what keeps each phase's context small). Artifact pointers now also **include
git references** — the branch and the commit range/SHAs carrying this phase's work — so the
next agent re-derives load-bearing decisions from `git log`/`git diff`, using history as a
durable cross-check against lossy compression rather than trusting the summary alone.

## Self-review before you compress

If the phase produced an artifact (code, a spec, a plan), the phase agent should have
**reviewed its own work with read-only review subagents** and addressed their
Critical/Important findings *before* you compress and hand forward. Read-only reviewers
preserve the single-writer rule — they find, the phase agent fixes. Compressing an
unreviewed artifact just carries its defects into the next phase's briefing. (The
`drovr:pipeline` phase-prompts encode this per phase.)

## Stop conditions — a bad handoff cascades

If any of these happen, **STOP and surface diagnostics** rather than seeding the next phase
with a broken briefing:

- `drovr phase wait` exits `1` (failed) or keeps exiting `2` (never completes).
- `drovr phase compress` errors, or `drovr collect` returns empty / missing the fixed
  sections.

Diagnose with `herdr agent read <pane>` (raw transcript), or `drovr attach <run>` to inspect
the pane live.
Seeding a fresh agent from a garbage handoff wastes the whole downstream chain — a broken
briefing is worse than a stopped run.

## Common mistakes

| Mistake | Fix |
|---|---|
| Skipping step 2 ("start seeds it") | It doesn't. The fresh agent sits idle until you `phase send`. |
| Backgrounding `drovr phase wait` | Block on it in the foreground — it's the sync primitive; the pane agent's work is invisible to your harness, so nothing else blocks you on it. |
| Letting the finishing agent write its own summary | Use `drovr phase compress` — a fresh reader, not self-summary. |
| Pasting file contents into the handoff | Use artifact pointers (paths); the successor re-reads. |
| Hardcoding `<phase>-HANDOFF.md` elsewhere | Read it via `drovr collect`; write it only via `drovr phase compress`. |
