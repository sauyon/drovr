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

1. **Read** the task brief and the accumulated interfaces below, then read the real code you
   will touch (read-only explorers for anything you only need to understand, not change).
2. **Implement test-first — apply `drovr:tdd`.** The test to write first is the one named in
   this task's verification; keep it scoped to this task's interfaces so the folded-forward
   contracts stay accurate.
3. **Verify before claiming done — apply `drovr:verification-before-completion`** on this
   task's tests and the build/linter.
4. **Self-review before reporting done — apply `drovr:code-review`** (read-only review
   subagents, foreground). Do NOT declare the task done off your own judgment. The skill has
   the how-to and the check order; the one constraint below is repeated here because the whole
   run depends on it: **run the review subagents in the
   FOREGROUND (blocking) — do NOT set `run_in_background`, and do NOT yield or schedule a
   wakeup waiting on them.** A backgrounded subagent leaves you parked mid-turn, which drovr
   cannot distinguish from completion and which stalls the run until a human nudges the pane;
   blocking keeps you working straight through to your final step. This is IN ADDITION to the
   pipeline's final review phase — catch it here, cheaply, before it cascades.
5. **Write a task report** to `~/.local/share/drovr/runs/<run>/task<N>-report.md`:
   - what changed (files + the interfaces you actually implemented, verbatim),
   - test/verification output proving it works,
   - the self-review: what the review subagents found and how you resolved each Critical/
     Important finding (and any you deferred, with why),
   - any interface that drifted from the plan, and why (the next task binds to reality, not
     the plan's guess),
   - anything the final review phase should still scrutinize.
6. **Signal completion — your FINAL action.** After the report is written, run:
   ```
   drovr phase done <run> implement-task-<N>
   ```
   This marker is the ONLY signal the driver uses to detect that this phase finished — herdr
   "idle" does not count (it also fires while you are merely waiting on a subagent). Run it
   last, once, after everything else; do not run it if you are stopping blocked.

## Done when

The task's tests pass, you have run read-only review subagents (in the foreground) and
addressed their Critical/Important findings, `task<N>-report.md` is written, and you have run
`drovr phase done <run> implement-task-<N>` as your final action. Your compressed handoff folds
your real interfaces forward to the next task, so record exact signatures. Reference source by
path; do not paste large diffs into the report.

If you cannot complete the task (blocked, or the plan contradicts reality), STOP and say so
plainly in the report — a failed task halts the loop rather than cascading a broken interface
forward.

---
TASK BRIEF + ACCUMULATED INTERFACES:
