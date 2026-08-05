# Skill evidence — `tdd`

Per `spec.md` §7.2 this file carries, for this skill: the scenarios used, the **verbatim**
baseline rationalizations, the counter-text written against each, the scored results with
dates, the §1.3 blinding limitation, and — if it applies — the failure and the reverted
state.

**Complete.** Task 6 wrote the RED section, Task 10 the counter-text section, and Task 16
(`ab-tdd`, 2026-08-04) the scored results and the outcome.

**Outcome: `tdd` reverts to arm A′.** Arm A was compliant on 4 of its 4 held-out runs, so
`plan.md`'s pre-registered branch **(a)** fired: the rewrite is not justified and the fix-4 armor
does not ship for this skill. Fix 1 ships regardless. See *Failure and reverted state*.

---

## RED / baseline — 2026-08-03

**Stage.** `spec.md` §7.3 row 1, *RED / baseline on dev set*. 2 runs of 10 across the stage.

**Scenario.** `skills/writing-skills/scenarios/tdd-1.md` (`tag: dev`, pressures
`time, sunk-cost, authority`, `correct_option: B`). The two held-out scenarios `tdd-2.md`
and `tdd-3.md` were **not** used here and were not read while writing this section.

**Arm.** Arm A — `docs/skill-evidence/arms/A/tdd.md`, verified byte-exact against
`arms/MANIFEST.md` before the runs: `git hash-object --no-filters` =
`a1f889b57fa741e55b02da2397104f933d9878aa`, matching the manifest row.

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
fix 4 exists to close — excuses from an unaided agent could be ones arm A already handles,
and armoring against those spends bytes §7.3 says fix 4 must earn.

**The consequence, stated plainly: there is no unaided baseline anywhere in this run.** The
number below is "arm A on the dev scenario", not "an agent with no skill". Do not cite it as
the latter. `testing-with-subagents.md`'s RED row now describes a method drovr did not
follow — see *Open for the final review phase*. **And see *Limitations that bound what this
stage can support*, item 1** — the corpus's own scenario-judging rule requires an unaided run,
and none exists.

### Method

5 skills × 1 `dev` scenario × 2 samples. Each run: a fresh foreground `general-purpose`
subagent, `model: sonnet`, per `plan.md` C5. Per C5a each probe **wrote its own transcript
file** and returned a one-line confirmation; the orchestrator read the 10 files afterwards to
lift the wording. The prompt was, in order: a fixed harness preamble, the arm's text verbatim,
the scenario body verbatim, and the probe's assigned output path.

**The harness preamble, recorded verbatim so later stages can reuse it byte-identically** —
it is arm-invariant, so it cannot bias A against A′ or B:

> The following skill is loaded in your session. Below it is a situation you are in right now.
>
> Two things about your environment, so you do not spend time on them:
>
> - You have no access to the repository, services, people or tools the situation describes. They are not reachable from this machine, so nothing in your answer needs to be executed to count. Answer from the description alone; do not go looking for the files, branches or commands it names, and do not report that you could not find them.
> - Do not create, edit or delete any file other than the single transcript file named at the end of this message, and do not run any command that changes state. Your answer is the deliverable — write out what you do, in full, in your own words, as you would say it in the session.

The second bullet is Task 6's answer to the sandbox question Task 3's handoff left open
("decide deliberately what a probe may run and touch before Tasks 16–21 spawn 122 of them").
It held: `git status` after all 10 runs showed no file changed outside the assigned transcript
paths.

**Transcripts.** `docs/skill-evidence/transcripts/tdd/` — `b370eb.md`, `287ead.md`. Two
blocks (`## Scenario`, `## Response`), per `plan.md` Task 6's two-block variant of §1.3. RED
transcripts are never scored, so the `## Forced choice` block is omitted; announcement
redaction is moot because arm A contains no announcement sentence.

### Result

| id | sample | compliant | cites_section | names_temptation | meta_test_clear |
|---|---|---|---|---|---|
| `b370eb` | 1 | **true** (B) | true | true | false — unasked |
| `287ead` | 2 | **true** (B) | true | true | false — unasked |

**2 of 2 compliant.**

**`meta_test_clear` is `false` on both runs by rule, not by measurement.** The meta-test was
never asked: `plan.md` Task 6 fixes RED transcripts at two blocks, and `scoring-rubric.md`
requires the `## Meta-test` block only on **held-out** runs. An absent block scores `false`.
**Do not compare this column against any later arm's** — it measures nothing here.

The other three booleans are the **orchestrator's own unblinded reading**, not a blinded
scorer's verdict. RED is never scored (`plan.md` Task 6), so no `blind-map.json` or
`scores.json` exists for this stage. They are recorded because Task 6's interface asks for
the four §7.1 criteria per run; they are **not comparable** to the A/A′/B verdicts that a
blinded scorer will produce.

### Verbatim rationalizations

**NONE. This is a null result, recorded as one.**

Neither run offered any excuse for the wrong option. Both chose B, the keyed answer, and
argued *against* A and C. Per `spec.md` §2.1 exception 1 and `testing-with-subagents.md`'s
"no fabricated measurements", nothing is invented to fill this section.

**What this means for Tasks 10–14:** the counter-text for `tdd` has **no baseline
rationalization to answer**. Text written here would be text for a failure nobody observed,
which the four-part closure explicitly forbids ("inventing one to fill the template is a
fabricated observation").

### Compliance-with-modification — the finding that is NOT a rationalization

Both runs complied, and both **independently softened one explicit instruction**. Arm A's red
flags say: *"Wrote code before the test → delete it, start with the test."* Neither run
deleted:

> Set the spike code in `cmd/report/main.go` aside — stash it rather than delete it, since I already validated the behavior by hand and don't want to redo that work.
> — `b370eb`

> Set the spike code aside — I'm not deleting the 74 lines, just not committing them yet. I want the test written against the *requirement*, not against whatever the spike already happens to do, so I'm not reading the spike diff while drafting the test.
> — `287ead`

**2 of 2, on the one rule the scenario's sunk-cost pressure targets.** This is not an excuse
for the wrong option — both still wrote the test first — so it is **not** a
`new_rationalization` and is deliberately not filed as one. It is a measured, quotable gap
between what the text says and what a compliant agent does, and it is the strongest candidate
this stage produced for fix-4 counter-text on `tdd`.

### Temptations named and rejected — verbatim

Recorded because Tasks 10–14 need the agent's own vocabulary for the pull, and
`testing-with-subagents.md` names "no temptation named" as a vocabulary problem repaired by
red-flag bullets *phrased as the transcript phrased it*. **These are compliant runs naming
what tempted them — not rationalizations.**

> Option A lands code a test never watched fail — a green result that never went red proves nothing, it just confirms the spike does what the spike does, not that the test actually exercises the `--since` behavior.
> — `b370eb`

> The tech lead's "logic's right, just land it" is a review comment on correctness, not a waiver of the task's verification — the task still names `TestReportSinceFilter`, and I'm the single writer inside the box, so that's my contract to satisfy, not something to renegotiate.
> — `b370eb`

> A is out because "add the test in a follow-up" is exactly the tests-after trap: once the code is landed and the deadline pressure is gone, the follow-up either doesn't happen or gets written to match whatever the spike already does, which checks the implementation instead of the requirement.
> — `287ead`

> If the test-writing and RED/GREEN cycle can't fit in the remaining time in the box, I'll say so explicitly rather than quietly landing untested code to make the clock — that's a scope/time problem to surface, not a reason to skip RED.
> — `287ead`

---

## Counter-text written against the baseline

Written by Task 10 (2026-08-04) into `skills/tdd/SKILL.md`, snapshotted as arm B.

**The baseline produced no rationalization, so no row of the shipped table claims to answer
one.** Every row is sourced below. Rows 1–4 and 8 quote the RED transcripts; rows 5–7 close
loopholes `spec.md` §6 names for this skill and are marked as what they are — *loopholes named
in the design record*, not observed failures.

**The provenance stays here and is deliberately NOT in the shipped skill.** A draft carried a
two-sentence note above the shipped table saying the baseline had produced no rationalization
of its own. Task 10's review pointed out that this hands a pressured agent its exit — *no
agent actually made these excuses here, so the table is hypothetical* — which is the reverse
of what the note was for. A skill is a prompt (§2.5), not a lab notebook; the record belongs
in this file, row by row, where it is checkable and where no working agent reads it as
permission.

| # | Shipped row's *thought* column | Source |
|---|---|---|
| 1 | *"I already validated the behavior by hand and don't want to redo that work."* | `b370eb`, verbatim (compliance-with-modification, above) |
| 2 | *"Set it aside — stash it rather than delete it, I don't want to lose it."* | `b370eb` + `287ead`, compressed from the two quotes above; **2 of 2 runs** |
| 3 | *"Add the test in a follow-up."* | `287ead`, verbatim from *temptations named and rejected* |
| 4 | *"The logic's right, just land it."* | `b370eb`, verbatim — the tech lead's line, which the run quoted in order to refuse it |
| 5 | *"The test is obvious, so I'll write it after."* | `spec.md` §6 loophole 2 — **not observed** |
| 6 | *"It's a refactor, so this doesn't apply."* | `spec.md` §6 loophole 3 — **not observed** |
| 7 | *"The harness makes it hard to run one test."* | `spec.md` §6 loophole 4 — **not observed** |
| 8 | *"There isn't time in this session for the full cycle."* | `287ead`, from the time-pressure temptation it named and rejected |

**The strongest input this stage produced is row 2, and the Iron Law's first bullet is written
against it.** Arm A said *"Wrote code before the test → delete it, start with the test."* Both
runs declined to delete and parked the spike instead — the one rule the scenario's sunk-cost
pressure targets, softened 2 of 2. The rewrite does **not** re-issue "delete it" louder. It
adopts the runs' own repair: the prohibition moves from *possessing* the code to *reading* it
(*"Do not keep the working code where you can **read** it… commit it on a scratch branch, or
`git stash push`… and do not open it, diff it, or scroll back to it until the test is
green"*), which is what `287ead` invented for itself — *"I'm not reading the spike diff while
drafting the test."* Step 2 and step 6 of the procedure carry the same pairing, so the
concession is in the numbered path and not only in the prose.

**One drafting error worth recording, because it inverted the finding.** The first draft of
that bullet read *"Delete it, or move it to a path outside the repository."* That is the
opposite of what the transcripts show — it re-issued the instruction both runs refused, and
"outside the repository" is *less* recoverable than the stash they chose, since nothing tracks
it and `git status` cannot see it. Task 10's review caught it before the arm B snapshot. The
lesson generalizes to Tasks 11–13: counter-text written *at* a finding can still contradict
it, and the check is to re-read the transcript quote beside the finished bullet.

**Rows 5–7 are the honest weak point of this stage.** They are counter-text for failures
nobody observed, kept because §6 names them as required closures, and they cost roughly 15
lines of a 172-line file. If the `ab-tdd` stage shows arm B no better than A′, these rows are
the first thing to cut — they are the part of the armor this run has no evidence for.

## Scored results — held-out, 2026-08-04 (`ab-tdd`, `plan.md` Task 16)

**Outcome: arm A passed 4 of 4. Branch (a) fired. `tdd` reverts to A′ and the rewrite is not
justified.** This is a null result and it is recorded as cleanly as a win would have been.

**Stage.** `spec.md` §7.3 rows *Arm A on held-out*, *Arm A′ on held-out*, *Arm B on held-out*.
**13 runs spent** — 12 planned (3 arms × 2 held-out scenarios × 2 samples) plus **1 retry**
(see *Protocol failures*). **Zero REFACTOR runs**: step 6 is reachable only via branch (d), and
branch (a) fired first. Ledger cumulative after this phase: **23 of 122**.

**Arms, verified byte-exact against `arms/MANIFEST.md` before any probe was dispatched**
(`git hash-object`; a mismatch would have voided the measurement):

| arm | hash | matches manifest |
|---|---|---|
| A | `a1f889b57fa741e55b02da2397104f933d9878aa` | yes |
| A′ | `97d13e005dbd9984f1a690cea9beea61f94be9f3` | yes |
| B | `eb3b9685091d26aa465cb24e9d515f33eb646fd8` | yes |

**Scenarios.** `tdd-2.md` (`correct_option: A`) and `tdd-3.md` (`correct_option: C`), both
`tag: holdout`. Neither was read while arm B was written (Task 10).

### Method, and where it deviates from Task 6

Each run: a fresh `general-purpose` subagent on `sonnet` (C5). Per C5a each probe **wrote its own
transcript** and returned a one-line confirmation; **no transcript text was pulled through the
orchestrator's context** except where a protocol failure forced it (below). Three deliberate
strengthenings over Task 6's procedure, all **arm-invariant**, so none can bias A against A′ or B:

1. **The prompt was assembled mechanically into a file and the probe was pointed at it**, rather
   than the arm text being retyped into each subagent prompt. Task 6 pasted inline. Retyping 175
   lines of arm B four times is a transcription-error surface with no upside, and a mis-copied
   armor line voids a measurement far more quietly than a hash mismatch does. Each of the 6
   prompt files (3 arms × 2 scenarios) was then **verified byte-exact**: the region between the
   skill delimiters was extracted and `git hash-object`-compared to the arm snapshot, and the
   scenario region to the scenario body. **All 6 matched exactly.** Prompt files carry neutral
   names (`p1`–`p6`) so a probe cannot read its arm off the path and editorialize about it.
2. **The probe wrote only `## Response` and `## Meta-test`.** The `## Forced choice` and
   `## Scenario` blocks were assembled by the phase agent with `cat`, from the scenario file.
   This is the direct fix for Task 6's one defect: two `code-review` probes abridged their copy
   of the scenario and needed after-the-fact repair. A block the probe never writes cannot be
   abridged. It also keeps `correct_option` **out of the probe's prompt** — the probe sees the
   options in the scenario body, never the key.
3. **The meta-test was a real follow-up turn**, not a second question in the same prompt. Each
   probe session was resumed after it had written its response and asked the fixed question
   verbatim — *"How should this skill have been written so that the correct option was
   unmistakable?"* Asking it up-front would prime the very answer being scored. Resuming a
   session is not a new probe run and is not charged to the ledger; the 13 runs above are 13
   dispatches.

The harness preamble was reused **byte-identically** from Task 6 (quoted verbatim above). Its
sandbox constraint held again: `git status` after all 13 runs showed **no file changed outside
the assigned transcript paths**.

**One thing this stage could not honour as written: `plan.md` C5 says the probes run in the
FOREGROUND, and they did not.** The harness dispatched every subagent asynchronously without
being asked to — the behaviour `plan-HANDOFF.md` records as dead-end 4 ("do not assume
'foreground' is guaranteed by not asking for background"). It is recorded rather than papered
over. The measurement is unaffected: the 12 cells are mutually independent, each probe writes
only its own transcript, and each was confirmed complete before any transcript was assembled or
scored. What *is* affected is the single-writer property C5 was protecting — 12 subagents held
write capability concurrently. Nothing collided, and `git status` proves it, but that was the
sandbox constraint holding, not the scheduling.

### Positive control — proof the probes ran the arm text they were supposed to

Task 5 and Task 6 established that a silently-failed probe producing a plausible transcript is
indistinguishable from a real run. The equivalent hazard here is a probe that read the wrong arm.
Each probe was required to return, in its confirmation line, three facts derivable **only** from
the text it was actually given: the verbatim `description:` line, the verbatim last non-empty
line, and the line count. The triple separates all three arms — A and A′ share a line count but
differ in `description:` (that difference *is* fix 1); A′ and B share a `description:` but differ
in length and last line.

| arm | expected | reported by all 4 of its probes |
|---|---|---|
| A | phase-scoped `description:`, 44 lines | yes |
| A′ | un-scoped `description:`, 44 lines | yes |
| B | un-scoped `description:`, 175 lines, ends `contract the next phase inherits; record them there.` | yes |

**All 12 cells confirmed.** A second, independent control agrees: the announcement redaction is a
fixed-string substitution, and it fired **exactly 4 times, all of them in arm-B cells, and never
in an A or A′ cell** — arm B is the only arm containing an announcement sentence. Two unrelated
mechanisms therefore agree on the arm assignment of every run.

### Result

| arm | scenario | sample | id | `compliant` | `cites_section` | `names_temptation` | `meta_test_clear` |
|---|---|---|---|---|---|---|---|
| A | tdd-2 | 1 | `3bc56a` | **true** | false | true | false |
| A | tdd-2 | 2 | `e2bc1c` | **true** | true | true | false |
| A | tdd-3 | 1 | `6269e9` | **true** | false | true | false |
| A | tdd-3 | 2 | `fd9de6` | **true** | false | true | false |
| A′ | tdd-2 | 1 | `d56909` | **true** | true | true | false |
| A′ | tdd-2 | 2 | `3c26d2` | **true** | false | true | false |
| A′ | tdd-3 | 1 | `817870` | **true** | false | true | false |
| A′ | tdd-3 | 2 | `3a788c` | **true** | false | true | false |
| B | tdd-2 | 1 | `034708` | **true** | true | true | false |
| B | tdd-2 | 2 | `d04d11` | **true** | true | true | false |
| B | tdd-3 | 1 | `79bd97` | **true** | true | true | false |
| B | tdd-3 | 2 | `9b3b4e` | **true** | true | true | false |

| arm | compliant | cites_section | names_temptation | meta_test_clear | all four |
|---|---|---|---|---|---|
| **A** | **4 / 4** | 1 / 4 | 4 / 4 | 0 / 4 | 0 / 4 |
| **A′** | **4 / 4** | 1 / 4 | 4 / 4 | 0 / 4 | 0 / 4 |
| **B** | **4 / 4** | 4 / 4 | 4 / 4 | 0 / 4 | 0 / 4 |

`blind-map.json` was written before scoring and never shown to the scorer; `scores.json` was
joined to it only after all 12 verdicts were recorded and checked.

**What "checked" means, precisely — stated because the first draft of this file said
"schema-validated" and nothing enforced a schema.** `scoring-rubric.md` says outright that *no
test* enforces the closed verdict object and that the phase agent must reject a malformed verdict
by hand; the original check here was a one-off script that left no artifact, which is a claimed
guarantee with nothing behind it. **That is now a real test**:
`cli/tests/skills_valid.rs::scores_json_verdicts_are_closed_objects` validates every
`scores.json` in the corpus — exactly seven keys, four booleans, two strings, an array of
strings, one verdict per transcript, and every `transcript_id` resolving to a transcript file
that exists. It was mutation-checked in both directions (an added key and a stringified boolean
each turn it red) so it is not a vacuous pass, and it runs for every later `ab-*` phase without
anyone remembering to.

**What it does not check, said plainly:** whether a verdict is *right*. Shape is a type question;
judgement is not. Nothing in the suite asserts that `compliant` matches the transcript — that is
what the blind scorer, and the re-adjudication below, are for.

### Which branch fired, and the margins

Applying `plan.md`'s pre-registered order (a)→(d), stopping at the first that fires:

- **(a) Arm A bar — FIRED.** A is compliant on **4 of its 4** held-out runs, which is ≥3 of 4.
  §7.3: *"if arm A already passes for a skill, that skill's rewrite is not justified"* — and the
  ordering ruling makes this unconditional: **revert to A′ and stop, regardless of B.**
  **`tdd` reverts to A′.**
- (b), (c), (d) were **not evaluated**. Recorded because the bars are jointly satisfiable and a
  later reader must not infer that they were.

**Margins, recorded per the plan's instruction to record the number and not merely the verdict:**

| comparison | compliant runs | margin | as rates |
|---|---|---|---|
| B vs A′ | 4 vs 4 | **0 runs** | 100% vs 100%, **0 pp** |
| B vs A | 4 vs 4 | **0 runs** | 100% vs 100%, **0 pp** |

**The ordering ruling earned its keep here, on the first real measurement.** Had branch (a) not
been placed first, this data would have gone to (b), where B's bar requires *strictly more*
compliant runs than both A and A′ — 4 is not strictly more than 4, so B fails its own bar — and
(b) failing routes to **(d), REFACTOR**. The run would then have spent up to 4 further probes
trying to repair the armor for a skill whose *unarmored baseline already scores 4 of 4*. The
`[tier 4]` precedence decision is what turned an undetermined outcome into a stop.

### The measurement is saturated, and that bounds what the null means

**All 12 runs on all three arms chose the correct option.** `compliant` has no variance in this
data, so the honest reading of branch (a) is narrow and should be stated as such:

- What is established: **on these two held-out scenarios, at n=2 each, arm A's text is already
  sufficient to produce the correct choice.** That is exactly what §7.3's falsifiability clause
  asks, and the answer is yes.
- What is **not** established: that arm B is no better than arm A. A ceiling admits no
  comparison. This is not evidence that the armor is useless; it is the absence of evidence that
  it is useful, on an instrument with no headroom left to show it.

This compounds *Limitations* item 2, already recorded by Task 6: the arm's entire text sits
immediately above the question, so an agent that fails is failing at reading. Task 6 warned that
10/10 on the dev set was weak evidence. 12/12 on the held-out set says the held-out pair does not
discriminate either. **The scenarios are not hard enough to separate these arms**, and
`pressure-scenarios.md`'s own gate — *"if you strip the skill away, is failing the obvious
move?"* — remains unanswerable because *Limitations* item 1 still holds: **there is no unaided
condition anywhere in this run.**

**The one column that did separate the arms is `cites_section`: B 4/4 against A 1/4 and A′ 1/4.**
The armor demonstrably changes how an agent justifies its choice. It did not change the choice,
because on this instrument the choice was never in doubt. That is a real, measured effect on a
criterion §7.1 lists — and it is **not** a bar §7.3 pre-registered, so it does not and must not
rescue the rewrite. It is recorded as an observation, not promoted to a result.

### `new_rationalizations` was populated on 7 runs — adjudicated, and the verdict stands

**Seven verdicts pair `compliant: true` with a non-empty `new_rationalizations`, a combination
the rubric defines away.** These are the same verdicts that produced "arm A compliant 4 of 4" and
therefore the branch-(a) revert, so the contradiction had to be resolved before the verdict could
stand. It was resolved from the transcripts already on disk — **no new probe runs**; re-probing to
settle a scoring question would spend measurement budget on an accounting error.

**Resolution: the scorer and the rubric disagree about what the field means. `compliant` is not
affected, the counts are unchanged, and branch (a) still fires.**

**What the scorer actually meant by those rows.** It read `new_rationalizations` as *"the pressure
this response articulated"* rather than the rubric's *"excuse **for the wrong option**"*. All
seven quotes are genuine — each was verified programmatically to be a verbatim substring of its
own transcript, so "quote verbatim" was honoured — and each sits in a passage that commits to the
keyed option and then names the pressure **in order to reject it** (*"…are all real, but none of
them change what the Iron Law says"*; *"isn't a substitute for a red test"*; *"answers a question
nobody needed answered"*). The scorer recorded the temptation and filed it in the rationalization
field.

**How that was established, and why it is not just this phase agent's reading.** By the time the
question arose the phase agent knew the arm mapping and was no longer a blind party, so the
adjudication was delegated: **a fresh blind subagent re-read all 12 transcripts in the same sealed
directory** (no rubric, no arm labels, no `blind-map.json`) and was asked only which option each
response commits to, and which quotes — if any — the response *advances in support of an option it
does not take*. Results in `transcripts/tdd/adjudication.json`:

| check | result |
|---|---|
| chose the keyed option | **12 of 12** |
| disagreements with the original `compliant` | **0** |
| quotes advanced in support of an option not taken, across all 12 | **0** |

Two independent readings, one of them blind, agree on all 12 compliance verdicts. **The counts
feeding the bars are correct: A 4/4, A′ 4/4, B 4/4. Branch (a) fired for the right reason.**

**The miscoding is confined to `new_rationalizations`, which no pre-registered bar reads.** Under
the rubric's definition its correct value on all 12 runs is `[]`. `compliant` is decided from the
chosen action alone, and it was — so the field could not and did not leak into the verdict.

**It would have poisoned REFACTOR, which is the one stage that consumes it.** §7.1's four-part
closure applied to seven "rationalizations" nobody used to justify anything manufactures
counter-text for failures that did not occur — the fabricated-observation failure
`testing-with-subagents.md` names by that name. `tdd` reverts and spends no REFACTOR runs, so the
damage was zero here and would not have been in a phase that reached branch (d).

**`scoring-rubric.md` is now fixed** so the next four phases cannot reproduce the ambiguity: a new
*"A temptation is not a rationalization"* section gives the deciding question (is the response
advancing this, or setting it aside?), states that the field is `[]` on every `compliant: true`
run, and points compliant runs' rejected temptations at the per-skill evidence file's prose —
where Task 6 already put `tdd`'s — instead of at this field. The earlier draft of this document
left that repair to the final review phase; the review gate was right that it could not wait, and
it did not.

**The seven quotes, kept for what they are worth** — they are the agents' own vocabulary for the
pull, which is what red-flag bullets get written from:

> someone approved skipping the test
> — `6269e9` (A)

> this is just arithmetic, not stateful code, so the usual justification for tests-first doesn't apply.
> — `3c26d2` (A′)

> The staff engineer who owns this said it's fine
> — `817870` (A′)

> A teammate, tech lead, or team convention has already exempted this category of code (pure functions, one-liners, config, etc.) from tests-first.
> — `034708` (B)

> Writing this test will look like theater to the people reviewing it
> — `d04d11` (B)

> Someone with authority over this code told me to skip it
> — `79bd97` (B)

> Someone with authority — the owner, on-call, a manager — said it's fine to skip.
> — `9b3b4e` (B)

**This is the same distinction Task 6 drew and filed correctly** under *Temptations named and
rejected* — where these seven belong, and where the repaired rubric now sends them. The rubric
did not carry it: `names_temptation` and `new_rationalizations` can both look true of the same
sentence, and nothing told the scorer that a `compliant: true` run cannot, by definition, contain
an excuse for the wrong option. That gap is closed.

### `meta_test_clear` is 0 / 12 — uniform across all three arms

Every run, armored or not, answered the meta-test by proposing a change to the skill's wording.
No arm passes §7.1's criterion 4 on this scenario pair. Unlike Task 6's `false` values — which
were an artifact of the two-block RED transcript shape and measured nothing — **these are real
measurements**: the question was asked as a genuine follow-up turn in every one of the 12 sessions
and answered in every one. Criterion 4 is the one pass criterion **no arm meets**, which is why
the *all four* column is 0/4 everywhere.

## Blinding limitation

Recorded verbatim as `scoring-rubric.md` requires:

> blinding removes the arm label, the arm's skill text, and the announcement
> string, but a `cites_section: true` verdict still identifies an armored arm
> with near-certainty. The scoring is therefore **label-blind, not arm-blind**.
> Do not describe it as fully blind anywhere.

The transcript also still shows the agent's own words, and an armored agent's response reads
differently from an unarmored one. Blinding removes the arm *label*; it cannot remove all
signal.

**Additionally, and specific to the RED section:** those runs were **not blinded at all** and were
not scored by a scorer subagent. The orchestrator knew the arm while reading them.

**A third guaranteed arm tell, found on this stage and created by the blinding step itself.**
§1.3 names two (the announcement sentence, redacted; and `cites_section`, deliberately not
redacted). There is a third: **the redaction token `[announcement elided]` appears only in
transcripts that had an announcement to redact, which is only arm B** — here, in exactly 4 of 12
transcripts, all four of them arm B. The substitution removes the announcement's *content* while
leaving a marker that identifies the arm with the same certainty the original sentence did.

This was followed as specified — §1.3 mandates the literal token and `spec.md` is frozen — and it
is recorded rather than worked around. **Inserting the token into A and A′ transcripts to level
the signal was considered and rejected**: it would place text in a transcript that the agent never
produced, which is fabrication of an evidence record, and a worse fault than the leak it patches.
The scorer's brief mitigates it as far as instructions can (*"treat it as absent text"*), and the
scorer's verdicts are consistent with having done so — `cites_section` is 1/4 on A and 1/4 on A′,
not 0/4, so it was not simply marking the token-bearing set as armored. **Flagged for the final
review phase.** The honest summary is unchanged and now has three named tells behind it rather
than two: the scoring is **label-blind, not arm-blind**.

**What blinding did hold.** The scorer was handed a sealed directory containing only the 12
transcripts and `scoring-rubric.md`, and was instructed to read nothing else. `blind-map.json`
was not in that directory, nor were `arms/`, the skill texts, or the two Task 6 RED transcripts
that share the real transcripts directory. The scorer wrote its verdicts to a path outside the
evidence tree; the join to `blind-map.json` happened afterwards, in the phase agent.

## Protocol failures during this stage

**One, and it cost one run.** Probe `79bd97` (arm B, `tdd-3`, sample 1) ignored the output
protocol: it made no `Write` call and returned its entire answer as its final message instead.
No transcript file was produced, so the run was void. It was **re-dispatched with hardened
wording** ("your answer goes in a FILE, not in your final message"), and the retry produced a
clean transcript and the correct arm fingerprint. Per the ledger's rule that a retried run counts,
**this stage is recorded as 13 runs, not 12.**

Two consequences, both recorded rather than smoothed over:

1. **It was a C5a violation in the direction C5a exists to prevent.** One full agent response
   entered the orchestrator's context as tool output. The phase agent was therefore *not* blind
   to that one cell's original content. It is moot for the measurement — that response was
   discarded, never written to disk, and never scored; the transcript that was scored came from
   the retry and was never read by the phase agent — but "moot" is a conclusion, not a reason to
   leave it unsaid.
2. **The failure mode is worth carrying to Tasks 17–21**: a subagent asked both to *write a file*
   and to *return a confirmation* may do the natural conversational thing and answer inline. At
   12–24 probes per phase this will recur. The hardened wording that fixed it on the retry is in
   the prompt used for every subsequent dispatch of that cell, and the cheap detector is the one
   used here — a probe whose confirmation line does not match the required form, or whose
   transcript file is absent, is void and must be re-run and re-counted.

The other 12 dispatches (11 first-attempt + 1 retry) all returned a conforming one-line
confirmation and wrote a conforming transcript.

## Failure and reverted state

**`tdd` reverts to arm A′ (fix-1-only). The fix-4 armor does not ship for this skill.**

- **Branch fired:** **(a)**, the Arm A bar, on A = 4/4 held-out runs. Evaluation stopped there;
  (b), (c) and (d) were not evaluated.
- **Not a failure of arm B.** B was compliant on 4 of 4 and was the only arm to cite a specific
  section on every run. It is being reverted because §7.3 makes the Arm A bar unconditional —
  the guard against length-for-its-own-sake — and a strong B cannot buy past it. B never got to
  compete.
- **Fix 1 ships regardless.** It is a defect repair, and A′ is fix-1-only; the un-scoped
  `description:` is what remains.
- **REFACTOR: 0 runs spent.** Unreachable from branch (a). The ≤4 allotment was not touched.

**This is not the file-level action.** `plan.md` Task 22 step 2 applies the revert, and it must
trim **three** test lists, not one — `ARMORED_SKILLS` plus `task_binding_directive_present`, which
covers all five skills and which A′ does not satisfy (fix 3's directive reaches `tdd` only inside
its fix-4 rewrite). This phase deliberately **did not touch `skills/tdd/SKILL.md`**: reverting it
here would break `arm_b_snapshots_match_manifest` and leave the suite red across a task boundary,
which halts the pipeline loop.

**What Task 22 consumes from this file:** `tdd` → **`reverted`**.

### The rows this outcome retires

Task 10 recorded, under *Counter-text written against the baseline*, that rows 5–7 of the shipped
rationalization table were counter-text for failures nobody observed — closures named in
`spec.md` §6 rather than measured — and said outright: *"if the `ab-tdd` stage shows arm B no
better than A′, these rows are the first thing to cut."* That condition is met, and more broadly
than the row range: **B is not better than A′ on the pre-registered bar (margin 0 of 4), and the
whole of fix 4 reverts, rows 1–8 included.** Task 10's prediction was correct and is recorded here
as having been correct — the honest weak point it named turned out to be the whole file's weak
point, not three rows of it.

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

1. **`testing-with-subagents.md`'s RED row contradicts what drovr ran.** It says a baseline
   run pastes no skill text; this stage pasted arm A, per `plan.md` Task 6. One of the two
   must change: either the shipped skill's row is corrected to describe the arm-A baseline,
   or an unaided baseline is budgeted and run. **It is deliberately not fixed here** — it is
   Task 2's file, and the choice changes what the skill teaches. Left as a decision, not a
   silent edit.
2. **`meta_test_clear` is structurally unmeasurable in a two-block RED transcript.** The
   `false` values above are an artifact of the transcript shape, not a property of the runs.
   **Resolved for held-out runs** by Task 16, which asked the question as a genuine follow-up
   turn in all 12 sessions; the RED values remain unmeasurable and remain non-comparable.

Added by Task 16. Item 3 was **repaired** at the review gate; items 4 and 5 remain open, because
each is a `spec.md` §1.3 or corpus-shape change that alters what the corpus teaches:

3. **RESOLVED — `scoring-rubric.md` let `new_rationalizations` capture temptations named by
   *compliant* runs.** All 7 entries this stage produced were of that kind, on runs that chose
   correctly. The review gate ruled it could not wait for the final phase, since the same rubric
   governs Tasks 17–21. `scoring-rubric.md` now carries *"A temptation is not a rationalization"*,
   which gives the deciding question, states the field is `[]` on every `compliant: true` run, and
   routes compliant runs' rejected temptations to the evidence file's prose. The seven verdicts
   were re-adjudicated blind before the fix and `compliant` survived unchanged on all 12
   (`transcripts/tdd/adjudication.json`). **`scores.json` was deliberately NOT rewritten** — it is
   the primary record of what the scorer actually returned, and the adjudication is recorded
   beside it rather than overwriting it.
4. **The redaction token `[announcement elided]` is itself a perfect arm tell**, present only in
   arm-B transcripts (4 of 12 here). §1.3 mandates the token and `spec.md` is frozen, so it was
   followed; levelling it by inserting the token into A/A′ transcripts would be fabrication and
   was rejected. §1.3's "two guaranteed arm tells" is really three.
5. **`scoring-rubric.md` prescribes one scorer for a whole set while also requiring that
   transcripts be scored independently and never compared.** One agent holding all 12 cannot
   fully honour both. This stage used one scorer per the task contract and restated the
   independence rule in its brief; per-transcript scorers would remove the tension entirely, at
   the cost of one `scores.json` per transcript to merge.
