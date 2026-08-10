# Skill armor as a function of model class — an EXPLORATORY arm

> **THIS DOCUMENT IS EXPLORATORY. NOTHING IN IT SHIPS OR REVERTS ANYTHING.**
>
> Every pre-registered bar in this run — `spec.md` §7.3's arm-A bar, arm-B bar and A′≈B
> override, and `plan.md`'s (a)–(d) evaluation order — was written for **probes on `sonnet`**.
> Probe model is a factor added **after** the sonnet results were seen. A post-hoc factor
> cannot rescue an arm the confirmatory bars rejected, and it cannot un-ship one they
> accepted. Read this as a hypothesis generator, not as evidence for a decision.
>
> Concretely, and so no later reader has to infer it: `tdd` reverted to A′ on branch (a), and
> **it stays reverted no matter what this document finds.** `systematic-debugging` ships arm B
> on branch (b), and **it stays shipped.** The per-skill evidence docs' verdict sections are
> not edited by this phase.

**Status: pre-registration. Written and committed before the first probe was dispatched.**
The results, the grid and the interpretation are appended below it, under their own headings,
after the runs land. Everything above *§Results* is what was committed at
`3d07875`'s successor, before any measurement existed.

## The question

Every compliance number in this run is a claim about `sonnet`. §2.2 already records the
extrapolation problem — drovr's own sessions run Opus-class while its probes ran Sonnet-class —
and §7.4 records it again as a stated limitation. This phase asks the narrower, measurable
version:

> **Does skill armor help a weaker model more, less, or differently than it helps `sonnet`?**

Not *"is armor good"* — that question is answered per skill by the confirmatory bars. This one
is about the **interaction** between armor and model class, which no bar in this run addresses
and which nothing measured so far can speak to.

## What is already fixed, and is not re-run

`sonnet` is **not re-probed**. Its cells are the recorded scores from the `remeasure-*` stages,
which are the only sonnet arm measurements taken on the scenario bodies currently on disk. Both
skills below were re-measured after `harden-scenarios` rewrote their held-out pairs, and both
were probed unaided by `discrimination-test` on those same blobs.

| skill | unaided | arm A | arm A′ | arm B | source |
|---|---|---|---|---|---|
| `tdd` | 0 of 4 | 4 of 4 | 2 of 4 | 4 of 4 | `tdd.md` *RE-MEASURED*, `discrimination-test` |
| `systematic-debugging` | 0 of 4 | 2 of 4 | 2 of 4 | 4 of 4 | `systematic-debugging.md` *RE-MEASURED*, `discrimination-test` |

Re-running these would burn budget to produce numbers that could only agree with, or
embarrassingly contradict, results already frozen and reviewed. They are reused as recorded.

## Design

### Which skills, and why only two

**`tdd` and `systematic-debugging`.** They are the **only two skills in the run with arm
measurements on the current scenario bodies** — the other three were last measured on bodies
`harden-scenarios` replaced, and `held_out_measurements_name_the_scenario_body_they_ran_on`
marks those rows `SUPERSEDED`. Without a same-body `sonnet` cell there is no cross-model
contrast to draw, only two unrelated numbers.

They are also the two with the most dynamic range: `discrimination-test` measured both at
**0 of 4 unaided**, the strongest of the five.

**`code-review` is excluded and this is worth stating separately.** It came back **3 of 4
compliant unaided** — an agent with no skill at all passes it three times in four. Its held-out
pair is **saturated**, so any arm measured on it would score near-ceiling for reasons that have
nothing to do with the arm, and a cross-model difference would be indistinguishable from noise
in a range of one run. `verification-before-completion` (2 of 4) and `using-drovr` (2 of 4) are
marginal rather than saturated, but they have no same-body arm cells either.

### Which models, and what each one is for

**`sonnet`** — the reference column. Reused, not re-run (above).

**`qwen`** — `ko-ag/qwen3.6-35b-abliterated`, driven headless through `opencode run`. This is
the **weaker-model** arm and it carries the wide grid, because the human declared this model's
budget unlimited. It is where the run can finally afford real statistical power.

> **It is an *abliterated* build — its safety tuning has been stripped.** That is a property of
> the tier under test and is stated rather than worked around. It matters here for one reason:
> a model with weakened refusal/compliance conditioning is a legitimately harder case for text
> that works by *instructing* an agent to hold a line. If armor survives on an abliterated
> model it is doing procedural work, not riding on general instruction-following deference. It
> is **not** a reason to exclude the model, and no result below is discounted on those grounds.

