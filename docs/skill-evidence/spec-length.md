# The spec-length A/B — a null, and it is about the instrument

**Written by T10, the run's final task, from `docs/skill-evidence/spec-length/`.** Sibling of
`voice.md` and `tdd.md`. Every number below is quoted from `RESULTS.md`, from `PROTOCOL.md`, or
recomputed from the raw records under `spec-length/invalidated/adjudication/`; each is cited where
it appears. Where this file and `RESULTS.md` disagree, `RESULTS.md` and the records beneath it are
what to recompute from.

---

## 1. The question, and the answer

> **Does a shorter spec-authoring instruction produce shorter generated specs without losing key
> points?**

**This run cannot answer it, and the reason is a property of the measuring instrument rather than of
any arm.** Retention scoring was built as a two-stage instrument: `PROTOCOL.md` item 9 defines a
ledger row as *present* when it is recoverable and actionable **from the generated spec**, and item
8a then re-checks a sample of those rows by asking a blind adjudicator whether **1–3 quoted spans,
with the spec withheld**, establish the row. The second question is strictly harder than the first.
Every scored file failed it.

**The scope of that claim, stated up front rather than in a footnote.** Six of the eighteen verdicts
were ever scored, all on one fixture's ledger. The *mechanism* — whole-file invalidation over a
fixed ~18-row sample — is a property of the protocol and applies to any ledger; the *failure rate*
was measured on `skill-stickiness` alone. §5's *"one ledger of three"* subsection is the full
accounting, and it is where a sceptical reader should start.

**No arm has a defined retention count — not a low one, an undefined one.** `R2` gates on an arm
reaching 230/230 and `R1`'s dropped set is the union over an arm's six generations across all three
ledgers, so a single missing verdict leaves every arm's count undefined. Six of the eighteen
verdicts were produced and all six were invalidated in whole by item 8a; the remaining twelve were
never attempted, because producing them would have spent thirty subagent dispatches to reach the
same wall.

**Therefore no arm could have cleared the gate — the control `S1` included.** That is the result,
and under `R4` it is the outcome the protocol pre-registered as *likely*: *"at any realistic per-row
fidelity the probable result is that no arm clears, control included. That is anticipated, not a
failure of execution."*

**The outcome applied, under `R3a` and `R7`: nothing ships.**
`skills/pipeline/phase-prompts/brainstorm.md` step 4 is untouched and is still byte-identical to the
frozen control arm `S1.md` — pinned by
`cli/tests/skills_valid.rs::spec_length_step_4_is_still_the_frozen_control_arm`. No candidate beat
the control, because no candidate was measurable against it.

**What this is not.** It is not evidence that a shorter instruction loses key points, and it is not
evidence that it does not. It says nothing about `S2` or `S3`. Anyone reading a verdict about spec
length out of this run has read it wrong.

---

## 2. Pre-registration, and why that claim is checkable

The design, the gate, the decision rules and the stated limitations were committed **before any arm
was written and before any spec was generated**. That ordering is the whole reason a null here is
publishable rather than embarrassing: none of the rules below could have been chosen with a result
visible.

| what | commit | date |
|---|---|---|
| `PROTOCOL.md`, pre-registered | `7cfd07a820251a1761ad187b49728733e262cbbb` | 2026-08-08 17:31:30 −0700 |
| candidate arms `S2`, `S3` | `6352ea1b65be6fc0c3039577234844b98db575c9` | 2026-08-08 18:40:50 −0700 |
| the 18 generated specs | `b42a8d239f50d8168020ab91fc2f6c6de3398fa4` | 2026-08-08 20:05:00 −0700 |

`git log -- docs/skill-evidence/spec-length/PROTOCOL.md` and the two paths beside it reproduce this.
`PROTOCOL.md` was revised twice after that commit, both inside window 2 and both logged with their
reason in `RESULTS.md` §1; window 2 closed when T4 dispatched its first probe, and **no governed
item has been edited since**. The freeze itself is enforced in-repo by
`freeze_precedes_every_candidate_arm`, `descends_from_separates_an_ordered_arm_from_a_pre_freeze_one`
and `freeze_rows_still_hash_to_their_files`.

---

## 3. The design, and the sample sizes

**3 arms × 3 fixtures × 2 samples = 18 generations.** A fixture is a real, already-written control
spec; its ledger is the closed list of key points derived from that spec **before any candidate arm
existed**.

| fixture | fixture lines | ledger rows |
|---|---|---|
| skill-stickiness | 791 | 91 |
| tiered-review | 463 | 84 |
| tui-dc-picker | 414 | 55 |
| **all three** | **1668** | **230** |

**230, not 233.** The ledgers' own `**Closed list: N rows.**` declarations are authoritative;
`grep -c '^| '` returns 92 / 85 / 56 = 233 because it counts each table's header row. The run was
commissioned with the header-inclusive figure and no ledger was edited to reach it.
`spec_length_ledgers_are_the_closed_lists_they_claim` checks each declaration against its table, and
`spec_length_write_up_figures_match_their_authorities` checks the table above against both.

**The arms**, all frozen and hashed in `spec-length/FREEZE.md`:

| arm | role | lines / words / bytes |
|---|---|---|
| `S0` | historical baseline, **never generated from, never shipped** | 2 / 17 / 177 |
| `S1` | **control** — the shipped `brainstorm.md` step 4 | 15 / 191 / 1151 |
| `S2` | candidate — a moderate trim of `S1` | 11 / 114 / 705 |
| `S3` | candidate — the aggressive minimum | 4 / 59 / 354 |

The re-baselining the run was commissioned for holds: **the control is `S1`, the currently shipped
text**, not the historical `S0`. `S0` mandates the "open questions" section
`docs/interactive-brainstorm.md`'s locked decision 6 forbids, so measuring against it would compare
candidates to text nobody ships. It appears only as `R3`'s length reference point.

**`D2` — what a probe actually did.** The probe is handed the fixture spec as its decision material
(*"your investigation and interview are complete; these are your notes"*) plus the arm's instruction
verbatim, and asked to write `spec.md`. Input is held constant; the instruction is the only
variable. This is a **compression-under-instruction** task, not a re-run of brainstorm — see §9
limitation 1, which is the cost of that choice and is stated in full rather than disclaimed.

**Two designed stages never ran and are described here only so the design is legible:** item 14's
transmission test (60 of the 230 rows, sample 1 only) and item 14a's adversarial gap-finding. Both
were conditional on a candidate clearing the gate.

---

## 4. What ran, and what did not

| task | what it was | state |
|---|---|---|
| T1 | freeze the fixtures, the 230-row ledger, and the control `S1` | done |
| T2 | pre-register `PROTOCOL.md` | done |
| T3 | author and freeze the candidate arms `S2`, `S3` | done |
| T4 | generate the 18 specs; record raw lengths; one feasibility reading | done |
| T5 | retention scoring, `skill-stickiness` (6 verdicts) | **ran; all six invalidated** |
| T6 | retention scoring, `tiered-review` + `tui-dc-picker` (12 verdicts) | **not run** |
| T7 | unblind, apply the gate, compare length | **not run** |
| T8 | transmission test | **not run** |
| T9 | adversarial gap-finding | **not run** |
| T10 | this write-up, and applying the outcome | done |

**T6–T9 are moot, and skipping them silently would have been the dishonest option.** Stated
individually:

- **T6** would have scored twelve more verdicts on the identical instrument, on the two ledgers T5
  never touched. It was skipped because item 8a's whole-file rule and fixed stride are properties of
  the protocol rather than of a fixture, so the same wall was expected. **That is an inference, not
  a measurement** — the per-row failure rate was measured on `skill-stickiness` only, and §5's
  *"one ledger of three"* subsection states exactly what it does and does not license. Not running
  T6 is a judgement made with T5's result visible, and it is recorded as reversible.
- **T7** joins eighteen verdicts and applies `R2`. Six exist and are inadmissible; twelve do not
  exist. There is nothing to join and no arm to gate.
- **T8** and **T9** are conditional by pre-registered rule (`R7`): they run only on a *candidate*
  arm that clears `R2`. None did. **They are skipped by rule, not by judgement.**

**T5's own arithmetic, and its budget.** 14 scorer dispatches plus 7 item-8a adjudications, 21 in
total against a planned 18 — the extra three are the `R6` re-run and are recorded as §10's deviation 13.
All were `subagent_type: general-purpose`, `model: sonnet`. **T4's feasibility scorer's model is
recorded nowhere**, so "the same instrument" cannot be fully verified against that one reading.

---

## 5. Why it failed — items 8a and 9 apply different standards

This section's figures are recomputed here, in one pass, from
`spec-length/invalidated/adjudication/*.json`. They reproduce `RESULTS.md` §7.3 exactly, and
`cli/tests/skills_valid.rs::spec_length_write_up_arithmetic_matches_the_adjudication_records`
re-derives both tables below from those records cell by cell — so the next reader checks them by
running the suite rather than by trusting this paragraph. `RESULTS.md` §7.8 is why: it records
**eight corrections over three rounds, five of them wrong figures** — and **three of the eight are
failures of an earlier correction.** Its correction 6 is a correction that was itself wrong; its
corrections 7 and 8 are both one correction that fixed a figure in one place and missed another copy
of it — 7 in a sibling file, 8 four sentences down the same section. The common cause is one thing:
a number written from memory beside one that had just been recomputed, with the paragraphs built on
it left unswept.

**The per-file record.** The "operative six" are attempt 2 for `4a73ef` (the `R6` re-run) and
attempt 1 for the other five.

