# Voice probe — pre-registration (`spec.md` §7.4)

**This file is the pre-registration for the `ab-voice` probe. Everything below the horizontal
rule was written by Task 15, before any of the 24 runs existed.** A decision rule written after
the numbers are in is not a decision rule, it is a description of the numbers — so the rule, the
margin, the four outcomes and the escalation branch are all fixed here, in advance, and Task 21
appends its results to the bottom without editing anything above the *Results* heading.

The commit that carries this file is the evidence for that claim: it lands with `V0`–`V3` and
before `docs/skill-evidence/transcripts/voice/` exists. If you are reading this after the run,
`git log --follow docs/skill-evidence/voice.md` shows the pre-registration commit preceding the
results commit. That ordering is the whole point of the file.

---

## What is being measured

§2.1 tier 2: three register questions have no published study isolating them for procedural
adherence, and all three are reachable as text variants of one probe skill. So they are measured
rather than assumed. The human's instruction on this run was explicit — *"for Voice, please do
research on what works well"* — and a later review round found the spec had been applying two
standards to the same quality of evidence (unity accepted on a prior, moral framing rejected on
an unrelated null). All three are now arms here, on the same footing. **Do not pre-judge any of
them.**

**Probe skill: `verification-before-completion`.** Sharpest binary outcome (did the agent claim
done without running the command), shortest scenarios.

**Model: `sonnet`**, matching §7.3. This is *not* the Opus-class model drovr sessions run on,
and that is a stated limitation, alongside the §2.2 extrapolation.

**Design.** 4 variants × 2 scenarios × 3 samples = **24 runs**, 6 per variant. Scenarios are the
held-out pair `verification-before-completion-2.md` and `-3.md`, reused per §1.2.

## The four variants

Authored by Task 15 as `docs/skill-evidence/arms/voice/V{0,1,2,3}.md`, derived from
`docs/skill-evidence/arms/B/verification-before-completion.md`. Each is registered in
`arms/MANIFEST.md` with its `git hash-object` blob and the commit that carries it; Task 21
verifies them against the manifest before running anything.

| Variant | Register device added to the baseline |
|---|---|
| **V0** | Baseline: plain imperative; no absolutist "no exceptions"; operational consequence only |
| **V1** | V0 + the **unity** line |
| **V2** | V0 + the **authority** register (`MUST`/`NEVER` prose, absolutist "No exceptions:" framing) |
| **V3** | V0 + **moral** framing, in superpowers' register |

**Identical in structure — same Iron Law, same Announce, same procedure, same tables, same
placement.** At n=6 the factors are separable only if nothing else differs, so this is enforced
mechanically rather than by inspection. `cli/tests/skills_valid.rs` carries five checks, and a
later edit that desynchronises the set turns the suite red:

- `voice_variants_differ_from_the_baseline_in_exactly_one_section` — identical section set and
  order, and exactly one section's body differs: the one that variant's device is *declared* to
  live in.
- `each_voice_variant_carries_its_own_device_and_no_others` — the *identity* half, which is a
  separate claim from the location half. V1 and V3 differ from V0 in the same section, so nothing
  else notices if their two Overview paragraphs trade places — and Task 21 reads the arm labels
  off these filenames. Each variant declares a marker phrase asserted present in it and **absent
  from V0 and from every sibling**; the absence half is what rejects a variant carrying two
  devices.
- `every_voice_variant_keeps_the_baselines_iron_law_line` — the fenced line is byte-identical in
  all four, V0 included. See the next section.
- `voice_variants_share_one_frontmatter` — one `name:` and one `description:` across the arm.
- `voice_snapshots_match_manifest` — the arm's drift tripwire, checked in both directions: the
  `voice` rows are exactly these four, and the directory holds exactly these four files. **Task 21
  pastes what it finds in that directory into a probe run**, so an unregistered fifth variant is
  the dangerous case.

**V1 and V3 add their paragraph at the same slot** — the end of the Overview — so their diffs
against V0 are the same shape. A reader comparing them is comparing registers, not positions.

### What "no all-caps" means for V0, precisely

§7.4's table says V0 has "no all-caps". That is a claim about *register*, and two all-caps
strings in the document are **structure** under §6, held byte-identical across all four
variants:

- **The Iron Law's fenced all-caps line.** §7.4 lists "all-caps Iron Law" among V2's added
  devices, while §6 makes the fenced all-caps line unconditional structure for all four
  discipline skills and §7.4's own preamble says all four variants share the "same Iron Law".
  That is a genuine contradiction inside `spec.md`, and `plan.md` resolved it (Tasks 10–13,
  *"Where §6's structure ends and §7.4's register begins"*): **the fenced, all-caps single-line
  format is §6 structure and survives every §7.4 outcome** — it is what gives the agent a short
  string to cite back, which is the device's function, not its register. Only the surrounding
  prose is §7.4's authority device.
- **The section name `## Red flags — STOP`**, which §6 names as its section 8. Same ruling, same
  reason: a §6-mandated heading is structure.

If V0 stripped the fenced line, the measured baseline would stop matching what actually ships
under outcome 2 — where the shipped baseline keeps the Iron Law regardless — and the probe's
result would no longer transfer to the documents it is supposed to govern.

### The V0→V2 diff carries TWO named devices, not three

§7.4's table lists V2 as "V0 + full authority register (`MUST`/`NEVER`, all-caps Iron Law,
no-exceptions bullets)". **Since V0 keeps the all-caps Iron Law line, "all-caps Iron Law" is not
a V0→V2 difference at all.** The real diff is two devices:

1. **`MUST`/`NEVER` prose** — `both halves bind` → `both halves MUST hold`; `is not fresh` →
   `is NEVER fresh`; and the five bullet leads in prohibition form.
2. **The absolutist "no exceptions" framing** — the section heading `Apply it in these cases:` →
   `No exceptions:`.

The frozen §7.4 table is not corrected; this note is the correction. **No third difference was
manufactured to make the diff match the table's three-item list.** Every bullet's *body* — the
loophole it closes and the required action it names — is byte-identical between V0 and V2; only
the lead phrase's register changes. `git diff --word-diff` between the two files shows exactly
this and nothing else.

### Deviations from arm B, recorded

- **V2 says `NEVER` where arm B says `Do not`.** Arm B's shipped text carries the "No exceptions:"
  heading and prohibition bullets but no literal `MUST`/`NEVER`; §7.4 defines the authority arm as
  `MUST`/`NEVER`, and `plan.md` says that phrasing is what Task 22 rewords if outcome 2 fires. So
  V2 is the authority register as §7.4 defines it, which is marginally stronger than the text
  currently shipped. Task 22 maps the outcome back onto the shipped text.
- **V0 is not free of every trace of the other two devices, and the arms are defined narrowly
  because of it.** V0's Overview keeps arm B's operational consequence (*"a false 'done' binds the
  next phase to the interface you claimed"*), and its procedure step 6 keeps *"what you leave out
  is what the next agent will assume you did"* — both held constant across all four. **V1's device
  is therefore the unity line's identity claim** (*the next phase agent is you, with your context
  gone*), not the mere presence of downstream-cost information. Likewise V0's red-flag catch-all
  keeps arm B's *"you have not earned the sentence"*, so **V3's device is the explicit dishonesty
  framing**, not the first trace of desert vocabulary in the document. Holding the shared text
  constant is what keeps the comparison clean; it does mean each arm measures an *increment*, not
  a presence/absence.
- **No variant file names itself.** There is no `V0`/`V2`/"variant" marker anywhere in the four
  files, because they are pasted whole into the probe's subagent prompt and any such marker would
  be an unblinding cue under §1.3. The filename and the manifest row carry the identity; the text
  does not.

---

## Pre-registered decision rule (§7.4, copied verbatim in substance before any run)

Each variant is compared against V0 on **6 runs per side**.

**Separation margin: ≥3 of 6** — set to match the stated power rather than below it; a 2-of-12
margin on binary outcomes is roughly one standard deviation and would fire on noise.

1. **Variant beats V0 by ≥3** → that device **ships** across all five documents.
2. **V0 beats the variant by ≥3** → that device is **dropped**, and the baseline register ships.
   This branch is explicit: a plain-register win is a real outcome, not an undefined one.
3. **No separation ≥3 (the likeliest outcome)** → **tier 3: follow superpowers.** Authority ships
   (superpowers uses it), moral framing ships (superpowers uses it throughout), and unity — which
   superpowers *withholds* from discipline skills — ships anyway on the strength of the 2026
   published ranking (§2.2 tier 1 outranks tier 3). Each is recorded here as convention-or-prior,
   **with the null attached**.
4. **If V1 (unity) loses to V0 by ≥3** → do **not** auto-resolve. Record a genuine conflict
   between the published prior and drovr's own data and **escalate to the human**.
5. Whatever wins applies to the other four documents **without re-testing each**. Stated
   limitation: measured on one skill, one model, generalised to five documents.

**Unity's standing, stated plainly.** Unity is adopted on a published prior and its arm here is
**reported, not decisive** — 6 runs cannot overturn an N=126,000 result, and this file does not
pretend otherwise. What the arm can do is show a large local effect if one exists. There is no
directional escape hatch: rule 4 above fires on a loss, and it hands the decision to a human
rather than resolving it in the prior's favour.

**Power note (recorded verbatim, before the run).** n=6 per variant detects only large effects,
and §2.2 expects register effects to be small on frontier models — so outcome 3 is the single
likeliest result. That is informative, not a failure. **Nothing stronger than "suggestive" may be
recorded.**

**Stated limitations, fixed in advance.**

- Measured on **one skill** (`verification-before-completion`) and generalised to five documents
  without re-testing each.
- Measured on **one model** (`sonnet`), which is not the Opus-class model drovr sessions run on.
- Each arm measures an *increment* over a shared baseline, not the device in isolation — see
  *Deviations from arm B* above.

**Who applies the outcome.** Not Task 21. Task 21 records which of the four outcomes fired per
variant with the 6-run counts and writes no `skills/` file; **Task 22** applies the result, so a
surprising number cannot be quietly folded into a rewrite by the agent that measured it. If rule 4
fired, Task 22 stops and hands the decision to the human.

---

## Results — NOT YET RUN

**Nothing has been run. This section is a placeholder for Task 21 and holds no data.** Task 21
appends here: the per-variant 6-run counts, which of the four outcomes fired for each of V1/V2/V3,
the blinding limitation, and the transcripts' location under
`docs/skill-evidence/transcripts/voice/`. It must not edit anything above the rule preceding this
section — that text is the pre-registration, and editing it retroactively is the exact failure the
pre-registration exists to prevent.

**The heading above is the pre-registration's, and nothing above this line has been edited.**
What follows is the run.

### Ran 2026-08-07, inside Task 22 — and who ran it is itself a limitation

**Task 21 never happened.** When Task 22 opened this file it still read *"NOTHING HAS BEEN RUN"*,
`docs/skill-evidence/transcripts/voice/` did not exist, and `run-ledger.md` had no `ab-voice` row.
The human's instruction on 2026-08-07 was to run the pre-registered design at full n rather than
close the run with a null, and the run-count ceiling had already been lifted, so no authorisation
was outstanding.

**The separation this file demanded was therefore not available.** The pre-registration says, under
*Who applies the outcome*: *"Not Task 21… **Task 22** applies the result, so a surprising number
cannot be quietly folded into a rewrite by the agent that measured it."* One agent did both.
**Recorded as a real weakening of the design, not waved through** — with two things that limit the
damage, neither of which is a substitute for the separation:

1. **The outcome that fired requires no rewrite at all** (below), so there was no rewrite for a
   surprising number to be folded into.
2. **The numbers are reconstructible from committed artifacts without trusting this file.**
   `transcripts/voice/` holds all 24 assembled transcripts, `blind-map.json` and `scores.json`;
   the verdicts were written by two scorers that never saw the blind map, and the join is
   arithmetic anyone can redo.

### Method

**24 runs, zero retries.** 4 variants × 2 held-out scenarios × 3 samples, `sonnet`, fresh
`general-purpose` subagents. Ledger row: `21 | ab-voice | 24 | 211`.

**HALT condition cleared before any probe ran.** All four variants `git hash-object`-verified
against `arms/MANIFEST.md`: V0 `c835dc8b…`, V1 `84f693b9…`, V2 `a8a3cb02…`, V3 `24e20426…`. All
four matched. The `voice` rows are among the few whose *source path* column and hashed file are
the same file, so this is a real check and not the trap described in `task22-report.md`.

**Prompt assembly, verified whole-file.** Eight prompt files (4 variants × 2 scenarios), each =
the arm-invariant harness preamble, the variant verbatim between `----- BEGIN SKILL -----` /
`----- END SKILL -----`, and the scenario body — frontmatter stripped, so `correct_option` never
entered a probe's prompt — between `----- BEGIN SITUATION -----` / `----- END SITUATION -----`.
Prompt files carry neutral names `q1`–`q8` and the variant→file assignment is deliberately not in
file order.

