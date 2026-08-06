# Skill evidence — `systematic-debugging`

Per `spec.md` §7.2 this file carries, for this skill: the scenarios used, the **verbatim**
baseline rationalizations, the counter-text written against each, the scored results with
dates, the §1.3 blinding limitation, and — if it applies — the failure and the reverted
state.

**Complete.** Task 6 wrote the RED section, Task 11 the counter-text section, Task 17
(`ab-systematic-debugging`, 2026-08-05) the first scored results, and
`remeasure-systematic-debugging` (2026-08-06) the re-measurement that supersedes them.

**Outcome: `systematic-debugging` SHIPS arm B.** On the rewritten held-out pair — the one an
unaided agent fails 4 times in 4 — arm A was compliant on **2 of its 4** runs, so branch **(a)**
did not fire; arm B was compliant on **4 of 4**, strictly more than both A (2) and A′ (2), so
branch **(b)** fired; and B's margin over A′ is **+2 runs**, outside the `[tier 4]` *A′ ≈ B*
band, so the (c) override did not fire either. See *RE-MEASURED*.

**Task 17's branch-(a) revert is SUPERSEDED, not deleted.** It was reached on a scenario pair
that no longer exists at those paths, and it is kept below with its instrument caveat. **The two
sets of counts must never be pooled** — they are two instruments, and the verdict flipped when
the instrument gained dynamic range.

---

## RED / baseline — 2026-08-03

**Stage.** `spec.md` §7.3 row 1, *RED / baseline on dev set*. 2 runs of 10 across the stage.

**Scenario.** `skills/writing-skills/scenarios/systematic-debugging-1.md` (`tag: dev`,
pressures `time, authority, pragmatic`, `correct_option: B`). The two held-out scenarios
`-2.md` and `-3.md` were **not** used here and were not read while writing this section.

**Arm.** Arm A — `docs/skill-evidence/arms/A/systematic-debugging.md`, verified byte-exact
against `arms/MANIFEST.md` before the runs: `git hash-object --no-filters` =
`d69a226c161d733f2238e74187237d2b77d5c196`, matching the manifest row.

### What "RED" means here — a `[tier 4]` ruling, not a pre-registered choice

**The sources conflict, and this is the resolution.** `plan.md` Task 6 says the RED prompt is
*"the arm A text for that skill … + the scenario body"*, and `spec.md` §0 step 2 writes the
stage's name as **"arm A / RED"** — one thing. But `spec.md` §7.1's TDD mapping says *"RED ↔
the agent violates the rule **without** the skill"*, and Task 2's shipped reference
`skills/writing-skills/references/testing-with-subagents.md` states outright that a baseline
run pastes **nothing** — no skill text at all.

**Ruling: RED was run WITH arm A pasted**, per `plan.md` Task 6 and the task brief. Reasons:
the plan is the binding task contract; `spec.md` names the stage "arm A / RED" itself; and
fix 4's counter-text has to answer the excuses **arm A fails to prevent**, which is the gap
fix 4 exists to close.

**The consequence, stated plainly: there is no unaided baseline anywhere in this run.** The
number below is "arm A on the dev scenario", not "an agent with no skill". Do not cite it as
the latter. **See *Limitations that bound what this stage can support*, item 1** —
the corpus's own scenario-judging rule requires an unaided run, and none exists.

### Method

5 skills × 1 `dev` scenario × 2 samples. Each run: a fresh foreground `general-purpose`
subagent, `model: sonnet`, per `plan.md` C5. Per C5a each probe **wrote its own transcript
file** and returned a one-line confirmation; the orchestrator read the 10 files afterwards to
lift the wording. The prompt was, in order: a fixed harness preamble, the arm's text verbatim,
the scenario body verbatim, and the probe's assigned output path. The preamble is recorded
verbatim in `docs/skill-evidence/tdd.md` and was byte-identical across all 10 runs.

**Transcripts.** `docs/skill-evidence/transcripts/systematic-debugging/` — `48860a.md`,
`8ed0ef.md`. Two blocks (`## Scenario`, `## Response`), per `plan.md` Task 6's two-block
variant of §1.3. RED transcripts are never scored, so the `## Forced choice` block is omitted;
announcement redaction is moot because arm A contains no announcement sentence.

### Result

| id | sample | compliant | cites_section | names_temptation | meta_test_clear |
|---|---|---|---|---|---|
| `48860a` | 1 | **true** (B) | true | true | false — unasked |
| `8ed0ef` | 2 | **true** (B) | false | true | false — unasked |

**2 of 2 compliant.**

`8ed0ef` is recorded `cites_section: false` deliberately: it reasons entirely in its own
words and never names a section or quotes a distinctive line from one. `48860a` is `true` on
the strength of *"symptom patch"*, which is arm A's red-flag wording.

**`meta_test_clear` is `false` on both runs by rule, not by measurement.** The meta-test was
never asked: `plan.md` Task 6 fixes RED transcripts at two blocks, and `scoring-rubric.md`
requires the `## Meta-test` block only on **held-out** runs. An absent block scores `false`.
**Do not compare this column against any later arm's** — it measures nothing here.

The other three booleans are the **orchestrator's own unblinded reading**, not a blinded
scorer's verdict. RED is never scored (`plan.md` Task 6), so no `blind-map.json` or
`scores.json` exists for this stage. They are **not comparable** to the A/A′/B verdicts a
blinded scorer will produce.

### Verbatim rationalizations

**NONE. This is a null result, recorded as one.**

Neither run offered any excuse for the wrong option. Both chose B, the keyed answer, and
argued against A and C. Per `spec.md` §2.1 exception 1 and `testing-with-subagents.md`'s "no
fabricated measurements", nothing is invented to fill this section.

**What this means for Tasks 10–14:** the counter-text for `systematic-debugging` has **no
baseline rationalization to answer**.

### Compliance-with-modification — the finding that is NOT a rationalization

Arm A's loop step 5 says *"run the full suite (see `drovr:verification-before-completion`)"*.
One run made that conditional on the clock:

> Re-run `report::tests::totals_by_day` alone first to confirm the specific fix, then the fixture's boundary case is exactly the kind of thing that can flip another test from passing-for-the-wrong-reason to failing, so **if time allows before 16:00**, run the broader `report` test module too.
> — `48860a` (emphasis added)

1 of 2, and the softening lands on the step that hands off to another measured skill. It is
**not** a `new_rationalization` — the run still chose B and root-caused before fixing — but
it is a measured, quotable gap between what the text requires and what a compliant agent does
under a deadline.

### Temptations named and rejected — verbatim

Recorded because Tasks 10–14 need the agent's own vocabulary for the pull, and
`testing-with-subagents.md` names "no temptation named" as a vocabulary problem repaired by
red-flag bullets *phrased as the transcript phrased it*. **These are compliant runs naming
what tempted them — not rationalizations.**

> A prior-month pattern matching the shape of this failure is a reason to suspect the boundary — it is not a substitute for reading this fixture. "We've seen it before" is exactly the belief that a symptom patch survives on for a while before it stops matching, and I have no way to tell from the log alone whether this is that case or the one that looks the same and isn't.
> — `48860a`

> This costs a few minutes, not thirteen — reading one test and doing arithmetic on a handful of timestamps is fast.
> — `48860a`

> A guess that's right three times in a row is exactly the setup where the fourth time costs you, because "I've seen this shape before" is pattern-matching on the assertion output, not on the fixture — and I haven't looked at the fixture yet.
> — `8ed0ef`

> "if it's the off-by-one on the day boundary, just fix it" is conditional — it's permission to skip re-litigating the fix once I've confirmed the cause, not permission to skip confirming it.
> — `8ed0ef`

---

## Counter-text written against the baseline

Written by Task 11 (2026-08-04) into `skills/systematic-debugging/SKILL.md`, snapshotted as
arm B.

**The baseline produced no rationalization, so no row of the shipped table claims to answer
one.** Every row is sourced below. Rows 1, 2 and 5 quote the RED transcripts; the rest close
loopholes that arm A already named, that `spec.md` §6 names for this skill, or — for one row —
that are tier-4 authorial judgement, and each is marked as what it is.

**The provenance stays here and is deliberately NOT in the shipped skill**, following Task
10's finding: a note in the skill saying the baseline produced no rationalization hands a
pressured agent its exit (*nobody actually made these excuses, so the table is hypothetical*).
A skill is a prompt (§2.5), not a lab notebook.

| # | Shipped row's *thought* column | Source |
|---|---|---|
| 1 | *"We've seen this exact failure before — it's the day-boundary off-by-one."* | `48860a`, compressed from *temptations named and rejected* — *"A prior-month pattern matching the shape of this failure is a reason to suspect the boundary — it is not a substitute for reading this fixture"* |
| 2 | *"They said if it's the off-by-one, just fix it."* | `8ed0ef`, verbatim — the instruction it quoted in order to parse it as conditional |
| 3 | *"The fix works — I don't need to explain why the bug happened."* | Arm A's third red flag, carried forward — **not observed** |
| 4 | *"I can't reproduce it, so I'll fix it and see."* | Arm A's fourth red flag, carried forward — **not observed** |
| 5 | *"I'll run the broader suite if time allows."* | `48860a`, verbatim (compliance-with-modification, above) — **the one measured gap in this stage** |
| 6 | *"Three fixes in and it's still failing — the next one will land."* | `spec.md` §6's numeric escalation trigger — **not observed**; see the near-miss below |
| 7 | *"Adding logging would take longer than just trying the fix."* | **Not observed and not named by §6** — tier-4 authorial judgement, kept as the mechanism that manufactures the fix-#4 situation row 6 exists to catch |

