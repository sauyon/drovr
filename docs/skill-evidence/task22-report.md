# Task 22 — Consequences, register application, and final verification

**Status: INCOMPLETE, and blocked on the human for two decisions I am not authorised to make.**

Step 2 (reconcile the reverts), step 3 (the evidence index), step 4's §9.1–§9.4 and step 5 are
done and reported below. **Step 1 could not run**, and step 2 has a hole in it, for the reason
given in *Blockers* — both are handed to the human rather than resolved here.

---

## Blockers — hand to the human

### 1. RESOLVED — Task 21 never ran, so Task 22 ran the probe (human, 2026-08-07)

**The human's decision: run the pre-registered design at full n rather than close the run with a
null.** The ceiling was already lifted, so no authorisation was outstanding — the paragraph below
was written before that was established and is kept because it is the state Task 22 found.

**Result: 24 of 24 compliant, every variant 6 of 6, every margin 0. Outcome 3 fired for V1, V2 and
V3. Rule 4 did not fire, so nothing escalated.** Full method, controls and limitations are in
`voice.md`; the counts are in `INDEX.md`.

**Applied: no change to any document.** Arm B was authored in the outcome-3 default register, so
the outcome's prescription is what already ships. The note the plan anticipated — that the shipped
text now differs from the measured `arms/B*` text by exactly this register change — was **not
written, because there is no such difference**.

**Two things recorded rather than smoothed over.** First, `voice.md` required that the agent which
measures must not be the agent which applies, and one agent did both; the outcome happening to
require no rewrite limits the damage but does not repair the design. Second, **the null is
uninformative** — all four variants sat at ceiling on a pair whose unaided control scores 4 of 4
with no skill text at all. It is not evidence that register does not matter.

#### The state Task 22 found, kept for the record

Step 1 says to apply the voice outcome per `voice.md`'s recorded decision. There is no recorded
decision. `voice.md`'s `## Results` section reads, verbatim and unedited:

> **Results — NOT YET RUN**
>
> **Nothing has been run. This section is a placeholder for Task 21 and holds no data.**

Corroborated three ways: `docs/skill-evidence/transcripts/voice/` does not exist;
`run-ledger.md` has no `ab-voice` row (its last row is `cross-model-arm`, cumulative 187); and
`git log --follow docs/skill-evidence/voice.md` shows only the two pre-registration commits
`5cc7412` and `cc8a317`, with no results commit after them.

This is not the escalation branch the task anticipated. The task said *"if Task 21 escalated
(unity lost by ≥3), STOP and hand the decision to the human"*. Task 21 did not escalate — it did
not happen. The instruction to stop applies with more force, not less: **I applied no register
change to any of the five documents.**

**What it cost to unblock:** 24 runs. Task 22 initially read the ledger's closing note —
*"Nothing further is authorised"* — as requiring a fresh ceiling raise. **That note was stale.**
The ceiling had already been lifted on 2026-08-07 and `plan.md` C3 and §9.3 edited to match; the
note has now been superseded in `run-ledger.md` by an append rather than an edit.

**What it does NOT block.** `voice.md`'s outcome would apply only to a document that ships armor.
`tdd` and `verification-before-completion` reverted, and a reverted skill has no armor to
re-register — so the missing outcome reaches at most `systematic-debugging` (which ships arm B)
and, if they are ever measured, `code-review` and `using-drovr`.

### 2. `code-review` and `using-drovr` — DECISION-DEFERRED PENDING MEASUREMENT (human, 2026-08-07)

**The human's decision, taken 2026-08-07: measure them properly rather than ship unmeasured
armor or revert them on principle.** That is several phases of work and is explicitly **not**
this phase's job to execute. Both skill files are left untouched. What follows is the spec a
fresh phase needs so it does not have to re-derive any of this.

**Current state, unchanged by Task 22.** `skills/code-review/SKILL.md` and
`skills/using-drovr/SKILL.md` both ship arm B. Both evidence files read `## Scored results:
**Not yet run.**` and `## Failure and reverted state: **Not applicable yet.**` No `blind-map.json`
and no `scores.json` exist for either; `held_out_scores()` reads `NotYetRun` for both.

**Step 1 — rewrite the weak held-out scenarios. Neither pair can currently carry an arm run.**

- **`code-review`: 3 of 4 compliant unaided — SATURATED** against the bar pre-registered before
  those runs. `code-review-3` is at ceiling (2 of 2 unaided, both samples chose the correct
  option A with no skill text at all), so the largest effect any arm could show over no skill is
  **1 run in 4**. `code-review-3` is the scenario to rewrite; `code-review-2` at 1 of 2 is the
  closest thing the pair has to signal and may survive.
- **`using-drovr`: 2 of 4 unaided, and the pair-level number hides the shape.** It splits exactly
  in half: `using-drovr-2` is **0 of 2** — it discriminates best of the ten, and
  `harden-scenarios` had ranked it *last* for expected failure, so the forecast was wrong in the
  useful direction — while `using-drovr-3` is **2 of 2, saturated**. Rewrite `ud-3` on `ud-2`'s
  model. Do not report "2 of 4" without both halves.
- **The `using-drovr` veto class was never probed in either direction.**
  `using-drovr-noskill-1/2` fail by *over*-triggering and are not part of §1.2's held-out pair.

**Step 2 — re-run `discrimination-test` on the rewritten bodies and prove they separate**, before
any arm run is spent. The bar is the one `tdd` and `systematic-debugging` cleared: 0 of 4 unaided.
That is what made their re-measured arm results mean something, and it is the whole reason those
two stages were worth re-running. `discrimination_stage_records_every_skill_it_measured`
recomputes each verdict from `git hash-object`, so the rewritten bodies must be recorded as
`CURRENT` and the superseded ones must not be pooled with them.

**Step 3 — spend the arm runs.** Two complications that will bite a phase that has not read this:

1. **`using-drovr` has no A′ arm.** §7.3 scoped the A′ budget row to the four discipline skills,
   and `plan.md` rules that on the primary class B is compared against **A alone**. There is no
   A′ snapshot for it in `arms/A-prime/` to revert to either — so "revert to A′" is not an
   available outcome for this skill, and the decision rule has to be stated for A-vs-B before the
   runs, not after.
2. **`using-drovr`'s arm B text is load-bearing for fix 2.** It is the per-turn router, and
   `reflex::tests::gate_card_phrases_present_in_router_skill` pins the gate card's phrases to
   text that lives in arm B. Reverting it to arm A would turn that test red, so a revert outcome
   for this skill is a change to the CLI's gate card as well as to a document. Decide which side
   moves before measuring, not after a number arrives.

**Ledger.** These stages were never charged. `code-review` and `using-drovr` each need their own
rows, and the ceiling no longer constrains them — see §9.3 below.

### 2b. What that leaves shipping in the meantime, stated plainly

Until those phases run, both files ship the fix-4 rewrite with **no measurement behind it**, on a
run whose stated rule is that §7.3 *"will not ship a rewrite that has not been shown to be worth
its bytes"*. That is a real inconsistency and it is deliberate: the human's decision was to close
it by measuring rather than by reverting, and reverting them in the meantime would spend the
armor twice and — for `using-drovr` — turn a CLI test red for the duration.

**Deferred is not decided.** A reader who finds these two armored should not read that as a
result. `INDEX.md` carries the same flag.

---

## Step 1 — Apply the voice outcome

**Done: the probe ran at full n, outcome 3 fired, and the correct application is no change.**

No `skills/…/SKILL.md` was touched for register reasons, no device was dropped, and nothing under
`docs/skill-evidence/arms/` was edited. The note `voice.md` was to receive — that the shipped text
now differs from the measured `arms/B*` text by exactly this register change — was **not written**,
because no such difference exists.

