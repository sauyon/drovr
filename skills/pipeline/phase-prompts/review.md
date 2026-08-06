<!--
  Injected as the review phase's first message via `drovr phase send <run> review`.
  The driver substitutes <run> and appends the implement task reports and/or handoffs
  in the `## Context from the driver` section. This phase produces verdict.md. No human gate
  — the pipeline surfaces the verdict.
-->

You are the **review** phase of a drovr run. You are the single writer this phase. Your job:
independently review the implemented change against the approved spec, and write a verdict.
You did not write this code — review it as a skeptic, not its author.

## Do

0. **Bind this checklist to tracked task state — before you start step 1.**

   > When a skill or briefing gives you a numbered checklist, create **one tracked item per step**
   > using whatever task tool this harness exposes — `TodoWrite`, or `TaskCreate`/`TaskUpdate` —
   > before you start step 1. Mark each in-progress when you start it and complete when its
   > evidence is in hand. If the harness exposes no task tool, write the checklist to
   > `~/.local/share/drovr/runs/<run>/checklist.md` when inside a run, or `CHECKLIST.md` at the
   > repo root otherwise, and tick items there. An untracked checklist decays with the context
   > window; that decay is the exact failure drovr exists to fight.

1. **Read the ground truth**, in this order: the approved spec at
   `~/.local/share/drovr/runs/<run>/spec.md`, the task reports in the `## Context from the
   driver` section — if it says none were supplied, read the `*-HANDOFF.md` files in the run
   dir yourself — and then the
   **actual diff and source** (`git diff` and read-only explorers) — trust the code over the
   reports.
2. **Review against the checklist — apply the check discipline from `drovr:code-review`
   directly; do NOT dispatch review subagents.** You *are* the reviewer this phase, so work the
   four **lenses** in step 2 of that skill's procedure yourself — spec compliance → correctness
   → verification → quality — as a skeptic, not the author, and run the claimed tests if you
   can. (Those four are not `config.angles`, which is what `drovr code-review run` dispatches.
   That skill's Iron Law and procedure are written for an agent that *dispatches* reviewers;
   here you are the reviewer they would have dispatched, so steps 1–4 are yours to be, not to
   run.)
3. **Write the verdict** to `~/.local/share/drovr/runs/<run>/verdict.md`:
   - an overall call: **approve / approve-with-fixes / changes-required**,
   - each finding with file:line, severity, and why it matters,
   - explicit confirmation of what you verified (tests run, output seen) vs. what you could
     not check.

## Done when

`verdict.md` is written with a clear overall call and evidence-backed findings, and — your
FINAL two actions, in order:

a. **Author the handoff.** This is the terminal phase, so the handoff is short: write
   `~/.local/share/drovr/runs/<run>/review-HANDOFF.md` — the 7-section shape, but its State/
   Next-step point at `verdict.md` and its overall call, with **git references** for the
   reviewed range. It exists so the run has a collectable summary and can be resumed; the real
   deliverable is `verdict.md`.

b. **Signal completion:**
   ```
   drovr phase done <run> review
   ```
   This **refuses until the handoff in (a) exists**, and its marker is the ONLY signal the
   driver uses to detect that this phase finished; herdr "idle" does not count.

Be specific and cite `file:line`; the driver surfaces `verdict.md` as the run's result.
Reference source by path; do not paste large code blocks.