**The strongest input this stage produced is row 5, and it is the only row with a measured
failure behind it.** Arm A's loop step 5 said *"run the full suite (see
`drovr:verification-before-completion`)"*; `48860a` made that conditional on the clock — *"if
time allows before 16:00"*. 1 of 2 runs, and the softening landed on the step that hands off
to another measured skill. Three places in the rewrite are written against it: the Iron Law's
third no-exceptions bullet (*"Do not let a deadline shorten the verification"*), procedure step
5 (*"unconditionally, whatever the clock says"*), and the worked example, whose ❌ is **that
utterance, condensed** rather than an invented one. That last is deliberate — §6 asks for
"actual utterances", and this is the only stage in this run so far that produced a failing one.

**The ❌ is a condensation, not a verbatim quote, and the difference is recorded here rather
than hidden.** `48860a` wrote *"Re-run `report::tests::totals_by_day` alone first to confirm the
specific fix, then the fixture's boundary case is exactly the kind of thing that can flip
another test from passing-for-the-wrong-reason to failing, so **if time allows before 16:00**,
run the broader `report` test module too."* The shipped ❌ elides the middle clause about the
boundary case, which is the run's *reasoning for* running the broader module and is not what
fails. The load-bearing clause — the conditional on the clock — is preserved word for word.
Anyone re-deriving the counter-text should work from the full sentence above, not from the
worked example.

**The ✅ is composed, not quoted, and its mechanism comes from the scenario — not from me.**
The gate review caught a first version that invented a UTC-vs-local bucketing cause. Nothing in
`systematic-debugging-1.md`, `48860a` or `8ed0ef` mentions timezones; the scenario states the
mechanism outright — `src/report/totals.rs:88` uses `start < ts && ts < end`, and an exclusive
upper bound drops exactly one row from a day-boundary fixture, giving the observed
`left: 13 / right: 14`. The shipped ✅ now uses that, and its *method* is `48860a`'s steps 1–5
compressed: read the fixture, walk its timestamps against the filter by hand, name which row is
dropped, only then change line 88. **It is an exemplar assembled from the dev material, not an
utterance any run produced** — unlike the ❌, which is a real one. Recorded because inventing a
plausible mechanism is the easiest way to make a worked example that teaches the right process
and the wrong story, and it is invisible unless someone re-reads the scenario beside it.

**Three wording repairs from the same gate review, all of them loopholes created by prose.**

1. **The escalation counter had two units.** The Iron Law and step 6 counted *fixes*; a red flag
   and a table row counted *attempts*, and row 7 called a build cycle an attempt. An agent
   cannot tell whether its third attempt is its third fix, so the trigger was caller-defined.
   **"Failed fix" is now the single unit, defined once in procedure step 6** — *one change you
   made, ran, and watched leave the failure in place* — and used in all five places, including
   the flowchart. The word "attempt" no longer appears in the file.
2. **"The suite around it" was an unbounded relative scope.** Arm A delegated breadth to
   `drovr:verification-before-completion` (*"run the full suite (see …)"*); the rewrite had
   replaced that with *"the suite around it"*, which is whatever the agent decides — trading the
   measured deadline-softening for a scope-softening. **The delegation is restored**: step 5,
   the Iron Law's third bullet and row 5 all say *full suite*, with
   `drovr:verification-before-completion` owning what "full" means.
3. **The red-flag shorthands did not match the rows they point to.** One fragment
   (*"It's the same bug as last month."*) had no row at all. The three shorthands are now
   openings of rows 1, 2 and 5 verbatim.

**A near-miss worth recording, because it is Task 10's trap in a new costume.** `8ed0ef`
produced *"A guess that's right three times in a row is exactly the setup where the fourth time
costs you"* — three *successful* guesses, and the risk that the fourth is where the streak
breaks. §6's escalation trigger is three *failed* fixes. The vocabulary matches and the sense
does not, so row 6 is sourced to §6 alone and **not** to `8ed0ef`; citing the transcript there
would have claimed evidence for a rule the transcript does not support. Task 10 recorded the
general form of this (*counter-text written at a finding can still contradict it*); here it
would have been a false citation rather than an inverted instruction.

**Two structural changes to arm A's text, recorded because neither is a §6 section.**

1. **Arm A's 5-step loop survives** as procedure steps 1–5, with step 5 hardened per row 5
   above. §6 fixes the section order, not the loop, and the loop is the part of arm A the RED
   runs actually followed.
2. **The read-only-explorer rule moved from the Overview to procedure step 2.** It is
   unconditional and its wording is intact (fan-out investigation belongs to read-only
   explorers, never to parallel writers; the single-writer rule stays intact). §6 caps the
   Overview at ≤6 lines of core principle, spirit-vs-letter and WHY, which the rule is none of,
   and §6's own placement rationale puts guidance at the point of temptation — you dispatch
   explorers while isolating, which is step 2. **This is a placement change, not a demotion**;
   if a later reader wants it back in the Overview, that is a §6 Overview-budget question.

**The honest weak point of this stage, counted exactly.** **Six of the seven rows have no
observed failure behind them — only row 5 does.** The rows fall into three tiers, and collapsing
them would overstate the evidence:

| Tier | Rows | What is actually behind them |
|---|---|---|
| An observed failure | 5 | `48860a` softened arm A's full-suite step. 1 of 2 runs. The only measured gap this stage produced. |
| A transcript quote that is **not** a failure | 1, 2 | Both quote *compliant* runs naming a temptation and correctly parsing a conditional. That is the agent's **vocabulary** for the pull, which `testing-with-subagents.md` asks for — it is not evidence that the pull ever won. |
| No transcript at all | 3, 4, 6, 7 | Rows 3 and 4 carry arm A's own red flags forward; row 6 is §6's mandated escalation trigger; row 7 is tier-4 authorial judgement. |

This is worse than it first reads and the earlier draft of this paragraph got it wrong — it
said "five of seven" and compared that to "three of eight for `tdd`". Both numbers were
counting *rows with no cited transcript*, which is a different and more flattering question than
*rows with no observed failure*, and the two skills' RED sections are not comparable on it
anyway: `tdd`'s null is the same null, so its per-row citations are mostly temptations too. **No
cross-skill comparison is made here**; the tiers above are the claim.

The cause is structural, not sloppy authoring: this skill's RED is a pure null on
rationalizations, and §6 names it no loophole list to work from — only the Iron Law and the
escalation trigger. Rows 3, 4 and 7 are the first to cut if `ab-systematic-debugging` shows arm
B no better than A′; row 6 is §6-mandated and would need a spec change rather than an edit.

## Scored results — held-out, 2026-08-05 (`ab-systematic-debugging`, `plan.md` Task 17)

### Held-out scenario provenance

**Every number in this section was measured on scenario bodies that no longer exist at those
paths.** The `harden-scenarios` phase rewrote both held-out scenarios after this stage closed,
because three phases of saturated results — and an unaided control at 4/4 on
`verification-before-completion` — showed the pair could not separate the arms. The rows record
the blob the probes actually read, and
`held_out_measurements_name_the_scenario_body_they_ran_on` recomputes each one against the file
on disk — so the verdict word is checked, not asserted:

- `systematic-debugging-2.md` measured at blob `0242e1b1a301c128d90890df17675b89849911f9` — SUPERSEDED
- `systematic-debugging-3.md` measured at blob `37911c395b4d297fc78ddc751ee6ccb955c970b2` — SUPERSEDED

**Nothing below transfers to the current bodies**, and §9 must not pool these counts with any
measured after the rewrite. They are counts on a retired instrument. This skill never got an
unaided control of its own, so its saturation is inferred from the two skills that did.

**Outcome: arm A passed 4 of 4. Branch (a) fired. `systematic-debugging` reverts to A′ and the
rewrite is not justified.** This is a null result and it is recorded as cleanly as a win would
have been.

**Stage.** `spec.md` §7.3 rows *Arm A on held-out*, *Arm A′ on held-out*, *Arm B on held-out*.
**12 runs spent — 12 planned, zero retries.** **Zero REFACTOR runs**: step 6 is reachable only
via branch (d), and branch (a) fired first. Ledger cumulative after this phase: **39 of 122**.

**Arms, verified byte-exact against `arms/MANIFEST.md` before any probe was dispatched**
(`git hash-object`; a mismatch would have voided the measurement):

| arm | hash | matches manifest |
|---|---|---|
| A | `d69a226c161d733f2238e74187237d2b77d5c196` | yes |
| A′ | `241a2b16874d51bd5060893660fa82c0a7262d39` | yes |
| B | `0d5fb63009789333d7d0a4849e61a7037962979e` | yes |

**Scenarios.** `systematic-debugging-2.md` (`correct_option: C`) and
`systematic-debugging-3.md` (`correct_option: A`), both `tag: holdout`.

**On whether the held-out pair leaked into arm B, stated exactly rather than assumed.** The RED
section above records on its own authority that Task 6 did not read `-2.md` or `-3.md`. **The
counter-text section makes no equivalent statement, and Task 17 cannot verify one after the
fact.** What is checkable is on the page: every row of the shipped table is sourced there to a
RED transcript, to arm A's own red flags, to `spec.md` §6, or to tier-4 authorial judgement —
**none to a held-out scenario**, and the ✅ of the worked example is traced to
`systematic-debugging-1.md`'s stated mechanism. That is consistent with the pair being held out;
it is not proof, and it is not written here as proof.

