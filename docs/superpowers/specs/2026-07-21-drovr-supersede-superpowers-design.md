# drovr v2 — Supersede the superpowers paradigm

**Date:** 2026-07-21
**Status:** Design — awaiting review
**Author:** Sauyon Lee (with Claude)

## Problem

`drovr` today is a *big-task orchestrator*: a Rust CLI plus three skills
(`using-drovr`, `handoff`, `pipeline`) that run a change through fresh-context
phases (brainstorm → plan → implement → review) connected by compressed HANDOFF
docs, with a human approval gate on `spec.md`. It only earns its keep when a task
is too large for one context; a quick bugfix gets nothing from it.

`superpowers` is the opposite shape: an *always-on, in-session discipline
library*. Its value is two things drovr lacks:

1. **An always-on reflex** — `using-superpowers` is injected at every session
   start and forces the agent to reach for the right discipline before any
   action, at any task size.
2. **A reusable methodology library** — ~15 skills (TDD, systematic-debugging,
   verification-before-completion, code-review, etc.) that apply independent of
   task size.

drovr already re-implements brainstorm/plan/review as **phase prompts**, but the
methodology superpowers owns lives in drovr only as *prose buried inside those
phase prompts* — so it fires only during a full pipeline, never for inline work.

**Goal:** make drovr *supersede* superpowers — become the default operating mode
for all work — by growing the two things it lacks, while keeping the edge
superpowers has no answer for (fresh-context phases + compressed handoffs).

## Evidence base

This design is grounded in a verified research pass (22 primary-sourced claims,
each 3-0 adversarially verified). Load-bearing findings:

- **Fresh/compacted context beats a growing transcript.** Chroma (18 models):
  reliability degrades as input grows *even on trivial tasks*, and *a single
  irrelevant distractor* drops accuracy below baseline. Carrying the full
  transcript is measurably harmful, not merely wasteful.
  [trychroma.com/research/context-rot; anthropic.com/engineering/effective-context-engineering-for-ai-agents]
- **Bounded per-phase scope is the fix Anthropic converged on.** Their
  long-running-agent harness failed by "trying to do too much at once … running
  out of context mid-implementation"; the remedy was "one feature at a time."
  [anthropic.com/engineering/effective-harnesses-for-long-running-agents]
- **A fresh phase is genuinely blind.** Claude subagents inherit *only the prompt
  string* — no history, no tool results, no parent system prompt — so the handoff
  must be self-sufficient (exact paths, errors, decisions). Distilled summaries
  target ~1–2k tokens. [platform.claude.com/docs/en/agent-sdk/subagents]
- **Single writer, no parallel writers.** Cognition/Devin: "actions carry
  implicit decisions; conflicting decisions carry bad results."
  [cognition.com/blog/dont-build-multi-agents]
- **Escalate on context-fullness, not task decomposition.** Real tools trigger
  compaction on a fullness signal (Claude Code ~95% capacity; Anthropic
  server-side default 150k input tokens; Cline "approaching the limit"). Anthropic
  explicitly warns *against* a phase relay for sequential-dependent or same-file
  work. Concrete escalation heuristic: **10+ files or 3+ independent work items.**
  [docs.cline.bot/features/auto-compact; claude.com/blog/subagents-in-claude-code]
- **Methodology is best encoded as progressively-disclosed skills**
  (~50 tok metadata → ~500 tok SKILL.md on demand → refs only when needed).
  [claude.com/blog/building-agents-with-skills-equipping-agents-for-specialized-work]
- **The reflex's efficacy is unproven either way.** No source shows that a
  mandatory skill-selection reflex improves outcomes vs. opportunistic selection;
  it is an open question. The choice to keep it rests on *mechanism and
  philosophy* (consistency, trust-vs-mandate), not on evidence. See "Open
  questions."

Doc corrections this design mandates: "context rot" is *not* Anthropic-coined
(Chroma/community); compaction is *one of three* context-engineering levers
(compaction + note-taking/git + sub-agents), not the singular answer.

## Approach

Five workstreams. Build order: **E → B → A → C → D** (corrections first, then the
library, then the reflex that points at it, then escalation, then handoff
hardening).

### A. Entry-point reflex — the "always-on" half *(new)*

- drovr ships a **global session-start hook** (plugin `hooks/`) that injects a
  mandatory `using-drovr` reflex into **human-facing agents only**.
- **Reach: global.** It fires in every Claude Code session, drovr run or not —
  this is what "supersede" means literally: drovr's discipline is the default
  operating mode for the human.
- **Suppression:** `drovr phase start` sets `DROVR_PHASE=<run>/<phase>` on the
  `claude` it spawns via herdr. The hook **no-ops when `DROVR_PHASE` is set** —
  drovr-spawned phases are driven purely by their injected handoff briefing, and
  re-deriving a discipline they were already told fights the handoff contract.
  The env var doubles as a phase-identity signal the phase prompts may use.
- The reflex skill is thin. It tells the main agent to: (1) apply the right
  methodology skill for the task, and (2) escalate to a phase/handoff when a task
  outgrows one context (see C). **It replaces `using-superpowers`.**

### B. Methodology library — the bulk of the replacement *(new)*

Extract the disciplines currently buried as prose in the phase prompts into
first-class, progressively-disclosed skills (~500 tok SKILL.md each):

- **Phase 1 (core):** `drovr:tdd`, `drovr:systematic-debugging`,
  `drovr:verification-before-completion`, `drovr:code-review`
- **Phase 2 (stretch):** `drovr:writing-skills`, `drovr:finishing-a-branch`,
  `drovr:worktrees`

**Single source of truth:** phase prompts stop duplicating this prose and instead
reference the skill (e.g. "apply `drovr:tdd`"). The reflex points to the same
skills. Written once, referenced by both the inline path and the phase path.

**Namespace decision:** these ship under a **`drovr:` namespace**, not reused
`superpowers:` names. Rationale: the stated goal is full replacement, so drovr
must be self-contained — uninstalling the superpowers plugin must lose nothing
drovr needs. Coexistence (both installed) is acceptable but not required; drovr
never *depends* on `superpowers:*`. **(Flagged for reviewer: this is the decision
that makes drovr replace rather than complement superpowers.)**

### C. Inline-first escalation — the runtime model *(new)*

Today the only path is the heavyweight pipeline. Add the inline path:

- **Default: do small work inline** in the main agent, applying methodology
  skills. Keep sequential-dependent and same-file work inline (Anthropic: "a
  single session handling the chain is cleaner").
- **Escalate on a context-fullness signal**, not task decomposition. Compress the
  current context in place and continue in a fresh phase. Secondary heuristic:
  **10+ files or 3+ independent work items.**
- **New self-serve primitive** — a command the *main agent* can invoke mid-task
  to compress its own context and continue fresh (distinct from the existing
  phase-boundary `drovr phase compress`, which a driver runs *between* phases).
  Working name: `drovr handoff self` (exact surface decided in the plan). This is
  the "escalate mid-flight" escape hatch, decided at runtime, not up front.

### D. Handoff hardening *(improve existing)*

- **Pair the handoff with git history** (Anthropic's progress-file + git-log
  pattern): the handoff references commits; the next phase reads `git log`/`git
  diff` to reconstruct state, not just trust the summary.
- **Guard against lossy compression** (Cognition's warning that lossy summaries
  drop load-bearing decisions): the handoff's Artifact-pointers section must point
  to the full transcript + git so a phase can re-derive. Keep the existing
  7-section HANDOFF template — the research validates its shape.

### E. Doc corrections *(small, do-now)*

- Fix "context rot" attribution across skills/README (Chroma/community, not
  Anthropic).
- Reframe compaction as *one of three levers* wherever the docs imply it is the
  singular mechanism.

## Architecture & components

```
Human session (main agent)                 drovr-spawned phase (DROVR_PHASE set)
──────────────────────────                 ─────────────────────────────────────
session-start hook fires                    session-start hook NO-OPS
  → injects `using-drovr` reflex              → agent runs on injected handoff only
        │                                             │
        ▼                                             ▼
  task arrives                              executes phase prompt, which
        │                                   references drovr:* methodology skills
  reflex: apply methodology skill(s)                  │
  (drovr:tdd / debug / verify / review)               ▼
        │                                    drovr phase done → compress → HANDOFF
  outgrows one context?
   ├─ no  → finish inline
   └─ yes → drovr handoff self (compress-in-place)
             → continue in fresh phase / pipeline
```

- **Delivery:** skills via the plugin `skills/` dir (existing pattern); the reflex
  hook via a plugin `hooks/` dir referenced from `plugin.json` (new).
- **CLI changes:** one new subcommand family for the self-serve mid-task handoff
  (C); `phase start` sets `DROVR_PHASE` (A). Everything else is skill/doc work.

## Interfaces / contracts

- **`DROVR_PHASE`** — env var set by `drovr phase start` to `<run>/<phase>`.
  Contract: present ⇒ this is a drovr-spawned phase ⇒ reflex hook suppresses
  itself. Absent ⇒ human-facing agent ⇒ reflex injected.
- **Reflex skill** — `skills/using-drovr/SKILL.md` (rewritten): the router +
  escalation contract; the always-on entry point for human agents.
- **Methodology skills** — `skills/<name>/SKILL.md` under the `drovr:` namespace,
  each ≤ ~500 tok, progressively disclosed, referenced by name from phase prompts.
- **Self-serve handoff** — new CLI surface (exact name/flags in the plan) that
  compresses the caller's context to a HANDOFF doc and prints the resume pointer.
- **HANDOFF template** — unchanged 7 sections; Artifact-pointers section now
  additionally required to reference git (commit range / branch).

## Scope boundaries

**In scope:** the five workstreams above.

**Out of scope (YAGNI):**
- No change to herdr, the review server protocol, or the `state.json` schema
  beyond what C strictly needs.
- No parallel-writer orchestration — single-writer rule stands.
- Phase-2 stretch skills are optional; core four (B/phase-1) are the bar for
  "replaces superpowers for day-to-day work."
- No automatic context-fullness *measurement* inside the CLI — escalation is
  judged by the agent via the reflex's heuristics, not a token meter drovr owns
  (the harness owns compaction signals; drovr provides the escalation *primitive*
  and the *discipline*, not the trigger telemetry).

## Testing strategy

- **Reflex suppression:** unit/integration test that `phase start` sets
  `DROVR_PHASE` and that the hook's decision logic no-ops when it is present and
  injects when it is absent.
- **Skills:** each methodology skill validated per `writing-skills` (frontmatter,
  disclosure size, self-containedness).
- **Self-serve handoff:** e2e that a mid-task compress produces a valid 7-section
  HANDOFF and a correct resume pointer, mirroring the existing compress tests.
- **Docs:** grep-level check that "context rot" attribution and the
  three-levers framing are corrected everywhere.
- Existing tests must stay green (baseline was 70 at the start of this work; the
  drovr-v2 implementation brought it to 85). The "59" figure in earlier drafts was
  stale.

## Open questions

1. **Reflex efficacy is unproven.** No evidence a mandatory reflex beats
   opportunistic selection. We keep it on mechanism/philosophy grounds (always-on
   consistency for the human), scoped to main agents only so the cost is bounded
   to one context. Revisit if it proves noisy in practice.
2. **Escalation trigger.** The agent judges context-fullness by feel + the
   10-files/3-items heuristic. Whether drovr should later surface a token signal
   to make this less subjective is deferred.
3. **Handoff lossiness has no measured quality bar.** The research found no
   evidence on compaction failure rates. D mitigates by pairing with git so state
   is re-derivable; a validation step is deferred until we see real losses.
4. **Sequential-dependent work.** Anthropic warns against a phase relay for it; C
   resolves this by keeping such work inline unless context pressure forces a
   boundary — but this is a judgment call encoded in the reflex, not a hard rule.
