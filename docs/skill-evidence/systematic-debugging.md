# Skill evidence — `systematic-debugging`

Per `spec.md` §7.2 this file carries, for this skill: the scenarios used, the **verbatim**
baseline rationalizations, the counter-text written against each, the scored results with
dates, the §1.3 blinding limitation, and — if it applies — the failure and the reverted
state.

**Complete.** Task 6 wrote the RED section, Task 11 the counter-text section, and Task 17
(`ab-systematic-debugging`, 2026-08-05) the scored results and the outcome.

**Outcome: `systematic-debugging` reverts to arm A′.** Arm A was compliant on 4 of its 4
held-out runs, so `plan.md`'s pre-registered branch **(a)** fired: the rewrite is not justified
and the fix-4 armor does not ship for this skill. Fix 1 ships regardless. See *Failure and
reverted state*.

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

## Failure and reverted state

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
3. **No unaided condition exists for this skill's held-out pair** — *Limitations* item 1's
   2026-08-05 update. `tdd` got one; the run-level call did not extend to Tasks 17–20, and Task
   17 did not take it on itself to extend it. If the final phase wants the corpus's own gate
   answered for `systematic-debugging`, it costs 4 runs on `sd-2` / `sd-3` with no skill pasted.
4. **`compliant` is saturated for the second skill running** — 12 of 12 here after 12 of 12 for
   `tdd`. If Tasks 18–20 land the same way, the honest summary for §9 is that the instrument had
   no headroom, not that four skills independently confirmed arm A.
5. **One scorer per set vs. score-independently** — *Limitations* item 6, and `tdd.md`'s open
   apparatus defect 3. Unchanged and unfixed; recorded twice now, by two phases.
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
