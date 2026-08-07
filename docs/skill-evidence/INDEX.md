# Skill-stickiness run — evidence index

**Written by Task 22 (`plan.md` §2), the run's final task.** One row per measured skill: the arm
counts, the decision, and what actually ships. Per `spec.md` §7.3, **nulls and negative results are
recorded here beside the positive ones** — two of the five skills were never measured at all, and
that is a row in this table rather than an omission from it.

This file is an index, not a verdict. Every number below is quoted from
`docs/skill-evidence/<skill>.md`, which stays the authority. Where a per-skill file supersedes its
own earlier verdict, the superseding number is the one quoted and the supersession is named.

---

## The five measured skills

| skill | unaided | arm A | arm A′ | arm B | branch | decision | ships today |
|---|---|---|---|---|---|---|---|
| `tdd` | 0/4 | **4/4** | 2/4 | 4/4 | (a) | **revert to A′** | arm A′ |
| `systematic-debugging` | 0/4 | 2/4 | 2/4 | **4/4** | (b) | **ship arm B** | arm B |
| `verification-before-completion` | 4/4 | **4/4** | 4/4 | 4/4 | (a) | **revert to A′** | arm A′ |
| `code-review` | 3/4 | — | — | — | none | **NOT MEASURED** | arm B, unmeasured |
| `using-drovr` | 2/4 | — | — | — | none | **NOT MEASURED** | arm B, unmeasured |

Counts are compliant runs out of 4 on the held-out pair. `unaided` is the `discrimination-test`
phase's no-skill control, which is an instrument measurement and enters no pre-registered bar.

### Margins, recorded rather than only the verdicts

| skill | B vs A′ | B vs A | A vs unaided |
|---|---|---|---|
| `tdd` | **+2 of 4** (+50 pp) | 0 | +4 of 4 (+100 pp) |
| `systematic-debugging` | **+2 of 4** (+50 pp) | **+2 of 4** (+50 pp) | +2 of 4 (+50 pp) |
| `verification-before-completion` | 0 | 0 | 0 |

---

## Per skill

### `tdd` — reverted to A′

- **Superseded once.** Task 16's `ab-tdd` verdict was reached on a pair an unaided agent passed
  3 of 4; `harden-scenarios` rewrote both bodies and `remeasure-tdd` re-ran the arms at 0/4
  unaided. The re-measured counts are the ones above. **The two sets must never be pooled.**
- **Branch (a) fired**: arm A compliant 4 of 4, which is ≥3 of 4, and §7.3 makes the Arm A bar
  unconditional. (b), (c), (d) were not evaluated.
- **The negative result, stated as the file states it:** branch (a) reverts `tdd` to the arm that
  scored *worst* of the three — A = 4/4, B = 4/4, A′ = 2/4. The B-over-A′ margin of +2 runs is the
  largest this run measured and it was **not acted on**, because the pre-registered order stops at
  the first branch that fires. That is a road not taken, not a second verdict.
- **REFACTOR: 0 runs spent** — unreachable from branch (a).

### `systematic-debugging` — ships arm B

- **The one positive result in the run.** Arm A 2/4, arm B 4/4, branch (b) fired, and the (c)
  override did not.
- **Superseded once, and the superseded section is kept.** Task 17 reached a branch-(a) revert on
  a scenario pair that did not discriminate; `remeasure-systematic-debugging` reversed it. That
  section is retained unedited under a `SUPERSEDED` banner because a superseded verdict with its
  reasoning intact is evidence about the instrument.
- **No mechanism is offered for the +2 margin**, and the file records that a draft of its own
  explanation was false and was cut: all three arms mention `bisect` exactly once, in the same
  position. The effect is recorded without a story attached to it.

### `verification-before-completion` — reverted to A′

- **Branch (a) fired** on arm A's 4 of 4.
- **The null that matters:** the unaided control also scored **4 of 4 with no skill text at all**,
  so this pair cannot separate arm A from no skill, let alone arm A from arm B. Every margin in
  the table is 0. The revert is correct under the rule but **rests on an absence of evidence, not
  on evidence that A′ suffices** — recorded here in the file's own words so no reader upgrades it.
- Its own limitations section says `vbc-2` should be rewritten before any re-measurement.