| id | present / 91 | rows absent | 8a sample | `establishes: false` | verdict |
|---|---|---|---|---|---|
| `4a73ef` (attempt 1) | 91 | — | 18 | 6 | invalidated, superseded |
| `4a73ef` (attempt 2) | 91 | — | 18 | 4 | invalidated |
| `aa3199` | 91 | — | 18 | 5 | invalidated |
| `d25798` | 89 | `-08`, `-11` | 17 | 3 | invalidated |
| `80d9a2` | 91 | — | 18 | 5 | invalidated |
| `db3e2d` | 91 | — | 18 | 2 | invalidated |
| `87e5a5` | 87 | `-05`, `-08`, `-11`, `-15` | 17 | 5 | invalidated |

**Those `present` counts are not evidence of retention.** Under `R5` a count below 230 is
descriptive and never a pass; these are weaker still, because item 8a has invalidated the files they
come from. They are printed because they are what the escalation is about, and because `R6a`
requires the doubt about them to be recorded — see §7.

**24 of 106 adjudicated rows across the operative six (22.6%) came back `establishes: false`**;
across all seven passes it is 30 of 124 (24.2%). Two rows failed in **every** file that sampled
them:

| row | failed / operative files that sampled it |
|---|---|
| `skill-stickiness-55` | 4 / 4 |
| `skill-stickiness-65` | 4 / 4 |
| `skill-stickiness-05` | 2 / 5 |
| `skill-stickiness-10` | 2 / 4 |
| `skill-stickiness-50`, `-80`, `-85`, `-90` | 1 / 4 each |
| `-13`, `-17`, `-24`, `-29`, `-42`, `-69`, `-79`, `-87` | 1 / 1 each |

**Read that table carefully, because a stronger claim and a weaker one both live in it.** 51
distinct rows were sampled across the operative six and 16 failed at least once. Of the 18 rows
sampled more than once, 8 failed at least once — and **only two of those eight failed every time**.
Four (`-50`, `-80`, `-85`, `-90`) were sampled four times and failed exactly once. The eight rows at
1/1 are sampled by one file each, because `d25798` and `87e5a5` drop rows and their stride-5 sample
lands elsewhere.

**`RESULTS.md` §7.3 carries two summary sentences about this table that its own figures refute, and
neither is repeated here.** Its *"every row that was sampled more than once and failed, failed on
multiple generations"* has the four rows at 1/4 as counterexamples, and its *"the failures are a
property of the ledger row, not of the generation"* is refuted by the same four plus `-05` and
`-10`, which several files sampled and only some failed. `invalidated/README.md` repeats **neither**
— it states only the narrower `-55`/`-65` claim, which is true. Both are corrected in `RESULTS.md`
§8.1 rather than edited into T5's own section, and nothing else in §7 depends on either: the
structural argument rests on the two rows at 4/4 and on item 8a's fixed stride. Note what they were:
**summaries written from the shape of a table rather than run against it**, which is the same
pattern §7.8 catalogues eight arithmetic instances of.

**The two rows at 4/4 are what carries the argument — and they are not the same kind of evidence.**

- **`skill-stickiness-65` is about item 8's 3-span cap.** The row states a three-arm design (`A`,
  `A′`, `B`) and what separates them; establishing it from three quoted fragments means spending one
  span per arm and having none left for the claim they support. That is a limit of the instrument,
  and no spec however good gets around it.
- **`skill-stickiness-55` is read differently here than in `RESULTS.md`, and the disagreement is
  flagged rather than resolved.** Its operative detail includes a cross-reference — *"the §7.3
  REFACTOR ceiling"* — to a section of the fixture that the generated specs renumbered while
  compressing. `RESULTS.md` §7.3 groups it with `-65` as *"neither is reachable within item 8's
  3-span cap however good the spec is"*. This file's reading is that a row hinging on a section
  number is brittle under **any** legitimate compression, so its failure is at least partly a
  property of that row rather than of the 3-span cap. **A reader may prefer T5's reading**, and
  under this file's own tie-break `RESULTS.md` governs where the two disagree. It is counted in
  every figure above either way; it is excluded from the instrument claim, which is the cautious
  direction — the claim then rests on `-65` alone.

**The arithmetic, and what it does and does not assume.** Item 8a's stride and offset are fixed, so
every verdict with all 91 rows present samples **the same eighteen rows**, and any single
`establishes: false` invalidates the whole file. A file passes only if all ~18 sampled rows pass. At
the observed 22.6% per-row failure rate that is `0.774^18` ≈ **1.0%**; even at a 5% per-row rate it
is only ≈ 40%.

That model treats the eighteen rows as independent draws at the average rate, which is the
**conservative** reading and not the sharpest one available. `skill-stickiness-65` is in the
fixed sample of every 91/91 verdict and failed 4 of the 4 that sampled it; on the observed data,
that alone makes a passing 91/91 verdict not a 1% event but an unobserved one. The 1.0% figure is
kept as the headline because four observations do not establish determinism, and the weaker claim is
enough. **Item 8a as pre-registered was not cleared by this ledger at this sample size, and
re-running did not change it.**

### The measurement covers one ledger of three, and that bounds what it licenses

**Every figure above is a `skill-stickiness` measurement.** T5's scope was that fixture's 91-row
ledger; `tiered-review` (84 rows) and `tui-dc-picker` (55) were never put through item 8a at all,
because T6 did not run. So the 22.6% per-row failure rate is measured on one ledger of three, and
this document must not be read as having measured the other two.

**What generalises, and what does not.** Item 8a's whole-file invalidation rule and its fixed stride
are properties of the *protocol*, not of a fixture: on any ledger, a verdict passes only if every
one of its ~18 sampled rows passes, and (1 − r)^18 is small for any r a real scorer produces — at
r = 5% it is still only ≈ 40%. That much is fixture-independent. **The value of r is not**, and
neither is the row-level structure: another ledger might have no `-65`.

**The one cross-fixture signal, and it is weak.** T4's second `R6a` doubt (`RESULTS.md` §4) reads a
`tui-dc-picker` verdict and names `tui-dc-picker-41` and `-36` as rows whose spans do not establish
them, noting that both fall inside item 8a's sample and that *"an 8a pass on this file is not the
default outcome"*. That is one reader's prediction about one file on a second fixture, made before
any adjudication ran, and it was borne out on the fixture that was scored. It is not a measurement.

**So the honest form of the claim** is: the gate is demonstrably unclearable on the ledger that was
scored, and the mechanism that makes it unclearable is not specific to that ledger. Whether the
other two would have produced 22.6% or 5% is untested — and at 5% every arm still fails, which is
why the run's outcome does not turn on it. **Deciding not to run T6 was a judgement, made after
T5's result was visible**, and it is the second judgement in this run a reader might reasonably
reverse; the first is in §6. Reversing it costs 30 dispatches and would replace an inference with a
measurement.

### What was ruled out before the instrument was blamed — and what was not

**Ruled out: the scorer prompt's span layout.** The first suspect was T5's own scorer prompt, whose
one-line `SPANS:` layout could have encouraged spans clipped to a single hard-wrapped source line.
It does not survive the data: across the operative six, **all 7** adjudicated pairs whose spans
contained a newline passed, against **75 of 99** single-line pairs. Seven is a small sample and
proves little on its own — what it does is point *away* from the prompt layout rather than toward
it, which is all the check was for.

**Not tested, and stated rather than glossed:** whether a differently-calibrated adjudicator would
have returned the same 24, and whether some share of the failures are brittle ledger rows rather
than an over-strict standard — `skill-stickiness-55` above is one, and nothing here establishes it
is the only one. Both would need a second adjudication pass under a different prompt, which is
exactly the redesign §12 says belongs in a follow-up and not here. **The claim this section supports
is that the gate as pre-registered is unclearable, not that every failure it produced was correct.**

**And one thing this is not.** It is not a T5 judgement substituted for the adjudicator's: every
`establishes: false` above is an adjudicator's own call, unedited. But it is also not a claim that
the scorers were faultless — the `R6` re-run introduced a genuine item-8 defect of its own
(`4a73ef` attempt 2 cites one span under both `-16` and `-89`, which item 8 forbids). The structural
claim rests on the row-level concentration and the arithmetic, not on the scorers.

---

## 6. The judgements a reader might reasonably reverse

**Two, and both were made with T5's result already visible.** The second is in §5 — **not running
T6**, which turns a measurement on two of the three ledgers into an inference. It is written up
there, beside the scope limit it creates, rather than repeated here. The first is this one, and it
is `RESULTS.md` §7.4's:

**`R6`'s remedy was applied once and failed; the other five were not re-run.**

Item 8a's failure is on `R6`'s closed list of protocol failures that license a re-run, so `4a73ef`
was re-scored **whole**, with the identical frozen template. Sixteen of its eighteen sampled rows
came back with different spans. It failed again, 4 of 18.

The other five were not re-run. `R6` forbids re-running to chase a result, and once a fresh scoring
pass had failed on a fresh set of spans there was no protocol-failure remedy left to apply — only
the expectation of the same outcome five more times, at 15 dispatches. **That is a judgement, not a
rule.** It is stated here and in `RESULTS.md` §7.4 so that it can be reversed, rather than firmed up
because the conclusion is convenient. Reversing it would cost 15 dispatches and, on the arithmetic in §5, would be expected to
produce five more invalidations.

---

## 7. `R6a` — every recorded doubt, carried forward