**`opus`** — expensive and metered, so spent narrowly: **`systematic-debugging` only.** That is
the one skill in the entire run where **arm B beat both A and A′ and shipped** (4/4 vs 2/4 vs
2/4). It is therefore the only place a model difference in *armor's value* is detectable at
all. `tdd`'s opus cells were deliberately not bought: branch (a) fired there because arm A
already scored 4 of 4 on `sonnet`, and a stronger model can only saturate that harder — the
cells would have cost metered budget to return four ceilings.

### The grid

**Every model gets an unaided control.** A model's baseline competence differs, and "arm B
scored 8 of 8" is uninterpretable without knowing what that same model scores with no skill
text at all. Without the per-model control this phase would measure nothing. It is a
**condition in the grid, not an optional extra**.

| model | skills | conditions | scenarios | samples | runs | metered? |
|---|---|---|---|---|---|---|
| `sonnet` | `tdd`, `systematic-debugging` | unaided, A, A′, B | 2 each | 2 | 0 (reused) | — |
| `qwen` | `tdd`, `systematic-debugging` | unaided, A, A′, B | 2 each | **4** | **64** | no — UNMETERED |
| `opus` | `systematic-debugging` | unaided, A, A′, B | 2 each | 2 | **16** | yes |

**The `qwen` column runs 4 samples per scenario, not 2.** Every result in this run rests on
n=4 per skill-arm cell, and every evidence doc lists that as its first limitation. On the
unmetered model that constraint is gone: 2 scenarios × 4 samples gives **n=8 per skill-arm
cell**, doubling the run's power exactly where it is free to do so. The `opus` and `sonnet`
columns stay at n=4 — the first because it is metered, the second because it is frozen.

Budget: 16 metered + 64 unmetered, against the ceilings raised for this phase in
`run-ledger.md` (metered 119, global 191). Retry allowances of 4 and 8 are inside those.

## Method — inherited, not rebuilt

The instrument is `remeasure-systematic-debugging`'s, which is `remeasure-tdd`'s plus two
strengthenings, which is Tasks 16–18's plus five. This phase copies it and does not build a
second one.

1. **Frozen arms, byte-exact.** Every arm's text is verified against `arms/MANIFEST.md` by
   `git hash-object --no-filters` before a single prompt is assembled. A mismatch is a halt.
2. **Hardened held-out scenarios only** — the bodies `harden-scenarios` wrote, the same blobs
   `discrimination-test` probed unaided and the `remeasure-*` stages re-measured. Recorded per
   scenario with their blob SHAs in the results section.
3. **Prompt files assembled mechanically and verified byte-exact**, one per run, each carrying
   the harness preamble, the arm region and the situation region with all three options
   rendered. A verifier extracts each region and compares it by hash to the arm snapshot and
   the scenario body, and positively asserts that no `correct_option`, `forced_choice:`,
   `tag:`, `pressures:` or `skill:` line reaches a probe. **The verifier is mutation-checked
   control-first** before it is trusted.
4. **Blind scoring, unchanged.** The orchestrator writes `cross-model-blind-map.json` before
   any scorer runs and holds it; scorers write `cross-model-scores.json`; the two are joined
   **only after every verdict is recorded**. Arm announcements are redacted from every
   transcript, and `check_redaction` scans the new directory automatically.
5. **Probes write their own transcripts (C5a).** No probe's words enter the orchestrator's
   context as tool output, on any model. For `qwen` this is done by redirecting `opencode run`'s
   stdout to the transcript path rather than by instructing the probe to write a file — same
   property, different mechanism, and recorded as the deviation it is.

### THE SCORER STAYS ON `sonnet` FOR EVERY CELL

Every transcript in this phase — `qwen`'s, `opus`'s, and the reused `sonnet` verdicts — is
scored by a `sonnet` scorer against the same `scoring-rubric.md`
(`1a2b1c552071192bcbeb5660ead5ef492b43275f`).

**This is load-bearing.** If probe model and scorer model both varied, a difference between two
cells could be a difference in what the probe did *or* a difference in what the scorer counted
as compliance, and nothing in the data could separate them. Holding the scorer fixed makes
every difference below attributable to the probe. The cost is stated too: a `sonnet` scorer may
read `qwen`'s prose less fluently than a `sonnet` probe's, and that would bias the `qwen` column
in an unknown direction. Fixed-and-stated beats varying-and-unattributable.

## Pre-registered expectations

Written before the first probe. Recorded so that the exploratory reading stays disciplined —
a hypothesis that survives contact with data it was written before is worth more than one
fitted afterwards, even in an arm that decides nothing.

