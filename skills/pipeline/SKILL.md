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

**REQUIRED SUB-SKILL:** every boundary uses `drovr:handoff` (start → wait → collect; the
phase agent authors its own handoff before `phase done`). This skill does not repeat those
mechanics — read it first. drovr composes each phase's briefing and injects it at
`phase start`; your job is the `--context` it cannot know.

You are the **driver** (single writer of the orchestration). The phase agents are the
single writers of their own artifacts. Read-only fan-out goes to explorers, never to
parallel writing agents.

## The flow

```
drovr new <run> --task "<goal>" [--dir <project>]     # cwd is the default project dir
```

Then, for each phase, run the `drovr:handoff` four steps, injecting the matching template
from `phase-prompts/` (combined with the prior phase's `drovr collect` output) at step 2.

| # | Phase | Injected template | Output artifact | Gate |
|---|---|---|---|---|
| 1 | brainstorm | `phase-prompts/brainstorm.md` + task | `spec.md` | **HUMAN GATE** |
| 2 | plan | `phase-prompts/plan.md` + brainstorm handoff | `plan.md` | auto (AI self-review) |
| 3 | implement | `phase-prompts/implement-task.md` per task | `task<N>-report.md` | auto |
| 4 | review | `phase-prompts/review.md` + reports + diff | `verdict.md` | auto |

Each finishing phase agent authors its own handoff before `drovr phase done`, so the next
phase is seeded from that briefing, not a raw transcript. `spec.md`, `plan.md`, reports, and
`verdict.md` all live in the run dir `~/.local/share/drovr/runs/<run>/`.

## The spec gate (after brainstorm only)

The review server is **always on** — you do not start it per run. `drovr review summary`/
`wait` auto-start it on demand (and reuse it if already up). The brainstorm agent is the
single writer of `spec.md`; you convey the reviewer's decisions. The run's page lives at
`http://127.0.0.1:8791/#/runs/<run>` (the root `/` is the session list of all runs).

1. **Nothing to start.** The agent's first `drovr review summary` brings the server up if it
   isn't already. Use a Tailscale host instead of localhost only on a trusted tailnet — there
   is no auth (`drovr serve --host <tailscale-host>` if you want to pre-bind it). **Do NOT
   hand the human the run URL yet** — the page is EMPTY until the first summary lands, and an
   empty page reads as "the tool is broken."

1b. **Wait for the first summary before announcing the URL or starting `review wait`.** The
   brainstorm agent writes `spec.md` then runs `drovr review summary`, flipping state
   `idle → ready`. Only once `spec.md` exists **and** state reports `ready` do you give the
   human the URL and start `drovr review wait`. Until then there is nothing to review and a
   `review wait` against a specless run just churns. If the agent stalls without producing a
   spec, inspect its pane (`drovr attach <run>`) — do not point the human at an empty page.
   (Background a poll on the run's state for the `ready` transition; don't busy-wait inline.)

2. **Per-run state machine** (`/api/runs/<run>/state` → `{state, turn}`; files in the run dir):

   ```
   idle ──agent: drovr review summary──▶ ready ──reviewer "request changes"──▶ waiting
     ▲                                                                            │
     └──────────────── agent revises + drovr review summary ──────────────────────┘
                          reviewer "approve"  ──▶  approved    (writes `approved` marker)
                          reviewer "cancel"   ──▶  cancelled   (writes `cancelled` marker)
   ```

   `approved` and `cancelled` are **terminal**: the server rejects a later `summary` or
   `submit` on such a run with HTTP 409, so a late revision from an agent that hasn't
   noticed yet cannot silently revive a decided run.

3. **The mandated discipline** (encoded in `brainstorm.md`): the agent writes/edits
   `spec.md` and, **after every edit, runs `drovr review summary <run> "<what changed>"`**.
   That is the only signal that flips `waiting`/`idle` → `ready`. No summary = the reviewer
   never sees the change. The agent edits the markdown; the server owns rendering and
   diffing (a real markdown parser — zero LLM drift).

4. **Wait for the reviewer — do NOT busy-poll state.** After the summary is posted,
   block on:
   ```
   drovr review wait <run>   # background it; exits when the reviewer acts
   ```
   It blocks while state is `idle`/`ready` and exits when the reviewer submits — the harness
   wakes you on the process exit. It is **resumable**: on timeout (exit 2, default 30 min)
   just re-run it, since the on-disk `approved`/`feedback.json` markers are the source of
   truth. Exit codes:

   | Exit | Meaning | Next |
   |---|---|---|
   | 0 | approved | Compress brainstorm; proceed to plan. |
   | 3 | changes requested | Forward `feedback.json` (step 5); wait again. |
   | 5 | **cancelled** by the reviewer (`cancelled` marker in the run dir) | Stop. Do not revise, do not wait again — tell the human and tear the run down (`drovr cleanup <run>`). |
   | 2 | timeout | Re-run `drovr review wait <run>`. |
   | 1 | error (server unreachable / could not auto-start) | Check `drovr serve` can run; try `drovr serve &` manually. |

   **Only exit 0 is approval.** A non-zero exit is never an approval — in particular exit 1
   means the wait *failed* (e.g. connection refused), not that the reviewer said yes. If you
   cannot read a clean 0, re-run the wait rather than proceeding.

5. **Forward feedback.** On exit 3 the reviewer's turn is in
   `~/.local/share/drovr/runs/<run>/feedback.json`:
   `{turn, decision, feedback, answers, annotations}`. Forward it to the agent:
   ```
   drovr phase send <run> brainstorm "Reviewer requested changes (see feedback.json). Revise spec.md, then run: drovr review summary <run> \"<what changed>\""
   ```
   The agent revises, calls `drovr review summary`, state → `ready`; re-run `drovr review wait`.

6. **Gate passes** when `drovr review wait` exits **0** (state `approved`; the server has
   written the `approved` marker file in the run dir). Only then let brainstorm finish
   (authoring brainstorm-HANDOFF.md as its final action) and proceed to plan.

If a reviewer submits **before** the agent's first `drovr review summary`, the server goes
straight `idle → waiting` — unusual but possible. It self-heals: the agent's next
`drovr review summary` still flips it to `ready`.

## The implement loop

`plan.md` lists tasks with per-task interfaces. Run **each task as its own fresh phase** so
context stays clean — do not reuse one long-lived agent:

```
for each task N in plan.md:
    drovr phase start <run> implement-task-<N> --context-file <ctx>   # composes the
                                                 # brief from phase-prompts/implement-task.md
                                                 # and injects it. <ctx> = task N brief from
                                                 # plan.md + accumulated interfaces so far.
                                                 # You never write the frame; inspect it with
                                                 # `drovr phase brief <run> implement-task-<N>`
    drovr phase wait  <run> implement-task-<N> --timeout-ms 3600000   # BACKGROUND it, then
                                                                      # end the turn
    # No separate compress step: the task agent authored implement-task-<N>-HANDOFF.md
    # itself as its final action (phase done refuses without it), so `wait` returning
    # done means the handoff already exists.
```

**Every blocking wait in this loop runs backgrounded** (`drovr phase wait`, and
`drovr code-review run` below) — see `drovr:handoff` step 3 for why. Foreground Bash is capped
at 600 000 ms, so a foreground wait reports a false timeout on any phase that runs past 10
minutes, and implement tasks routinely do. Background the call, end the turn, and let the
harness wake you with the exit code. **Do no work of your own while it runs** — that is the
single-writer rule, and it is the reason to go idle rather than the reason to foreground.

**Fold interfaces forward:** each task's handoff carries the interfaces it introduced;
pass those as the next task's `--context` so later tasks bind to real signatures.
`drovr phase start` appends any unseen phase name, so `implement-task-<N>` phases are created
on demand alongside the four seeded phases.

### Review each task until clean — driver-run panel

After a task's `drovr phase wait` returns done (so the agent has already authored its
handoff), and **before** starting the next task, the **driver** runs the automatic review
panel (see `drovr:code-review`). The implementer records the base at
task start — `implement-task.md` has it run `drovr code-review base <run> task-<N>` before
writing any code, so `HEAD` is the pre-task SHA. Then the driver runs the blocking panel and
branches on its exit code:

```
drovr code-review run <run> task-<N> --context "<what this task changed>"
                                              # blocking; spawns one reviewer per angle.
                                              # BACKGROUND it and end the turn — the panel
                                              # runs well past the 600 000 ms foreground cap.
case $? in
  0)  # clean — proceed to task N+1
  3)  # findings — re-enter implement, forward the review, re-run the panel (loop)
  2)  # timeout — reviewers are slow, not broken. Re-run the SAME command: it resumes
      #   the panel in flight, banks the angles already in, waits only on stragglers.
      #   Loop on 2 as freely as on 3. Never add --fresh to "unstick" it.
  1)  # error — STOP and diagnose (see Failure model)
esac
```

On **exit 3**, re-enter the implement phase for task N and forward the merged findings:

```
drovr phase send <run> implement-task-<N> "Review found changes (see <run_dir>/task-<N>-review.json). Fix every Important AND every nit, then re-author your handoff and report."
drovr phase wait <run> implement-task-<N> --timeout-ms 3600000   # backgrounded, then end turn
drovr code-review run <run> task-<N>                             # re-run the panel (same)
```

Re-entry needs **no `drovr phase start`**: `drovr phase done` only writes a marker — it never
closes the pane (panes live until `drovr cleanup`), so the task's agent is still alive and
`drovr phase send` reaches it directly. The agent drops a fresh `drovr phase done` marker when
it finishes the fix, which the following `phase wait` sees. `phase send` clears the previous
iteration's completion first, so that wait really does wait.

