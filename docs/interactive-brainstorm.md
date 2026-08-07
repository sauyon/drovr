# Interactive brainstorm: the ask channel, and shorter specs

**Status:** approved design (spec gate turn 3, 2026-08-06). T1–T3 implemented.

Reconstructed into the repo after the original `spec.md` was destroyed with the run directory —
see `known-issues.md`, *"`cargo test` deleted the real `~/.local/share/drovr`, twice"*. The
approved text lived only in the run dir and is gone; this is the design as approved, rewritten
from the driver's context. `plan.md`'s 10 tasks are lost with it, but T1–T3 are committed and
their handoffs describe what they built.

## Problem

Two faults, one cause.

**The brainstorm phase cannot ask a question before it commits.** The prompt sends the agent
from investigation straight to writing `spec.md`, and states that its only human channel is the
review gate. Every ambiguity is therefore *guessed and written down* rather than *asked*.

**Specs are far too long** — 230–791 lines across real runs (`skill-stickiness` 791,
`tiered-review` 463, `tui-dc-picker` 414). That is downstream of the first fault: with no
question channel, the spec is where every alternative and open question must be parked, because
parking it there is the only way to put it in front of a human.

## Locked decisions

1. The **brainstorm phase** conducts the interview. The driver stays out — the Q&A must not
   enter the driver's context.
2. The human answers **in the review web UI**. Each question carries the context needed to
   answer it; nothing is assumed from a chat transcript.
3. **Never block.** A blocking call dies on a timeout when the human is away, and a dead call
   loses the question. Post and wait are split, and the question is durable on disk.
4. **CLI, not MCP.** Phase agents already drive drovr through its CLI. Once post/wait is split,
   an MCP layer buys only a typed schema.
5. **`questions.json` is replaced**, not kept alongside. One question channel.
6. **A spec never carries open questions.** They are resolved through the ask channel *before*
   the spec reaches the gate. Deliberately **not** machine-checked: a heading regex is renamed
   trivially, so it would report compliance it cannot verify, and pairing an authoritative rule
   with a heuristic backstop buys false confidence.
7. **The ask channel is for every phase** — plan, implement and review, not just brainstorm —
   and each phase prompt must explicitly instruct the agent to ask when **new information is
   discovered** or **a question is found**. `review-angle.md` is the one exclusion: it briefs
   read-only panel reviewers that report through a findings file and never write.
8. **Retention in the spec-length A/B is a gate, not a rubric.**

## The ask channel

```
drovr ask <run> --question <text|@file> [--context <text>] [--context-file <path>]
                [--option <value>=<label>]... [--recommend <value>]
drovr ask wait <run> [--timeout-ms N]
```

`ask` appends a pending record, prints the ask-id and the reviewer's page URL, and **exits
immediately**. `ask wait` is backgrounded; exit `0` answered (answer JSON on stdout), `2`
timeout, `5` run cancelled, `1` io error. **A timeout costs nothing** — the question is still on
disk and on screen, so the caller re-arms. This reuses the `review summary` / `review wait`
contract rather than reinventing it.

`<run_dir>/interview.jsonl` is **append-only**; one JSON object per line. Append-only is
load-bearing, not an implementation detail: it is what makes an N-round interview recoverable,
and it is the defect `feedback.json` has today (see `known-issues.md`, *"`feedback.json` is
overwritten every turn"*).

Available **at any time**, including after `spec.md` exists — a reviewer's annotation can itself
be unclear, and the agent must be able to ask what a note meant rather than guess. Cancelling a
run terminates a pending `ask wait` with exit `5`.

Server: `GET interview` and `POST answer`; `GET questions` is deleted. `POST answer` appends,
and does not touch `review.state.json`. UI: an interview panel replaces the questions panel and
`renderQuestions()`.

Retires `known-issues.md`, *"Review-server Submit button does nothing when `questions.json` is
not a bare array"* — by deletion rather than by fix.

## The spec-authoring change

The instruction becomes a **decision record**: what was decided, the interfaces it binds, what is
out of scope. Alternatives and rationale live in the interview log, not the spec. The
"open questions" section is deleted outright, per decision 6.

## The spec-length A/B

Run under `drovr:writing-skills`. Arms are the current text plus candidate rewrites; fixtures are
real run tasks whose specs already exist, so the control length is measured rather than guessed.

**Retention is a gate.** A rubric cannot carry this — a scorer will call a spec complete while a
decision has quietly vanished. So:

1. **A frozen key-point ledger.** Before any candidate text is written, an independent subagent
   enumerates every decision, interface, constraint and scope boundary from each fixture's
   *control* spec. Frozen and hashed, so it cannot be trimmed afterwards to flatter an arm.
   Derived from the control, never from a shortened spec, or the experiment grades itself.
2. **Per-item binary checks, blind.** N hard questions instead of one soft verdict.
3. **100% retention or elimination.** Length is compared only among arms that lose nothing.
4. **A transmission test.** A fresh agent reconstructs the decisions from an arm's spec alone;
   a point it cannot recover was lost in practice even if a text search finds it.
5. **Adversarial gap-finding.** Questions an implementer would still have to ask.

3–5 are each independently fatal, so "no arm is shorter without loss" is a valid result.

**Ordering is load-bearing:** the freeze precedes every candidate arm. Implemented in T1 and
enforced by `freeze_precedes_every_candidate_arm`, with `descends_from_separates_an_ordered_arm_from_a_pre_freeze_one`
and `freeze_rows_still_hash_to_their_files` alongside it. As frozen: 3 fixtures, **233 ledger
rows** (92 / 85 / 56), control arm `S0`.

`S0` is a measurement baseline and **never ships** — it mandates the very "open questions"
section decision 6 forbids.

## Implemented so far

| task | state |
| --- | --- |
| T1 — freeze the A/B | done; 7 commits; 3 guards green |
| T2 — `cli/src/interview.rs` | done; append-only log + fold; 26 tests |
| T3 — `drovr ask` / `ask wait` | done; see `implement-task-3-HANDOFF.md` |

Two interfaces drifted from the plan during T3 and bind for everything downstream:
`--context <text>` + `--context-file <path>` (matching every other drovr command, rather than
the plan's `--context <path>`), and `ask wait` with nothing pending printing the folded
interview rather than a bare `[]`.
