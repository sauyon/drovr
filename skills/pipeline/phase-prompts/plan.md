<!--
  Injected as the plan phase's first message via `drovr phase send <run> plan`.
  The driver substitutes <run> and appends the brainstorm HANDOFF (`drovr collect <run>
  brainstorm`) below this template. This phase produces plan.md. No human gate — the plan
  self-reviews and the pipeline auto-proceeds.
-->

You are the **plan** phase of a drovr run. You are the single writer this phase. Your input
is the approved spec plus the brainstorm handoff appended below. Your job: produce an
implementation plan broken into independently-executable tasks. You are NOT implementing.

## Do

1. **Read the approved spec** at `~/.local/share/drovr/runs/<run>/spec.md` and the brainstorm
   handoff below. Read the real source (read-only explorers for fan-out) so tasks bind to
   actual signatures, not guesses.
2. **Write the plan** to `~/.local/share/drovr/runs/<run>/plan.md` as an ordered task list.
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
   are read-only, so drovr's single-writer discipline holds — they find, you fix. **Run them
   in the FOREGROUND (blocking) — do NOT set `run_in_background` or yield waiting on them; a
   backgrounded subagent parks you mid-turn, which drovr cannot tell from completion.** Address
   every Critical/Important finding before finishing.
4. **Signal completion — your FINAL action.** After `plan.md` is complete, run:
   ```
   drovr phase done <run> plan
   ```
   This marker is the ONLY signal the driver uses to detect that this phase finished — herdr
   "idle" does not count. Run it last, once.

## Done when

`plan.md` is complete with per-task interfaces, you have run read-only review subagents (in
the foreground) and addressed their Critical/Important findings, and you have run
`drovr phase done <run> plan` as your final action. The implement phase runs each task as its
own fresh agent seeded from your compressed handoff, so the handoff must carry the task list
and the interface contracts. Reference source by path; do not paste implementations.

---
BRAINSTORM HANDOFF:
