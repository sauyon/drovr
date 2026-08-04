# Skill evidence — `systematic-debugging`

Per `spec.md` §7.2 this file carries, for this skill: the scenarios used, the **verbatim**
baseline rationalizations, the counter-text written against each, the scored results with
dates, the §1.3 blinding limitation, and — if it applies — the failure and the reverted
state.

**Only the RED section below is filled in.** Task 6 wrote it. The counter-text and scored
sections belong to later tasks and are marked as not-yet-run.

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
| 6 | *"Three attempts in — the next one will land."* | `spec.md` §6's numeric escalation trigger — **not observed**; see the near-miss below |
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

## Scored results

**Not yet run.** Arm A / A′ / B on the held-out pair belongs to the `ab-systematic-debugging`
phase (`plan.md` Tasks 16–21). No scores, no `blind-map.json`, no `scores.json` exist yet.

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

## Failure and reverted state

**Not applicable yet.** No bar has been evaluated for this skill.

## Limitations that bound what this stage can support

Four, stated so no later reader has to infer them. The first is the one that matters.

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

## Open for the final review phase

1. **`testing-with-subagents.md`'s RED row contradicts what drovr ran** — see the `[tier 4]`
   ruling above. Deliberately not fixed here; it is Task 2's file.
2. **`meta_test_clear` is structurally unmeasurable in a two-block RED transcript.**