Loop with **impact-scaled judgement** — no hardcoded floor or ceiling on iterations. Stop when
the panel is clean *and* converged for the change's impact (a small change may need one pass; a
risky one, several), or when iteration stops converging (the same class of finding recurs
without the diff improving) — then surface it rather than looping forever. The reviewer fixes
Important **and** nits on re-entry; only critical/important block the clean gate, but a clean
task should not ship known nits it can cheaply fix.

**Single-writer invariant:** the panel is the only reviewer activity in flight, and every
reviewer exits (drops its `drovr phase done` marker) before the implementer re-enters to fix.
Never have a reviewer pane alive while the implementer writes — that breaks the single-writer
rule. `code-review run` blocks until all angles finish, so the driver naturally serializes them.

## Self-review before a phase reports done — REQUIRED

Every phase that produces an artifact (plan, and each implement task) must **review its own
work before it declares done** — do not rely on the phase agent's own judgment, and do not
wait for the final review phase. The phase agent launches one or more **read-only** review
subagents (Claude Code Agent tool, `subagent_type: general-purpose`, model `sonnet`) to
adversarially review its output, then addresses every Critical/Important finding before
finishing.

Review subagents are **read-only**, so drovr's single-writer discipline holds — they find,
the phase agent fixes. This is encoded in the phase-prompts (`implement-task.md`, `plan.md`);
as the driver, do not accept a phase as done until its report shows the self-review happened. This
is IN ADDITION to the pipeline's final review phase (step 4): self-review catches defects one
phase early, where they are cheap; the final review is the independent cross-check over the
whole change.

## Failure model — stop, don't cascade

- A phase `wait` exits `1` (failed) or never leaves `2` (timeout) → **STOP**, name the phase,
  surface `herdr agent read <pane>` diagnostics (or `drovr attach <run>` to inspect the pane
  live). Do not proceed.
- `drovr phase done` keeps refusing (agent never authored the handoff), or `drovr collect`
  yields an empty or malformed handoff → **STOP**.
- A failed implement task **halts the loop** naming that task — later tasks depend on its
  interfaces.

A bad handoff poisons every phase downstream; a stopped run is recoverable, a cascade is not.

## Common mistakes

| Mistake | Fix |
|---|---|
| Manually babysitting a per-run server | The server is always on and auto-starts on demand; just run `drovr review summary`/`wait`. |
| Announcing the URL / starting `review wait` before the first summary | Wait for `spec.md` + state = `ready`; an empty page reads as broken and a specless `review wait` churns. |
| Agent edits `spec.md` but reviewer sees nothing | Agent must run `drovr review summary` after each edit. |
| Busy-polling state for the decision | Background `drovr review wait <run>`; it exits when the reviewer acts. |
| Gating plan/implement/review | Only `spec.md` gates. The rest run unattended. |
| One agent for all implement tasks | One fresh phase per task; fold interfaces forward. |
| Proceeding past a failed/empty handoff | Stop and diagnose — never seed the next phase with garbage. |
| Skipping the review panel between tasks | Run `drovr code-review run <run> task-<N>` after each task completes; loop on exit 3. |
| Reviewer pane alive while the implementer fixes | `code-review run` blocks until all reviewers exit; only then re-enter implement. Single writer. |
| Looping the panel forever on recurring findings | Impact-scaled stop: when it stops converging, surface it — don't loop. |
| Running `phase wait` / `code-review run` in the foreground | Background them and end the turn. Foreground Bash caps at 600 000 ms, so long healthy phases report a false exit `2`. |