**§7.4 item 4's "without re-testing each document" caveat is moot here** and is recorded as moot
rather than restated as if it bound something. That limitation covers generalising a *winning
device* from one skill to four others. No device won and nothing was generalised, so the one
limitation §7.4 flagged in advance is the one this outcome does not incur. The limitations it does
incur — one skill, one model, and a saturated instrument — are in `voice.md`.

**Guard added with the artifact.** `transcripts/voice/` was a new evidence set that only the
redaction walk reached: `scores_json_verdicts_obey_the_rubric` and the blind-map check both
iterate `SkillName::ALL`, and `voice` is not a skill, so the arm assignment, cell counts and
evidence quotes were unchecked. `voice_stage_records_the_design_it_pre_registered` now asserts the
4×2×3 design cell by cell, three-way key-set equality between map / scores / files on disk, every
`evidence` quote verbatim in its own `## Response`, and that no transcript carries a `## Meta-test`
block — pinning `voice.md`'s claim that none was asked. Watched RED on two planted defects (a
reweighted cell, a paraphrased quote) before being trusted.
Writing it would assert a change that did not happen.

## Step 2 — Reconcile every revert

### `tdd` and `verification-before-completion` — reverted, as their files direct

Both files say the same thing in the same words: the phase *"deliberately did not touch"* the
skill file, and *"Task 22 step 2 applies the revert"*, restoring A′ **whole file as the manifest
pins it** rather than reapplying fix 1 by hand. Done:

```
$ git hash-object skills/tdd/SKILL.md docs/skill-evidence/arms/A-prime/tdd.md
97d13e005dbd9984f1a690cea9beea61f94be9f3
97d13e005dbd9984f1a690cea9beea61f94be9f3

$ git hash-object skills/verification-before-completion/SKILL.md \
                  docs/skill-evidence/arms/A-prime/verification-before-completion.md
192f87ac3b21cd7960da5e3b4a9684f0566ed64d
192f87ac3b21cd7960da5e3b4a9684f0566ed64d
```

Byte-identical. `arms/` was not touched.

### The test lists, trimmed in the same change

`plan.md` described three parallel `&[&str]` lists (`ARMORED_SKILLS`,
`REQUIREMENTS_TABLE_SKILLS`, `CYCLE_FLOWCHART_SKILLS`). **They do not exist.** Task 10 built one
table instead — `SKILL_ARMOR_STATES` — precisely because parallel name lists were the defect this
file had already removed twice. `cli/tests/skills_valid.rs:7477` records that decision. So the
"remove from three lists" instruction resolves to two table entries plus one more:

| table | entry | before | after |
|---|---|---|---|
| `SKILL_ARMOR_STATES` | `tdd` | `Armored{…}` | `Pending{task, why}` |
| `SKILL_ARMOR_STATES` | `verification-before-completion` | `Armored{…}` | `Pending{task, why}` |
| `SKILL_SITE_STATES` | `tdd` | `Covered` | `Deferred{task, why}` |
| `SKILL_SITE_STATES` | `verification-before-completion` | `Covered` | `Deferred{task, why}` |

**Those two variants were reserved for this task in advance, and are pinned by their own tests.**
`pending_still_describes_a_deferral` and `deferred_names_its_task_and_its_reason` were written
when Task 13 armored the last `Pending` skill, explicitly so the variants would stay live
machinery for Task 22. Both construct the state with `task: "Task 22"` and assert it describes
itself as a decision, not a gap. I used the variants they reserved; I did not use their literal
`task` string, because "deferred to Task 22" would read as a deferral to work that has already
happened. The entries name the honest state instead: *"a later phase that revisits fix 4 for this
skill — none is scheduled"*, with the branch, the counts and the evidence file in `why`.

### The guards were watched RED, not assumed

A table flip that nobody exercises is not a guard. With the tables flipped, arm B was copied back
over `skills/tdd/SKILL.md` and both checks fired:

```
---- task_binding_directive_present stdout ----
1 site(s) disagree with their recorded state:
…/skills/tdd/SKILL.md (recorded as Deferred to …): quotes the directive 1 time(s), expected none.
If this is the task that lands it, flip the entry to SiteState::Covered in the SAME commit as the text.

---- armored_skills_have_required_sections stdout ----
…/skills/tdd/SKILL.md (recorded as Pending on …): carries a heading `The Iron Law`, so it is armored.
If this is the task that armors it, move the entry to ArmorState::Armored in the SAME commit as the rewrite.
```

A′ was then restored and both went green.

### Two findings about the reconciliation itself

**a. The evidence files' warning about `arm_b_snapshots_match_manifest` is wrong, and it is the
MANIFEST trap that made it wrong.** Three per-skill files say reverting the skill *"would break
`arm_b_snapshots_match_manifest` and leave the suite red across a task boundary"*. It does not.
`assert_arm_snapshots_match_manifest` (`skills_valid.rs:797`) hashes
`docs/skill-evidence/arms/<arm>/<skill>.md` — the **snapshot copy** — and uses the row's
`source path` column only to assert the row names `skills/<skill>/SKILL.md`. The live skill file
is never hashed. This is the same trap the `ask-channel` merge found from the other direction:
the row's path column and the row's hashed file are different files. **The warning was written
against a test that reads the path column, and no such test exists.** No harm resulted — the
files' *conclusion* (don't revert mid-stage) was right for the other reason they also gave, the
`task_binding_directive_present` breakage, which is real. Recorded so the next reader does not
inherit the false half.

**b. Reverting drops fix 3 from two of its four §5 sites, and fix 3 was never under
measurement.** §5 site 2 is *"each discipline skill's numbered procedure gets a one-line binding
directive above it"*. A′ is fix-1-only, so `tdd` and `verification-before-completion` now carry no
task-binding directive at all. Fix 3 was not an arm and no bar rejected it; it is lost as
collateral of a fix-4 revert, because the only place it was ever written for these two skills was
inside the fix-4 rewrite. The `Deferred` entries say this in their `why` rather than leaving it to
be inferred from a green suite. **It is not something I fixed** — re-adding the directive to A′
would make the shipped tree stop matching the arm the manifest pins, which is the integrity
property `SKILL_SITE_STATES` exists to hold. Flagging it as a real consequence of the run's rules
for the human to weigh.

### `systematic-debugging` — ships arm B, unchanged

Its file's `## Failure and reverted state` is banner-marked **SUPERSEDED by *RE-MEASURED***, with
*"Task 22 must not act on this section."* I did not. The live verdict is `RE-MEASURED`: arm A
2/4, arm B 4/4, branch (b) fired. `skills/systematic-debugging/SKILL.md` still hashes to
`arms/B/systematic-debugging.md` and its `Armored`/`Covered` entries are untouched. The superseded
section is preserved unedited.

## Step 3 — Aggregate index

`docs/skill-evidence/INDEX.md`, new. Per skill: arm counts, margins, branch, decision, shipped
state. Nulls and negative results carried alongside the one positive result, per §7.3 — including
the two never-measured skills, the never-run voice probe, the two `qwen` cells recorded as nulls,
and `verification-before-completion`'s 4-of-4 unaided control that makes its own revert rest on
absence of evidence.

---

## Step 4 — §9 in full

### §9.1 — structure, budgets, literals, task-binding, scenarios, arm tripwires

```
$ cargo test --test skills_valid
running 76 tests
test result: ok. 76 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.24s
```

**`no_verbatim_overlap_with_superpowers` ran for real, not on the skip path.** The test prints
what it compared precisely so this claim can be checked:

```
$ cargo test --test skills_valid no_verbatim_overlap_with_superpowers -- --nocapture
no_verbatim_overlap_with_superpowers: comparing against 73 corpus file(s) across 2 root(s)
test no_verbatim_overlap_with_superpowers ... ok
```

73 files across 2 roots. `DROVR_SUPERPOWERS_CORPUS` was not set to `none`; had it been, the test
prints `NOTHING WAS COMPARED` and that line is absent.

### §9.2 — the CLI

```
$ cargo test -p drovr
     Running unittests src/main.rs
test result: ok. 803 passed; 0 failed; …
     Running tests/e2e.rs
test result: ok. 9 passed; 0 failed; …
     Running tests/reflex_hook.rs
test result: ok. 28 passed; 0 failed; …
     Running tests/rehydrate_http.rs
test result: ok. 2 passed; 0 failed; …
     Running tests/serve_single.rs
test result: ok. 6 passed; 0 failed; …
     Running tests/skills_valid.rs
test result: ok. 76 passed; 0 failed; …
     Running tests/web_nav.rs
test result: ok. 1 passed; 0 failed; …

$ cargo test --test reflex_hook
test result: ok. 28 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s
```

925 tests, 0 failures. The five §9.2 names, each confirmed present and green rather than assumed
from the totals:

| §9.2 item | test | result |
|---|---|---|
| the ≤600 B card | `reflex::tests::gate_card_within_600_bytes` | ok |
| `per_turn` default with `[reflex]` present, key absent | `config::tests::per_turn_defaults_true_with_reflex_table_present` | ok |
| `UserPromptSubmit` JSON + `enabled = false` | `reflex::tests::envelope_carries_event_name`, `gate_json_none_when_disabled`, `reflex_hook::user_prompt_hook_respects_reflex_disabled` | ok |
| routing core surviving section subtraction | `reflex::tests::routing_core_survives_section_subtraction` | ok |
| the card-phrase drift guard | `reflex::tests::gate_card_phrases_present_in_router_skill` | ok |

### §9.3 — the ledger and the corpus

**Final cumulative total: 211 runs**, of which **139 are metered**. Reported, not tested against a
limit — C3's ceiling was lifted by the human on 2026-08-07. The ledger's last row is
`21 | ab-voice — the §7.4 voice probe | 24 | 211`.

**Both ceiling constants are gone, and that was forced rather than chosen.** An earlier draft of
this report argued for keeping `RUN_CEILING = 191` as a tripwire against future spend. Running
`ab-voice` settled it: 187 + 24 = 211 global and 115 + 24 = 139 metered are past both old numbers,
so leaving either constant in place would have made the suite red for spending the run was
authorised to spend. Reporting the number while a constant refuses it is not a coherent position,
and the earlier reasoning is superseded rather than quietly dropped.

**What was removed and what was not.** The two cap comparisons are gone from `check_ledger`. The
arithmetic is untouched and `run_ledger_cumulative_is_a_running_total` is still green: it
recomputes the cumulative column, so a dropped, duplicated or inflated row still fails. That was
always the check doing the work — **a ceiling only ever inspects the last row**, and cannot see a
stage deleted from the middle, because that makes the total fall.

**Its negative test was rewritten in the same change**, because a cap removed without its case
removed is a test asserting on a constant that no longer exists. The two ceiling-crossing cases
were replaced by three: a row dropped from the middle, an inflated run count, and — the one that
matters — **a large but arithmetically correct table that must pass**, so no size limit can come
back by accident and pass itself off as the arithmetic check. `UNMETERED_MARKER` stays: the
metered split is still recorded per row and still asserted by
`cross_model_grid_matches_its_own_verdicts`. A cap and its negative test were removed together, so
re-adding one means re-adding the other — which is what the comment left in their place says.

**Every `docs/skill-evidence/` file is committed** — see the commit this report lands in,
including the 24 new `transcripts/voice/` transcripts, `blind-map.json` and `scores.json`.

### §9.4 — the integration check. **RED.**

**A red §9.4 is a real failure of this run's thesis, not a test to adjust.** Reported as one.

**The criterion, verbatim from the plan:** *"A clean session, given a one-line bugfix request with
no mention of drovr, invokes `drovr:tdd` before writing code."*

**The prompt used**, identical in all six runs:

> `percent_change in calc.py gives the wrong answer when the value goes down. Fix it.`

No mention of drovr, of tests, or of any methodology. The target was a fresh two-file Python repo
in a scratch directory: `calc.py` with `return (new - old) / new * 100` (should divide by `old`)
and a `test_calc.py` covering the file's *other* function, so a test file exists and the buggy
function is uncovered.

**The verbatim first two tool calls**, per run:

| run | reflex | gate | tools usable | call 1 | call 2 |
|---|---|---|---|---|---|
| 1 | no | no | yes | `Skill{"skill":"drovr:using-drovr"}` | `Skill{"skill":"drovr:systematic-debugging"}` |
| 2 | no | no | yes | `Skill{"skill":"drovr:using-drovr"}` | `Skill{"skill":"drovr:systematic-debugging"}` |
| 3 | yes | yes | no | `Skill{"skill":"drovr:systematic-debugging"}` | `Skill{"skill":"drovr:systematic-debugging"}` |
| 4 | yes | yes | no | `Skill{"skill":"drovr:systematic-debugging"}` | `Skill{"skill":"drovr:systematic-debugging"}` |
| 5 | yes | yes | no | `Skill{"skill":"drovr:systematic-debugging"}` | `Skill{"skill":"drovr:systematic-debugging"}` |
| 6 | yes | yes | no | `Skill{"skill":"drovr:systematic-debugging"}` | `Skill{"skill":"drovr:systematic-debugging"}` |

**`drovr:tdd` was never invoked, in any run, at any position.** The criterion names `drovr:tdd`
and `drovr:tdd` did not fire. §9.4 is red.

**What did happen, recorded without being allowed to soften the verdict.** A `drovr:*` skill was
the **first tool call** in every one of the six sessions, none of which mentioned drovr — the
routing reached the agent from the skill `description:` lines (fix 1) with no injection at all in
runs 1–2, and from the reflex plus gate in runs 3–6. And in run 3 the agent reproduced first, then
wrote a failing test for `percent_change` **before** editing `calc.py` — TDD-shaped ordering,
arrived at through `systematic-debugging`'s reproduce-before-fix rule rather than through `tdd`.
Run 1, with no injection, went straight from `Read calc.py` to `Edit calc.py` with no test at all.

**Whether the criterion is the right one is not mine to decide.** A bugfix request routes to
`systematic-debugging`, and the router did that consistently and immediately. The plan names
`drovr:tdd`; I ran it as written and it is red. Rewriting the criterion to match the observed
behaviour is exactly the adjustment the task forbids.

**How it was run, and three limitations that bound it.**

Command shape (run 6; runs 3–6 identical but for the scratch project):

```
env -u DROVR_PHASE PATH=<worktree>/cli/target/release:$PATH claude -p \
  --plugin-dir <worktree> --settings <hook-wiring.json> \
  --output-format stream-json --verbose --permission-mode acceptEdits \
  "percent_change in calc.py gives the wrong answer when the value goes down. Fix it."
```

1. **`--plugin-dir` alone does not wire the plugin's hooks — only its skills.** §8's deployment
   note prescribes `CLAUDE_PLUGIN_ROOT=<worktree>` so the check does not depend on the flake pin.
   That loads `skills/`, and runs 1–2 confirm the skills work that way, but no `UserPromptSubmit`
   hook fired and the gate card never reached the model. Fix 2's whole hook layer was therefore
   **absent** from the method the plan prescribes. Runs 3–6 wire `hooks/hooks.json`'s two entries
   explicitly through `--settings`; the wrapper log proves both fired
   (`SessionStart rc=0 bytes=9493`, `UserPromptSubmit rc=0 bytes=646`). Filed in
   `docs/known-issues.md`.
2. **Runs 1–2 were not clean sessions.** `DROVR_PHASE` is set in this task's own shell, and
   `hooks/session-start` correctly no-ops when it is — the suppression contract working as
   designed. Runs 3–6 unset it. Runs 1–2 are kept in the table as the no-injection condition, not
   discarded.