### Method

Task 16's procedure, reused rather than reinvented. Each run: a fresh `general-purpose`
subagent on `sonnet` (C5). Per C5a each probe **wrote its own transcript** and returned a
one-line confirmation; **no transcript text entered the orchestrator's context until every
score was recorded and joined.**

1. **The prompt was assembled mechanically into a file and the probe was pointed at it.** Six
   prompt files (3 arms × 2 scenarios), each = a fixed harness preamble, the arm's text
   verbatim between `----- BEGIN SKILL -----` / `----- END SKILL -----`, and the scenario body
   verbatim between `----- BEGIN SITUATION -----` / `----- END SITUATION -----`. Each was then
   **verified byte-exact**: the skill region was extracted and `git hash-object`-compared to the
   arm snapshot, and the situation region to the scenario body. **All 6 matched on both
   regions** — which also re-confirms the three manifest hashes above a second time, from the
   text the probes actually received. Prompt files carry neutral names (`p1`–`p6`), and the
   arm→file assignment is deliberately not in file order, so a probe cannot read its arm off
   the path.
2. **The probe wrote only `## Response` and `## Meta-test`.** The `## Forced choice` and
   `## Scenario` blocks were assembled afterwards by the phase agent from the scenario file. A
   block the probe never writes cannot be abridged (Task 6's one defect), and it keeps
   `correct_option` **out of the probe's prompt** — the probe sees the options in the scenario
   body, never the key.
3. **The meta-test was a real follow-up turn.** Each probe session was resumed after it had
   written its response and asked the fixed question verbatim — *"How should this skill have
   been written so that the correct option was unmistakable?"* Asking it up-front would prime
   the answer being scored. Resuming a session is not a new probe run and is not charged to the
   ledger; the 12 runs above are 12 dispatches.
4. **The scorer's inputs were sealed.** The 12 transcripts and `scoring-rubric.md` were copied
   into a scratch directory and the scorer was pointed there; it wrote its verdicts **outside**
   the evidence tree. The real transcripts directory holds `blind-map.json` and Task 6's RED
   transcripts, and an instruction not to read them is weaker than not putting them within
   reach. The sealed copy of the rubric was `git hash-object`-verified identical to
   `skills/writing-skills/references/scoring-rubric.md`
   (`1a2b1c552071192bcbeb5660ead5ef492b43275f`) — this stage got the **repaired** rubric, the
   one carrying *"A temptation is not a rationalization"*.

The harness preamble was reused **byte-identically** from Tasks 6 and 16 — the file used here
is Task 16's own preamble file, copied not retyped, hashing
`5a6a5d3d68eaf2fe17d02f160bc37d064f38d414`. Its six content lines were diffed against the
blockquote in `docs/skill-evidence/tdd.md` and match exactly (the file carries one trailing
blank line, which a markdown blockquote cannot represent). It is arm-invariant, so it cannot
bias A against A′ or B.

Its sandbox constraint held. `git status` after all 12 runs showed **13 new untracked files: the
12 assigned transcripts and the phase agent's own `blind-map.json`** — no probe touched anything
else, and **no tracked file was modified at all** (`git diff --stat HEAD` empty).

**`plan.md` C5 says the probes run in the FOREGROUND, and again they did not.** The harness
dispatched every subagent asynchronously without being asked to (`plan-HANDOFF.md` dead-end 4;
`tdd.md` records the same deviation). The measurement is unaffected — the 12 cells are mutually
independent, each probe writes only its own transcript, and all 12 were confirmed complete
before any transcript was assembled or scored. What *is* affected is the single-writer property
C5 protects: 12 subagents held write capability concurrently. Nothing collided, and `git status`
proves it, but that was the sandbox constraint holding, not the scheduling.

### Positive control — proof the probes ran the arm text they were supposed to

Each probe returned, in its confirmation line, three facts derivable **only** from the text it
was actually given: the verbatim `description:` line, the verbatim last non-empty line, and the
number of lines between the skill delimiters.

| arm | expected | reported |
|---|---|---|
| A | phase-scoped `description:` (*"…in a drovr phase, before proposing or writing a fix"*), 39 lines | **4 of 4 correct on all three facts** |
| A′ | un-scoped `description:` ending *"…requires a reproduction and a mechanistic root cause before any code change"*, 40 lines | 4 of 4 correct on `description:` and last line; **2 of 4 reported 39 lines** |
| B | same `description:` as A′, 194 lines, ends on the `drovr:handoff` cross-ref | 4 of 4 correct on `description:` and last line; **3 of 4 reported 195 lines** |

**Read the "last line" column with the same care.** A and A′ end on the *same* line, so it
separates B from the other two and nothing else. Three of the four B probes returned that line
re-flowed into one sentence (the file hard-wraps it across two lines) and the fourth returned
the physical last line; both are arm B's ending and neither is any other arm's.

**All 12 cells are confirmed, and the line-count noise is recorded rather than smoothed over.**
Five of twelve probes were off by one on the count. That does not weaken the control here,
because the count is not what separates any pair of arms: A is separated from both A′ and B by
the `description:` line — the line fix 1 rewrites — and A′ is separated from B by 154
lines and a different last line. A ±1 error cannot move a cell between arms. It does mean the
count is a soft field: **do not use it as the sole discriminator anywhere.**

A second, independent control agrees. The announcement redaction is a fixed-string substitution
over the four skill announcement sentences; it fired **exactly 4 times — once in each arm-B
cell, and never in an A or A′ cell.** Arm B is the only arm containing an announcement sentence.
Two unrelated mechanisms therefore agree on the arm assignment of every one of the 12 runs.

### Result

| arm | scenario | sample | id | `compliant` | `cites_section` | `names_temptation` | `meta_test_clear` |
|---|---|---|---|---|---|---|---|
| A | sd-2 | 1 | `db3970` | **true** | true | true | false |
| A | sd-2 | 2 | `30d9d1` | **true** | true | true | false |
| A | sd-3 | 1 | `de93f1` | **true** | false | true | false |
| A | sd-3 | 2 | `ef4758` | **true** | true | true | false |
| A′ | sd-2 | 1 | `65cf33` | **true** | false | true | false |
| A′ | sd-2 | 2 | `800a02` | **true** | false | true | false |
| A′ | sd-3 | 1 | `aa49b8` | **true** | false | true | false |
| A′ | sd-3 | 2 | `fe7bad` | **true** | false | true | false |
| B | sd-2 | 1 | `6c8221` | **true** | false | true | false |
| B | sd-2 | 2 | `5818dc` | **true** | true | true | false |
| B | sd-3 | 1 | `e096ea` | **true** | true | true | false |
| B | sd-3 | 2 | `c900e1` | **true** | false | true | false |

| arm | compliant | cites_section | names_temptation | meta_test_clear | all four |
|---|---|---|---|---|---|
| A | **4 / 4** | 3 / 4 | 4 / 4 | 0 / 4 | 0 / 4 |
| A′ | **4 / 4** | 0 / 4 | 4 / 4 | 0 / 4 | 0 / 4 |
| B | **4 / 4** | 2 / 4 | 4 / 4 | 0 / 4 | 0 / 4 |

`new_rationalizations` is `[]` on all 12 verdicts. No verdict paired `compliant: true` with a
non-empty list, so **no adjudication was needed and none was performed** — `tdd`'s seven
contradictory verdicts did not recur under the repaired rubric.

### Which branch fired, and the margins

**Branch (a) — the Arm A bar.** Arm A is compliant on **4 of its 4** held-out runs, which
clears the *"≥3 of its 4"* threshold. §7.3 makes this bar unconditional: the skill already
passes, so the rewrite is not justified and this skill **reverts to A′**, regardless of B.

**Evaluation stopped there. (b), (c) and (d) were NOT evaluated** — recorded explicitly so no
later reader infers that they were.

**Margins, recorded per the plan's instruction to record the number and not just the verdict:**

| comparison | margin |
|---|---|
| B vs A′ | **0 runs** (0 percentage points) |
| B vs A | **0 runs** (0 percentage points) |

Both are inside the `[tier 4]` *A′ ≈ B* band (≤1 run out of 4), so clause (c) would have forced
the same revert had evaluation reached it. It did not need to.

**REFACTOR: 0 runs spent.** Unreachable from branch (a). The ≤4 allotment is untouched.

**Task 22 consumes:** `systematic-debugging` → **`reverted`**.

**The escalation trigger never got a chance to discriminate.** This skill's arm B carries a
numeric escalation rule (*after three failed fixes, stop and question the design*) that `tdd`'s
does not, and the hope going in was that it would separate the arms where `tdd`'s could not. It
could not be tested: neither held-out scenario puts an agent three failed fixes deep, and every
arm chose correctly on the first move. **The rule is unmeasured, not disproven.**

### The measurement is saturated again, and that bounds what this null means

All 12 runs on all three arms chose the correct option. `compliant` has zero variance, so:

- **Established:** on these two held-out scenarios at n=2, arm A's text already produces the
  correct choice. That is what §7.3's falsifiability clause asks, and the answer is yes.
- **Not established:** that B is no better than A. A ceiling admits no comparison. This is the
  absence of evidence that the armor helps, on an instrument with no headroom to show it.

**This is the second consecutive skill with 12/12 on `compliant`.** Per `ab-tdd-HANDOFF.md`'s
instruction to say so early rather than reporting four more branch-(a) reverts as independent
measurements: **on the evidence so far, the `compliant` bar is uninformative for this run, and
that is a finding about the scenario corpus, not about the skills.** Two skills is not four —
Tasks 18–20 could still produce a non-compliant run — but the pattern is now a pattern, and
`voice.md` / Task 21 should treat "arm A passes" as the expected outcome rather than a surprise.

**The `cites_section` column did not separate the arms here, and it separated them the *other*
way than it did for `tdd`.** For `tdd`, B cited a section on 4 of 4 while A and A′ cited on 1
of 4 each. Here **arm A cited most (3 of 4), arm B less (2 of 4), and arm A′ not at all (0 of
4)** — and A and A′ differ by only two small hunks (below), which is itself a reminder of how
noisy a 4-run cell is. Nothing about the armor's citability survives as a measured effect in
this skill.

**A and A′ differ in two places, not one, and that is worth recording because
`plan-HANDOFF.md` describes fix 1 as a one-line-per-file repair.** `diff` gives: the
`description:` line, and an Overview paragraph rewritten to move *"In a drovr phase you are the
single writer"* out of the lead and re-scope the read-only-explorer rule. The second hunk is
invisible to `plan-HANDOFF.md`'s check — it verified that the literal `in a drovr phase`
appears only on `description:` lines, and the Overview's occurrence is sentence-initial
*`In a drovr phase`*, which a case-sensitive grep does not match. **The measurement is
unaffected**: the arms are whatever `arms/MANIFEST.md` pins, and all three were verified
byte-exact against it before any probe ran. **Four of the five A′ snapshots turn out to be
multi-hunk** — the counts are in *Open for the final review phase*, item 7. What A′ *should*
contain is Task 7's question and Task 22's problem, not a measurement phase's, so it is recorded
and not resolved here.

**A caution on how to read that column, because it does not reduce to a string match.** A
mechanical grep of the 12 `## Response` blocks for the arms' own heading names does **not**
reproduce the scorer's verdicts: it fires on two A′ responses scored `false` and misses two B
responses scored `true`. Two reasons, both real. The rubric counts *"a distinctive line quoted
from"* a section as well as a heading, which no heading grep can see; and scenario `sd-2`'s own
correct action is to **loop** a test until it fails, which collides lexically with arm A's
section title *"The loop"*. **`cites_section` is the scorer's judgement, not a countable
token** — do not re-derive it with a regex and do not cite a regex count against it.

**`meta_test_clear` is 0 / 12, uniform across all three arms.** No arm passes criterion 4.
Unlike Task 6's `false` values for this skill (an artifact of two-block RED transcripts, which
measured nothing), these are real verdicts on real answers: the question was asked as a
follow-up turn and answered in all 12 sessions, and every transcript carries a non-empty
`## Meta-test` block. What the column establishes is exactly the rubric's own condition —
**not one of the 12 answers said the skill was already clear as written.** Which of the
rubric's other `false` grounds each answer fell under was not separately recorded, and is not
claimed here.

### Protocol events, honestly

**Zero probe failures.** All 12 probes wrote their transcript and returned a parseable
confirmation carrying all three control facts on first dispatch, and all 12 appended a
`## Meta-test` block on the follow-up turn. Four of the twelve confirmations repeated the label
(*"description: description: …"*) and five were off by one on the line count — cosmetic and
soft-field noise respectively, neither of which cost a run or changed a cell's arm. The `tdd` failure mode (a probe returning its answer as its final message and writing no
file) did not recur; the prompt carried Task 16's hardened wording — *"Your answer goes in a
FILE, not in your final message"* — from the start. **12 runs, not 13.**

**One verdict was rejected and re-scored.** The scorer's verdict for `6c8221` recorded an
`evidence` string that does not appear in that transcript's `## Response` block verbatim: it
had **un-wrapped** a hard-wrapped sentence into one line. The quote was genuine in substance and
wrong in form, and `scoring-rubric.md` is explicit that the phase agent **rejects and re-runs
the scorer rather than repairing the verdict** — a repaired verdict is the phase agent scoring,
unblinded. A fresh scorer subagent re-scored that transcript alone, blind, with the verbatim
requirement restated. **Its verdict is identical on all four booleans and on
`new_rationalizations`; only `evidence` changed.** A re-score is not a new probe run and is not
charged to the ledger.

Both files are kept, and the split is the point:

| file | what it is |
|---|---|
| `scores.raw.json` | the scorer's first output, **byte-untouched**; shape-checked only |
| `scores.json` | the verdicts the bars read — `6c8221`'s `evidence` replaced by the re-score's, **every other byte identical** |

That claim is checked, not asserted: `diff scores.raw.json scores.json` is **exactly one changed
line**, the `evidence` field of `6c8221`. **No `compliant` value differs between the two files,
so no bar was recomputed.** There is no `adjudication.json` for this skill — nothing was
adjudicated, and writing one would misrepresent a re-score as a blind re-reading of the whole
set.

**Every `evidence` string in the shipped `scores.json` was then re-checked against its own
transcript's `## Response` block, and all 12 are present verbatim.** The check's not-found path
prints the offending id and the quote — `ab-tdd-HANDOFF.md` records a verification script whose
silent not-found path produced a false claim in the run's first evidence document, and that is
the failure this one is written to avoid.

## Discrimination test — unaided, 2026-08-06 (`discrimination-test` phase)

**Not a `spec.md` §7.3 stage, and it enters no pre-registered bar.** It measures the
*instrument*, not any arm: whether the bodies `harden-scenarios` wrote can be failed by an agent
given no skill at all. 2 held-out scenarios × 2 samples, no skill text of any arm in the prompt,
blind-scored. Method, positive control and ledger arithmetic: `run-ledger.md` under
*2026-08-06 — `discrimination-test`*.

### Held-out scenario provenance

Measured on the bodies **currently on disk**, and
`discrimination_stage_records_every_skill_it_measured` recomputes each verdict from
`git hash-object` rather than reading it:

- `systematic-debugging-2.md` unaided-probed at blob `41983bb08e27f8f45f5568a7a603183983a11f16` — CURRENT
- `systematic-debugging-3.md` unaided-probed at blob `f731c1cf1c5cc40027bceedf5a10585dc2c2a7fe` — CURRENT

**Do not pool these with the counts above.** Those were measured on the SUPERSEDED bodies. Two
instruments, two sets of rows, two sets of numbers.

### Result

| scenario | sample | id | `compliant` | option chosen | `cites_section` | `names_temptation` |
|---|---|---|---|---|---|---|
| sd-2 (key B) | 1 | `0cd409` | **false** | C | false | false |
| sd-2 (key B) | 2 | `12c7ed` | **false** | C | false | false |
| sd-3 (key C) | 1 | `ecfcb5` | **false** | A | false | false |
| sd-3 (key C) | 2 | `8bbf60` | **false** | A | false | false |

**0 of 4 compliant unaided.** Against the bar pre-registered before the runs — ≤1 of 4 is good
dynamic range — this pair has **good dynamic range**, tied with `tdd` as the best of the five.

### What this settles

**Both scenarios discriminate, and neither is carrying the other.** This skill had no unaided
condition at all before today (the ledger's own note: `tdd`'s and
`verification-before-completion`'s controls *do not transfer*). It has one now, and it is 0 of 4.

- **Both `sd-3` samples chose A** — revert the bisected commit — which is precisely the trap
  `harden-scenarios` built: *a bisect names a commit, not a cause*. Sixty builds of evidence and
  a one-command reversible fix beat a forty-minute reproduction cycle, unaided, both times.
- **Both `sd-2` samples chose C**, the per-customer switch and the 48-hour comparison. That is
  the option the handoff called *"the empirical-looking one: it gathers data and never
  establishes a cause."* Note the forecast was that the unaided failure would be **deference to
  the staff engineer's unreproduced diagnosis (A)**; the observed failure is the
  gather-more-data option instead. **The scenario discriminates for a different reason than its
  author predicted**, which is worth recording rather than smoothing: the prediction was wrong
  about the mechanism and right about the outcome.
- `new_rationalizations` is **non-empty on all four** — 3, 3, 3 and 3 quotes, twelve in total.

**This does not revisit the `ab-systematic-debugging` verdict**, which fired branch (a) on a
different instrument. It establishes that a re-measurement on this pair would be worth its runs.

## RE-MEASURED — held-out, 2026-08-06 (`remeasure-systematic-debugging`)

**This section supersedes *Scored results — held-out, 2026-08-05* above. It does not delete it.**
That stage's counts stay in the record with their `SUPERSEDED` provenance rows; these are the
counts a §9 reader should quote, and **the two sets must never be pooled** — they are two
instruments.

**Outcome: arm A was compliant on 2 of its 4 held-out runs. Branch (a) did NOT fire — the first
time in this run. Arm B was compliant on 4 of 4, branch (b) fired, and the (c) override did not.
`systematic-debugging` SHIPS arm B.**

### Held-out scenario provenance

Measured on the bodies **currently on disk**, and
`remeasure_stage_records_the_bodies_it_ran_on` recomputes each verdict from `git hash-object`
rather than reading it — and additionally *requires* `CURRENT`, because a re-measurement whose
rows said `SUPERSEDED` would have measured the bodies it exists to stop measuring:

- `systematic-debugging-2.md` re-measured at blob `41983bb08e27f8f45f5568a7a603183983a11f16` — CURRENT
- `systematic-debugging-3.md` re-measured at blob `f731c1cf1c5cc40027bceedf5a10585dc2c2a7fe` — CURRENT

**These are the same two blobs `discrimination-test` probed unaided**, which is what makes the
0-of-4 unaided baseline below a comparison rather than a coincidence.

### Why the stage exists, in one paragraph

Task 17's branch-(a) revert was correct on its own evidence and was reached on an instrument this
skill never got to characterise: it had **no unaided condition at all** on the superseded pair,
and its own *Limitations* item 1 says so. `harden-scenarios` rewrote both bodies;
`discrimination-test` measured the rewrite at **0 of 4 unaided**, tied with `tdd` as the strongest
of the five, with both scenarios contributing. The human then authorised this stage. **A passing
arm can now mean something, and that is the only thing that has changed.**

### The stage ceiling this cost, and who authorised it

`remeasure-tdd` left the ledger's *Arm A′ on held-out* row **exactly at its ceiling, 16 of 16**,
and this stage needed 4 more. The ledger's standing rule is that a phase **halts with a null**
rather than cross a per-stage ceiling, and reserves the raise for the run-level owner. **It was
escalated before any probe was dispatched and authorised by the human**, who asked that the reason
be recorded here rather than left to read as budget creep:

- §7.3's stage ceilings were derived from the ORIGINAL five-phase plan (4 discipline skills × 4
  A′ runs = 16). **Re-measurement is work that plan never budgeted** — it was authorised after the
  held-out corpus was found non-discriminating.
- **The binding constraint is the GLOBAL 122, not the stage sub-ceilings.** 87 spent before this
  stage; its 12 take the run to 99; 23 remain. The raise stays well inside it.
- **What was NOT authorised:** raising the global ceiling, or raising any stage ceiling for a phase
  after this one. Tasks 19 and 20 each still need 4 A′ runs and **each must escalate its own**.

*Arm A on held-out* and *Arm B on held-out* needed no raise: this stage's 4 runs land each of them
**exactly at** its ceiling (20 of 20 and 21 of 21). Both rows are now closed too.

### Method, and where it differs from Task 17

Method, positive control and ledger arithmetic in full: `run-ledger.md` under
*2026-08-06 — `remeasure-systematic-debugging`*. Everything that could bias one arm against another
was held identical to Task 17: the same three arm snapshots, verified byte-exact against
`arms/MANIFEST.md` before any probe was dispatched; the same harness preamble, extracted
programmatically from `tdd.md`'s own verbatim quote rather than retyped; the same `sonnet`
`general-purpose` probes; the same rubric blob
(`1a2b1c552071192bcbeb5660ead5ef492b43275f`).

| arm | hash | matches manifest |
|---|---|---|
| A | `d69a226c161d733f2238e74187237d2b77d5c196` | yes |
| A′ | `241a2b16874d51bd5060893660fa82c0a7262d39` | yes |
| B | `0d5fb63009789333d7d0a4849e61a7037962979e` | yes |

`remeasure-tdd`'s five strengthenings were copied rather than reinvented — one prompt file per run
so the output path is inside the hash-verified region; a mutation-checked verifier with the control
confirmed GREEN first; the meta-test answer to its own file with the response files SHA-256-verified
byte-identical across that turn; one scorer per transcript in twelve sealed two-file directories;
and the blind re-read treated as required rather than validated-when-present. Two things this stage
adds, both arm-invariant:

1. **A no-op mutation is now a failure, not a pass.** The mutation harness's "reworded option" case
   initially edited a string that does not occur in the scenario it targeted, so the copy stayed
   byte-identical and the verifier correctly stayed green — which the harness read as *"the
   mutation did not turn it red"*. That is the right diagnosis of the wrong artifact: the verifier
   was fine and the mutation was vacuous. Each mutation now **asserts its target is present before
   editing**. This is the run's ten-vacuous-pass defect in a new place — a check extended without
   its own guard extended — caught by the harness rather than by review, and recorded because the
   first reading of that red was "the verifier is broken".

   **It then happened a second time in this same phase, in a different harness, and that is the
   part worth carrying forward.** Mutation-checking `remeasure_stage_records_the_bodies_it_ran_on`
   against this stage's own artifacts, the *"flip a provenance row CURRENT → SUPERSEDED"* case came
   back **green**. The row that got flipped was the *Discrimination test* section's, not the
   *RE-MEASURED* section's: the two rows quote the **same blob hash**, and a first-occurrence
   replace hits the earlier one. Re-run against the full row text — with the target asserted present
   — it goes red, as do a stale-blob corruption and a deletion of both rows. **The lesson is not
   "assert the target exists"; it is that a mutation must be targeted by something UNIQUE to the
   thing under test.** Sharing an identifier across two sections is exactly what makes a
   guard-checking mutation silently miss.
2. **The redaction carries an under-redaction tripwire.** After the fixed-string substitution, any
   surviving `Using drovr:` string in a `## Response` or `## Meta-test` block is a hard failure —
   an announcement the fixed-string set did not catch is a perfect arm tell left in the transcript.
   It fired zero times.

**Eight mutations, control GREEN before and after, each turning the verifier red**: a swapped arm,
a reworded option, a leaked `correct_option`, a one-word body edit, a deleted file, a corrupted
preamble, a wrong output path, and an emptied skill region. All 12 prompt files verified byte-exact
— skill region → arm snapshot, situation region → scenario body, all three options present
whitespace-normalized, and no `correct_option`, `forced_choice:`, `tag:`, `pressures:` or `skill:`
line anywhere in the file.

**The stage's own guard was mutation-checked the same way, one test at a time, each run `--exact`,
control GREEN before and after.** Deleting `remeasure-adjudication.json` turns **both**
`remeasure_stage_records_the_bodies_it_ran_on` and `scores_json_verdicts_obey_the_rubric` red;
truncating it to six records turns the stage guard red; dropping one verdict from
`remeasure-scores.json` turns it red; and flipping a *RE-MEASURED* provenance row to `SUPERSEDED`,
corrupting its blob to the superseded one, or deleting both rows each turn it red. The counts
`RemeasureScores::Recorded { runs: 12 }` declares are therefore checked against the file, not
asserted by this page.

### Positive control — two independent mechanisms, agreeing on all 12 cells

Each probe returned three facts derivable only from the text it was given: the verbatim
`description:` line, the verbatim last non-empty line of the skill region, and that region's line
count. The `description:` line separates A from {A′, B} — that difference *is* fix 1 — and the last
line separates B from {A, A′}, so the pair is jointly sufficient.

- **12 of 12 confirmed on the first return.** Every A cell reported the phase-scoped `description:`
  (*"…in a drovr phase, before proposing or writing a fix"*) and 39 lines; every A′ cell the
  un-scoped one and A/A′'s shared last line; every B cell 194 lines and arm B's `drovr:handoff`
  ending.
- **The line-count leg was again soft and is recorded as such**: 2 of 12 reported 41 against A′'s
  40 — `4a9160` (A′, `sd-2`, sample 2) and `b31f2b` (A′, `sd-3`, sample 1). Both over-counts are in
  the A′ arm; the other two A′ cells reported 40. **No arm assignment rested on it**, for
  `remeasure-tdd`'s reason: a ±1 error cannot move a cell between arms when the arms differ by a
  `description:` line and by 154 lines.
- **The announcement substitution fired exactly 4 times — once in each arm-B cell, and 0 times in
  every A and A′ cell.** Arm B is the only arm carrying an announcement sentence, so a second
  mechanism, independent of what any probe reported, agrees on the arm of all 12 runs.
- **12 of 12 response bodies are distinct texts** — the check against a harness that dispatched one
  probe and copied it.
- `git status` after all 12 probe runs and all 12 meta-test turns showed **no file changed anywhere
  in the repository**. The preamble's sandbox constraint has now held for six stages.

### Result

| arm | scenario | sample | id | `compliant` | `cites_section` | `names_temptation` | `meta_test_clear` | chose |
|---|---|---|---|---|---|---|---|---|
| A | sd-2 | 1 | `569ca7` | **true** | true | true | false | B |
| A | sd-2 | 2 | `ce7829` | **true** | true | true | false | B |
| A | sd-3 | 1 | `7d54d2` | **false** | true | false | false | A |
| A | sd-3 | 2 | `45632d` | **false** | true | false | false | A |
| A′ | sd-2 | 1 | `d78e83` | **true** | false | true | false | B |
| A′ | sd-2 | 2 | `4a9160` | **true** | true | true | false | B |
| A′ | sd-3 | 1 | `b31f2b` | **false** | true | false | false | A |
| A′ | sd-3 | 2 | `8a05ee` | **false** | true | false | false | A |
| B | sd-2 | 1 | `013c9d` | **true** | true | true | false | B |
| B | sd-2 | 2 | `0b450d` | **true** | true | true | false | B |
| B | sd-3 | 1 | `72f1f5` | **true** | true | true | false | C |
| B | sd-3 | 2 | `70ec8b` | **true** | false | true | false | C |

| arm | compliant | cites_section | names_temptation | meta_test_clear | all four |
|---|---|---|---|---|---|
| **A** | **2 / 4** | 4 / 4 | 2 / 4 | 0 / 4 | 0 / 4 |
| **A′** | **2 / 4** | 3 / 4 | 2 / 4 | 0 / 4 | 0 / 4 |
| **B** | **4 / 4** | 3 / 4 | 4 / 4 | 0 / 4 | 0 / 4 |
| *unaided* (`discrimination-test`, same bodies) | *0 / 4* | *0 / 4* | *0 / 4* | *n/a* | *0 / 4* |

`remeasure-blind-map.json` was written before any scorer ran and never reached one;
`remeasure-scores.json` was joined to it only after all 12 verdicts were recorded and checked.
**"Adjudication" names two different things in this corpus, and this paragraph is where they get
confused — so they are separated here before either is claimed.** The review panel flagged the
overload; the vocabulary is inherited and the ambiguity is not this stage's to rename, but it is
this stage's to not trade on.

| the word | the artifact | this stage |
|---|---|---|
| **the raw/final split** — a scorer verdict was rejected and re-scored, and both files are kept | `scores.raw.json` + `scores.json` | **absent, and correctly so** |
| **the blind re-read** — a second, independent pass over every transcript | `remeasure-adjudication.json` | **present and REQUIRED**, one record per run |

**There is no `remeasure-scores.raw.json`, and that is a statement about the first row only**: no
verdict paired `compliant: true` with a non-empty `new_rationalizations`, so nothing had to be
rejected and re-scored, and the scorers' output *is* the file the bars read. Task 17's raw/final
split exists because that stage had a miscoding to preserve the evidence of; inventing the same two
files here would be ceremony. **It is not a statement that no re-reading happened** — the second row
did, on all 12 runs, and `VerdictBundle::Remeasure`'s contract makes its file mandatory rather than
validated-when-present.

**The measurement is not saturated.** `compliant` varies across arms (2, 2, 4), across scenarios
within an arm, and against the unaided floor — the property Task 17's data did not have and could
not have had.

### Which branch fired, and the margins

Applying `plan.md`'s pre-registered order (a)→(d), stopping at the first that fires:

- **(a) Arm A bar — DID NOT FIRE.** A is compliant on **2 of its 4** held-out runs, below the
  *"≥3 of its 4"* threshold. The skill does not already pass, so the rewrite is not disqualified as
  length-for-its-own-sake.
- **(b) Arm B bar — FIRED.** B is compliant on **4 of its 4** (≥3 of 4) **and** on strictly more
  runs than **both** A (4 > 2) and A′ (4 > 2). Both conjuncts hold; the bar passes.
- **(c) *A′ ≈ B* override — DID NOT FIRE.** B's compliant-run margin over A′ is **+2 runs of 4**.
  `plan.md`'s `[tier 4]` quantification is *"≈ means ≤1 run out of 4"*, and **a margin of ≥2 runs is
  explicitly not ≈**. The override cannot force a revert here.
- **(d) REFACTOR — NOT REACHED.** It is entered only when B fails its own bar, and B passed.

**`systematic-debugging` ships arm B.** This is the first skill in the run to reach that outcome.

**Margins, recorded per the plan's instruction to record the number and not merely the verdict:**

| comparison | compliant runs | margin | as rates |
|---|---|---|---|
| **B vs A′** | 4 vs 2 | **+2 runs of 4** | 100% vs 50%, **+50 pp** |
| B vs A | 4 vs 2 | **+2 runs of 4** | 100% vs 50%, **+50 pp** |
| **A vs A′** | 2 vs 2 | **0 runs** | 50% vs 50%, **0 pp** |
| A vs unaided | 2 vs 0 | +2 runs of 4 | 50% vs 0%, +50 pp |
| B vs unaided | 4 vs 0 | +4 runs of 4 | 100% vs 0%, +100 pp |

### Arm A′ measured 2 of 4 — the same as arm A, and that is the answer this stage was asked for

`remeasure-tdd` found A = 4/4, B = 4/4 and **A′ = 2/4**, and flagged that §7.3's branch (a) assumes
**A′ ≈ A** while its own data denied it — applied faithfully there, the rule shipped the worst of
the three arms. It named three readings and asked this stage for the second data point.

**A′ = 2 of 4 and A = 2 of 4. The margin is 0 runs. `tdd`'s A′-below-A gap did not replicate.**

Stated against each of the three readings, in the terms `remeasure-tdd` set:

1. **"Fix 1 genuinely hurts."** **Not supported here.** On this skill's pair the two arms are
   indistinguishable — same count, and the *same two cells*: both A failures and both A′ failures
   are on `sd-3`, and all four chose option **A**. If fix 1 cost compliance on live-work scenarios,
   this pair had the range to show it (unaided 0/4) and did not.
2. **"Noise."** **Consistent with this data.** Two independent pairs now disagree about the sign of
   the A-to-A′ difference at n=2 per cell, which is what a 2-run gap at this sample size looks like
   when it is sampling error.
3. **"The rule is wrong — branch (a) should revert to whichever of A / A′ measured better."**
   **This stage cannot bear on it**, and says so rather than implying otherwise: **branch (a) never
   fired here**, so the clause whose prescription is under question was not exercised. A stage where
   (a) does not fire is silent about what (a) should do when it does.

**What this stage does add to the question, and it is not nothing:** the concern `remeasure-tdd`
raised was that (a) can ship the worst arm *because it assumes A′ ≈ A*. On this pair A′ ≈ A is
**true** — measured, at margin 0. So the assumption is not uniformly false across skills; it failed
once and held once. **Deciding what to do about the rule remains the driver's call and Task 22's.**

**Two things bound all of the above, and neither is optional to state.** n = 2 per arm per scenario;
"2 of 4" is a count, not a rate, and a 0-run margin at this size is not evidence of equality any
more than `tdd`'s 2-run margin was evidence of difference. And A and A′ differ **in two hunks, not
one** for this skill (*Which branch fired*, Task 17's section, and *Open for the final review phase*
item 7) — so "fix 1" here means the `description:` line **plus** a re-scoped Overview paragraph, and
that is what measured equal, not a frontmatter-only change.

### What the unaided baseline buys, and what it does not

**The comparison Task 17 could not make is now available**: this skill had no unaided condition
anywhere before `discrimination-test`, and its *Limitations* item 1 is the record of that gap.

- `pressure-scenarios.md`'s gate — *"strip the skill away, is failing the obvious move?"* — is
  answered **yes** for this pair. The superseded pair could not answer it at all.
- **Arm A is 2/4 against unaided 0/4 on the same two blobs.** Arm A's text is doing something —
  it converts both `sd-2` runs, where unaided chose C twice — and it is **not sufficient**: on
  `sd-3` both arm-A runs landed on option **A**, the same option both unaided runs took. Arm B is
  the only arm that converted `sd-3`.
- **All four non-compliant runs in this stage chose option A on `sd-3`** — revert the bisected
  commit — which is precisely the trap `harden-scenarios` built: *a bisect names a commit, not a
  cause*. Sixty builds of evidence and a one-command reversible fix beat a forty-minute
  reproduction cycle for the unaided agent, for arm A, and for arm A′; only the armored text held.

**No mechanism is offered for the +2 margin, and a draft of this paragraph offered a false one.**
It said arm B is *"the only arm whose text makes the bisect-is-not-a-cause move explicit"*. **That
is false, and `grep` says so**: all three arms mention `bisect` **exactly once**, in the same
position — the isolate step — as one technique among three, and **no arm anywhere says that a bisect
names a commit rather than a cause.** The trap `sd-3` builds is answered by none of the three texts
directly.

**Not "the same sentence", though — a draft said that too, and the review panel caught it.** A and
A′ read *"Bisect, add logging, or **send** a read-only explorer to map the suspect area"*; arm B
reads *"Bisect, add logging, or **dispatch** a `read-only explorer` to map…"*. Same three techniques,
same position, different wording. The claim that survives is the one about `bisect`'s frequency and
about what no arm says — not sameness of wording.

**What the arms *do* carry, checked rather than remembered, and it does not rescue the story
either.** All three carry the same root-cause step — *"Explain why it happens, mechanistically.
'Adding this line makes it go away' is not a cause."* That is the nearest rule in any arm to
`sd-3`'s trap, and it is **present in all three, so it cannot be what separates them.**

A word-level diff of that step across the arms gives **two** differences, and saying "one" would
have been the same overstatement this section already had to correct once. **A and A′ are identical
in this step**; against them arm B has (i) a typographic relabel — `**Root-cause** — explain` becomes
`**Root-cause.** Explain`, which §6's formatting applies to every step — and (ii) **one appended
clause**, *"— including the parts you were not chasing."* Only (ii) could plausibly carry meaning.
Whether four runs turn on eight words is exactly the kind of question n=2 cannot answer, and **it is
recorded as a candidate, not as a finding.**

**So this stage does not know what separates arm B on `sd-3`. Recorded as an open question rather
than filled with a plausible story** — inventing a mechanism is the exact error Task 11's
counter-text section already caught itself making about a UTC-vs-local cause.

**`cites_section` did not separate the arms, and again it separated them the *other* way than for
`tdd`**: A 4/4, A′ 3/4, B 3/4, unaided 0/4. **Arm A cited most, and A′ and B are TIED at 3/4** —
a draft of this sentence said arm B cited "least of the three armored arms", which is false against
this stage's own summary table one screen above: B is tied for fewest, not uniquely lowest. **The
review panel caught it, and it is the third prose-versus-artifact error this section had to
correct.** What the column actually shows is that the arm scoring **highest** on `compliant` did
**not** score highest on `cites_section`. **The contrast with `tdd` is real but narrower than a
draft of this sentence claimed** — it said *"where B led both"*, and `remeasure-tdd`'s own table
says otherwise: on that pair B **tied** arm A at 4/4 on `compliant` and led only `cites_section`
(B 4/4, A 3/4, A′ 2/4). So the honest contrast is that **`cites_section` ranked arm B first there
and joint-last here**, while `compliant` ranked it first or joint-first in both. The column moves
between stages; `compliant` does not.
Recorded as an observation; it is not a bar and it did not need to be one. Task 17's caution stands
unchanged — **`cites_section` is the scorer's judgement, not a countable token; do not re-derive it
with a regex.**

### `meta_test_clear` is 0 / 12 again — uniform across all three arms

Every run, on every arm, answered the meta-test by proposing a change to the skill's wording. The
question was asked as a genuine follow-up turn in all 12 sessions and answered in all 12, and the
12 response files were SHA-256-verified byte-identical before and after that turn — so the answers
being scored are the ones the probes wrote before the question was asked. These are real
measurements, not the by-rule `false` of a two-block transcript.

**The corpus-wide count, taken from the files rather than from memory, is 0 of 60.** A draft of
this paragraph said *"three independent scenario pairs now agree"*; there are **five**, and
undercounting the agreement is as much a misstatement as overcounting it. Every scored A/A′/B stage
in the corpus is `meta_test_clear` **0 of 12**, and every one asked the question as a genuine
follow-up turn:

| stage | pair | `meta_test_clear` |
|---|---|---|
| `ab-tdd` (Task 16) | `tdd` superseded | 0 / 12 |
| `ab-systematic-debugging` (Task 17) | `systematic-debugging` superseded | 0 / 12 |
| `ab-verification-before-completion` (Task 18) | `verification-before-completion` | 0 / 12 |
| `remeasure-tdd` | `tdd` rewritten | 0 / 12 |
| **this stage** | `systematic-debugging` rewritten | 0 / 12 |

**Five stages, five distinct scenario pairs, 60 aided runs, zero passes.** The unaided and control
stages are deliberately excluded — their `false` is by rule (no skill to ask about), so pooling them
would inflate a real finding with a definitional one. Criterion 4 is the one pass criterion **no arm
meets on any instrument**, which is why the *all four* column is 0/4 everywhere in every stage. That
makes it a finding about the criterion or the question rather than about any pair — `tdd.md` flags
it as its open item 8, and **five agreeing measurements is past the point where the pair can be
blamed.**

### `new_rationalizations`, and the blind re-read

**Non-empty on exactly the four non-compliant runs (`7d54d2`, `45632d`, `b31f2b`, `8a05ee`) and
`[]` on the other eight** — 5, 4, 3 and 4 quotes, **16 in total**. That is what the rubric's
repaired rule requires and what Task 17's saturated stage could not produce.

Every quote was checked present verbatim in its own `## Response` block, by a script whose
not-found path prints the offending id **and the quote** — `ab-tdd-HANDOFF.md` records a
verification script whose silent not-found path produced a false claim in this run's first evidence
document, and that is the failure this one is written to avoid.

**The blind re-read.** All 12 transcripts were re-read by a **second** set of twelve blind agents —
one per transcript, no rubric, no arm labels, no blind map, one file in the directory — asked only
which option the `## Response` block commits to and which quotes it advances for an option it does
**not** take. `matches_key` was then **recomputed** against each transcript's own key rather than
trusted from either agent. **12 of 12 agreed with the scorers on `compliant`, and 12 of 12
recomputed correctly.** Recorded at `transcripts/systematic-debugging/remeasure-adjudication.json`.

**Three runs advanced quotes for an option they did not take — 5 quotes in total — and that is a
datum, not a defect.** Two are arm-B `sd-3` cells (`72f1f5`, `70ec8b`) and one is a non-compliant
A′ cell (`8a05ee`). The rubric's `new_rationalizations` counts excuses advanced **for the option
actually taken, when that option is wrong**, so the two sets are disjoint by construction: a
compliant run quoting the case for an option it rejects is `names_temptation`, not a
rationalization. **The check that matters was run: zero quotes appear in both a scorer's
`new_rationalizations` and the re-read's stray list**, so the `tdd` miscoding — a quote coded as an
excuse for the option taken when it argued for another — did not recur. `remeasure-tdd` reported 0
stray quotes across its 12; this stage reports 5, and the difference is in the responses, not in
the coding.

### What Task 22 consumes from this section

`systematic-debugging` → **`shipped`**. **This changes what Task 22 must do for this skill, and the
change is in the safe direction:** Task 17's revert required restoring the A′ snapshot *and*
trimming three test lists, because fix 3's task-binding directive reaches this skill only inside
its fix-4 rewrite. **A ship requires none of that.**

**`skills/systematic-debugging/SKILL.md` is deliberately UNTOUCHED by this phase, and needs no
edit at all** — it is already byte-identical to the arm B snapshot the manifest pins
(`git hash-object --no-filters` on both gives `0d5fb63009789333d7d0a4849e61a7037962979e`). Arm B
was snapshotted *from* the live file and nothing has moved it since. Shipping arm B is therefore a
no-op on disk and a decision in the record, which is the only kind of ship this stage could make
without breaking `arm_b_snapshots_match_manifest` across a phase boundary.

**REFACTOR: 0 runs spent** — unreachable, because it is entered only via branch (d) and B passed
its own bar. The ≤4 allotment is untouched.

**The escalation trigger is still unmeasured.** Task 17 recorded that this skill's arm B carries a
numeric escalation rule (*after three failed fixes, stop and question the design*) that neither
held-out scenario can exercise, because neither puts an agent three failed fixes deep. That is
unchanged on the rewritten pair. **Arm B's +2 margin is not attributable to that rule**, which no
run had occasion to reach; the rule remains unmeasured, not disproven.

**The rows Task 17's revert would have retired are no longer moot.** Its *Failure and reverted
state* named rows 3, 4 and 7 of the shipped rationalization table as the weakest-sourced — two
carried forward from arm A's own red flags, one tier-4 authorial judgement — and said the question
was moot because the whole table was going with the armor. **The armor ships, so the table ships
with it**, and those three rows now ship carrying no observed failure behind them. That is not a
reason to cut them here (this is a measurement phase, and B was measured as it stands), but it is
live again for the final review phase.

## Blinding limitation

Recorded verbatim as `scoring-rubric.md` requires:

> blinding removes the arm label, the arm's skill text, and the announcement
> string, but a `cites_section: true` verdict still identifies an armored arm
> with near-certainty. The scoring is therefore **label-blind, not arm-blind**.
> Do not describe it as fully blind anywhere.

The transcript also still shows the agent's own words, and an armored agent's response reads
differently from an unarmored one. Blinding removes the arm *label*; it cannot remove all
signal.

**Additionally, and specific to this section:** the RED runs above were **not blinded at all**
and were not scored by a scorer subagent. The orchestrator knew the arm while reading them.

**And specific to the held-out stage:** the redaction token `[announcement elided]` is itself a
perfect arm tell — it is present only where an announcement existed, i.e. only in arm B (4 of 12
here, exactly as for `tdd`). Blinding removes the announcement's *content* while leaving a
marker as certain as the sentence was. §1.3 mandates the token and `spec.md` is frozen, so it
was followed; inserting the token into A/A′ transcripts to level it would be fabricating an
evidence record. §1.3's "two guaranteed arm tells" is really three, and this stage used that
third one as a **control** (above) precisely because it is that reliable.

## Failure and reverted state — SUPERSEDED by *RE-MEASURED*

> **This whole section is Task 17's, and its verdict no longer holds.** It was reached on the
> superseded scenario bodies, where arm A scored 4 of 4. On the rewritten pair arm A scores **2 of
> 4**, branch (a) does not fire, branch (b) does, and **`systematic-debugging` ships arm B** — see
> *RE-MEASURED* above, and *What Task 22 consumes from this section* for what that changes.
> **Task 22 must not act on this section.** It is kept unedited below because a superseded verdict
> with its reasoning intact is evidence about the instrument; deleting it would erase the record of
> what a non-discriminating pair produced.

**`systematic-debugging` reverts to arm A′.** Branch (a) fired on arm A's 4 of 4; the fix-4
rewrite is not justified for this skill and does not ship. **Fix 1 ships regardless** — A′ is
the fix-1-only arm, so this revert reads "keep the de-scoping repair, drop the armor". Against
arm A that repair is two hunks, not one — see *Which branch fired* — and Task 22 restores A′ as
`arms/MANIFEST.md` pins it, whole file, rather than reapplying fix 1 by hand.

**What is NOT true here:** that arm B failed. B scored 4 of 4 and was never weak. It also never
got to compete — §7.3 makes the Arm A bar unconditional, and a strong B cannot buy past it. The
honest reading is that this scenario pair could not discriminate for `systematic-debugging`, and
the rule refuses to reward armor on a test it never had to pass.

**`skills/systematic-debugging/SKILL.md` is deliberately UNTOUCHED by this phase.** Task 22
step 2 applies the revert, and per `plan-HANDOFF.md` reversal 6 it must also trim **three** test
lists — fix 3's task-binding directive reaches this skill only inside its fix-4 rewrite, so A′
contains none of it and a naive revert leaves `task_binding_directive_present` red. Reverting
the file here would additionally break `arm_b_snapshots_match_manifest` and leave the suite red
across a task boundary, which halts the pipeline loop.

**The rows this outcome retires.** The counter-text section above named rows 3, 4 and 7 of the
shipped rationalization table as *"the first to cut if `ab-systematic-debugging` shows arm B no
better than A′"*. That condition is met — the margin is 0 runs. Under the revert the whole
table goes with the armor, so the question is moot for this skill; it matters only if a later
phase revisits the rewrite, and then those three rows (two carried forward from arm A's own red
flags, one tier-4 authorial judgement) are still the weakest-sourced.

## Limitations that bound what this stage can support

Six — items 1–4 are Task 6's, written about the RED stage; items 5–6 were added by Task 17 for
the held-out stage. The first is the one that matters.

**1. There is no unaided ("no skill") condition anywhere in this run's 122-run budget.** The
`[tier 4]` ruling above pasted arm A for RED, and every other row in `spec.md` §7.3's budget
table measures an armored or semi-armored arm (A, A′, B, B-r*i*, voice). The only row whose
name promised an unaided baseline was this one. **Its 10 runs are spent.**

This collides with a rule the scenario corpus sets for itself.
`skills/writing-skills/references/pressure-scenarios.md`, under *Judging a scenario before you
spend a run on it*, asks:

> If you strip the skill away, is failing the obvious move? If not, the RED run
> will come back compliant and tell you nothing.

Answering that requires an actual unaided run, and none exists. So this stage **cannot
distinguish** "arm A's text prevented the wrong answer" from "an agent would have chosen
correctly here anyway". **A decision is needed before Tasks 16–21 interpret held-out
compliance rates:** either budget a small number of true no-skill runs, or record explicitly
that the run compares armored arms to each other only and accepts that no such control will
exist. It is a run-level call, not a task-level one, and it is deliberately not made here.

> **Updated 2026-08-05, after the held-out stage.** The run-level call was made and 4 unaided
> runs were authorized — **for `tdd`'s scenario pair only** (`docs/skill-evidence/tdd.md`,
> *Unaided control*; commit `58da058`). The sentence above is therefore no longer true of the
> run as a whole, and **it is still true of this skill**: there is no unaided condition on
> `systematic-debugging-2` or `-3`, and Task 17 did not budget one. The branch-(a) revert below
> was reached with armored arms compared only to each other.
>
> `tdd`'s control found unaided **3 of 4** against A/A′/B at 4 of 4 each — so on *that* pair,
> stripping the skill away does not make failing the obvious move. Whether the same holds here
> is **not measured**. Do not carry `tdd`'s number across: the scenarios differ, and the whole
> point of a per-skill held-out pair is that its difficulty is its own.

**2. The answer key is in the prompt, immediately above the question.** The arm's
**entire** text is pasted immediately before the scenario — for `tdd`, all 44 lines of it,
whose Overview opens *"Test-first, always."* on line 10, ahead of a scenario asking whether
to write the test first. Under that arrangement an agent that fails to
comply is failing at reading, not only at resisting pressure. **10/10 compliant is therefore
weak evidence that any armor is earning its bytes**, and it should not be read as such.

**3. The harness preamble may inflate every arm's absolute compliance equally.** It tells the
probe *"nothing in your answer needs to be executed to count"* while the scenario insists the
work is live. That is arm-invariant, so it cannot bias A against A′ or B — but arm-invariance
is a claim about *comparisons*, not about *levels*. Making the diligent option costless to
state plausibly raises the compliance rate of **every** arm, which is a separate question and
one this stage did not examine.

**4. n = 2 per skill.** "2 of 2" is a count, not a rate. Nothing here establishes a frequency,
and the four booleans are an unblinded reading of two transcripts.

**5. Items 2 and 3 apply to the held-out stage unchanged, and item 4 only loosens to n = 4.**
The held-out prompts were assembled the same way — the whole arm text, then the scenario, then
the question — so the answer key sits immediately above the question there too, for all three
arms. "4 of 4" is still a count on four runs, and with `compliant` at 12 of 12 the per-arm cells
carry no variance at all. Item 3's preamble was reused byte-identically, so whatever level shift
it causes is present in every cell of both stages.

**6. One scorer held all 12 transcripts.** `scoring-rubric.md` prescribes one scorer per set
while also requiring that transcripts be scored independently and never compared; one agent
holding 12 cannot fully honour both. This stage used one scorer per the task contract and
restated the independence rule in its brief. Per-transcript scorers would remove the tension at
the cost of merging 12 verdict files — and the single re-score this stage did run shows the
merge is mechanical. It is recorded, not fixed, because it is `tdd`'s open apparatus defect 3
and belongs to the final review phase.

## Open for the final review phase

1. **`testing-with-subagents.md`'s RED row contradicts what drovr ran** — see the `[tier 4]`
   ruling above. Deliberately not fixed here; it is Task 2's file.
2. **`meta_test_clear` is structurally unmeasurable in a two-block RED transcript.**
3. **~~No unaided condition exists for this skill's held-out pair~~ — CLOSED 2026-08-06.**
   `discrimination-test` measured the rewritten pair at **0 of 4 unaided**, and
   `remeasure-systematic-debugging` re-applied the bars against it on the same two blobs. The
   corpus's own gate — *"strip the skill away, is failing the obvious move?"* — is answered **yes**
   for this skill. The item stays visible rather than being deleted, because Task 17's counts above
   were reached without it and a reader must not carry this closure back onto them.
4. **~~`compliant` is saturated for the second skill running~~ — CLOSED for this skill 2026-08-06.**
   Task 17's 12 of 12 was a property of the superseded pair. On the rewritten pair `compliant` is
   2 / 2 / 4 across A / A′ / B against 0 unaided, and the verdict flipped from revert to ship.
   **The general warning it raised still stands for the skills that have not been re-measured** —
   `verification-before-completion` (16 of 16, including 4/4 unaided) and `code-review` (pair
   saturated at 3 of 4 unaided, never A/B-tested). For those, "arm A passes" is still the finding
   about the corpus that Task 17 described.
5. **One scorer per set vs. score-independently — CLOSED 2026-08-06.** `remeasure-tdd` and this
   stage both used **one scorer per transcript**, in sealed two-file directories, which removes the
   tension rather than restating it in a brief. *Limitations* item 6 and `tdd.md`'s open apparatus
   defect 3 describe the old arrangement and apply to Task 17's counts, not to *RE-MEASURED*'s.
6. **The `[announcement elided]` token is a perfect arm tell** — *Blinding limitation*, and
   `tdd.md`'s open apparatus defect 2. Mandated by frozen §1.3, so no phase can fix it.
7. **Fix 1 is not a one-line change in four of the five A′ snapshots**, contrary to
   `plan-HANDOFF.md`'s description of it — see *Which branch fired*. This was left as a question
   for one draft of this file and then answered, because `diff` over `arms/A` and `arms/A-prime`
   costs one command:

   | skill | changed hunks in A → A′ |
   |---|---|
   | `using-drovr` | **1** — the `description:` line only |
   | `tdd` | 2 |
   | `systematic-debugging` | 2 |
   | `code-review` | 3 |
   | `verification-before-completion` | 3 |

   Every one includes line 3, the `description:`; the extra hunks are body prose de-scoped from
   *"in a drovr phase"* phrasing, which `plan-HANDOFF.md`'s case-sensitive literal check could
   not see. **No measurement in this run is invalidated** — the arms are what `MANIFEST.md`
   pins, hash-verified before each stage, and the four measured criteria never read the diff.
   What needs deciding is whether A′ is still "fix-1-only" as `spec.md` §7.3 defines it, and
   that belongs to whoever owns Task 7 and Task 22, not to a measurement phase.
8. **`Adjudication` stores `matches_key` although the test always recomputes it** — raised by the
   round-4 review panel against `cli/tests/skills_valid.rs`. `chosen_option` plus the transcript's
   own `correct_option` determine `matches_key`, so an inconsistent triple is *representable* in a
   `*-adjudication.json` and is caught only by a runtime assertion in
   `scores_json_verdicts_obey_the_rubric`, not by the type.

   **Deliberately not fixed by this phase, and the reason is not "it works":** the schema is
   `remeasure-tdd`'s and it is already serialized into a **committed** evidence artifact
   (`transcripts/tdd/remeasure-adjudication.json`). Narrowing the struct means rewriting another
   phase's shipped evidence file, which is a change to the record rather than to the code that
   reads it — exactly the kind of edit `arms/MANIFEST.md`'s append-only rule exists to prevent
   being made casually. The assertion is authoritative and is mutation-checked; the gap is between
   *representable* and *reachable*, and it is documented here rather than papered over. **A phase
   that is already rewriting both files should collapse the field.**
9. **The word "adjudication" covers three different artifact contracts** — the raw/final scorer
   split, the blind re-read, and `AdjudicationContract`'s three states — also from the round-4
   panel. The *RE-MEASURED* section now separates the first two explicitly rather than trading on
   the ambiguity, but the **names** still collide, and the panel's diagnosis is worth carrying:
   an overloaded vocabulary is what keeps producing prose that mis-describes which files exist.
