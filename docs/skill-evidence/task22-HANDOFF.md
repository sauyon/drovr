# Handoff — `skill-stickiness` task-22

**Committed to git rather than left in the run directory.** A drovr test bug (fixed in `1731440`)
wiped `~/.local/share/drovr/` once already and destroyed this run's `spec.md` and `plan.md`. A copy
lives in the run directory; **git is the authority.**

## Objective

`plan.md` Task 22 — apply the measured ship/revert outcomes across the corpus, aggregate the
evidence, and run spec §9 end to end.

## State

**Three commits on `drovr/skill-stickiness`, all green at 926 tests:**

| commit | what |
|---|---|
| `c103aa4` | the decided reverts, the evidence index, §9 first pass |
| `f74d6fc` | `known-issues.md`: the opencode reviewer seed bug |
| `ce173a8` | the §7.4 voice probe at full n, and the ceiling removal it forced |

**Done.** Steps 1–5 of the task, including the voice probe that Task 21 never ran.

**NOT done, and not closable here: the review gate.** `drovr code-review run skill-stickiness
task-22` cannot deliver its seed (below). `drovr phase done` has **deliberately not been run** —
the task's own done-condition requires a clean review round, and claiming one that did not happen
is the failure this run exists to study. The driver closes it after the reviewer is fixed.

## Decisions + rationale

1. **`tdd` and `verification-before-completion` reverted to arm A′**, byte-identical to the
   manifest-pinned snapshots, per their own evidence files. Their `SKILL_ARMOR_STATES` and
   `SKILL_SITE_STATES` entries moved to the `Pending` / `Deferred` variants that Tasks 10–13
   reserved for exactly this. Both guards were watched RED with arm B restored before being
   trusted.
2. **`systematic-debugging` ships arm B, unchanged.** Its `## Failure and reverted state` is
   banner-marked SUPERSEDED and says *"Task 22 must not act on this section"*. It did not.
3. **The voice probe ran, outcome 3 fired, and nothing changed.** Arm B was already authored in
   the outcome-3 default register, so applying the outcome is a no-op.
4. **Both ledger ceilings removed.** `ab-voice` took the total to 211 global / 139 metered, past
   both. The ceiling was lifted; a constant that refuses authorised spend is not a guard. The
   arithmetic check stays and its negative test was rewritten in the same change.
5. **`code-review` and `using-drovr` left untouched**, shipping unmeasured arm B, per the human's
   decision to measure rather than revert.
6. **The user's global `~/.config/drovr/config.toml` was NOT edited** to work around the reviewer
   bug, though `review_agent = "opencode"` is the suspected cause. Swapping the reviewer changes
   what reviewed the work; that is the driver's call.

## Interfaces / contracts

- `SKILL_ARMOR_STATES` / `SKILL_SITE_STATES` (`cli/tests/skills_valid.rs`) — `Pending` and
  `Deferred` now have live entries again. Both assert the armor / directive is **absent**, so a
  future task that re-armors either skill must flip the entry in the same commit as the text.
- `RUN_CEILING` and `METERED_RUN_CEILING` **no longer exist**. A comment stands in their place
  explaining why. Re-adding a cap means re-adding its negative test in
  `ledger_check_refuses_a_table_that_does_not_add_up` — they were removed together.
- `voice_stage_records_the_design_it_pre_registered` — new; the guard for `transcripts/voice/`.
- `docs/skill-evidence/INDEX.md` — new; the per-skill aggregate index.

## Open questions

Two, both already decided by the human and both needing their own phase.

### A. Measure `code-review` and `using-drovr` — several phases

