---
name: pipeline
description: Use when a feature or change is too large for one session and needs a human approval gate on its spec before any code is written
---

# Pipeline

## Overview

Pipeline drives a full run through four phases — **brainstorm → plan → implement → review**
— where every boundary is a `drovr:handoff` and exactly one boundary is a **human gate**:
the reviewer must approve `spec.md` before any code is written. Everything after the gate
runs unattended unless a phase fails.

**REQUIRED SUB-SKILL:** every boundary uses `drovr:handoff` (start → inject → wait →
compress → collect). This skill does not repeat those mechanics — read it first. Injecting
each phase's briefing is **your** job; the CLI spawns a plain `claude` and seeds nothing.

You are the **driver** (single writer of the orchestration). The phase agents are the
single writers of their own artifacts. Read-only fan-out goes to explorers, never to
parallel writing agents.

## The flow

```
drovr new <run> --task "<goal>" [--dir <project>]     # cwd is the default project dir
```

Then, for each phase, run the `drovr:handoff` five steps, injecting the matching template
from `phase-prompts/` (combined with the prior phase's `drovr collect` output) at step 2.

| # | Phase | Injected template | Output artifact | Gate |
|---|---|---|---|---|
| 1 | brainstorm | `phase-prompts/brainstorm.md` + task | `spec.md` | **HUMAN GATE** |
| 2 | plan | `phase-prompts/plan.md` + brainstorm handoff | `plan.md` | auto (AI self-review) |
| 3 | implement | `phase-prompts/implement-task.md` per task | `task<N>-report.md` | auto |
| 4 | review | `phase-prompts/review.md` + reports + diff | `verdict.md` | auto |

Compress after every phase so the next one is seeded from the briefing, not a raw
transcript. `spec.md`, `plan.md`, reports, and `verdict.md` all live in the run dir
`~/.local/share/drovr/runs/<run>/`.

## The spec gate (after brainstorm only)

`drovr new` does **not** start the review server — you start it. The brainstorm agent is the
single writer of `spec.md`; you convey the reviewer's decisions.

1. **Start the server** (it blocks, so background it):
   ```
   drovr serve <run> --host 127.0.0.1 --port 8791 &
   ```
   Use a Tailscale host instead of localhost only on a trusted tailnet — there is no auth.
   Tell the human the URL.

2. **Server state machine** (`GET /state` → `{state, turn}`; files in the run dir):

   ```
   idle ──agent: drovr review summary──▶ ready ──reviewer "request changes"──▶ waiting
     ▲                                                                            │
     └──────────────── agent revises + drovr review summary ──────────────────────┘
                          reviewer "approve"  ──▶  approved   (writes `approved` marker)
   ```

3. **The mandated discipline** (encoded in `brainstorm.md`): the agent writes/edits
   `spec.md` and, **after every edit, runs `drovr review summary <run> "<what changed>"`**.
   That is the only signal that flips `waiting`/`idle` → `ready`. No summary = the reviewer
   never sees the change. The agent edits the markdown; the server owns rendering and
   diffing (a real markdown parser — zero LLM drift).

4. **Drovr feedback.** When state is `waiting`, the reviewer's turn is in
   `~/.local/share/drovr/runs/<run>/feedback.json`:
   `{turn, decision, feedback, answers, annotations}`. Forward it to the agent:
   ```
   drovr phase send <run> brainstorm "Reviewer requested changes (see feedback.json). Revise spec.md, then run: drovr review summary <run> \"<what changed>\""
   ```
   The agent revises, calls `drovr review summary`, state → `ready`, human refreshes.

5. **Gate passes** when state is `approved` (the server has written the `approved` marker
   file in the run dir). Only then compress brainstorm and proceed to plan.

If a reviewer submits **before** the agent's first `drovr review summary`, the server goes
straight `idle → waiting` — unusual but possible. It self-heals: the agent's next
`drovr review summary` still flips it to `ready`.

## The implement loop

`plan.md` lists tasks with per-task interfaces. Run **each task as its own fresh phase** so
context stays clean — do not reuse one long-lived agent:

```
for each task N in plan.md:
    drovr phase start <run> implement-task-<N> --seed <run_dir>/plan-HANDOFF.md
    drovr phase send  <run> implement-task-<N>  "<phase-prompts/implement-task.md>
                                                 + task N brief
                                                 + accumulated interfaces so far"
    drovr phase wait  <run> implement-task-<N> --timeout-ms 900000
    drovr phase compress <run> implement-task-<N>     # writes implement-task-<N>-HANDOFF.md
```

**Fold interfaces forward:** each task's handoff carries the interfaces it introduced;
include those in the next task's injected briefing so later tasks bind to real signatures.
`drovr phase start` appends any unseen phase name, so `implement-task-<N>` phases are created
on demand alongside the four seeded phases.

## Self-review before a phase reports done — REQUIRED

Every phase that produces an artifact (plan, and each implement task) must **review its own
work before it declares done** — do not rely on the phase agent's own judgment, and do not
wait for the final review phase. The phase agent launches one or more **read-only** review
subagents (Claude Code Agent tool, `subagent_type: general-purpose`, model `sonnet`) to
adversarially review its output, then addresses every Critical/Important finding before
finishing.

Review subagents are **read-only**, so drovr's single-writer discipline holds — they find,
the phase agent fixes. This is encoded in the phase-prompts (`implement-task.md`, `plan.md`);
as the driver, do not compress a phase until its report shows the self-review happened. This
is IN ADDITION to the pipeline's final review phase (step 4): self-review catches defects one
phase early, where they are cheap; the final review is the independent cross-check over the
whole change.

## Failure model — stop, don't cascade

- A phase `wait` exits `1` (failed) or never leaves `2` (timeout) → **STOP**, name the phase,
  surface `herdr agent read <pane>` diagnostics (or `drovr attach <run>` to inspect the pane
  live). Do not compress or proceed.
- `drovr phase compress`/`drovr collect` yields an empty or malformed handoff → **STOP**.
- A failed implement task **halts the loop** naming that task — later tasks depend on its
  interfaces.

A bad handoff poisons every phase downstream; a stopped run is recoverable, a cascade is not.

## Common mistakes

| Mistake | Fix |
|---|---|
| Expecting `drovr new` to serve the gate | Start `drovr serve <run> &` yourself before the gate. |
| Agent edits `spec.md` but reviewer sees nothing | Agent must run `drovr review summary` after each edit. |
| Gating plan/implement/review | Only `spec.md` gates. The rest run unattended. |
| One agent for all implement tasks | One fresh phase per task; fold interfaces forward. |
| Proceeding past a failed/empty handoff | Stop and diagnose — never seed the next phase with garbage. |