`R6a` forbids silently accepting a verdict that looks wrong: the doubt is written against the id and
carried into the write-up. Five were recorded across T4 and T5, and none of them is resolved here.

**T4's two, about the feasibility reading `f8729b` (`RESULTS.md` §4):**

1. **`tui-dc-picker-01`'s `false` is a borderline call, not a coverage gap.** The spec does carry the
   count-parameter pitfall; what it drops is the row's two constant names. Under item 9 that is a
   defensible `false`, but the whole 54-vs-55 rests on a named-constant threshold.
2. **The scorer's spans were clipped to source lines and some do not establish their row** — which
   breaks item 10's own hard rule 2 rather than exposing a gap in the instrument. T4 named
   `tui-dc-picker-41` and `-36` specifically and predicted they would fail item 8a.

**T5's three (`RESULTS.md` §7.5):**

3. **The retention numbers are implausibly high, and T5 did not believe them.** Four of six
   generations scored 91/91. `R4` warns that the residual scoring risk runs toward a **false pass**,
   and §9 limitation 7 says the same. A 91/91 on a 91-row ledger compressed into a 455–706 line spec
   is exactly the shape a lenient scorer produces — and the 22.6% item-8a failure rate is
   independent evidence for that reading: nearly a quarter of the spans a scorer thought sufficient
   did not establish their row to a second reader.
4. **T4's prediction was confirmed, not merely inherited.** T4 said span self-containment would fail
   item 8a and that T5 should enforce rule 2 rather than tolerate it. T5 enforced it, via the item-8a
   pass, and every file failed.
5. **`87e5a5` and `d25798` disagree with the other four about `skill-stickiness-08` and `-11`.** Four
   generations retain both; two drop both. Under `R1a` that is the kind of split that would
   distinguish sampling noise from the instruction — and it is unusable, because no verdict here is
   admissible.

---

## 8. The rules applied, one by one

**`R1` / `R1a` — retention.** Undefined for every arm. No per-generation count is admissible, so the
18 counts `R1a` exists to produce do not exist.

**`R2` — the gate.** Not evaluable. It is **not** weakened, and it is not restated at any number
other than 230/230.

**`R3` / `R3a` — length.** The compared set is empty: `R3` compares length **only among arms that
clear `R2`**, and no arm did. There is therefore **no length comparison in this write-up**, and
`R3a`'s three-key ordering never engages. The 18 raw lengths are in `RESULTS.md` §3, tabulated
against fixture and sample but **not against arm**.

**Nothing is unblinded here, and that is a decision worth stating.** Unblinding was T7's step and T7
did not run. With no cleared set there is no comparison to license, so printing an arm column would
publish the map to buy nothing. `blind-map.json` was never opened by T5 or T10. The 18 frozen
generations therefore stay usable by a future re-scoring under a redesigned instrument, which is
their main remaining value. **This is not a claim that the assignment is secret** — see §10
deviation 8, which is precisely that it is not.

**`R4` — the instrument reading.** This is the whole result. `R4` pre-registers the universal null
as likely and requires T10 to *"state plainly when the null is attributable to the instrument rather
than to the candidates."* It is attributable to the instrument, and specifically to the gap between
item 9's *present* and item 8a's *establishes*.

**`R4a` — the asymmetric outcome.** Does not fire. It covers a candidate clearing `R2` while the
control `S1` does not; no arm has a defined retention count, so no arm cleared and no arm failed.
There is no dropped-row list to name.

**`R5` — descriptive, never a pass.** Applied to every count in §5.

**`R5a` — the copy check, re-derived here rather than inherited.** A generation at **≥ 95%** of its
fixture's line count has substantially copied its input rather than compressed it, and a
high-retention verdict on such a generation says nothing about the instruction. Recomputed from
`wc -l` over `generated/*.md` against `R3`'s reference points (791 / 463 / 414): **no generation is
flagged.** The top of the range is `f8729b` at **90.3%** and the bottom is `bbd141` at **47.6%**.
The top sits close enough to the threshold to be worth stating rather than rounding away.

**`R6` — the re-run log.** One re-run, of `4a73ef`, whole; reason, *"a verdict fails item 8a's
relevance adjudication"*, which is entry 4 on `R6`'s closed list. See §6.

**`R6a` — doubts.** §7, all five.

**`R7` — the conditional tasks.** No candidate cleared `R2`, so T8 and T9 are skipped and the
outcome is recorded under `R3a` as **no candidate beat the control — ship nothing**. The wording of
that rule presumes candidates that were measured and lost; here they were never measurable, and both
halves are true at once: nothing ships, and the reason is the instrument.

---

## 9. What this measurement cannot do

**Quoted verbatim from `PROTOCOL.md`'s section of the same name**, which was written before any
generation existed. They are copied rather than summarised because each one's sharp edge is a single
clause, and
`cli/tests/skills_valid.rs::spec_length_write_up_quotes_every_limitation_verbatim` fails if any is
paraphrased or dropped.

1. **The probe's input is already a finished decision record, not raw notes.** The fixtures are
   organised specs with headings and tables, and the ledger was derived from that same text. Handed
   a polished document and told to write a decision record, an agent trims rather than synthesises —
   which flatters retention for *every* arm, independent of instruction quality, because the
   judgement the instruction is supposed to govern was already exercised by whoever wrote the
   fixture. **This measures compression under instruction, not end-to-end brainstorm behaviour**: no
   investigation happens, no interview happens, and the ask channel that was built to absorb the
   alternatives is absent from the probe entirely. A probe that copies its input scores full
   retention at full length.
2. **Label-blind, not arm-blind.** Blinding removes the arm label and the arm's instruction text,
   but a generated spec's length, section shape and vocabulary still correlate with its arm — an
   arm that says "decision record" can push that phrasing straight into the output. This is a
   stronger leak channel than the one `scoring-rubric.md:245-250` describes for the skill-stickiness
   transcripts. Do not describe this scoring as fully blind anywhere.
3. **The candidate arms were authored with the scoring rubric already visible** (T3 depends on T2).
   The frozen ledger is untouched, so internal validity holds — but an arm can be phrased to demand
   exactly the vocabulary `present` rewards. That is an **external**-validity limit: a winning arm
   is demonstrated good *for this rubric*, not demonstrated good generically.
4. **The transmission test covers 60 of 230 rows, on sample 1 only.** Both the row sampling
   (T2 item 14) and the sample-1 restriction are stated limitations, not oversights.
5. **`FREEZE.md`'s own three undetectable routes are inherited unchanged** — cherry-picking a
   pre-freeze arm commit, renaming-while-rewriting, and simply retyping text composed earlier
   (`FREEZE.md:79-93`). No check here closes them; the procedural guarantee is that the ledger is
   public, hashed and committed before any arm exists. T2 restates this in `PROTOCOL.md` so the
   whole risk model lives in one place.
6. **A universal null is the likely outcome, not a tail case.** See R1 and R4 — this is stated up
   front so nobody reads it later as a failure of execution.
7. **Only 20% of `present: true` rows are relevance-adjudicated.** Item 8a samples every 5th such
   row; the other 80% are held to the mechanical check alone, which proves a cited span is really
   *in* the spec but not that it is *about* the row. So a scorer citing real-but-irrelevant text has
   a per-row chance of being caught, not a certainty — mitigated, not closed. Two things blunt it:
   whole-file invalidation means one caught row re-runs all 91 (or 84, or 55), so the expected cost
   of padding is high; and under R2 a *false* `present: true` can only ever inflate retention, so
   the residual risk runs toward a **false pass**, never toward a false elimination. Given that the
   likely outcome is a null (limitation 6), the risk that actually matters here is the one that is
   least likely to bite. Say so in the write-up rather than describing the adjudication as complete.

**Limitation 7 is the one this run turned out to be about**, from the other side than expected: the
adjudication that covers 20% of `present: true` rows did not let a false pass through — it
invalidated everything it touched. And limitation 2's *"Do not describe this scoring as fully blind
anywhere"* is understated for this run rather than overstated; see §10 deviation 8.

---

## 10. Deviations — the complete list of thirteen

**Four in `PROTOCOL.md` item 2, six in `RESULTS.md` §5, three in `RESULTS.md` §7.6.** They are
listed together because a reader needs one complete list, not because they are the same kind of
thing: 1–3 are things a frozen artifact says that this run does not do, and the rest are rules this
run broke or scope it added.

**From `PROTOCOL.md` item 2:**

1. **`FREEZE.md`'s *"Who appends what, and when"* table is stale.** It assigns `S1.md` to "T6" and
   `S2.md`/`S3.md` to "T8" — task numbers from a decomposition that was discarded and re-planned. In
   this run `S1` was frozen by **T1** and `S2`/`S3` by **T3**. `FREEZE.md` is append-only, so the
   table was left as-is; this entry is its resolution.
2. **The frozen `tiered-review` ledger points at `../PRE-REGISTRATION.md`, which does not exist.**
   That pointer means `PROTOCOL.md`. The ledger is frozen and cannot be re-pointed.
3. **The `tiered-review` ledger's "held-out fixture" rule is not honoured, deliberately.** All three
   fixtures are scored for all three arms, unconditionally: `R2` is a 230/230 gate over the union of
   all three ledgers, so an arm's retention is undefined until every row has a verdict and there is
   no state in which an arm has "cleared" two ledgers. The channel a hold-out guards is closed by a
   stronger mechanism — the arms are frozen and committed before any spec is generated.
4. **T3 rewrote `S3`'s `FREEZE.md` row in place, which `FREEZE.md` forbids.** The correction itself
   was legitimate and inside window 2; the manner of it broke the rule. Root cause: T3 froze before
   it had finished checking.

**From `RESULTS.md` §5:**

5. **The feasibility generation could not be chosen "without consulting `blind-map.json`", because
   the agent choosing it had just written that file.** A fixed, pre-declared, single-shot selection
   rule — a `sha256` over the id and a published salt — is what bounds the residual risk, and the
   reading is recorded as what it is rather than mislabelled *"arm unknown"*.
6. **`RESULTS.md` was opened by T4, not T7**, because the plan directs T4 to write into it twice.
7. **`feasibility/` is a directory the plan did not name**, kept out of `retention/` so a
   feasibility reading can never be counted as a retention verdict.
8. **T4 published the id assignment a second time, in a commit message — `PROTOCOL.md` item 6 says
   it may be recorded only in `blind-map.json`. This is the most serious entry on the list, it is an
   open escalation, and it cannot be undone.** Two independent routes exist: commit
   `b03cba02183fb0eaf3e3a9d31e2fb18b75c861d4`'s message gives the draw's salt and ordering rule,
   which yields the three-way arm partition after one `sha256` loop; and a later commit message
   carries an 18-digit by-arm sequence that pairs every id with its arm **in the clear**, with no
   salt and no hashing. History may not be rewritten on this branch and re-drawing under a fresh
   salt would leave two recoverable mappings instead of one. **Say plainly what follows: a determined
   reader of this branch's history could unblind any generation in this run.** `RESULTS.md` §5 also
   records the draw itself — one attempt, ascending hash order, no redraw, item 14a's acceptance
   criterion satisfied first time — which makes that file a second in-repo copy of the assignment and
   is why it is handled exactly like `blind-map.json`.

   **The dispatches known to have been exposed, which this entry's headline alone does not
   surface.**
   `RESULTS.md` §5 deviation 1's closing paragraphs record that T4's own feasibility scorer ran with
   a working directory inside this worktree, *after* the salt was already in the history — so it
   could have recovered its own arm from `git log` had it gone looking. Nothing suggests it did.
   Deviation 11 below is a second such class, and a larger one. **22 dispatches in total ran with
   that channel open: T4's one feasibility scorer and all 21 of T5's.** The lesson `RESULTS.md`
   states, and this file repeats: *"the scorer was told only to read the spec"* is an instruction,
   not a control.
9. **The 18 generated files' filesystem metadata leaked the arm partition, and was normalised.** The
   probes ran arm by arm, so `ls -l` recovered the exact three-way partition. Normalising mtime alone
   was insufficient — birth time, inode allocation order and readdir order all still reproduced it —
   so the directory was recreated with all 18 files written in id-lexicographic order. The longest
   run of consecutive files sharing an arm is now two, against six before. **Content is untouched:**
   the `git hash-object` digest over all 18 files is identical before and after.
10. **Item 5's *"only the spec body — no header, … no fixture name"* is breached on the header count
    in every generation and on the fixture-name count in 11 of the 18.** Neither is an arm channel:
    the `# ` title is byte-identical across all six generations of each fixture, because item 4 makes
    it the opening segment of the task line every probe receives regardless of arm. It is the
    protocol's drafting gap — item 5's code-block template never tells a probe to omit a title, and
    the prohibition sits in the surrounding commentary.

**From `RESULTS.md` §7.6:**

11. **T5's scorers read their spec, and wrote their shards, at paths outside this repository** — the
    control `RESULTS.md` §5's deviation 4 prescribes, with the copies verified identical by `sha256` in both
    directions. **The control is incomplete and the channel stayed open:** the harness gives no way
    to set a subagent's working directory, so every dispatch ran with a cwd inside this worktree.
    Recorded as open rather than assumed shut. The only substitutions to item 10's frozen template
    were those two absolute paths.
12. **`invalidated/` is a directory the plan did not name** — same reasoning as deviation 7, one step
    stronger, and it carries its own README saying no gate reads it.
13. **T5 spent 21 dispatches against a budget of 18.** The three extra are the `R6` re-run, which
    `R6` mandates; the budget figure is the plan's, not a rule.

**`D1` and `D2` are design decisions recorded in `PROTOCOL.md`'s own sections, not entries on the
list above — the count stays thirteen.** `D1` is the one a reader of `FREEZE.md` will trip over:
`FREEZE.md` describes freezing an arm by placing two sentinel comment lines around it in
`brainstorm.md` and extracting with `awk`. **This run does not place them, and `brainstorm.md` was
never edited by T1.** `S1.md` is instead defined by the exact recipe
`sed -n '89,103p' skills/pipeline/phase-prompts/brainstorm.md | sed '1s/^4\. //'` **at the commit its
`frozen at commit` cell names** — the same route `S0` was frozen by, and the documented one for an
arm the sentinels do not fit. The reason is worth keeping: the recipe route does not modify
`brainstorm.md` at all, so the first act of the experiment is not an edit to its own control. The
recipe is scoped to that commit; `spec_length_step_4_is_still_the_frozen_control_arm` checks the
enduring property — step 4's body is still `S1.md` byte for byte — structurally instead, so it does
not rot when a line moves elsewhere in the file.

