<!--
  Composed and injected as ONE implement task's first message by
  `drovr phase start <run> implement-task-<N> --context …`. drovr substitutes <run> and <N>
  and appends the run's task plus the driver's `--context`, which should carry (a) this
  task's brief from plan.md and (b) the accumulated interfaces from earlier tasks'
  handoffs. One fresh agent per task keeps context clean.
-->

You are implement **task <N>** of a drovr run. You are the single writer this phase. Your
scope is EXACTLY the one task brief in the `## Context from the driver` section — not the whole
plan. If that section says none was supplied, your scope is this task's entry in `plan.md`. Do
not start other
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

1. **Keep the ask channel open — the whole phase, not just at a gate.**

   > **Ask the human when you need to, mid-phase — do not guess and write the guess down.** Two
   > triggers, either one is enough: **new information is discovered** that the spec or plan did not
   > anticipate, or **a question is found** that you cannot resolve from the code or the run's
   > artifacts. Post it and carry on with whatever does not depend on the answer:
   >
   >     drovr ask <run> --question "<what you need decided>" \
   >       [--context <text> | --context-file <path>] \
   >       [--option <value>=<label>]... [--recommend <value>]
   >
   > `ask` returns immediately, printing the ask id and the page to point the human at. Then
   > background `drovr ask wait <run> [--timeout-ms <ms>]` and end your turn: `0` answered, `2`
   > timeout — re-arm, the question is still on disk and still on screen — `5` the run was cancelled,
   > `1` error. On `0` stdout carries the answers as JSON: the asks that wait was armed on, each with
   > its latest answer, or — when nothing was outstanding — the whole folded interview, which is how
   > a wait re-armed just after the human answered still hands you the answer. A timeout costs
   > nothing; a guess costs the run.

   **`ask wait` is the ONE wait you may end your turn on — and step 6 is not it.** The rule there
   is the same rule, not a different one: never park on something nothing will come to wake you
   from. A backgrounded review panel or subagent is exactly that, which is why step 6 forbids it.
   An ask is the opposite — it puts a question in front of a human, and their answer is what
   un-parks you. So: post the ask, do whatever does not depend on the answer, and only then
   background the wait and end the turn.

2. **Read** the task brief and the accumulated interfaces in the `## Context from the driver`
   section, then read the real code you
   will touch (read-only explorers for anything you only need to understand, not change).
3. **Record the review base — before writing any code.** Run
   `drovr code-review base <run> task-<N>` so `HEAD` is captured as this task's pre-task SHA
   (the automatic review panel diffs `base..HEAD`). Do this first; editing before recording
   would move the base past your own changes. **Re-entry:** if this message seeds you with a
   `~/.local/share/drovr/runs/<run>/task-<N>-review.json` (the driver re-entered you after the
   panel found changes), do NOT re-record the base — fix **every Important AND every nit** in
   that file, then run the rest of the steps (verify, self-review, report — steps 5–8).
4. **Implement test-first — apply `drovr:tdd`.** The test to write first is the one named in
   this task's verification; keep it scoped to this task's interfaces so the folded-forward
   contracts stay accurate.
5. **Verify before claiming done — apply `drovr:verification-before-completion`** on this
   task's tests and the build/linter.
6. **Self-review before reporting done — apply `drovr:code-review`** (read-only review
   subagents, foreground). Do NOT declare the task done off your own judgment. The skill has
   the how-to and the check order; the one constraint below is repeated here because the whole
   run depends on it: **run the review subagents in the
   FOREGROUND (blocking) — do NOT set `run_in_background`, and do NOT yield or schedule a
   wakeup waiting on them.** A backgrounded subagent leaves you parked mid-turn, which drovr
   cannot distinguish from completion and which stalls the run until a human nudges the pane;
   blocking keeps you working straight through to your final step. This is IN ADDITION to the
   pipeline's final review phase — catch it here, cheaply, before it cascades.

   **Two roles, one gate.** Anyone may RUN the panel, as often as they like —
   it is a test suite. Only the **driver's** run is the **gate**: a clean
   verdict on a panel you ran yourself is evidence, never permission to
   report done.

   So `drovr code-review run <run> task-<N>` is yours to use, freely — but a clean verdict
   from a panel *you* invoked buys you a fix list, not a finish. You are done when this task's
   own verification passes (step 5) and you have resolved what review found; never *because* a
   panel came back clean, and never with a panel standing in for step 5. Record each one in your
   report's `## Author-run panels` table (step 7). The driver runs its own panel after you
   report, and that verdict is the one that decides whether the task advances — it has already
   caught an Important on the identical commit an author-run panel called clean.

   Two mechanics if you do run it. **Commit first, and keep committing:** the panel's committed
   scope is `git diff base..HEAD`. Commit nothing and that range contains nothing —
   `drovr code-review run` now refuses that outright (exit 1), because it used to return a clean
   verdict on it (see `docs/known-issues.md`). Committing *once* is not enough either: whatever
   you leave uncommitted after that is outside `base..HEAD`, and while the reviewer's brief does
   put the working tree in scope alongside the diff, it does not reliably reach it — untracked
   files never appear in a `git diff` at all. What you have not committed may not be reviewed.
   **Keep it in the foreground and loop on exit 2:** re-running the
   same command resumes the panel in flight, so a slow one costs you a loop, not a stall. Do not
   background it and do not yield waiting on it — the driver may background its waits because it
   can end its turn; you cannot.
7. **Write a task report** to `~/.local/share/drovr/runs/<run>/task<N>-report.md`:
   - what changed (files + the interfaces you actually implemented, verbatim),
   - test/verification output proving it works,
   - the self-review: what the review subagents found and how you resolved each Critical/
     Important finding (and any you deferred, with why),
   - **an `## Author-run panels` section**, one table row per `drovr code-review run` you
     invoked yourself, including the ones that came back clean. Three columns, so the claim can
     be checked against the artifacts rather than taken on trust:

     | Iteration `<i>` | Head SHA, from `<run_dir>/task-<N>-review-<i>.head` | Verdict |

     The iteration is the panel's own number, not your round count — read it off the
     `task-<N>-review-<i>-<angle>.json` files it wrote. **Verdict is exactly one of five
     words**, matching how the run actually ended (drovr's `ReviewOutcome`, exit code in
     brackets):

     - `clean` [0] — a real range was reviewed, nothing blocking. Derive it the way drovr
       does, not from memory: `changes` if any finding in that iteration's per-angle files
       has severity `critical` or `important`, else `clean`. (`<task>-review.json` is
       overwritten each pass and is NOT evidence about iteration `<i>`; the per-angle files
       are.)
     - `changes` [3] — at least one blocking finding.
     - `timeout` [2] — reviewers had not all finished. Not a verdict about the code.
     - `empty-range` [1] — refused: `base..head` contained nothing. Not a verdict either,
       and the one most easily mistaken for a pass.
     - `error` [1] — setup failure; the panel never ran.

     A row you cannot classify is a row you should not summarise — say which and why.
     Write "none" if you ran no panel; an absent section is indistinguishable from an
     undisclosed one,
   - any interface that drifted from the plan, and why (the next task binds to reality, not
     the plan's guess),
   - anything the final review phase should still scrutinize.
8. **Author your handoff, then signal completion — your FINAL actions, in order.**
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
