# Interactive brainstorm: the ask channel, and shorter specs

**Status:** approved design (spec gate turn 3, 2026-08-06). **T1–T9 implemented** — the whole
ask channel ships. The spec-length A/B was deliberately out of scope for this run and **has since
been run separately, ending in a null attributable to its measuring instrument** — see
`docs/skill-evidence/spec-length.md`, and *"What was not built"* at the end.

Reconstructed into the repo after the original `spec.md` was destroyed with the run directory —
see `known-issues.md`, *"`cargo test` deleted the real `~/.local/share/drovr`, twice"*. The
approved text lived only in the run dir and is gone; this is the design as approved, rewritten
from the driver's context. The original plan's 10 tasks went with it and were **not**
reconstructed: the work was re-decomposed into **T1–T9** under a replacement plan, so the task
numbers here do not correspond to the lost T4/T5/T6/T7/T10.

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
drovr ask <run> --question <text|@file> [--context <text> | --context-file <path>]
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

Retired `known-issues.md`'s *"Review-server Submit button does nothing when `questions.json` is
not a bare array"* — **by deletion rather than by fix**, so that heading is gone from
`known-issues.md` and searching for it will find nothing. The schema cannot recur because the
schema no longer exists.

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
and `freeze_rows_still_hash_to_their_files` alongside it. As frozen: 3 fixtures, **230 ledger
rows** (91 / 84 / 55), control arm `S0`. Those are the ledgers' own `**Closed list: N rows.**`
declarations, which are authoritative; counting table rows gives 233 because it picks up each
file's header.

`S0` is a measurement baseline and **never ships** — it mandates the very "open questions"
section decision 6 forbids.

## Implemented so far

All nine tasks are done. Each row's detail is in its `implement-task-<n>-HANDOFF.md` and
`task<n>-report.md` in the run dir.

| task | state |
| --- | --- |
| T1 — freeze the A/B | done; 7 commits; 3 guards green |
| T2 — `cli/src/interview.rs` | done; append-only log + fold; 26 tests |
| T3 — `drovr ask` / `ask wait` | done; post and wait split, never blocking |
| T4 — server routes | done; `GET interview` + `POST answer` live, `GET questions` returns 404 |
| T5 — UI: the interview panel | done; one pending ask on screen behind a `1 of N` counter |
| T6 — UI: retire the questions panel | done; panel and `renderQuestions()` deleted, keyboard cursor retargeted onto the answer rows |
| T7 — the ask directive in the phase prompts | done; every writer phase prompt carries it, `review-angle.md` excluded per decision 7, both halves test-pinned |
| T8 — rewrite `brainstorm.md` | done; investigate → interview → decision-record spec → gate; no `questions.json`, no "open questions" |
| T9 — docs | done; this document, `README.md`, `known-issues.md` (+ triage), `skills/pipeline` and `skills/handoff` |

### Interfaces that drifted from the plan

These bind, and the plan's text does not:

1. **`--context <text>` and `--context-file <path>`**, mutually exclusive — matching every other
   drovr command, rather than the plan's single `--context <path>`. (T3.)
2. **`ask wait` with nothing pending prints the folded interview**, not a bare `[]`. (T3.) This
   closes a re-arm race: a `[]` would pair exit `0` ("answered") with no answer when the human
   answers in the seconds between a timeout and the caller re-arming.

## What was not built

Two things a reader will reasonably expect to find and will not:

1. **The spec-length A/B was deliberately out of scope for this run** — no *candidate* arm was
   written (only the control `S0`, which T1 froze), no fixture scored, no outcome applied. T1 of
   this run is what created the frozen ledger under `docs/skill-evidence/spec-length/`
   (3 fixtures, 230 ledger rows, control arm `S0`).

   **It has since been run, and the answer is a null about the instrument rather than about spec
   length — see `docs/skill-evidence/spec-length.md`.** That run re-baselined the control onto
   `S1`, the shipped step 4, rather than the historical `S0`; froze two candidate arms; and
   generated 18 specs. Retention scoring then failed its own pre-registered relevance
   adjudication in every file it produced, so **no arm has a defined retention count and none
   could have cleared the gate, the control included**. Nothing shipped: step 4 is untouched, and
   `spec_length_step_4_is_still_the_frozen_control_arm` pins that. A redesigned instrument,
   pre-registered and re-frozen before any new arm is measured, is legitimate follow-up work; the
   18 generated specs are frozen and re-scorable, so it would not have to start over.
2. **The review page cannot show an *answered* interview.** The panel renders only the pending
   ask and empties itself once nothing is pending; `interview.jsonl` is on disk and served at
   `GET /api/runs/<run>/interview`, but a human reviewing the spec never sees the Q&A that
   produced it. This is why `brainstorm.md` requires the spec to stand on its own for a reviewer
   who was not in the interview, rather than deferring to the log. Rendering the folded log for
   the reviewer is a real, unowned follow-up.

**One cost, stated plainly.** The run's goal was shorter *specs*, and `brainstorm.md` itself got
longer: **103 lines on `main` → 160 here.** Ten of those 57 came with `drovr/skill-stickiness`'s
`4920987`, which the unlanded branch this run sits on (`drovr/brainstorm-rework`) carries;
`brainstorm.md` was 113 lines there. The other
**47 are this run's own** — T7 added the ask directive
(113 → 135) and T8 the interview loop and decision-record framing (135 → 160). A longer prompt
costs context in **every** brainstorm phase, forever, and that is a real price paid up front
against a spec-length saving that has not yet been measured.

**The A/B in (1) has now run, and it did not settle this.** It ended in a null attributable to the
measuring instrument — no arm, control included, has a defined retention count — so the trade is
**still unmeasured**, and the 57 lines are still being paid. That is a weaker statement than "the
trade did not pay", and deliberately so: nothing in that run says a shorter step 4 would have lost
key points, and nothing says it would not.