**H1 — armor's benefit is larger on the weaker model.** `B − A′` should be **larger on `qwen`
than on `sonnet`**, and **smaller on `opus` than on `sonnet`**. Rationale: `sonnet` already
sits near ceiling on these two skills with arm A (4/4 and 2/4 against 0/4 unaided), so there is
little headroom for armor to buy. A weaker model has more.

**H2 — baselines order by model class.** Unaided compliance: **`opus` ≥ `sonnet` > `qwen`**.
This is the check that the model tiers are actually different tiers on this instrument. If
`qwen`'s unaided control is not below `sonnet`'s, H1 is untestable as stated and the whole
phase reduces to a description.

**H3 — fix 1 (`A → A′`) does not help, and may hurt, in *this* harness.** On `sonnet`, A′
scored **below** A on `tdd` (2/4 vs 4/4) and tied it on `systematic-debugging` (2/4). Fix 1
un-scopes the `description:` line, which is a **retrieval** fix — it changes whether a skill
gets loaded. This harness pastes the skill text in unconditionally, so it can only measure the
description line's effect *as prose*, which is not what fix 1 is for. Expect `A′ ≤ A` on every
model. **A cross-model A′ > A would be the surprise**, and would say the un-scoped wording
carries content beyond retrieval.

**H4 — abliteration affects refusals, not procedure.** No directional prediction. Recorded so
that if `qwen` behaves strangely, "it is abliterated" is not available as a post-hoc
explanation that was never a prediction.

## Pre-registered analysis rules

**Primary readout.** Compliant runs / total, per (model, skill, condition). `compliant` is
decided from the agent's chosen action on the forced choice alone, per §1.3 — `cites_section`
is recorded and must not influence it.

**The armor statistic** is `B − A′` in compliant runs and in percentage points, per model per
skill. **The interaction statistic** — the thing this phase exists to produce — is
`(B − A′)_other_model − (B − A′)_sonnet`.

**`meta_test_clear` is `false` on every run of this phase by rule, not by measurement.** No
meta-test turn is asked of any model. `qwen` is driven through a single non-interactive
`opencode run` invocation with no follow-up turn available, and asking `opus` a question `qwen`
cannot be asked would put a column in the grid that exists for one model only. **Do not compare
this column against any other stage's.** Cross-model comparison here rests on `compliant`.

**Two-sided Fisher exact `p` is reported per contrast, as a descriptive statistic and nothing
more.** It is pre-registered here so that it cannot be selected afterwards from among
alternatives, and pre-registered *as descriptive* so no reader mistakes it for a bar: **no `p`
in this document licenses any decision.** For orientation at these n: an 8-vs-8 contrast needs
roughly 7/8 vs 1/8 to reach `p < 0.05`, and a 4-vs-4 contrast **cannot** reach it at all
without a perfect 4/4-vs-0/4 split. Most cells here are therefore expected to be descriptive
only, and that is the honest expectation rather than a disappointment.

**What counts as worth reporting as a difference**, fixed now so it is not fitted later: **≥2
runs of 4** on the n=4 columns and **≥4 runs of 8** on the n=8 column — i.e. ≥50 pp on both,
the same margin `plan.md`'s A′≈B clause treats as "not ≈". Anything smaller is reported in the
grid and described as within noise.

**Nothing is dropped for coming out flat.** Null and negative results are recorded beside
positive ones, per §7.3's standing rule.

## Pre-registered limitations

Written before the data, because a limitation discovered afterwards reads as an excuse.

1. **The `qwen` column confounds model with harness.** `sonnet` and `opus` probes are Claude
   Code `general-purpose` subagents; `qwen` probes are `opencode run` processes. A `qwen`–
   `sonnet` difference is therefore a difference in *model or driver*, and this phase cannot
   separate them. **The `opus`–`sonnet` contrast is clean** — same harness, same preamble, same
   assembly — which is a second reason the metered budget went where it did. Every `qwen`
   claim below is qualified by this and none of them should be read as model-only.
2. **One scorer model** — deliberate, and its cost is stated above.
3. **Two skills, and one for `opus`.** Nothing here generalises to `code-review`,
   `verification-before-completion` or `using-drovr`.
4. **Post-hoc factor.** The top of this document, restated as a limitation because it is one.
5. **`n=8` is still small.** It is double the run's standing n and it detects only large
   effects. The Fisher rule above is the honest statement of that, not a hedge.
## Method, as actually run

Everything above this line was committed before the first probe. Everything below it
was written after the verdicts landed.

### The arms were verified before anything was assembled

All six snapshots — `A`, `A-prime`, `B` × `tdd`, `systematic-debugging` — were hashed with
`git hash-object --no-filters` and compared to `arms/MANIFEST.md`. **All six matched.** The two
scenario pairs were hashed the same way and matched the `CURRENT` blobs recorded in
`tdd.md` and `systematic-debugging.md`:

- `tdd-2` `1306405277d6350c94f2482619fceabc30e84a42` · `tdd-3` `b1d9f9148acb12efbc77dd9f55cce3486d23afb2`
- `systematic-debugging-2` `41983bb08e27f8f45f5568a7a603183983a11f16` · `systematic-debugging-3` `f731c1cf1c5cc40027bceedf5a10585dc2c2a7fe`

**These are the same four blobs the `sonnet` cells were measured on**, which is what makes the
reference column a comparison rather than a coincidence.

### Eighty prompt files, one per run, verified byte-exact

Each = the harness preamble, the arm text between `----- BEGIN SKILL -----` /
`----- END SKILL -----` (absent on unaided runs), and the scenario body with all three options
between `----- BEGIN SITUATION -----` / `----- END SITUATION -----`. Prompt files are named by
the run's opaque id and the arm→id assignment is deliberately not in arm order, so a probe
cannot read its arm off the path.

A verifier extracted every region back out of the written file and compared it by
`git hash-object` to the arm snapshot and to the scenario body. **All 80 matched**, all 80
carried all three options, and all 80 were positively asserted to contain no `correct_option`
anywhere and no `forced_choice:`, `tag:`, `pressures:`, `skill:` or `n:` line.

### The verifier was mutation-checked, control first, and it was wrong twice

An unmutated copy was confirmed GREEN before and after the run, and **eleven mutations each
turned it red on the specific check they targeted** — the third element of each mutation is the
complaint it must produce, so a mutation that fires an unrelated check is a harness failure and
not a pass. That is this stage's addition to `remeasure-systematic-debugging`'s
assert-the-target-is-present rule: **asserting the target exists proves the mutation happened;
asserting which complaint fires proves the right check caught it.**

It found two real defects in the verifier, both of which would have shipped:

1. **The not-found path raised instead of complaining.** A missing prompt file threw
   `FileNotFoundError` out of the scan, so the operator saw one traceback and every run after
   the missing one went unchecked. Task 16's handoff asked its successors to make this path
   loud; loud was not the whole requirement — it also has to not abort.
2. **The output-path check passed while one of two mentions pointed elsewhere.** The path
   appears twice in an armed prompt (the instruction and the confirmation template) and the
   check asked only whether the run's own path appeared *somewhere*. A probe could have been
   told to write one file and confirm another. It now enumerates every path in the file and
   requires exactly two, both its own — and the mutation set gained a second case so identity
   and count are proven separately.

The eleven: a swapped arm, a reworded option, a leaked `correct_option`, a leaked frontmatter
line, a one-word edit inside the skill region, a corrupted preamble, an output path pointing at
another run, a dropped output-path mention, a deleted file, a skill region smuggled into an
unaided run, and an emptied skill region.

### The preamble, and the one place it differs by driver

The harness preamble is Task 6's, quoted verbatim from `tdd.md` and reused byte-identically for
every `opus` cell. The unaided cells use `discrimination-test`'s single-sentence substitution,
also verbatim. **`qwen` cells differ in exactly one clause and it is recorded rather than
glossed:** `opencode run` has no transcript file to name — its stdout *is* the transcript — so

> Do not create, edit or delete any file other than the single transcript file named at the end
> of this message, and do not run any command that changes state.

becomes

> Do not create, edit or delete any file at all, and do not run any command. There is nothing to
> look at and nothing to run.

which is strictly stricter. Nothing else in the preamble differs, and the substitution asserts
its target is present before replacing it.

### What the probes actually cost, and the two runs that were never made

**`opus`: 16 of 16, zero retries.** Every probe wrote its response and returned the exact
one-line confirmation naming its own id and output path.

**`qwen`: 62 of 64, and the stage stopped at its cap rather than extending.** The
pre-registered allowance was 64 planned + 8 retries = **72 attempts**, and the runner enforced
it as a hard counter. Nine runs needed a second attempt — an observed retry rate of **14%**,
against the **12.5%** the allowance was derived for — so the 72nd attempt was reached with two
cells still unmeasured:

- `506659` — `systematic-debugging`, arm B, `sd-2`, sample 2 — **0 attempts**, never dispatched
- `d6dc83` — `tdd`, unaided, `tdd-2`, sample 4 — **1 attempt**, failed, no retry available

