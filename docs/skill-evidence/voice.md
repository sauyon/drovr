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

### Task 22 looked here and found nothing — 2026-08-07

**This note is Task 22's, not Task 21's, and it is not data.** It is here so a reader who lands on
this file alone does not have to work out whether the placeholder above is stale.

Task 22 is the task that applies the outcome. It ran, read this section, and **applied no register
change to any of the five documents**, because there is no outcome to apply. Corroborated three
ways beyond the placeholder itself: `docs/skill-evidence/transcripts/voice/` does not exist,
`run-ledger.md` carries no `ab-voice` row, and `git log --follow` on this file shows only the two
pre-registration commits with no results commit after them.

The instrument is built and unused: `V0`–`V3` exist, are hash-pinned in `arms/MANIFEST.md`, and
pass all five separability checks. Running the probe now costs 24 runs against a ledger whose
closing note reads *"Nothing further is authorised"*, so it needs a human's ceiling raise. Nothing
above this heading has been edited. See `task22-report.md`.