3. **In runs 3–6 the environment blocked every tool except `Read`**, with
   `classifier unreachable: HTTP Error 404`. This is environmental and unrelated to drovr: a bare
   `claude -p "run: echo hello"` with no plugin and no settings fails identically. It appeared
   part-way through this task — runs 1–2 predate it. **The first-two-tool-calls datum is
   unaffected** (the calls are made, then error), so the §9.4 verdict stands; what cannot be
   observed in a fully clean session right now is the *"before writing code"* half. Run 3's
   attempted ordering (test written before the fix) is the best evidence available for it and is
   an attempt, not an applied edit. Filed in `docs/known-issues.md`.

## Step 5 — clippy and fmt. **RED, and pre-existing.**

```
$ cargo clippy --all-targets -- -D warnings
error: writing `&PathBuf` instead of `&Path` …          --> tests/web_nav.rs:83:25
error: manual implementation of `split_once`            --> tests/e2e.rs:71:16, :91:14, :104:24
error: this `if` statement can be collapsed             --> src/herdr.rs:1104:13
error: this assertion has a constant value              --> tests/skills_valid.rs:10686:5
error: this expression creates a reference which is immediately dereferenced
                                                        --> tests/skills_valid.rs:11693:30
error: manual implementation of `split_once`            --> src/review.rs:3195:20
error: doc list item without indentation                --> src/run.rs:1968:9, :1969:9

$ cargo fmt --check
Diff in … blocked.rs, brief.rs, config.rs, herdr.rs, interview.rs, main.rs, phase.rs,
          reflex.rs, review.rs, run.rs, rehydrate_http.rs, skills_valid.rs, web_nav.rs
```

**None of it is mine, and that was checked rather than assumed**, by the procedure
`docs/known-issues.md` prescribes for exactly this — extract the base version and format the
copy:

```
$ git show HEAD:cli/tests/skills_valid.rs > /tmp/head.rs
$ rustfmt --check --edition 2024 /tmp/head.rs   | grep -c 'Diff in'  → 36
$ cargo fmt --check | grep -c 'skills_valid.rs' → 36
```

Identical count, and every one of the 36 hunks is outside the four regions this task edited (they
run from line 4416 up; my edits sit at 6286–6320 and 7540–7580, and no hunk falls in either).
Both clippy sites in `skills_valid.rs` are likewise pre-existing — 10686 is the deliberate
`METERED_RUN_CEILING + 61 <= RUN_CEILING` guard-of-a-guard, untouched by me.

**I did not run tree-wide `cargo fmt`, and I did not "fix only files this task touched".** The one
file I touched is `cli/tests/skills_valid.rs`, whose entire drift predates me; formatting it would
sweep 36 unrelated hunks into this branch, which is the failure mode
`docs/known-issues.md`'s *"`main` is not `cargo fmt` clean"* entry says rebuilt `land-mcp-findings`
from scratch. §9.5 is red at the review base and red at this commit, by the same causes, in the
same files.

`docs/known-issues.md` already carries this as a known open item, including the standing fix
(one formatting-only commit on `main`, which needs a quiet moment rather than a branch mid-run).

---

## What a reader should take from this task

1. **Two of the run's eight measurement stages and its one pre-registered probe never happened.**
   `ab-code-review`, `ab-using-drovr` and `ab-voice` are all unrun, and the budget is closed.
2. **The reverts that *were* decided are applied and reconciled**, byte-identical to the pinned
   A′ snapshots, with both guards watched red first.
3. **One skill of five ships its rewrite** (`systematic-debugging`), on a +2-of-4 margin with no
   mechanism offered for it. Two revert. Two ship unmeasured armor, which is the second blocker.
4. **§9.4 is red.** The discipline engages — a drovr skill is the first tool call in a session
   that never mentions drovr — but not the skill the criterion names.
5. **§9.5 is red and was red before this task**, in files it did not touch.
