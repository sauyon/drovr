<!--
  Injected as ONE implement task's first message via
  `drovr phase send <run> implement-task-<N>`. The driver substitutes <run> and <N>, and
  appends: (a) this task's brief from plan.md, and (b) the accumulated interfaces from
  earlier tasks' handoffs. One fresh agent per task keeps context clean.
-->

You are implement **task <N>** of a drovr run. You are the single writer this phase. Your
scope is EXACTLY the one task brief appended below — not the whole plan. Do not start other
tasks; later tasks run as their own fresh phases.

## Do

0. **Bind this checklist to tracked task state — before you start step 1.**

   > When a skill or briefing gives you a numbered checklist, create **one tracked item per step**
   > using whatever task tool this harness exposes — `TodoWrite`, or `TaskCreate`/`TaskUpdate` —
   > before you start step 1. Mark each in-progress when you start it and complete when its
   > evidence is in hand. If the harness exposes no task tool, write the checklist to
   > `~/.local/share/drovr/runs/<run>/checklist.md` when inside a run, or `CHECKLIST.md` at the
   > repo root otherwise, and tick items there. An untracked checklist decays with the context
   > window; that decay is the exact failure drovr exists to fight.

1. **Read** the task brief and the accumulated interfaces below, then read the real code you
   will touch (read-only explorers for anything you only need to understand, not change).
2. **Record the review base — before writing any code.** Run
   `drovr code-review base <run> task-<N>` so `HEAD` is captured as this task's pre-task SHA
   (the automatic review panel diffs `base..HEAD`). Do this first; editing before recording
   would move the base past your own changes. **Re-entry:** if this message seeds you with a
   `~/.local/share/drovr/runs/<run>/task-<N>-review.json` (the driver re-entered you after the
   panel found changes), do NOT re-record the base — fix **every Important AND every nit** in
   that file, then run the rest of the steps (verify, self-review, report — steps 4–7).
3. **Implement test-first — apply `drovr:tdd`.** The test to write first is the one named in
   this task's verification; keep it scoped to this task's interfaces so the folded-forward
   contracts stay accurate.
4. **Verify before claiming done — apply `drovr:verification-before-completion`** on this
   task's tests and the build/linter.
5. **Self-review before reporting done — apply `drovr:code-review`** (read-only review
   subagents, foreground). Do NOT declare the task done off your own judgment. The skill has
   the how-to and the check order; the one constraint below is repeated here because the whole
   run depends on it: **run the review subagents in the
   FOREGROUND (blocking) — do NOT set `run_in_background`, and do NOT yield or schedule a
   wakeup waiting on them.** A backgrounded subagent leaves you parked mid-turn, which drovr
   cannot distinguish from completion and which stalls the run until a human nudges the pane;
   blocking keeps you working straight through to your final step. This is IN ADDITION to the
   pipeline's final review phase — catch it here, cheaply, before it cascades.
6. **Write a task report** to `~/.local/share/drovr/runs/<run>/task<N>-report.md`:
   - what changed (files + the interfaces you actually implemented, verbatim),
   - test/verification output proving it works,
   - the self-review: what the review subagents found and how you resolved each Critical/
     Important finding (and any you deferred, with why),
   - any interface that drifted from the plan, and why (the next task binds to reality, not
     the plan's guess),
   - anything the final review phase should still scrutinize.
7. **Author your handoff, then signal completion — your FINAL actions, in order.**
   a. **Author the handoff.** Compress your own context — you hold the whole session — into the
      fixed 7-section handoff (see `drovr:handoff` / the handoff template) and write it to
      `~/.local/share/drovr/runs/<run>/implement-task-<N>-HANDOFF.md`, **git pointers
      mandatory**. This folds your real interfaces forward to the next task; nothing compresses
      it for you. Record exact signatures; report your dead-ends honestly.
   b. **Signal completion.** Run:
      ```
      drovr phase done <run> implement-task-<N>
      ```
      This **refuses until the handoff in (a) exists** — the handoff and this marker are one
      atomic step. The marker is the ONLY signal the driver uses to detect that this phase
      finished — herdr "idle" does not count (it also fires while you are merely waiting on a
      subagent). Run it last, once, after everything else; do not run it if you are stopping
      blocked.

## Done when

The task's tests pass, you have run read-only review subagents (in the foreground) and
addressed their Critical/Important findings, `task<N>-report.md` is written, you have authored
`implement-task-<N>-HANDOFF.md`, and you have run `drovr phase done <run> implement-task-<N>` as
your final action. Reference source by path; do not paste large diffs into the report or handoff.

If you cannot complete the task (blocked, or the plan contradicts reality), STOP and say so
plainly in the report — a failed task halts the loop rather than cascading a broken interface
forward.

---
TASK BRIEF + ACCUMULATED INTERFACES:
