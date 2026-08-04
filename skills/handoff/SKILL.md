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

1. **Start the phase — briefed.** drovr composes the brief (frame, scope, completion
   contract) and injects it; your `--context` is the part it cannot know, which across a
   handoff boundary is the PRIOR phase's handoff:
   ```
   drovr collect <run> <prev> > /tmp/<prev>-handoff.md   # prints <prev>-HANDOFF.md
   drovr phase start <run> <phase> --context-file /tmp/<prev>-handoff.md
   ```
   For the first phase there is no prior handoff — omit `--context` and the run's task
   carries it. Read exactly what the agent will be told with
   `drovr phase brief <run> <phase>`; do not retype or paraphrase it.

2. **Check it landed.** `phase start` prints `briefed phase '<phase>'` on success. If it
   exits 2 the pane is up but UNBRIEFED — re-deliver once the agent is at its composer:
   ```
   drovr phase brief <run> <phase> --context-file /tmp/<prev>-handoff.md \
     | drovr phase send <run> <phase> -
   ```
   `phase send` also stays available for free-form mid-flight nudges; a message is not a
   brief, and you never author the frame.

   The composed brief carries the **completion contract**, whose final two
   actions are, in order:
   1. **Author the handoff.** Write `~/.local/share/drovr/runs/<run>/<phase>-HANDOFF.md` — the
      fixed 7-section document (see `HANDOFF-template.md`), compressed from your own context,
      **git pointers mandatory**. This is *your* job as the finishing agent; nothing compresses
      it for you.
   2. **Signal done:** run `drovr phase done <run> <phase>`. It **refuses with an error until
      the handoff above exists, is non-empty, and has no section left at the scaffold's
      `TODO`** — the handoff and the done-marker are one atomic completion step, and a form
      still holding its placeholders carries nothing for the next phase to inherit. The
      error names the unfilled sections.

   Also tell the agent that any review subagents it launches must run in the **foreground**
   (never `run_in_background`, never yield waiting on them), so step 3 can detect completion.

3. **Wait for done — background the wait, then end your turn.**
   ```
   drovr phase wait <run> <phase> --timeout-ms 3600000     # Bash run_in_background: true
   ```
   Then **stop and end the turn immediately**: no other command, no edits, no scheduled
   wakeup. The harness wakes you with the exit code when the process exits — that notification
   *is* your synchronization signal. `drovr phase wait` is still the sync primitive; the only
   change is that the harness, not the Bash call, holds the block.

   **Why not foreground?** A foreground Bash call is hard-capped at **600 000 ms (10 min)** —
   you cannot ask for more. Real phases routinely run longer, so a foreground wait does not
   block until the phase finishes: it dies at the cap and reports exit `2` on a phase that is
   still running healthily. You then re-run it, burning a turn and a fresh context read every
   10 minutes. Backgrounding removes the cap — one wait, one wake-up, when the phase is
   actually done. It is also the only way a `--timeout-ms` above 600 000 is reachable at all.

   **The rule the old "always foreground" advice was protecting still holds**, and it is the
   part that matters: *never do your own work while a phase agent is writing.* That is drovr's
   single-writer rule, not a property of foreground-ness. Backgrounding is only dangerous if
   you keep working — so background the wait and go idle.

   This POLLS the filesystem for the marker the agent drops via `drovr phase done` (step 2's
   final action). Completion is that marker, never herdr's `idle` status — `idle` also fires
   while the agent is parked awaiting its own subagent. **Note the default `--timeout-ms` is
   30 000 (30 s), which is far too short for a real phase; always pass an explicit value.**

   Handle the exit code the wake-up hands you:

   | Exit | Meaning | What to do |
   |---|---|---|
   | `0` | done — the handoff exists | Go to step 4. |
   | `4` | **blocked** — the agent hit a safety/permission prompt; the diagnostic names the class | Answer the prompt (`herdr agent send-keys <pane> …`), then **re-arm**. Do not treat this as failure. |
   | `2` | timed out; the phase may still be running healthily | **Re-arm**, or investigate if it has now timed out twice. |
   | `5` | **superseded** — a newer `phase start` re-entered the phase while this wait ran, so it was watching a pass that no longer exists | Nothing is wrong with the phase. **Re-arm** to follow the live pass. Never triage this as a stuck agent. |
   | `1` | I/O error | STOP — see *Stop conditions*. |

   **Re-arm** = run the exact same backgrounded `drovr phase wait` again and end the turn again.
   The wait is stateless and resumable: it polls an on-disk marker, so nothing is lost by
   re-issuing it and a phase that finished during the gap is detected immediately.