Full spec in `task22-report.md` §2. In short: **rewrite the weak scenarios first**
(`code-review` is saturated at 3/4 unaided, `code-review-3` at ceiling; `using-drovr` splits
0-of-2 / 2-of-2, rewrite `ud-3` on `ud-2`'s model), **re-run `discrimination-test` and require
0 of 4 unaided** before spending an arm run, then measure. Two complications a fresh phase must
know before it starts:

- **`using-drovr` has no A′ arm** — §7.3 scoped that budget to the four discipline skills, there
  is no snapshot in `arms/A-prime/` for it, and `plan.md` rules it is compared B-against-A alone.
  "Revert to A′" is not an available outcome; state the rule for A-vs-B before the runs.
- **`using-drovr`'s arm B text is load-bearing for fix 2.** `gate_card_phrases_present_in_router_skill`
  pins the CLI's gate card phrases to text living in arm B. A revert outcome is a change to the
  card as well as to a document. Decide which side moves before measuring.

### B. Debug the opencode reviewer seed path — its own phase

It blocks every future review gate, not just this one. **Do not conflate it with the cursor bug
the driver briefed on — they are two different failures.**

**Exact reproduction.** Both iterations, same result:

```
$ drovr code-review run skill-stickiness task-22
drovr: code-review run failed: phase 'review:task-22:1:correctness' … the seed was NOT
delivered — herdr saw no state change after the prompt, and the payload is nowhere in the
agent's composer, so it was swallowed rather than left unsubmitted.

$ drovr code-review run skill-stickiness task-22        # re-run WITHOUT --fresh, to resume
drovr: code-review run failed: phase 'review:task-22:2:correctness' … (identical)
```

**What the pane showed — this is the discriminator.** Reviewer pane `wDM:p4`:

```
$ herdr pane read wDM:p4 --source visible --lines 45
   ┃  Ask anything... "Fix a TODO in the codebase"
   ┃  Plan · Qwen3.6 35B A3B (abliterated) ko.ag
```

A **virgin opencode session**: empty composer, no conversation history, nothing pending. The
cursor bug leaves the payload visible as `→ [Pasted text #1 +N lines]` and `herdr pane send-keys
<pane> Enter` recovers it. **That recovery is wrong here** and drovr's own error text explains why
it refuses to press a key blind. The pane is also titled `OpenCode`, not `Correctness Reviewer` —
a second cheap way to tell the two apart.

**The seed drovr wrote, for comparing sent-vs-arrived:**
`~/.local/share/drovr/runs/skill-stickiness/task-22-review-correctness-seed.md`
(plus `task-22-review-1.head` and `task-22-review-2.head`).

**Suspected cause, not confirmed:** `review_agent = "opencode"` in
`~/.config/drovr/config.toml`, which on this host resolves to a local Qwen3.6 35B. Diagnosis
stopped at two attempts rather than becoming a third. Next steps are in `known-issues.md`.

## Next step

Spawn phase B (the opencode reviewer), because it gates everything else. Then re-run
`drovr code-review run skill-stickiness task-22` and close this task. Phase A is independent and
can run in parallel.

## Artifact pointers

- `docs/skill-evidence/task22-report.md` — the full §9 output, every verdict, and the two RED
  results reported as failures rather than adjusted.
- `docs/skill-evidence/INDEX.md` — per skill: arm counts, margins, branch, decision, shipped state.
- `docs/skill-evidence/voice.md` — the probe's method, controls, result and limitations, appended
  below an untouched pre-registration.
- `docs/skill-evidence/transcripts/voice/` — 24 transcripts, `blind-map.json`, `scores.json`.
- `docs/known-issues.md` — three new entries: the opencode reviewer, `--plugin-dir` not wiring
  hooks, and nested `claude -p` losing every tool but `Read`.

## What is RED, and is meant to stay red until someone decides otherwise

- **§9.4, the integration check.** A clean session given a one-line bugfix request with no mention
  of drovr invokes `drovr:systematic-debugging`, never `drovr:tdd`. Six runs, unanimous. The
  criterion names `tdd`, so it fails. **This is a real failure of the run's thesis as written, not
  a test to adjust** — a bugfix routes to `systematic-debugging` and arguably the criterion is
  wrong, but rewriting it to match the observation is precisely the move the task forbids. The
  driver decides.
- **§9.5, clippy and fmt.** Tree-wide pre-existing toolchain drift, in files this task did not
  touch, proved by formatting the base version and getting the identical hunk count. No tree-wide
  `cargo fmt` was run. Net effect of this task: **one pre-existing clippy error removed, none
  added.**
