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