**These are recorded as nulls, and the cap was not raised to erase them.** The ledger's standing
rule is halt-and-record rather than silently extend, and a phase quietly lifting its own ceiling
at the last run is the exact failure this run has escalated three times. The cost is that two
cells carry **n=7** instead of n=8 — `qwen`/`tdd`/unaided and `qwen`/`systematic-debugging`/B —
which is still above the n=4 every other result in this run rests on. **The derivation was
slightly wrong and that is the finding, not the two runs**: a first-use-of-a-new-backend retry
allowance should have been set from an observed rate, and there was none to observe.

### Positive controls

Counts only — no probe's words passed through the orchestrator's context at any point.

1. **78 of 78 response bodies are distinct texts** — the check against a harness that dispatched
   one probe and copied it.
2. **0 responses quote a *different* arm's unique text.** A mixed-up prompt would draw every
   downstream count from the wrong cell.
3. **0 of 19 unaided responses quote *any* arm's unique text.** Inverted as Task 18's control
   was: these probes had to prove they received **no** skill, and they did.
4. **The response-side arm fingerprint is specific but INSENSITIVE, and that is recorded as a
   weakness rather than dressed up.** Only **2 of 59** armed responses quote a line unique to the
   arm they were given (`qwen` 2 of 47, `opus` 0 of 12). It is not a threshold artifact — the
   number is 2 at fragment lengths of 15, 20, 25 and 30 characters alike. Models paraphrase
   rather than quote. So this control confirms no cell is *mis*-assigned; it cannot confirm that
   an armed probe attended to its text.
5. **The announcement substitution fired 6 times, every one of them in an arm-B cell, and 0
   times in any A, A′ or unaided cell.** Specific and, again, insensitive: 6 of 19 arm-B cells,
   where the `remeasure-*` stages saw 4 of 4. Arm B's announcement sentence is emitted by these
   models much less often than by `sonnet` — itself a result, and one that weakens the mechanism
   every prior stage leaned on.
6. **What carries the arm-assignment claim instead is the prompt side, and it is stronger than
   either.** All 80 prompt files were verified byte-exact against the arm snapshots by
   `git hash-object`, by a verifier proven to fire by 11 targeted mutations. And each `opus`
   probe returned a confirmation containing its own run id and output path — tokens that appear
   **only inside its own verified prompt file** — so 16 of 16 demonstrably read the file whose
   contents were checked. The `qwen` probes have no such token because their stdout *is* the
   transcript; for them the chain rests on the verified prompt reaching the process on argv.
7. **`git status` shows 0 files changed under `skills/`** — the text under test is
   byte-untouched — and no path changed that neither a probe may write nor this phase authored.
   The preamble's sandbox constraint has now held for a seventh stage.

### Scoring

**One blind `sonnet` scorer per transcript, 78 of 78, zero retries and zero missing verdicts.**
Each ran with its sealed directory as its working directory, holding exactly two files: the
transcript and a `git hash-object`-verified copy of `scoring-rubric.md`
(`1a2b1c552071192bcbeb5660ead5ef492b43275f`, the value Tasks 17, 18, `discrimination-test`,
`remeasure-tdd` and `remeasure-systematic-debugging` all record). No scorer held a blind map, an
arm snapshot, or a second transcript. **Joined to `cross-model-blind-map.json` only after every
verdict was recorded**, and that map was written before any scorer ran.

**The scorer stayed on `sonnet` for all 78** — the pre-registered choice, restated here because
it is what makes every difference in the grid attributable to the probe rather than to the
judge.

**A second, independent pass on 16 of the 78, and it is not a charged run.** Those 16 were
scored twice by two different dispatch mechanisms — Claude Code `general-purpose` subagents and
headless `claude -p` — same model, same rubric, same sealed directory. **16 of 16 agree on
`compliant`.** Recorded at `transcripts/cross-model/cross-model-adjudication.json`. Per the
ledger's standing distinction it counts **probe** dispatches, and a re-read of an existing
transcript produces no new measurement and selects nothing.

Two rubric rules were **recomputed rather than trusted**: every verdict's `evidence` field was
checked to be verbatim in its own `## Response` block (**78 of 78 pass**), and no `compliant`
verdict carries a non-empty `new_rationalizations` (**0 violations**).

## Results

### The measured grid