### `code-review` — NOT MEASURED

- `## Scored results` reads **"Not yet run."** No `blind-map.json`, no `scores.json`, no arm
  runs. `held_out_scores()` reads `NotYetRun`. `## Failure and reverted state` reads **"Not
  applicable yet. No bar has been evaluated for this skill."**
- The `discrimination-test` control returned **3 of 4 compliant unaided — SATURATED** against the
  bar pre-registered before those runs, and `code-review-3` is saturated at 2 of 2. The recorded
  consequence is that this pair is not worth spending arm runs on until `code-review-3` is
  rewritten.
- **`skills/code-review/SKILL.md` ships arm B, and arm B was never measured for it.** That is
  stated here as the open item it is, not resolved. See `task22-report.md`.

### `using-drovr` — NOT MEASURED

- `## Scored results` reads **"Not yet run."** Same state as `code-review`. Note the plan's ruling
  that this skill would have been compared B-against-A alone, with no A′ arm.
- The `discrimination-test` control returned **2 of 4 unaided**, and the pair splits exactly in
  half: `using-drovr-2` discriminates at 0 of 2, `using-drovr-3` is saturated at 2 of 2. Reporting
  only "2 of 4" would hide that, so both halves are here.
- **The veto class (`using-drovr-noskill-1/2`) was never probed in either direction.**
- **`skills/using-drovr/SKILL.md` ships arm B, and arm B was never measured for it.**

---

## The voice probe (§7.4) — PRE-REGISTERED, NEVER RUN

`voice.md` is a genuine pre-registration: the rule, the ≥3-of-6 margin, the four outcomes and the
escalation branch were all fixed by Task 15 before any run existed, and `git log --follow` shows
the pre-registration commits (`5cc7412`, `cc8a317`) with no results commit after them.

**Task 21 never ran.** `voice.md`'s `## Results` section still reads *"NOT YET RUN. Nothing has
been run. This section is a placeholder for Task 21 and holds no data."*
`docs/skill-evidence/transcripts/voice/` does not exist, and `run-ledger.md` carries no `ab-voice`
row. The four variants `V0`–`V3` exist, are hash-pinned in `arms/MANIFEST.md`, and pass all five
separability checks — the instrument was built and never used.

**Consequence:** Task 22 applied no register change to any document. See `task22-report.md`.

---

## The cross-model arm — EXPLORATORY, binds nothing

`cross-model.md` records 78 probes on `qwen` and `opus`. It was added **after** the pre-registered
bars and is explicitly **not confirmatory**. It changed no ship/revert decision and is excluded
from every table above. Two of its planned cells are recorded as **nulls** rather than paid for
with a raised cap:

- `506659` — `systematic-debugging`, arm B, `sd-2`, sample 2 — 0 attempts, never dispatched.
- `d6dc83` — `tdd`, unaided, `tdd-2`, sample 4 — 1 attempt, failed, no retry available.

Two cells therefore carry n=7 rather than n=8.

---

## Run cost

**187 probe runs, cumulative, as `run-ledger.md`'s final row records.** Reported, not tested
against a limit — the run-count ceiling was lifted by the human on 2026-08-07. The ledger's
cumulative column is still checked as arithmetic by
`run_ledger_cumulative_is_a_running_total`, so a dropped or inflated row is still caught.

Scorers are not charged: 78 blind scorer passes ran, plus a second independent pass over 16 of
them which agreed 16 of 16. A scorer re-reading an existing transcript produces no new
measurement.

---

## What every result here is bounded by

Carried forward from the per-skill files rather than restated more weakly:

1. **n=4 per arm per skill.** Only large effects are detectable. Nothing here is more than
   suggestive.
2. **Label-blind, not arm-blind.** Blinding removes the arm label, its skill text and the
   announcement string, but a `cites_section: true` verdict identifies an armored arm with near
   certainty, and an armored agent's prose reads differently. Do not describe the scoring as fully
   blind anywhere.
3. **Two skills of five were never measured**, and one probe (voice) was pre-registered and never
   run. Three of the run's eight planned measurement stages did not happen.
4. **The RED baselines were not blinded at all** and were read by the orchestrator, who knew the
   arm.
