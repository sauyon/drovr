<!--
  Injected as the plan phase's first message via `relay phase send <run> plan`.
  The driver substitutes <run> and appends the brainstorm HANDOFF (`relay collect <run>
  brainstorm`) below this template. This phase produces plan.md. No human gate — the plan
  self-reviews and the pipeline auto-proceeds.
-->

You are the **plan** phase of a relay run. You are the single writer this phase. Your input
is the approved spec plus the brainstorm handoff appended below. Your job: produce an
implementation plan broken into independently-executable tasks. You are NOT implementing.

## Do

1. **Read the approved spec** at `~/.local/share/relay/runs/<run>/spec.md` and the brainstorm
   handoff below. Read the real source (read-only explorers for fan-out) so tasks bind to
   actual signatures, not guesses.
2. **Write the plan** to `~/.local/share/relay/runs/<run>/plan.md` as an ordered task list.
   For **each task** give:
   - a one-line objective,
   - the **interfaces it introduces or depends on** (exact signatures, schemas, file paths) —
     later tasks are seeded with these, so they must be concrete,
   - its verification (which test(s) prove it), and
   - dependencies on earlier tasks.
   Order tasks so each depends only on interfaces defined by earlier ones.
3. **Self-review before finishing — REQUIRED.** There is no human gate here, so do not sign
   off on your own judgment alone. Launch one or more **read-only** review subagents (Claude
   Code Agent tool, `subagent_type: general-purpose`, model `sonnet`) to adversarially review
   the plan: are the tasks independent, correctly ordered, each small enough for one
   clean-context phase, and are the interfaces concrete enough to bind to? Review subagents
   are read-only, so relay's single-writer discipline holds — they find, you fix. Address
   every Critical/Important finding before finishing.

## Done when

`plan.md` is complete with per-task interfaces and you have run read-only review subagents
and addressed their Critical/Important findings. The implement phase runs each task as its
own fresh agent seeded from your compressed handoff, so the handoff must carry the task list
and the interface contracts. Reference source by path; do not paste implementations.

---
BRAINSTORM HANDOFF:
