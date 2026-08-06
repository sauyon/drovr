# `merge-main` — HANDOFF

Merged `main` (`14b9877`) into `drovr/skill-stickiness`. Merge base `99540bd`; 224 commits
brought in, 158 already ahead. Review base for this phase:
`313691438109c28e5f68c8735393de3da40d723e` (recorded in
`~/.local/share/drovr/runs/skill-stickiness/merge-main-base.sha`).

**All 66 `skills_valid` tests pass, and the whole suite is green** — 817 tests across 7 binaries,
including every evidence guard: `arm_a_snapshots_match_manifest`,
`arm_a_prime_snapshots_match_manifest`, `arm_b_snapshots_match_manifest`,
`voice_snapshots_match_manifest`, `manifest_commits_contain_their_snapshots`,
`run_ledger_cumulative_is_a_running_total`, `cross_model_grid_matches_its_own_verdicts`.
No guard was weakened, disabled, or re-baselined to make the merge land.

`docs/skill-evidence/` was **not touched** — main never edits under `arms/`, and `git status`
showed the directory clean at every point of the merge.

---

## 1. Conflicts and how each was resolved

Seven files conflicted. Twenty-three more merged clean.

### `docs/known-issues.md` — union (HAZARD 4)

Two conflict regions, both pure prepend-vs-prepend at the top of the file and at the entry
boundary near line 1970. Resolved by deleting the conflict markers only; **no entry from either
side was dropped**. Verified mechanically, not by eye:

- Every one of the 175 lines the branch added since the merge base is present in the result.
- Every one of the 2235 lines main added since the merge base is present in the result.
- 80 `##` entries in the merged file vs 21 (ours) + 72 (theirs) − 13 shared.
- 42 `- **` Resolved-section bullets vs 2 + 41 − 1 shared.

One ours-side heading is absent by design: `## drovr phase send still lands a large briefing
unsubmitted (post-readiness-fix)`. Main deliberately deduped and retitled that entry
(`7d082a0`, `b9f08aa`) and the branch never touched it, so the region auto-merged to main's
rewrite. The entry survives under main's title, `## drovr phase send returns success with the
prompt left unsubmitted` (line 1370). Nothing was lost.

### `cli/tests/skills_valid.rs` — main's change ported, not taken wholesale (HAZARD 3)

Main's only change to this file was re-baselining `const BODY_BUDGET: usize` 2200 → 3200 with
a doc comment explaining two prior re-baselines. The branch had already deleted that constant:
budgets now ride per-skill in the `skill_names!` macro table (12_000 for the four discipline
skills, 9_000 for `using-drovr`).

Resolved by keeping the branch's imports (`Component`, `process::Command`) and dropping main's
const block — **superseded, not discarded**: 3200 is well under the 12_000 the four discipline
skills now carry, so nothing main capped is uncapped here. Main's rationale is preserved as a
paragraph on the `skill_names!` doc comment, which now records why the single global constant
kept having to move and what the per-skill table fixes.

The file is 10,722 lines; the entire measurement instrument is intact.

### `cli/src/config.rs` — union

Main added the `reap_finished_panes: bool` field (`1b32ac1`); the branch rewrote the doc comment
on the adjacent `reflex` field. Both kept.

### `cli/src/main.rs` — union

Main added the `Commands::McpFindings { run, task, iter }` dispatch arm (`5c0358d`, `de7271d`);
the branch replaced `Commands::Reflex { skill }` with `Commands::Reflex { skill, gate }` and
`ReflexMode::from_flags`. Both arms kept. `mod mcp_findings` and the `McpFindings` enum variant
merged clean, and `cargo build` is warning-free.

### `skills/pipeline/phase-prompts/review-angle.md` — union

Branch's step 0 (the checklist-binding block) kept; main's expanded step 1 ("Read the change,
then the code it lands in" — `069d9c0`, reviewers get the whole repo) taken over the branch's
shorter step 1. Neither side's content was authored as measured armor; this file is not
snapshotted under `arms/`.

### `skills/pipeline/phase-prompts/implement-task.md` — union

Branch's step 0 kept; main's step 1 rewording (`## Context from the driver` instead of "below")
taken. Same reasoning as above.

### `skills/code-review/SKILL.md` and `skills/using-drovr/SKILL.md` — branch text preserved byte-exact (HAZARD 2)

**Both files are byte-identical to their pre-merge state on this branch.** `git diff HEAD --`
on each is empty. Every main-side edit to them is listed in §2 below.

This was not the first resolution I tried. I initially restored main's whole `## Automatic
panel` section into `code-review/SKILL.md` — the section the branch's rewrite had deleted and
main had then substantially extended — on the reasoning that it is operational fact about the
tool rather than armor, and that a section deleted on one side and rewritten on the other is
the classic silent-loss case. `checked_skills_within_body_budget` rejected it: 13,334 bytes
against a 12,000 cap. Same outcome for `using-drovr`, where main's brief-composition correction
took the body to 9,218 against a 9,000 cap; a condensed rewrite still landed at 9,137.

**Measured headroom: `code-review` has 28 bytes, `using-drovr` has 97.** Neither can absorb any
main-side prose. The two available moves were to raise the caps or to defer, and raising them is
wrong here on both counts: the caps are measurement parameters for the arms, and the content
decision they would be bent around belongs to task 22, which may revert these skills to A′ and
free thousands of bytes anyway. So the content is deferred, in full, to §2.

---

## 2. RE-APPLY AFTER TASK 22 RESOLVES

Task 22 (apply the measured outcomes — ship arm B vs revert to A′) owns the content of the five
measured skills. Whichever way it goes, **these main-side changes must land afterward.** They
are real corrections and new rules from main, dropped here only because the body-size caps left
no room and the ship/revert decision is not yet made. If task 22 reverts a skill to A′, the
body shrinks and most of this fits directly.

### `skills/code-review/SKILL.md`

| # | Main commit | What must be re-applied |
|---|---|---|
| 1 | `2f24a4e`, `68a5d52` | **"Two roles, one gate."** Anyone may run the panel as often as they like — it is a test suite. Only the **driver's** run is the gate: a clean verdict on a panel you ran yourself is evidence, never permission to report done. |
| 2 | `b042815` | **"Never write a reviewer's prompt."** It is the output of `drovr code-review brief <run> <task> --angle <angle> [--context "<what changed>"]` — pass that verbatim. drovr owns the frame (angle, scope, schema); you contribute only `--context`. **This directly contradicts the branch's step 2**, which tells the agent to compose the reviewer's instructions itself. Task 22 must decide which stands; they cannot both. |
| 3 | `8e69565`, `38db0bd` | **Exit 1 is "error *or empty range*."** The branch's step 3 and its Requirements table both say "1 error". `code-review run` now refuses an empty `base..HEAD` rather than reporting it clean, and checks the range *contains* changes rather than that two SHAs differ. |
| 4 | `658e37a` | **Exit 2 is slow, not broken.** Re-running the *same* command RESUMES: it keeps the angles already banked and waits only on stragglers. Loop on 2 as freely as on 3. `--fresh` throws it away and pays for a new one — never use it to unstick a timeout. |
| 5 | `547a98e` | **A panel is re-run, never rehydrated.** `drovr phase rehydrate` refuses a reviewer: findings arrive through an MCP server handed over at launch, which no resumed session can be re-attached to — it could never deliver. |
| 6 | `547a98e`, `993459c` | **The panel closes its own panes** once findings merge, so read a reviewer's pane *before* the run returns if you want its reasoning; `<task>-review.json` is what survives. The implementer's pane is untouched. |
| 7 | `de7271d`, `fbb4b39` | **The panel needs a herdr workspace** (`drovr new` records one) and a review agent with a herdr integration. If it is unavailable or wedged, use `code-review brief` and spawn the reviewer yourself. |
| 8 | `df94d62` | `code-review run` and `code-review brief` both take `--context …`; the heading should carry it. |
| 9 | `df94d62` | **"Resolving findings" rewording:** recording a deferral *clears review*; it is not what makes you done — an empty finding list clears it vacuously. |

Partial mitigation, already in the tree: items 4, 5 and 6 are also stated in
`skills/pipeline/SKILL.md`, which merged clean and took main's text (see lines 207, 232–259,
331). A driver following the pipeline skill still gets them. An agent reading only
`drovr:code-review` does not.

### `skills/using-drovr/SKILL.md`

| # | Main commit | What must be re-applied |
|---|---|---|
| 10 | `cbb6c72`, `61b7bca`, `b042815` | **The REQUIRED BACKGROUND paragraph is now factually wrong on this branch.** It says `drovr phase start` "spawns `claude` and does **not** inject the seed (injecting the briefing is the skill's job, via `drovr phase send`)". That stopped being true on main: `drovr phase start <run> <phase> --context "<what this phase needs that drovr cannot know>"` composes the phase's brief and injects it. `drovr phase brief` prints what an agent will be told; `drovr code-review brief … --angle <angle>` does the same for a reviewer. `phase send` remains for free-form nudges — a message is not a brief. **This is the highest-priority item in this list**: it is a false operational claim about the tool, in the always-on router, injected in full at every `SessionStart`. |

---

## 3. Decisions I made that a reviewer should check

1. **Merged local `main` (`14b9877`), not `origin/main` (`e0940b5`).** The phase brief named
   `main` and its stated counts (224 behind / 158 ahead / 15 files on both sides) match local
   `main` exactly — I verified the both-sides file list is identical to the brief's. `origin/main`
   is 9 commits further along (a merge of `drovr/blocked-watchers`, plus 7 review rounds and
   `9df9cba feat: notify watchers when an agent gets blocked`). Those 9 touch
   `docs/known-issues.md`, `skills/handoff/SKILL.md`, `README.md`, `cli/src/{herdr,main,phase,review}.rs`
   and the web UI — i.e. they will conflict with this branch again. **A follow-up merge of
   `origin/main` is owed**, and it is smaller than this one. I did not widen scope to include it.
2. **Deferring rather than raising the body-size caps** — reasoning in §1 above. If a reviewer
   disagrees, the change is one number each in `skill_names!` (`cli/tests/skills_valid.rs:1869`),
   plus restoring the §2 content.
3. **`review-angle.md` step 1 and `implement-task.md` step 1: I took main's wording over the
   branch's.** Neither file is snapshotted under `arms/` and neither step was authored as armor —
   the branch's only edits to these files were adding the step-0 checklist block, which I kept.
4. **Main's `BODY_BUDGET` rationale was folded into a doc comment rather than dropped.** It is
   history about why a single global cap kept moving; the per-skill table is the fix, and saying
   so where the table lives is worth ~8 lines.

## 4. What did NOT change

- `docs/skill-evidence/arms/**` — untouched, verified clean throughout.
- `skills/code-review/SKILL.md`, `skills/using-drovr/SKILL.md` — byte-identical to pre-merge.
- The `tiered-review` pin `361de71eddb43bfeef665d5f53035685ffe6a44c` — no rebase, no amend, no
  history rewrite of any kind. The merge is a merge commit; the pinned commit is untouched and
  still reachable.
- No tree-wide `cargo fmt`. No `git add -A` — every path was staged explicitly.

## 5. Verify it yourself

```
cd cli && cargo test          # 817 tests, 7 binaries, all green
git diff HEAD~1 -- skills/code-review/SKILL.md skills/using-drovr/SKILL.md   # empty
git status --short docs/skill-evidence/                                       # empty
```