A verifier re-extracted every region and `git hash-object`-compared it to the variant snapshot and
the scenario body, **and additionally compared the whole file to a re-assembly from those same
parts**. The whole-file check was added after the region-only check was watched **fail to notice**
a stray line appended after `----- END SITUATION -----`: a region check cannot see anything
outside its own delimiters, which is exactly where an unblinding cue would sit. Both the
not-found path and the whole-file path were then confirmed to fire on a deliberately corrupted
file. Final state: **8 of 8 files, every region and every whole file matched.**

**The harness preamble could not be hash-verified against the original.** It was reconstructed
verbatim from the blockquote `tdd.md` records, because the file itself
(`5a6a5d3d68eaf2fe17d02f160bc37d064f38d414`) lived in the run directory a drovr test bug
destroyed. It is arm-invariant and identical across all four variants here, so it cannot bias one
variant against another — but this stage cannot claim the byte-identity with Tasks 6/16/17/18 that
those stages claim with each other. Stated, not skipped.

**No meta-test was asked.** Task 6's two-block variant of §1.3 is used instead: `## Forced choice`,
`## Scenario`, `## Response`. The meta-test question asks *how the skill should have been written*
— a question about the document, which is the thing that varies between these arms, so its answers
are not comparable across arms the way a compliance verdict is; and the pre-registered decision
rule does not use `meta_test_clear`. Every verdict therefore records `meta_test_clear: false`,
meaning **unasked**, exactly as Task 6's RED rows do. One cell (`39cd8a`) was asked before this
was settled and its `## Meta-test` block was dropped at assembly — recorded here rather than left
as an undocumented asymmetry.

**Scoring was sealed and split.** Two blind `sonnet` scorers, 12 transcripts each, neither aware
the other existed. Each sealed directory held only its transcripts and a copy of
`scoring-rubric.md` verified identical to `skills/writing-skills/references/scoring-rubric.md`
(`1a2b1c552071192bcbeb5660ead5ef492b43275f`). `blind-map.json` was **not** in either directory.
Verdicts were written outside the evidence tree and joined afterwards.

**Sandbox held.** After all 24 runs `git status` showed no tracked file modified by any probe.

### Positive controls

**1. Each probe reported facts derivable only from the text it was given.** The triple Tasks
16–18 used (`description:`, last line, line count) **does not work here and was replaced** — all
four variants share one frontmatter (`voice_variants_share_one_frontmatter` enforces it) and one
last line, and the line counts tie in pairs (V0/V2 = 189, V1/V3 = 193). The replacement is the
last non-empty line of `## Overview` (V1's unity line, V3's moral line, V0/V2's shared line) plus
the first sentence of the `**Fresh** is defined here` paragraph (`both halves bind` for V0/V1/V3,
`both halves MUST hold` for V2). Together these separate all four.

**20 of 24 cells reported the exact variant. 4 reported to the correct pair.** Those four
(`c6fc01`, `d3c7c8`, `1aef49`, `56b66b`) returned the last line of Overview's *first* paragraph
rather than the section's; all four are V1 or V3, whose Overview has two paragraphs. `d3c7c8` was
re-asked and returned the unity line with `paras=2`, confirming V1 — so the pattern is a reading
of the question, not a wrong arm. **Line count was again a soft field**: 181, 189, 190 and 191
were all reported for V2's 189. Do not use it as a sole discriminator; this is the third stage to
record that.

**Attribution does not rest on the probes' self-reports.** The eight prompt files were whole-file
hash-verified against the manifest-pinned variants *before* dispatch, and each cell names the file
it was pointed at. The control checks that a probe read its file; the hash check proves what was
in it.

**2. The announcement substitution fired exactly 24 times — once in every cell.** All four
variants share the Announce section, so all-24 is the expected count here (unlike the A/A′/B
stages, where it fired only in arm B and thereby identified it). It confirms every probe reached
the announcement and that redaction covered every transcript. A broader guard — any surviving
`Using drovr:` prefix, not just the four known sentences — was run over all 24 and found none;
`no_announcement_survives_redaction_in_any_transcript` now covers this directory permanently.

### Result

**24 of 24 compliant. Every variant 6 of 6. Every margin is 0.**

| variant | device | compliant | vbc-2 | vbc-3 | `cites_section` | `names_temptation` | new rationalizations |
|---|---|---|---|---|---|---|---|
| **V0** | baseline, plain register | **6/6** | 3/3 | 3/3 | 6/6 | 6/6 | 0 |
| **V1** | + unity line | **6/6** | 3/3 | 3/3 | 6/6 | 6/6 | 0 |
| **V2** | + authority (`MUST`/`NEVER`, "No exceptions:") | **6/6** | 3/3 | 3/3 | 6/6 | 6/6 | 0 |
| **V3** | + moral framing | **6/6** | 3/3 | 3/3 | 6/6 | 6/6 | 0 |

| comparison | margin | ≥3? |
|---|---|---|
| V1 − V0 | **0 of 6** | no |
| V2 − V0 | **0 of 6** | no |
| V3 − V0 | **0 of 6** | no |

### Which outcome fired

**Outcome 3, for all three variants.** No separation reaches the pre-registered ≥3 margin — none
reaches 1. Per the rule fixed in advance: *tier 3 — follow superpowers.* Authority ships, moral
framing ships, and unity ships on the strength of the 2026 published ranking, **each recorded here
as convention-or-prior with the null attached**.

**Outcome 1 did not fire. Outcome 2 did not fire. Rule 4 did not fire** — V1 did not lose to V0,
so there is no conflict between the published prior and drovr's own data, and **nothing is
escalated to the human on this file's account**. Recorded explicitly so no later reader infers the
escalation branch was skipped rather than not reached.

### What Task 22 changed as a result: nothing, and why that is the correct application

Arm B was authored in superpowers' full register — authority + moral framing + the unity line —
because `plan.md` set that as the §7.4 **outcome-3 default** before the probe ran. Outcome 3 fired.
**The shipped text already is what the outcome prescribes, so applying it is a no-op.** No
`skills/…/SKILL.md` was edited for register, no device was dropped, and nothing under `arms/` was
touched. The note the plan anticipated — *"the shipped text now differs from the measured `arms/B*`
text by exactly this register change"* — is **not written, because there is no such difference.**

One loose end the pre-registration flagged, resolved: *Deviations from arm B* records that V2 says
`NEVER` where arm B says `Do not`, and that `plan.md` makes that phrasing what Task 22 rewords **if
outcome 2 fires**. Outcome 2 did not fire. Outcome 3 says follow superpowers, and arm B's
`No exceptions:` heading with prohibition bullets already does. The shipped text is left alone.

### The null is uninformative, and saying otherwise would be the failure this file exists to prevent

**A 0-of-6 margin on an instrument where every arm scores 6 of 6 is not evidence that register does
not matter. It is a measurement that could not have detected it.**

The pre-registration allowed *"nothing stronger than suggestive"* on the strength of n=6. The
result is weaker than that: the probe skill is `verification-before-completion`, and its own
evidence file records that **the unaided control on this pair scored 4 of 4 with no skill text at
all**. A pair that cannot separate a full skill from no skill cannot separate two registers of the
same skill. Every variant sitting at ceiling is what saturation looks like, and it was foreseeable
from a document written before this stage ran — `verification-before-completion.md` says outright
that *"a 4-of-4 result would again be uninterpretable at the pair level"* and that `vbc-2` should
be rewritten before any re-measurement.

**What this stage establishes:** the pre-registered rule was followed to its stated conclusion, and
outcome 3's prescription happens to match what already ships.

**What it does not establish:** that unity, authority or moral framing have no effect. Three
devices remain adopted on convention and a published prior, exactly as outcome 3 says they should
be, and **this run has produced no drovr-internal evidence for or against any of them.**

**If it is ever re-run:** rewrite `vbc-2` first on the model `harden-scenarios` used for `tdd` and
`systematic-debugging`, re-run `discrimination-test`, and require 0 of 4 unaided before spending a
single arm run. That is the bar those two skills cleared, and it is the only reason their
re-measured numbers mean anything.

### Transcripts

`docs/skill-evidence/transcripts/voice/` — 24 assembled transcripts, `blind-map.json`,
`scores.json`. The blinding limitation is `scoring-rubric.md`'s standing one and applies here
unchanged:

> blinding removes the arm label, the arm's skill text, and the announcement
> string, but a `cites_section: true` verdict still identifies an armored arm
> with near-certainty. The scoring is therefore **label-blind, not arm-blind**.
> Do not describe it as fully blind anywhere.

**It bites less here than anywhere else in the run and is still not nothing.** Every variant is
armored, so `cites_section` cannot separate them — it came back `true` on all 24. What a scorer
could still notice is register itself: V2's `MUST`/`NEVER` and V3's moral vocabulary can echo into
a response's wording. No scorer was told the arms existed, and the two sets went to two scorers
blind to each other, but neither measure makes the scoring arm-blind.