4. **Collect + hand forward.**
   ```
   drovr collect <run> <phase>
   ```
   Prints the handoff (the file the agent authored). It becomes the next phase's
   `--context-file` in step 1 — drovr puts it in the brief it composes.

## Quick reference

| Step | Command | Note |
|---|---|---|
| **start (briefed)** | `drovr phase start <run> <phase> --context-file <prev handoff>` | composes the brief AND injects it; prints `briefed phase …`. Exit 2 = pane up but unbriefed |
| inspect | `drovr phase brief <run> <phase> [--context …]` | prints exactly what the agent is told, spawning nothing |
| re-brief | `drovr phase brief … \| drovr phase send <run> <phase> -` | for a phase already running; `phase send "<text>"` is for free-form nudges only |
| wait | `drovr phase wait <run> <phase> --timeout-ms <ms>` | **run backgrounded, then end the turn**; polls for the `done` marker (not herdr idle). `0`=done → step 4 · `4`=blocked on a prompt → answer it, re-arm · `2`=timeout → re-arm · `5`=superseded by a newer pass → re-arm, not a stuck agent · `1`=io-error → stop. Default timeout is only 30 s — always override. Foreground Bash caps at 600 000 ms, so a foreground wait times out on healthy long phases |
| done | `drovr phase done <run> <phase>` | run by the AGENT as its final action; **refuses until `<phase>-HANDOFF.md` exists**; drops the marker `wait` polls |
| scaffold | `drovr handoff-scaffold <run> <phase>` | writes the empty 7 sections for the AGENT to fill; refuses to overwrite an authored one. Structure only — drovr does not guess which commits are yours. `phase done` refuses while any section is still `TODO` |
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

**Not a stop condition: `phase done` refusing with `$DROVR_PASS is not set`.** The marker must
carry the pass token the agent was launched under, so `drovr phase done` only works from inside
the phase's own pane. Run by hand from a plain shell it refuses — and prints the exact command
that would work, token included. The general form, for when you are composing it yourself:

```
DROVR_PASS=$(jq -r '.phases[]|select(.name=="<phase>").pass' <run_dir>/state.json) \
  drovr phase done <run> <phase>
```

`<run_dir>` is `~/.local/share/drovr/runs/<run>`; a reviewer phase lives under
`.review_phases[]` instead. This is the escape hatch for a phase whose agent is gone but whose
work is finished — it bypasses the pane, not the contract: `<phase>-HANDOFF.md` must still
exist, with no `TODO` sections left.

## Common mistakes

| Mistake | Fix |
|---|---|
| Writing the brief yourself | drovr composes it. You supply `--context`; `phase brief` shows the result. A hand-written frame drifts from the contract and nothing detects it. |
| Ignoring exit 2 from `phase start` | The pane is up and UNBRIEFED. Re-deliver with `phase brief \| phase send -`; a `phase wait` on it never returns. |
| Expecting a separate compress step | There isn't one. The finishing agent authors the handoff itself, as its final action, before `phase done`. |
| `phase done` failing "handoff missing" | The agent must author `<phase>-HANDOFF.md` *before* running `phase done`; the marker won't drop without it. |
| `phase done` naming sections "still at the scaffold's TODO" | A scaffolded handoff was never filled in. Write those sections from your own context — nothing else will — then re-run. |
| Foregrounding `drovr phase wait` | Background it and end the turn. Foreground Bash is capped at 600 000 ms, so a long healthy phase reports a false exit `2`. |
| Backgrounding the wait and then working | Background the wait *and go idle*. The single-writer rule is what forbids working here, not foreground-ness. |
| Pasting file contents into the handoff | Use artifact pointers (paths + git refs); the successor re-reads. |
| Hardcoding `<phase>-HANDOFF.md` elsewhere | Read it via `drovr collect`; the finishing agent writes it directly (no CLI command authors it). |