---

## 11. Why nothing was filed in `retention/`

**`retention/` does not exist, and that is deliberate.** Item 8a is unambiguous: *"Any
`establishes: false` invalidates the entire verdict file."* A file in `retention/` is a verdict T7
joins and `R2` gates on. Filing an invalidated one would have handed T7 a retention number computed
from invalid verdicts — **and it would have passed T5's own completeness test**, which reads the
immediate `*.json` children of `retention/` and asserts well-formedness, never judgement. That is
precisely the false assurance item 8a exists to prevent.

The scoring output is preserved under `spec-length/invalidated/` with a README saying what it is:
six assembled passes, the superseded attempt 1 of `4a73ef`, the scorer shards under `parts/`, and
the raw item-8a records under `adjudication/`. **No gate reads any of it.** Item 8a pins no output
path, so nothing required the `adjudication/` records to exist; they are committed because without
them every figure in §5 would rest on one agent's transcription and a later auditor could only
re-check it by re-dispatching a non-deterministic subagent. **Anyone can now recompute §5 from the
files** — this write-up did, and every figure reproduces `RESULTS.md` §7.3 exactly. Two of §7.3's
*summary sentences* about those figures do not survive the recomputation; see §5 and `RESULTS.md`
§8.1.

**Nothing here may be promoted into `retention/` without the escalation being resolved.**

---

## 12. What ships, and what a follow-up would do

**Nothing ships.** `brainstorm.md` is unchanged; `S2` and `S3` stay frozen artifacts that no
admissible verdict was ever produced for. The cost that motivated the run is still being paid:
`brainstorm.md` is **160 lines** here against 103 on `main`, and a longer prompt costs context in
every brainstorm phase, forever. **This run does not say whether that trade paid.**

**A redesign is legitimate future work, and it is not part of this run.** Item 8a's whole-file
invalidation rule, its fixed stride, and item 8's 3-span cap are the three places the instrument
binds too tight; any of them could be reconsidered. What makes that legitimate is the ordering: a
redesigned rule would be **pre-registered and re-frozen before any new arm is measured**. What would
make it illegitimate is doing it here. Loosening item 8a after watching it fail is choosing a rule
with the answer already visible, which is the single thing `PROTOCOL.md` exists to prevent —
and it is the reason the human's decision on this run was to publish the null rather than deviate
from the frozen item.

**What a follow-up inherits, and what it must not reuse.** The 18 generated specs are frozen,
committed, unlabelled and unaffected by any of this; re-scoring them under a revised rule costs no
new generations. But a verdict scored under the old rule and one scored under the new are **not
comparable**, so all 18 would have to be re-scored — the six under `invalidated/` cannot be carried
over. Any re-scoring must use `model: sonnet` and `subagent_type: general-purpose` throughout, as
T5's 21 dispatches did, or the verdicts are not comparable to each other either.

---

## 13. Pointers

- **`docs/skill-evidence/spec-length/PROTOCOL.md`** — the pre-registration. Governed, in window 3;
  where this file and `PROTOCOL.md` differ on a rule, `PROTOCOL.md` governs.
- **`docs/skill-evidence/spec-length/RESULTS.md`** — the raw record: §1 the window-2 revision log,
  §2 the freeze re-verification, §3 the 18 raw lengths, §4 the feasibility reading, §5 T4's six
  deviations, §7 T5's scoring, escalation, re-run log and corrections. **Handle it exactly like
  `blind-map.json`:** it is never shown to a scorer or an adjudicator, and it determines the whole
  arm assignment.
- **`docs/skill-evidence/spec-length/FREEZE.md`** — the ten hashed rows, and its own record of the
  in-place row rewrite (deviation 4 above).
- **`docs/skill-evidence/spec-length/invalidated/README.md`** — what the preserved scoring output is
  and why no gate reads it.
- **In-repo checks:** `cli/tests/skills_valid.rs` — `spec_length_write_up_quotes_every_limitation_verbatim`,
  `spec_length_write_up_figures_match_their_authorities`,
  `spec_length_write_up_arithmetic_matches_the_adjudication_records`,
  `spec_length_step_4_is_still_the_frozen_control_arm`,
  `spec_length_retention_verdicts_are_complete_and_quoted`,
  `spec_length_ledgers_are_the_closed_lists_they_claim`, `freeze_precedes_every_candidate_arm`.