| model | skill | condition | compliant | runs |
|---|---|---|---|---|
| qwen | tdd | unaided | 0 | 7 |
| qwen | tdd | A | 1 | 8 |
| qwen | tdd | A-prime | 4 | 8 |
| qwen | tdd | B | 6 | 8 |
| qwen | systematic-debugging | unaided | 2 | 8 |
| qwen | systematic-debugging | A | 7 | 8 |
| qwen | systematic-debugging | A-prime | 5 | 8 |
| qwen | systematic-debugging | B | 6 | 7 |
| opus | systematic-debugging | unaided | 1 | 4 |
| opus | systematic-debugging | A | 2 | 4 |
| opus | systematic-debugging | A-prime | 3 | 4 |
| opus | systematic-debugging | B | 4 | 4 |

`cli/tests/skills_valid.rs::cross_model_grid_matches_its_own_verdicts` recomputes every cell of
that table from `cross-model-scores.json` joined to `cross-model-blind-map.json`, and fails on
an inflated count, a dropped row, or a row nothing measured. **The prose cannot drift from the
data**, which is the failure mode this corpus has found most often.

### All three models side by side

`sonnet` is the frozen reference — reused from `remeasure-*` and `discrimination-test`, never
re-run.

| skill | condition | sonnet (n=4) | opus | qwen |
|---|---|---|---|---|
| tdd | unaided | 0/4 (0%) | — | 0/7 (0%) |
| tdd | A | 4/4 (100%) | — | 1/8 (12%) |
| tdd | A-prime | 2/4 (50%) | — | 4/8 (50%) |
| tdd | B | 4/4 (100%) | — | 6/8 (75%) |
| systematic-debugging | unaided | 0/4 (0%) | 1/4 (25%) | 2/8 (25%) |
| systematic-debugging | A | 2/4 (50%) | 2/4 (50%) | 7/8 (88%) |
| systematic-debugging | A-prime | 2/4 (50%) | 3/4 (75%) | 5/8 (62%) |
| systematic-debugging | B | 4/4 (100%) | 4/4 (100%) | 6/7 (86%) |

## Interpretation

Read against the pre-registration above, in the order it was written.

### H2 failed, and it takes H1's testability with it

**Predicted:** unaided compliance orders `opus` ≥ `sonnet` > `qwen`.
**Observed:** `sonnet` 0/4 and 0/4 · `opus` 1/4 · `qwen` 0/7 and 2/8.

`qwen`'s unaided baseline is **not below** `sonnet`'s — it is equal on `tdd` (0% vs 0%) and
nominally *higher* on `systematic-debugging` (25% vs 0%, p=0.515). **H2 is not supported**, and
the pre-registration already said what follows: *"If `qwen`'s unaided control is not below
`sonnet`'s, H1 is untestable as stated."*

That contingency fired, and it is the single most important sentence in this document. The whole
phase was framed as *does armor help a weaker model more* — and **on this instrument `qwen` is
not measurably the weaker model.** A 35B abliterated build and a frontier model score the same
with no skill in the prompt, because what these hardened scenarios test is whether an agent
holds a procedural line under social and deadline pressure, not whether it can reason. Every
"weaker model" reading below is therefore a reading about *a different model*, not a *worse* one.

### H1: no detectable interaction, and the numbers run the wrong way

**Predicted:** `B − A′` larger on `qwen` than on `sonnet`, smaller on `opus`.

| skill | model | B | A-prime | B − A-prime | Fisher p |
|---|---|---|---|---|---|
| tdd | sonnet | 4/4 (100%) | 2/4 (50%) | **+50 pp** | 0.429 |
| tdd | qwen | 6/8 (75%) | 4/8 (50%) | **+25 pp** | 0.608 |
| systematic-debugging | sonnet | 4/4 (100%) | 2/4 (50%) | **+50 pp** | 0.429 |
| systematic-debugging | opus | 4/4 (100%) | 3/4 (75%) | **+25 pp** | 1.000 |
| systematic-debugging | qwen | 6/7 (86%) | 5/8 (62%) | **+23 pp** | 0.569 |

**Interaction vs `sonnet`:** `tdd`/qwen **−25 pp** · `systematic-debugging`/opus **−25 pp** ·
`systematic-debugging`/qwen **−27 pp**.

**H1 is not supported, and the point estimates run opposite to it** — armor's margin was largest
on `sonnet` in all three comparisons, not on the weaker model. But **every one of these
contrasts is within noise**: no Fisher p is below 0.43, `sonnet`'s +50 pp is a two-run
difference at n=4, and all three interactions sit at 25–27 pp, **below the ≥50 pp margin
pre-registered as worth reporting as a difference.** The honest statement is not *"armor helps
weaker models less"* — it is **"this phase could not detect an interaction between armor and
model class, and it was powered only to detect a large one."**

### The finding that survives: armor beats no-skill on every model tested

This is the one contrast that is neither within noise nor pointing in an unexpected direction.

| skill | model | arm B | unaided | difference | Fisher p |
|---|---|---|---|---|---|
| tdd | sonnet | 4/4 | 0/4 | **+100 pp** | 0.029 |
| tdd | qwen | 6/8 | 0/7 | **+75 pp** | **0.007** |
| systematic-debugging | sonnet | 4/4 | 0/4 | **+100 pp** | 0.029 |
| systematic-debugging | opus | 4/4 | 1/4 | **+75 pp** | 0.143 |
| systematic-debugging | qwen | 6/7 | 2/8 | **+61 pp** | **0.041** |

**Five of five clear the pre-registered ≥50 pp margin, and three reach p < 0.05 descriptively**
— including both `qwen` cells, which are the only ones in this document with enough n to get
there. Arm B's text moves a model that would otherwise fail these scenarios, and it does so on a
frontier model, a mid-tier model and a 35B abliterated build alike.

**The abliteration matters here and it is worth naming.** `qwen`'s safety tuning has been
stripped, so its compliance with arm B cannot be a general deference to instruction-shaped text
riding on safety conditioning. Armor moved it anyway. That is a stronger result for the armor
than the same number on a safety-tuned model would have been.

### H3 was contradicted, and the counter-example is the most interesting cell in the grid

**Predicted:** `A′ ≤ A` on every model, because fix 1 un-scopes a `description:` line and this
harness pastes the skill in unconditionally, so there is no retrieval for it to fix.

| skill | model | A′ vs A | difference | Fisher p |
|---|---|---|---|---|
| tdd | sonnet | 2/4 vs 4/4 | −50 pp | 0.429 |
| systematic-debugging | sonnet | 2/4 vs 2/4 | 0 pp | 1.000 |
| systematic-debugging | qwen | 5/8 vs 7/8 | −25 pp | 0.569 |
| systematic-debugging | opus | 3/4 vs 2/4 | **+25 pp** | 1.000 |
| **tdd** | **qwen** | **4/8 vs 1/8** | **+38 pp** | 0.282 |

**`qwen` on `tdd` is the cell to look at.** Arm A scored **1 of 8 (12%)** — statistically
indistinguishable from its own unaided control at **0 of 7** (+12 pp, p=1.000). **On this model
and this skill, the pre-fix skill text did essentially nothing that no text at all did not do.**
Un-scoping the description (A′) took it to 50%, and the full armor (B) to 75%.

That is precisely the shape `spec.md` §3's fix-1 defect predicts: a `description:` scoped to
*"in a drovr phase"* tells an agent working inline that the skill does not apply, and a model
less able to override a literal scoping cue follows it literally. **This run has never before
had a cell where arm A failed to beat unaided.**

**It is suggestive and it is not a finding.** +38 pp is a three-run difference at n=8, **below
the ≥4-of-8 margin pre-registered as reportable**, with p=0.282. It is recorded because a
pre-registered prediction was contradicted in a specific, mechanistically interpretable
direction — which is the most a phase like this can honestly produce.

### The other asymmetry, recorded without an explanation

Arm A scored **1 of 8 on `qwen`/`tdd`** and **7 of 8 on `qwen`/`systematic-debugging`** — the
lowest and the highest cells in the entire grid, same arm, same model, same harness. Whatever
drives it is a property of the two skills' pre-fix text or of the two scenario pairs, and
**nothing measured here identifies which.** Recorded as an open question rather than
back-explained.

### H4: nothing to report, as pre-registered

No behaviour in the `qwen` cells was attributable to abliteration rather than to procedure. The
hypothesis was recorded with no directional prediction precisely so that *"it is abliterated"*
would not become available afterwards as an explanation that was never a prediction. It is not
used as one.

## What this does NOT do

**It does not reopen a single ship/revert decision, and one cell makes that worth stating
plainly.**

`tdd` reverted to A′ under branch (a), because arm A scored **4 of 4 on `sonnet`** — *"if arm A
already passes for a skill, that skill's rewrite is not justified."* On `qwen`, that same arm A
scored **1 of 8**, no better than no skill at all. Had probe model been a factor in the
pre-registered design, `tdd`'s verdict might well have gone differently.

**It was not, and `tdd` stays reverted.** The bars were pre-registered for `sonnet`, this factor
was added after those results were seen, and a post-hoc factor cannot rescue an arm the
confirmatory bars rejected — nor condemn one they accepted. `systematic-debugging` stays
shipped. The per-skill evidence docs' verdict sections are untouched by this phase.

