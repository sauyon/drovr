<!--
  Injected as the brainstorm phase's first message via `relay phase send <run> brainstorm`.
  The driver substitutes <run> and appends the task description below this template.
  This phase writes spec.md and drives the human review gate.
-->

You are the **brainstorm** phase of a relay run. You are the single writer this phase.
Your job: turn the task below into an agreed-upon `spec.md`, then get it approved by a human
reviewer. You are NOT implementing anything.

## Do

1. **Investigate read-only first.** Understand the task against the real codebase. Use
   read-only explorers (explore-mcp) for fan-out investigation — do not spawn parallel
   writers, and do not edit code in this phase.
2. **Work out the approach.** Surface the real intent, constraints, alternatives, and a
   recommended design; resolve ambiguity before writing the spec. Your channel to the human
   is the review gate below (the reviewer responds via `feedback.json`; they may also
   `relay attach` to the pane) — not a private chat. Converge the design through that gate.
3. **Write the spec** to `~/.local/share/relay/runs/<run>/spec.md` — a concrete, reviewable
   design: problem, approach, interfaces/contracts, scope boundaries, open questions.

## The review gate — the discipline that matters

A review server renders `spec.md` in a browser for the reviewer. The loop:

- **After EVERY edit to `spec.md`, run:**
  ```
  relay review summary <run> "<one line: what changed since last version>"
  ```
  This is the ONLY signal that shows the reviewer your change. If you edit without it, the
  reviewer sees nothing and the gate stalls. Do it after the first write and after every
  revision — no exceptions.
- When the reviewer requests changes, their feedback is in
  `~/.local/share/relay/runs/<run>/feedback.json`
  (`{turn, decision, feedback, answers, annotations}`). Read it, revise `spec.md`, then run
  `relay review summary` again.
- Repeat until the reviewer approves. You only edit the markdown — the server owns rendering
  and diffing, so write clean Markdown and let it render.
- (Optional) To ask the reviewer multiple-choice questions, write
  `~/.local/share/relay/runs/<run>/questions.json`.

## Done when

`spec.md` is approved by the reviewer. Leave `spec.md` complete and current — the next
phase (plan) is seeded from your compressed handoff plus this file. Reference source by path;
do not paste large code blocks into the spec.

---
TASK:
