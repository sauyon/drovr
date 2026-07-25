---
name: handoff
description: Use when finished work in one agent context must be carried across a phase boundary to a fresh clean-context agent
---

# Handoff

## Overview

A **handoff** is the atomic drovr boundary: run one phase agent, have it compress its own
work into a `<phase>-HANDOFF.md`, then seed the next fresh agent with that doc. This is the
building block `drovr:pipeline` loops over; use it directly for a single boundary.

**The load-bearing move is the finishing agent authoring its own handoff, in-context, as its
final action.** It has the *entire* session and knows what is load-bearing, so it writes the
7-section briefing itself and then signals done — `drovr phase done` refuses until that file
exists. Preserve decisions + interfaces, drop narration.

> This reverses drovr's earlier "fresh reader, never self-summary" design. That design feared
> *context rot* (Chroma: output degrades as the window fills), but the fresh reader could only
> see a lossy pane snapshot via `herdr agent read` — so it summarized the wrong thing (e.g.
> regurgitating the injected seed instead of the phase's real artifact; see
> `docs/known-issues.md`). A context-aware author of the *complete* session beats a neutral
> reader of an *incomplete* one. Context rot is mitigated where it matters: compression is
> recall + selection (rot-resistant), it can happen at the fullness threshold rather than the
> ceiling, and git pointers make the handoff an index into re-derivable truth, not the sole
> truth. Compaction is one of drovr's three context-engineering levers (with note-taking/git
> and sub-agents — see `drovr:using-drovr`); this skill is the compaction lever.

## The four steps

Assume a run exists (`drovr new <run> --task "..."` — see `drovr:using-drovr` for setup).
Run name is `<run>`; the phase you are running is `<phase>`.

1. **Start the phase.** Seed = the PRIOR phase's handoff (omit `--seed` for the first phase).
   ```
   drovr phase start <run> <phase> --seed ~/.local/share/drovr/runs/<run>/<prev>-HANDOFF.md
   ```
   This spawns a **plain `claude`** and records the seed path in `state.json`. It does
   **not** inject anything into the agent yet.

2. **Inject the briefing + completion contract — THIS IS REQUIRED.** The CLI does not seed the
   agent; you do. Compose the first message = the phase's instructions + the prior handoff
   content, and send it:
   ```
   drovr collect <run> <prev>                          # prints <prev>-HANDOFF.md
   drovr phase send <run> <phase> "<instructions>\n\n<pasted handoff content>"
   ```
   `drovr phase send` writes the text and submits it. For the first phase, the briefing is the
   task itself, not a prior handoff.

   The instructions you inject MUST end with the **completion contract**, whose final two
   actions are, in order:
   1. **Author the handoff.** Write `~/.local/share/drovr/runs/<run>/<phase>-HANDOFF.md` — the
      fixed 7-section document (see `HANDOFF-template.md`), compressed from your own context,
      **git pointers mandatory**. This is *your* job as the finishing agent; nothing compresses
      it for you.
   2. **Signal done:** run `drovr phase done <run> <phase>`. It **refuses with an error until
      the handoff above exists and is non-empty** — the handoff and the done-marker are one
      atomic completion step.

   Also tell the agent that any review subagents it launches must run in the **foreground**
   (never `run_in_background`, never yield waiting on them), so step 3 can detect completion.

3. **Wait for done.**
   ```
   drovr phase wait <run> <phase> --timeout-ms 600000
   ```
   This POLLS the filesystem for the marker the agent drops via `drovr phase done` (step 2's
   final action). It deliberately does NOT watch herdr's agent status: `idle` is not a
   completion signal — it also fires while the agent is parked awaiting its own subagent. Exit
   `0` = done · `2` = still running (timed out; wait again or investigate) · `1` = an I/O error.
   Use a generous timeout — real phases take minutes.

4. **Collect + hand forward.**
   ```
   drovr collect <run> <phase>
   ```
   Prints the handoff (the file the agent authored). It becomes the `--seed` for the next
   phase's step 1, and the pasted content for its step 2.

## Quick reference

| Step | Command | Note |
|---|---|---|
| start | `drovr phase start <run> <phase> [--seed <path>]` | plain claude; records seed path only |
| **inject** | `drovr phase send <run> <phase> "<text>"` | **you must do this — CLI won't**; end the text with the completion contract (author handoff → `phase done`) |
| wait | `drovr phase wait <run> <phase> --timeout-ms <ms>` | polls for the `done` marker (not herdr idle); 0=done 2=timeout 1=io-error |
| done | `drovr phase done <run> <phase>` | run by the AGENT as its final action; **refuses until `<phase>-HANDOFF.md` exists**; drops the marker `wait` polls |
| collect | `drovr collect <run> <phase>` | reads `<phase>-HANDOFF.md` |

## The HANDOFF doc shape

The finishing agent authors a fixed 7-section document (objective · state · decisions+rationale ·
interfaces/contracts · open questions · next step · artifact pointers). See
`HANDOFF-template.md` in this directory for the contract and what each section is for.
**Artifact pointers are paths, not pasted content** — the next agent re-reads source on
demand (that is what keeps each phase's context small). Artifact pointers **must include git
references** — the branch and the commit range/SHAs carrying this phase's work — so the next
agent re-derives load-bearing decisions from `git log`/`git diff`, using history as a durable
cross-check against lossy compression rather than trusting the summary alone.

## Self-review before you author the handoff

If the phase produced an artifact (code, a spec, a plan), the phase agent should have
**reviewed its own work with read-only review subagents** and addressed their
Critical/Important findings *before* authoring the handoff and handing forward. Read-only
reviewers preserve the single-writer rule — they find, the phase agent fixes. Compressing an
unreviewed artifact just carries its defects into the next phase's briefing. (The
`drovr:pipeline` phase-prompts encode this per phase.)

## Stop conditions — a bad handoff cascades

If any of these happen, **STOP and surface diagnostics** rather than seeding the next phase
with a broken briefing:

- `drovr phase wait` exits `1` (failed) or keeps exiting `2` (never completes).
- `drovr phase done` keeps failing because the agent never authored the handoff, or
  `drovr collect` returns empty / missing the fixed sections.

Diagnose with `herdr agent read <pane>` (raw transcript), or `drovr attach <run>` to inspect
the pane live.
Seeding a fresh agent from a garbage handoff wastes the whole downstream chain — a broken
briefing is worse than a stopped run.

## Common mistakes

| Mistake | Fix |
|---|---|
| Skipping step 2 ("start seeds it") | It doesn't. The fresh agent sits idle until you `phase send`. |
| Expecting a separate compress step | There isn't one. The finishing agent authors the handoff itself, as its final action, before `phase done`. |
| `phase done` failing "handoff missing" | The agent must author `<phase>-HANDOFF.md` *before* running `phase done`; the marker won't drop without it. |
| Pasting file contents into the handoff | Use artifact pointers (paths + git refs); the successor re-reads. |
| Hardcoding `<phase>-HANDOFF.md` elsewhere | Read it via `drovr collect`. |