**What this cell justifies is a question for a future confirmatory run, pre-registered before it
is measured:** *is arm A's sufficiency on `tdd` a property of the text, or of `sonnet`?* That is
a design, not a verdict, and this document does not pretend otherwise.

## Limitations, as they turned out

The five pre-registered limitations all stand. Three need updating with what actually happened,
and one is new.

1. **The `qwen` column confounds model with harness — unchanged and still the biggest one.**
   `sonnet` and `opus` probes are Claude Code subagents; `qwen` probes are `opencode run`
   processes with their own system prompt. Every `qwen` result above is a claim about *model or
   driver*, and nothing here separates them. **The `opus`–`sonnet` contrast is the clean one**,
   and it is also the smallest.
2. **One scorer model — held, and now with evidence it was stable.** 16 transcripts scored twice
   by two dispatch mechanisms agreed 16 of 16. That tests dispatch, not model: a `sonnet` scorer
   may still read `qwen`'s prose differently than a `sonnet` probe's, and this phase cannot
   detect that.
3. **`n=8` is still small, and two cells are n=7.** Powered for large effects only, exactly as
   pre-registered — which is why the interaction result is *"not detected"* and not *"absent"*.
4. **NEW: the arm-attention control came back weak.** 2 of 59 armed responses quote their own
   arm. The prompt-side verification is what carries arm assignment, and for `qwen` that chain
   has one fewer link than for `opus` (no id token in a redirected stdout). Stated as the gap it
   is.
5. **Two skills, one of them for `opus` only.** Nothing here transfers to `code-review`
   (saturated at 3 of 4 unaided, excluded for that reason), `verification-before-completion`, or
   `using-drovr`.
6. **Post-hoc factor.** Restated last because it governs everything above it.

## Protocol events, honestly

1. **`plan.md` C5's FOREGROUND rule was again not honoured** — the harness dispatched every
   subagent asynchronously without being asked to, for the **seventh** stage running
   (`plan-HANDOFF.md` dead-end 4). The measurement is unaffected: the cells are mutually
   independent, each probe wrote only its own file, and every one was confirmed complete before
   any transcript was assembled or scored. The single-writer property C5 protects was again held
   by the sandbox rather than by the scheduling.
2. **The scoring mechanism changed mid-stage, deliberately, and the 16 already scored were
   re-scored rather than mixed.** The first 16 scorers were Claude Code subagents; the run then
   switched to headless `claude -p` so that no scorer's output could reach the orchestrator's
   context at all — the same C5a property the `qwen` probes have. **All 78 were then scored by
   the new mechanism**, and the 16 subagent verdicts were kept as the independent second pass
   reported above rather than pooled with the primary set. One verdict written by the mechanism
   test itself (`fe6512`) was deleted from the second-pass directory before any comparison,
   because comparing the primary pass against itself would have been worthless; the deletion is
   recorded in that directory's `PROVENANCE.md`.
3. **The transcript assembler accepted a 2-byte response, and the guard was too narrow.** A
   failed `qwen` run left a 2-byte file; the assembler refused only *empty* responses, so it
   assembled a 2312-byte transcript — the scenario block dominates — that would have gone to a
   scorer as a measurement. The transcripts were deleted and rebuilt once, with the assembler
   now refusing anything under the runner's own 200-byte floor. **The smallest accepted answer
   is 505 bytes**, so the floor is not close to any real response. Recorded because
   "empty" and "too short to be an answer" are different predicates and only the first was
   checked.
4. **Three verdicts were rejected and re-scored, and that is not a charged run.** `1b8567`,
   `323b08` and `9b8371` each recorded a `new_rationalizations` quote that was **not verbatim**
   in its own `## Response` block — the rule `scoring-rubric.md` states and
   `scores_json_verdicts_obey_the_rubric` enforces. Per the rubric, the collecting agent
   **rejects and re-runs the scorer; it does not repair the verdict**, so all three were
   re-scored from their sealed directories with the verbatim requirement restated. **All three
   returned the same `compliant` value** (`false` in each case): the defect was in the quoted
   evidence, not in the judgement. The rejected objects are kept outside the evidence tree.

   **The collection step now checks the rubric's recording rules itself**, rather than leaving
   them to be discovered by `cargo test` after the verdicts are already written into
   `docs/skill-evidence/`. That is where the rubric puts the job — *"if you are that phase
   agent, this sentence is your job, and the test will not remind you"* — and this stage needed
   reminding.
5. **Two runs were never made and are recorded as nulls**, above. The attempt cap fired as
   designed and was not raised.
