---
name: using-drovr
description: Use at the start of every session as the always-on router — establishes the single-writer discipline, points to the right drovr:* methodology skill for the task, and defines when to escalate a task into its own phase
---

# Using Drovr

<SUBAGENT-STOP>
If you were dispatched as a subagent to execute a specific task, ignore this
reflex — it is for the human-facing agent, not for you. Do your task.
</SUBAGENT-STOP>

## What this is

This is your **always-on router**. Drovr is a working **discipline**, not just an
orchestration engine, and it is your default operating mode. This skill does not
do the work — it tells you *how* to work and *when* a task has outgrown one
context and must become its own phase.

Drovr exists because model output degrades as a context window fills (Chroma's
*context rot*, trychroma.com/research/context-rot). A fresh, tight context beats
a long one. Everything below serves that.

## The principle (always)

<!-- reflex:section:single-writer -->
**Single writer, read-only explorers.** One agent edits at a time. Fan-out
investigation goes to read-only explorers (e.g. `explore-mcp`), never to parallel
writers. This holds whether you stay inline or escalate to phases.
<!-- /reflex:section:single-writer -->

<!-- reflex:section:always-review -->
**Always review.** Any change you write gets reviewed before you call it done —
invoke **`drovr:code-review`** (read-only review subagents; they find, you fix).
This is not optional and not gated on the change's size; it runs whether you
worked inline or across phases.
<!-- /reflex:section:always-review -->

<!-- reflex:section:methodology -->
## For the task in front of you, apply the right methodology skill

Pick by what you are doing — invoke it via the `Skill` tool before you act:

- Implementing a feature or bugfix → **`drovr:tdd`** (test-first, watch it fail).
- Chasing a bug, test failure, or unexpected behavior → **`drovr:systematic-debugging`**.
- About to claim something is done/fixed/passing → **`drovr:verification-before-completion`**.
- Reviewing an artifact (code, spec, plan) → **`drovr:code-review`**. Per the
  *Always review* rule above, this runs on every change — not only before shipping.
<!-- /reflex:section:methodology -->

<!-- reflex:section:escalation -->
## Escalation contract — inline first

**Default: do the work inline in this session.** Do not reach for phases for a
task that fits. Escalate only when a single context cannot hold the work well.

- **Primary signal — context fullness.** If the transcript is filling to the
  point where output would rot, escalate.
- **Secondary heuristic** (a proxy for the above, not a hard rule): roughly
  **10+ files** touched, or **3+ independent work items**.

When you must escalate, pick the smallest tool that fits:

- **Mid-flight escape hatch — author a HANDOFF yourself.** You are deep in a task
  and this context is filling. You have the whole session in context, so compress
  it *yourself*: write a `HANDOFF.md` (the 7-section shape from `drovr:handoff`,
  git pointers included), then continue in a fresh agent seeded from it. Nothing
  compresses it for you — the finishing agent always authors its own handoff.
- **A single planned boundary → `drovr:handoff`.** Hand finished work across one
  phase boundary to a fresh clean-context agent.
- **A full gated run → `drovr:pipeline`.** Brainstorm → plan → implement → review
  with a human approval gate on `spec.md` before any code is written.

**Putting *any* spec/design in front of a human for approval — even a one-off, not a
full pipeline — goes through the review gate, and the gate has two halves you must run
together:** `drovr review summary <run> "<what changed>"` to present `spec.md`, **and** a
backgrounded `drovr review wait <run>` — the *watch*. (The server itself is global and
always-on: `drovr serve` takes **no** run argument and auto-starts on demand, so serving is not
a step you perform per run.) `summary` opens the gate and prints the reviewer's page URL plus
the exact watch command; it does **not** start the watch. Without the watch you never learn the
decision and fall back to hand-polling `GET /state` (the anti-pattern).
`review wait` blocks on the gate and the harness wakes you when the reviewer acts
(exit `0` approved · `3` request-changes + `feedback.json` · `5` cancelled — the human
abandoned the run, stop · `2` timeout, re-run · `1` error, which is **never** an approval).
Revise loop: edit `spec.md` →
`drovr review summary <run> "<what changed>"` → re-background `review wait`. Full mechanics
(state machine, diff baselining, gotchas) live in `drovr:pipeline` → "The spec gate".

**REQUIRED BACKGROUND:** the downstream skills assume this file's contracts —
single-writer rule, the run dir at `~/.local/share/drovr/runs/<name>/`, and that
`drovr phase start` spawns a plain `claude` and does **not** inject the seed
(injecting the briefing is the skill's job, via `drovr phase send`).
<!-- /reflex:section:escalation -->
