# PROTOCOL-2 — pre-registration for the spec-length A/B, second attempt

**Run:** `spec-length-ab2` · **Branch:** `drovr/spec-length-ab2` · **Written by T1, 2026-08-10.**

This file is the **pre-registration for the second attempt**. It writes down every rule that decides
the outcome, and it is committed **before any new generation, verdict or measurement exists** — that
ordering is the whole claim, and it is checkable:
`cli/tests/skills_valid.rs::spec_length_2_protocol_precedes_every_generation` requires this file's
introducing commit to precede every commit that introduces `calibration-2.json`, `generated-2/`,
`retention-2/`, `adjudication-2/`, `escalation-2/`, `transmission-2/`, `gaps-2/` and `RESULTS-2.md`.
Unlike `PROTOCOL.md`, whose own preamble says *"No test parses this file"*, this one **is** parsed:
`spec_length_2_protocol_inherits_its_verbatim_blocks` checks the blocks below that claim to be
inherited verbatim against `PROTOCOL.md` itself, and
`spec_length_2_protocol_stops_moving_before_the_first_probe` checks that this file stopped moving
before the first probe. Every ordering claim this run makes is executed, never left to a human
reading `git log`.

**Why there is a second attempt.** The first attempt returned a null **about its instrument, not
about spec length**. `PROTOCOL.md` item 8a made any single `establishes: false` invalidate the entire
verdict file, and `RESULTS.md` §7.3 records that items 8a and 9 apply different standards — so
verdict files were invalidated at a rate that left retention **undefined** for every generation, the
control included. No arm could pass and no arm could fail; there was nothing to gate on.
`docs/skill-evidence/spec-length/RESULTS.md` §7 is the record of that, and it stands unedited. **This
file redesigns item 8a and nothing about the question being asked.**

**This file is authoritative over `spec.md` and over the plan on every rule it states**, exactly as
`PROTOCOL.md:12-20` is for the first attempt. The approved spec
(`~/.local/share/drovr/runs/spec-length-ab2/spec.md`) and the plan
(`~/.local/share/drovr/runs/spec-length-ab2/plan.md`) named most of these rules first, but neither is
itself pre-registered. Where they and this file differ on a rule, this file governs and the
difference is recorded in `RESULTS-2.md`. Two such differences already exist and are recorded in item
15: `R8`'s denominator, and `R2`'s.

**This file supersedes `PROTOCOL.md` for this run only. `PROTOCOL.md` is not edited**, and neither is
any other first-attempt artifact. See item 15 for the full statement of that relationship.

**What is protected: every item in this file.** The **governed items** are **every numbered item
below, without exception** — 1, 2, 3, 4, 5, 6, 7, 7a, 8, 8a, 8b, 9, 10, 11, 12, 12a, 13, 14, 14a and
15, all twenty. Item 12 is not special and is not the boundary: `R1` and `R2` are *defined in terms
of* item 9's `present` test, item 8's schema, item 8a's sample, item 8b's escalation and item 10's
instrument; the id pool (item 6) and the probe template (item 5) determine what is measured at all;
and item 11 governs what may be claimed about the result. Loosening any of them moves the outcome as
surely as rewriting `R2` would. This is stated as "everything" rather than as a list because
`PROTOCOL.md` records that an earlier draft of its own clause protected item 12 alone, and the next
draft protected a nine-item subset that still left the probe template and the id pool outside it —
each time, the carve-out was the bug.

**What may change, and when. Three windows, and there is no fourth.** `PROTOCOL.md:32-50`'s rule is
inherited unchanged in substance; its **boundaries are re-pinned**, because they are named after
tasks of the first attempt's decomposition that this run does not have. The substitution is recorded
in item 15.

1. **Window 1 — until the first calibration probe of item 12a is dispatched.** Nothing has been
   measured under the new instrument. Corrections are legitimate and need no log beyond the revision
   table below. **This is the only window in which a rule may be added and the only one in which a
   governed item may be weakened.** The test-writing that lands
   `cli/tests/skills_valid.rs::spec_length_2_*` sits here deliberately: writing the checker is how an
   ambiguity in item 8's boundary rule gets found, and finding it here is free.
2. **Window 2 — from that first calibration dispatch until the first probe of item 5 is dispatched.**
   Calibration output exists but no new arm has been measured. Corrections are still legitimate,
   because nothing that decides the outcome has been measured, but **no governed item may be weakened
   — only clarified**, and every edit is logged in
   `docs/skill-evidence/spec-length/RESULTS-2.md` with its reason and its commit.
3. **Window 3 — after the first probe of item 5 is dispatched.** **No governed item may be revised at
   all**, and any edit to this file whatsoever is a protocol deviation, logged in `RESULTS-2.md` with
   its reason and its commit.

A rule chosen once a result is visible is not a rule; it is the result wearing a rule's clothes.

**Every edit in every window appends a row to the revision table below**, in the same commit that
makes the edit. Windows 2 and 3 log to `RESULTS-2.md` **as well**, not instead.

**Two of those obligations are mechanical rather than trusted, which is the one place this file is
stricter than `PROTOCOL.md`.** `spec_length_2_protocol_stops_moving_before_the_first_probe` asserts
that **every** commit touching this file is an ancestor of `generated-2/`'s introducing commit — that
is window 3's rule, executed — **and** that every such commit beyond the first is named by SHA in
`RESULTS-2.md` — that is window 2's logging obligation, executed. `PROTOCOL.md`'s own revision count
was wrong twice, both times by one, with `git log -p` the only witness; this file does not rely on a
human counting rows.

| commit | what changed |
|---|---|
| the commit that added this file (`git log --oneline -- docs/skill-evidence/spec-length/PROTOCOL-2.md` resolves it; it is the **last** line, and `FREEZE-2.md`'s one row names the same commit by SHA) | first commit — items 1–15, including 7a, 8a, 8b, 12a and 14a; rules `R1`, `R1a`, `R2`, `R3`, `R3a`, `R4`, `R4a`, `R5`, `R5a`, `R6`, `R6a`, `R7`, `R8`. |

**A row cannot name its own commit, and this file does not pretend otherwise.** `PROTOCOL.md` wrote
`the commit that added this row` for the same reason and later had to relabel it, which produced an
ambiguity when a second row wanted the same words. Here the first row is the **only** row that may
use that phrasing, and it is disambiguated by `FREEZE-2.md`, which records the SHA in a separate
commit. **Every later row names a SHA.**

**This file does not restate the freeze.** `docs/skill-evidence/spec-length/FREEZE-2.md` is this
attempt's hash record, `docs/skill-evidence/spec-length/FREEZE.md` is the first attempt's and is
**not edited**, and `docs/skill-evidence/arms/MANIFEST.md` is the provenance record. All three are
authoritative and none is copied here.

---

## 1. What is being measured, and the D2 probe protocol

`skills/pipeline/phase-prompts/brainstorm.md` step 4 is drovr's spec-authoring instruction. It grew
from 2 lines (`S0`, the historical baseline) to **15 lines / 1151 bytes** in the shipped rewrite
(`S1`). The question this experiment answers is:

> **Does a shorter spec-authoring instruction produce shorter generated specs without losing key
> points?**

Retention is a **gate at 100%**, not a score (`docs/interactive-brainstorm.md`, locked decision 8).
Length is compared **only among arms that clear the gate**. An arm that drops any ledger row is
eliminated, whatever its length.

### D2 — the probe is a compression-under-instruction task, not a re-run of brainstorm

A fixture is a *control spec*, not a task brief, and the ledger rows are derived from fixture spec
content. A probe cannot re-run an investigation and a human interview. So:

> The probe is handed **the fixture spec as its decision material** ("your investigation and
> interview are complete; these are your notes"), plus **the arm's instruction verbatim**, and asked
> to write `spec.md`. Input is held constant across arms; the instruction is the only variable.

This is the only protocol under which the ledger is a fair gate — every row is reachable from the
material the probe holds, so a dropped row is attributable to the instruction rather than to the
probe not knowing the answer. The costs of that choice are real, and they are stated in full
immediately below rather than in a one-line disclaimer.

### What this measurement cannot do

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
7. **Only ~20% of `present: true` rows are relevance-adjudicated, and the redesign makes an
   adjudication failure *less* consequential than it was.** Item 8a samples every 5th such row;
   the other ~80% are held to the mechanical check alone, which proves a cited span is really *in*
   the spec but not that it is *about* the row. Under the first attempt a single caught row
   invalidated all 91, so the expected cost of a lenient scorer was high. **Under this protocol a
   caught row is escalated to tier 3, which sees the spec and may restore it, and nothing is
   invalidated.** That is a deliberate loosening: it is what makes retention *defined*, and it is
   also what makes the residual scoring risk run harder toward a **false pass** than it did in the
   first attempt. Two things bound it, and neither closes it: tier 3 answers item 9's own question
   with more context than a 46-row batch scorer had, and under `R2` a false `present: true` can
   only ever inflate retention, never eliminate an arm. **The write-up states this as a loosening,
   reports the tier-3 overturn rate, and does not describe the adjudication as complete.**
8. **The tier-3 overturn rate is a statistic about the sampled rows only, and reading it as a
   correction factor for the unsampled ~80% is an extrapolation.** It is defined here so it cannot
   be defined once its value is visible: **overturn rate = (rows tier 3 marked `present: false`
   that tier 1 marked `present: true`) / (rows escalated to tier 3)**, reported per arm and
   overall. If it is high, `RESULTS-2.md` states that retention is overstated — and states that the
   ~80% tier 2 never sampled were never given the chance to be overturned at all, so the true
   overstatement is bounded below by what is observed, not estimated by it.
9. **Tier 3 sees the generated spec**, so unlike the first attempt's 8a it can be swayed by how good
   the spec looks. That is accepted deliberately: 8a's spec-blindness is what made it measure the
   wrong thing — a span judged out of context — and item 8b withholds the tier-1 spans instead, so
   what tier 3 cannot be swayed by is the scorer's own evidence.
10. **Label-blind, not arm-blind, and two file-access channels stay open.** Item 11 carries all of
    it, including the channel `RESULTS.md` §7.6 deviation 7 recorded and this run inherits
    unchanged. **This scoring is never described as fully blind anywhere.**
11. **A null remains a valid, publishable result, and this redesign changes *which* null is
    reachable.** The first attempt's was *"nothing could be judged"*; this one's, if it comes, is
    *"every arm has a number, and here are the rows each dropped"*. That is the deliverable, not a
    pass, and item 12's `R4` says the same thing about the outcome the design expects.
12. **The `R8` carve-out is a post-hoc adoption**, decided after the first attempt failed and before
    any new measurement was taken. It is legitimate only because it is **arm-symmetric** — a
    function of the union across all arms, the control included — and that symmetry is executed by
    `spec_length_2_r8_exclusion_is_arm_symmetric` rather than asserted here. The write-up says
    plainly that it was adopted after a failure and why that is sound; it is not buried.
13. **The instrument this file freezes has itself never been run.** Tiers 2 and 3 are new in
    combination, item 12a's calibration pass is the only evidence about them that will exist before
    the first new generation is scored, and it runs on six generations of one fixture. A protocol
    whose instrument is validated by one pass on one fixture is better evidenced than the first
    attempt's, which had none, and that is the whole of the claim.

---

## 2. The arms

**No arm is authored, edited or re-frozen by this run.** `S1`, `S2` and `S3` are **reused exactly as
frozen by the first attempt**, and the hashes below are copied from `FREEZE.md` rather than
recomputed into a second authoritative record. Nothing about the arms failed; the instrument did.

| arm | role | path | `git hash-object --no-filters` | frozen at commit | size |
|---|---|---|---|---|---|
| `S1` | **control**, the shipped text | `docs/skill-evidence/arms/spec-length/S1.md` | `bb0d5cdcf2903e9d47e705820911a2464c73ab22` | `a9eef3a3de9303213cce4a689dee3133f75c2ac8` | 15 lines / 191 words / 1151 bytes |
| `S2` | candidate — a moderate trim of `S1` | `docs/skill-evidence/arms/spec-length/S2.md` | `8126b24fabadf3aff9391afd132f79676a288459` | `6352ea1b65be6fc0c3039577234844b98db575c9` | 11 lines / 114 words / 705 bytes |
| `S3` | candidate — the aggressive minimum | `docs/skill-evidence/arms/spec-length/S3.md` | `978f4c46c545e7d0df09158ad9cbfeca550c3eca` | `6a56a21f676c33f58a3936adaa25ca1d97129fee` | 4 lines / 59 words / 354 bytes |

**The three hashes and three sizes above — and `S0`'s blob hash in the paragraph below — were
re-verified against the files on disk at this file's commit**, with `git hash-object --no-filters`
and `wc`, which is what `FREEZE.md`'s closing section says a task owes at a gate rather than at CI
time. All four matched `FREEZE.md` exactly.

`S1` was frozen at commit `a9eef3a3de9303213cce4a689dee3133f75c2ac8`, and that is the commit both
its `FREEZE.md` and its `MANIFEST.md` rows name.

**`S0` is not an arm of this run.** It is the historical baseline — `brainstorm.md`'s two-line step
3 at `370e211174fcb23cfc48a9732fc528754e9b02c6`, blob
`db89be9ee06913386afcb6f1053597fdb9728a3a`, 2 lines / 177 bytes. It is never generated from, never
scored, and never shipped: it mandates the "open questions" section locked decision 6 forbids. It
appears in this experiment only as the length reference point in item 12's R3. **The control is the
shipped text, `S1`** — measuring against `S0` would compare candidates to text nobody ships.

### D1 — `S1` is defined by an exact recipe, not by sentinels

`FREEZE.md`'s *"The frozen sentinels"* section describes two comment lines placed around the arm in
`brainstorm.md`, with extraction by `awk`. **This run does not place them, and `brainstorm.md` was
not edited by T1.** `S1.md` is instead defined by the exact recipe

```
sed -n '89,103p' skills/pipeline/phase-prompts/brainstorm.md | sed '1s/^4\. //'
```

at the commit its `frozen at commit` cell names. Two reasons, and neither is that the sentinels
cannot render — that claim was checked against `cmark` and is false:

- The instruction for this task was to freeze the shipped text *exactly as `S0` was frozen*, and
  `S0` was frozen by exact recipe (`FREEZE.md`, *"`S0.md` predates the sentinels…"*). The recipe is
  the documented route for an arm the sentinels do not fit.
- The recipe route **does not modify `brainstorm.md` at all**, so the control arm is literally the
  shipped bytes, with no edit of the measured file interleaved between the measurement and the thing
  measured. Placing sentinels would make the first act of the experiment an edit to its own control.

`S1.md`'s shape — which `S2` and `S3` must match, because each is a drop-in replacement for step 4's
body: **no `4. ` numeral** on the first line, continuation lines keep their three-space indent, the
blank separator lines inside the item are truly empty, exactly one trailing newline.

### Deviations from what the frozen artifacts say

Four, all recorded here because the artifacts that state them are frozen or append-only and so
cannot be corrected in place. T10 repeats all four in the write-up. **Deviations 1–3 are things a
frozen artifact says that this run does not do; deviation 4 is a rule this run broke.** They are
listed together because T10 needs one complete list, not because they are the same kind of thing.

1. **`FREEZE.md`'s *"Who appends what, and when"* table is stale.** It assigns `S1.md` to "T6" and
   `S2.md`/`S3.md` to "T8". Those are task numbers from a decomposition that was discarded and
   re-planned (`docs/interactive-brainstorm.md` records the re-decomposition). In this run `S1` was
   frozen by **T1** and `S2`/`S3` are frozen by **T3**. `FREEZE.md` is append-only, so the table was
   left as-is rather than rewritten; a reader holding `FREEZE.md` without this file sees a
   contradiction, and this paragraph is its resolution.
2. **The frozen `tiered-review` ledger points at `../PRE-REGISTRATION.md`, which does not exist.**
   That pointer means **this file**. It is named `PROTOCOL.md` because the plan and every later task
   bind to that name by item number; the ledger is frozen and cannot be re-pointed. Following the
   dangling name lands nowhere, so it is recorded here instead.
3. **The `tiered-review` ledger's "held-out fixture" rule is NOT honoured by this run, deliberately.**
   `docs/skill-evidence/spec-length/ledger/tiered-review.md` says: *"**This is the held-out fixture**
   … an arm must clear `tui-dc-picker` and `skill-stickiness` before this one is scored."* This run
   scores **all three fixtures for all three arms, unconditionally**, in T5 and T6. Two reasons:

   - **The hold-out is incompatible with the gate as locked.** R2 is a 230/230 gate over the union
     of all three ledgers. An arm's retention is undefined until every one of its 230 rows has a
     verdict, so there is no state in which an arm has "cleared" two ledgers and is then admitted to
     the third. Sequencing the scoring would not withhold anything; it would only make the gate
     unevaluable.
   - **The channel the hold-out guarded is closed by a stronger mechanism.** A held-out fixture
     protects against arms being tuned against a fixture whose results are already visible. Here the
     arms are **frozen and committed before any spec is generated** (T3 precedes T4), there is
     **exactly one generation round** (R6), and no rule may be revised after T4's first probe. An
     arm cannot react to a result that does not exist yet.

   This deviation is stated as a deviation, not smoothed over: it was written into a frozen artifact
   and this run does not follow it.

4. **T3 rewrote `S3`'s `FREEZE.md` row in place, which `FREEZE.md` forbids.** Its own review found
   `S3` violated item 13 — it dropped the exclusivity half of `S1`'s first ask rather than stating it
   briefly — so T3 corrected the arm (`6a56a21f`) and updated its row (`2845f11d`). `FREEZE.md:14-16`
   says *"none of them rewrites a row above … a wrong hash is a finding, not an edit."* Appending a
   corrected row instead is impossible: `freeze_rows_still_hash_to_their_files` re-hashes **every**
   row, so the superseded one would sit permanently red. The correction itself is legitimate — it is
   squarely inside window 2, which permits it and requires only that it be logged — but the *manner*
   of it broke `FREEZE.md`'s rule. **`FREEZE.md`'s own closing section records it in full**, and this
   entry exists so that a T10 author reading this list is not told there are only three.

   **Root cause, so it is not repeated: T3 froze before it had finished checking.** Had item 13's
   review run to completion *before* the freeze commit, one commit would have carried the right bytes
   and no row would ever have needed rewriting. **T4 onward: finish every check the protocol names,
   then freeze.**

---

## 3. The design

**3 arms × 3 fixtures × 2 samples = 18 generations.**

| fixture | control spec | lines | ledger | rows |
|---|---|---|---|---|
| `skill-stickiness` | `fixtures/skill-stickiness.spec.md` | 791 | `ledger/skill-stickiness.md` | 91 |
| `tiered-review` | `fixtures/tiered-review.spec.md` | 463 | `ledger/tiered-review.md` | 84 |
| `tui-dc-picker` | `fixtures/tui-dc-picker.spec.md` | 414 | `ledger/tui-dc-picker.md` | 55 |

**230 ledger rows in total (91 / 84 / 55).** These are the ledgers' own `**Closed list: N rows.**`
declarations, which `spec_length_ledgers_are_the_closed_lists_they_claim` checks against the tables
beneath them, and they are authoritative.

**233 is a miscount, and here is exactly which command produces it.** `grep -c '^| '` returns
92 / 85 / 56 = **233**: it counts each table's header row (`| id | kind | item |`, which has a space
after the leading pipe) on top of the data rows, while excluding the separator row (`|---|---|---|`,
which has none). Counting `grep -c '^|'` instead returns 93 / 86 / 57 = **236**, picking up both.
Only the `**Closed list: N rows.**` declarations are authoritative, and no task may "fix" a ledger
to reach 233 or 236.

Every generation is scored against **its own fixture's** ledger only. An arm's retention is the
union rule in item 12's R1 over its six generations.

---

## 4. The three task lines

**Inherited verbatim from `PROTOCOL.md` item 4**, and checked as such by
`spec_length_2_protocol_inherits_its_verbatim_blocks`.

One per fixture, **identical across arms** — the task line is part of the held-constant input, not
part of the variable. Each is the fixture's `# ` title plus a one-sentence restatement of its
problem statement, naming no decision the ledger scores. They are written out here so no later task
re-derives them.

- **`skill-stickiness`:**

  > Spec: skill stickiness — drovr's skills read well and do not hold: the documents describe
  > correct practice, but nothing in them binds an agent working under context pressure.

- **`tiered-review`:**

  > Tiered (cascade) code review for drovr — `drovr code-review run` sends every review angle to the
  > same expensive model over the whole `base..head` range, so cost scales with angles × range size.

- **`tui-dc-picker`:**

  > Spec: TUI deploy-config picker — browse any config in the project — the deployment browser's `V`
  > key can only switch between versions of the config a deployment is already linked to, so every
  > other config in the project is unreachable from it.

---

## 5. The probe prompt template

Inherited from `PROTOCOL.md` item 5 with **one path change: `generated/` becomes `generated-2/`.**
Nothing else changes — not a sentence, not the return line. The change is recorded in item 15. (The
*"two substitutions"* the inherited sentence below names are a different thing: the template's own
two placeholders, the task line and the arm file's bytes.)

Arm-invariant except for the two substitutions. T4 dispatches 18 of these, foreground,
`subagent_type: general-purpose`, `model: sonnet`.

```
You are the brainstorm phase of a drovr run for this task:

    <TASK LINE for the fixture>

Your investigation and your interview with the human are COMPLETE. Everything that was
decided is in <abs path to fixtures/<fixture>.spec.md>. Read it. It is your notes, not
your output.

Write the spec to <abs path to generated-2/<id>.md>, following this instruction exactly.
Where it names `~/.local/share/drovr/runs/<run>/spec.md`, write to the path above instead.

--- BEGIN INSTRUCTION ---
<the arm file's bytes, verbatim>
--- END INSTRUCTION ---

Write the file yourself. Return exactly one line and nothing else:
{"transcript_id":"<id>","wrote":"<path>","ok":true}
```

**The probe is never given the ledger, the arm label, any other arm's text, or any other generated
spec.** The generated file holds **only the spec body** — no header, no arm name, no fixture name,
no id. Anything else is a channel to the scorer.

---

## 6. The id pool

**A fresh pool of eighteen opaque 6-hex tokens, disjoint from `PROTOCOL.md` item 6's pool.** The two
pools share no token, so `generated/<id>.md` and `generated-2/<id>.md` can never name the same `<id>`
and no prompt, path join or table row is ambiguous about which attempt it belongs to. Disjointness
was checked against item 6 of `PROTOCOL.md` at authoring time and is re-checkable from the two files.

**T4 assigns them to (arm, fixture, sample) triples in an order of its own choosing and records the
assignment only in `blind-map-2.json`**, so the listing order here maps to nothing and cannot be read
as an assignment. The list below is in plain lexicographic order for exactly that reason: a sorted
list carries no information about the draw at all.

```
031cc4  054872  08ae18  26d7a2  2c4295  2d2629
47173f  48527b  66530f  6e7393  a9fcf9  b2b8cf
b49ff1  e085f2  e790f5  fd230c  fd2c24  fe4059
```

**The constraint item 14a places on this pool is inherited and binds T4: T4 may not use any property
of an id — its value, its lexicographic rank, or its position in the pool listing — as a function of
the arm it is assigned to.** Item 14a's `A`/`B` rule rests on it, and
`spec_length_2_id_assignment_does_not_track_the_arm` checks it over `blind-map-2.json`.

**One further constraint, which the first attempt learned the expensive way.** `RESULTS.md` §5
deviation 5 records that the *filesystem* reconstructs an assignment the map is withholding: mtime,
inode order and `readdir` order over `generated-2/` all track the order the files were written, and
an arm-major dispatch order therefore publishes the arm partition to anyone who runs `ls -i` or
`stat`. **T4 dispatches out of arm order and rewrites all 18 files in id-lexicographic order before
committing them.** That is a constraint on item 6's assignment, not a suggestion, and it is recorded
here rather than in a task note because it is part of what the blinding claim rests on.

---

## 7. `blind-map-2.json` schema

`docs/skill-evidence/spec-length/blind-map-2.json`, written by T4 **before any scoring**, committed
before the first scorer is dispatched. **Never shown to a scorer, an adjudicator or an escalator**,
and not read by the tier-1, tier-2 or tier-3 tasks at all — those tasks depend on its existence, not
its contents. Item 7a exists so that they never need to open it.

```json
{ "031cc4": { "arm": "S1", "fixture": "tui-dc-picker", "sample": 1 } }
```

Eighteen entries, covering each (arm, fixture, sample) triple **exactly once**. The cell is a
**closed** object of exactly three keys — `arm`, `fixture`, `sample`. An extra key, a missing key, a
wrong type or a `null` is a hard error. It is joined to the verdicts **only after every one of the 18
verdicts is recorded**.

**The draw is recorded here and nowhere else, and that is a procedure, not a field.** The first
attempt's `RESULTS.md` §5 deviation 4 is the most serious entry on that page: T4 published the salt
and the ordering rule in a commit message, which gives up the three-way arm partition. So, for this
run:

- **T4 chooses the assignment, and `blind-map-2.json` is the only artifact that records any part of
  it.** No commit message, no `PROTOCOL-2.md` revision, no `RESULTS-2.md` line and no write-up
  sentence states the salt, the ordering rule, the sort direction, the within-arm fixture order, the
  within-fixture sample order, or any per-file digit sequence, at any point before the unblinding
  task has joined the verdicts.
- **There is deliberately no `salt` field.** The schema above is closed at three keys, so adding one
  would be rejected by the schema's own check — and a salt written into the map would be a second
  copy of the secret with nothing gained, since the map already holds the answer the salt derives.
  What the first attempt needed the salt for was *reproducibility of the draw*; what it cost was the
  partition. This file chooses the map.

**No context that has opened `blind-map-2.json` may author or dispatch tier-1, tier-2, tier-3 or the
unblinding join, and none may write the write-up.** Reviewing the commit that adds the map means
checking its *shape* — through `spec_length_2_generations_are_unlabelled_and_cover_the_design` and
`spec_length_2_id_assignment_does_not_track_the_arm` — **not reading its cells**.

---

## 7a. `fixture-map-2.json` schema

`docs/skill-evidence/spec-length/fixture-map-2.json`, written by T4 in the same commit as
`blind-map-2.json`:

```json
{ "031cc4": "tui-dc-picker" }
```

Eighteen entries, one per id in the pool, `{"<id>": "<fixture>"}` and nothing else. **It never
carries an arm, and it never carries a sample.**

**Why it exists.** The tier-1, tier-2 and tier-3 tasks must know which ledger each id is scored
against in order to shard at all. Under the first attempt the only file carrying that fact was
`blind-map.json`, which also carries the arm — so every scoring task had a standing reason to open
the file item 7 says it must not open. This map is the arm-free half of that fact, published
deliberately, so that no agent in the scoring chain ever has a legitimate reason to read the blind
map.

**It agrees with `blind-map-2.json`'s `fixture` cell for every one of the 18 ids**, and
`spec_length_2_generations_are_unlabelled_and_cover_the_design` checks that agreement. A disagreement
is a hard error, not a fallback: if the two maps disagree, nobody knows which ledger a verdict was
scored against.

**What it leaks, stated rather than assumed.** It partitions the 18 ids into three groups of six by
fixture. It does **not** distinguish the two samples within a fixture and it does **not** distinguish
the three arms, so knowing it tells a scorer the ledger it was already handed and nothing more. The
arm partition — the secret — cuts across it.

---

## 8. The tier-1 retention verdict schema

`docs/skill-evidence/spec-length/retention-2/<id>.json`, one file per generated spec, assembled by
the phase agent from its scorer shards:

```json
{
  "spec_id": "031cc4",
  "ledger": "tui-dc-picker",
  "rows": [
    { "id": "tui-dc-picker-01", "present": true,  "quotes": ["<verbatim substring>", "<another>"] },
    { "id": "tui-dc-picker-02", "present": false, "quotes": [] }
  ]
}
```

Closed object, three keys; each row object closed, three keys. An extra key, a missing key, a wrong
type or a `null` is a hard error. `rows` carries **every** ledger id for that fixture, **exactly
once, in ledger order** — no gaps, no duplicates, no extras.

**`quotes` is an array of 1–5 spans, not one string.** `PROTOCOL.md` item 8 capped it at 3 for a
reason that still holds — compression reorders, so a row's operative detail can end up split across
two non-adjacent sentences, and a single-span schema would force the scorer to either cite a fragment
that does not substantiate the row or mark it absent when it is plainly present. **The cap rises from
3 to 5 because 3 was too low, and `skill-stickiness-65` is the proof**: a row with four operative
parts cannot be evidenced by three spans, so the cap itself was manufacturing false negatives on
exactly the reorganised prose a shorter instruction produces. So: `present: true` requires **1–5**
non-empty spans, each a verbatim substring of the generated spec, which together establish the row.
`present: false` requires `[]`.

Every span is lifted from the **generated spec**, never from the ledger. That prevents a scorer
inventing text; **it does not by itself prevent a scorer citing real-but-irrelevant text** — items 8a
and 8b are what cover that, and the three must not be confused.

**No span may be cited for more than one row within a verdict file.** A near-boilerplate sentence
pressed into service for six rows is the cheapest way to fake retention.

### Every span must be self-contained

**This is new, and it is the mechanical half of what tier 2 catches by judgement.** A span that
starts or stops mid-phrase is malformed: it can look like evidence while establishing nothing. The
worked example is on disk — `invalidated/db3e2d.json`'s two spans for `skill-stickiness-55`, lifted
from `generated/db3e2d.md:451-452`, which are the row's operative content clipped at a hard wrap into
one fragment ending mid-phrase and one beginning mid-phrase. **Both must be rejected**, and the rule
below rejects both. The rule:

> **Some occurrence of the span in the generated spec must begin on a boundary and end on a
> boundary.** A span may occur more than once; one clean occurrence is enough, because the scorer is
> quoting the document and not an offset into it.
>
> **A span begins on a boundary** when any one of these holds: it starts at offset 0 of the file; or
> it starts at the first non-marker character of its line — markers being leading whitespace, a run
> of `#`, a `-`, `*` or `+` bullet, a `>`, a `|`, or `<digits>.` — **and** the preceding line is
> blank, is a heading, is a table row, or ends in one of `.!?:;`; or it is immediately preceded on
> the same line by `. `, `! ` or `? ` (any run of spaces); or it is immediately preceded by `| `.
>
> **A span ends on a boundary** when any one of these holds: it ends at end of file; or its last
> character is one of `.!?:;`; or it ends at end of line **and** the next line is blank or opens a
> new block (`|`, `#`, `-`, `*`, `+`, `>`, `<digits>.`); or it is immediately followed by `|` or
> ` |`.

**The two ends are deliberately symmetric, and that is not decoration.** The real failure this rule
was written against is a span clipped across a hard wrap, which produces **two** fragments: one
ending mid-phrase and one starting mid-phrase. A rule that required a terminator only on the
right-hand end would accept the second fragment — the half that happens to end in a full stop — and
reject only the first, which would make the catch an accident of which half of the clip the scorer
happened to quote.

**What this rule does not catch, stated here rather than left to be discovered.** A span lifted
cleanly out of a container is **accepted** by it: a well-formed sentence that is a bullet under an
*Out of scope* heading begins and ends on boundaries, so the mechanical check passes it even though
the container inverts its meaning. **That case is tier 2's and tier 3's to catch, not this rule's**,
and no reader should take a green mechanical check as evidence that a span is relevant.

**A verdict file whose spans violate this rule is malformed** and is re-run whole under `R6`, never
patched.

### Sharding, and what is checked

Each shard writes `docs/skill-evidence/spec-length/retention-2/parts/<id>-<k>.json`, `k` being the
1-based shard number. **A shard file has the same closed shape as this item** — `spec_id`, `ledger`,
`rows` — with `rows` covering exactly its own shard's ids, in ledger order. The phase agent then
**concatenates the shards' `rows` into `retention-2/<id>.json`** in ledger order; this item's
completeness rule is what proves the concatenation lost nothing. Shards under `parts/` are working
material and are deliberately unchecked by any test; the assembled file is the evidence.

`cli/tests/skills_valid.rs::spec_length_2_retention_verdicts_are_complete_and_quoted` enforces every
rule in this item over the immediate `*.json` children of `retention-2/`. It asserts
**well-formedness, never judgement** — nothing in it can say a verdict is *right*. **An empty or
absent directory is a pass**, which is the correct state until tier-1 scoring runs.

---

## 8a. Tier 2 — the relevance adjudication pass, redesigned

This is the item the first attempt got wrong, and the redesign is **entirely in its disposition**.
`PROTOCOL.md` item 8a ended *"Any `establishes: false` invalidates the entire verdict file"*, and
`RESULTS.md` §7 records what that produced: every verdict file of the one fixture that was scored was
invalidated, retention was undefined for all six generations, and the run's null was about the
instrument. **The mechanism below is unchanged from `PROTOCOL.md` item 8a. Only the consequence
changes.**

For each verdict file, an **independent blind adjudicator** subagent is given a pre-registered **20%
sample of that file's `present: true` rows** as `(ledger row text, the cited spans)` pairs, **and
nothing else: not the generated spec, not the arm, not the other rows**. Because it never sees the
spec, it cannot be swayed by how good the spec looks.

**The sample is fully determined, offset included, so two dispatches cannot select different rows.**
Take the file's `present: true` rows in ledger order and number them 1, 2, 3, … *among themselves*
(this index is over `present: true` rows only, not over ledger rows). The sample is **every row whose
index is divisible by 5** — the 5th, 10th, 15th, and so on. That is `floor(n/5)` rows out of `n`.

**"20%" is the nominal figure, and `floor(n/5)/n` is the real one** — they coincide only when `n` is
a multiple of 5, and otherwise the sample is slightly *under* a fifth (`n = 9` gives 1 row, 11%). The
mechanics above are what binds; the round number is shorthand, including where limitation 7 of item 1
uses it. Do not adjust the stride or the offset to hit a true 20%. If a verdict has fewer than five
`present: true` rows the sample is **empty**: record that in `RESULTS-2.md` against the verdict id and
do **not** substitute a different sample, a different offset or a smaller stride to manufacture one.

**The fixed stride is kept deliberately.** A random or per-file sample would audit each generation on
different rows, and the comparison between arms is only a comparison because every generation is
audited on the same rows of its own ledger.

`cli/tests/skills_valid.rs::spec_length_2_adjudication_sample_matches_the_stride_rule` recomputes the
sample from `retention-2/<id>.json` and requires `adjudication-2/<id>.json`'s pairs to be **exactly**
it, in order, with no substitutions. Without that check a dispatch that sampled the wrong rows would
be invisible to the whole suite.

Its prompt, fixed here and identical for every verdict — this is `PROTOCOL.md` item 8a's prompt with
**one substitution, *"one to three spans"* → *"one to five spans"***, following item 8's raised cap:

```
For each numbered pair below you are given a key-point row from a frozen ledger, and one to
five spans of text quoted from a document. Decide, for each pair independently:

    does the quoted text establish that row?

"Establish" means an implementer reading only those spans would build what the row states,
without stopping to ask. A span that is merely on the same topic does not establish the row.

You are not judging the document, which you have not seen, and you are not judging whether the
row is a good row. Only whether these spans establish this row.

--- BEGIN PAIRS ---
<n>. ROW: <ledger row item text>
    SPANS: <span 1> | <span 2> | <span 3>
--- END PAIRS ---

Return exactly one line and nothing else, a JSON array of one object per pair in order:
[{"n":1,"establishes":true},{"n":2,"establishes":false}]
```

**The `SPANS:` line still shows three placeholders, and that is deliberate rather than an oversight.**
It is a *join format* — spans separated by ` | ` — and not a cap; the cap is stated in the sentence
above it, which now says five. Rewriting the placeholder line would be a second, unpinned change to a
frozen prompt, and the substitution this file is authorised to make is the one named above.

**Its output lands in `docs/skill-evidence/spec-length/adjudication-2/<id>.json`:**

```json
{
  "spec_id": "031cc4",
  "ledger": "tui-dc-picker",
  "sample_rule": "every 5th present:true row in ledger order, 1-based among present:true rows",
  "pairs": [ { "n": 1, "row": "tui-dc-picker-05", "establishes": true } ]
}
```

**The disposition, which is the whole redesign: an `establishes: false` flags that one row for tier 3
and invalidates nothing.** Not the row, not the file, not the generation. The flagged row's final
`present` is decided by item 8b and by nothing else. A verdict file is never re-run on account of a
tier-2 result, and **`R6`'s closed list no longer contains "a verdict fails 8a"** (item 12).

**What this costs is stated in item 1's limitation 7 and is not softened here:** it makes an
adjudication failure *less* consequential than it was, which is a deliberate loosening toward a false
pass. What it buys is a defined retention count for every generation, which is the thing the first
attempt did not have.

---

## 8b. Tier 3 — escalation, and the call that governs

**New in this attempt.** One dispatch per generation that has **at least one** row flagged by item
8a. A generation with no flagged rows gets **no dispatch and no file** — the absence of
`escalation-2/<id>.json` is the record that nothing was flagged, and item 12's join reads it that way.

The escalator is handed **the generated spec** and **the ledger text of each flagged row**. **The
tier-1 spans are withheld**, so this is a fresh reading of the spec against the row, and not a review
of the scorer's evidence. It answers item 9's question, verbatim, with item 9's own definition in the
prompt.

**Tier 3's call governs that row**, replacing tier 1's, and is recorded per row with its reason.

**Why tier 3 governs rather than tier 1.** It is an independent reader judging one row with the whole
spec in front of it, where tier 1 judged up to 46 rows in a single batch. It is the same standard as
item 9, applied with more context and less load. **Its cost is limitation 9 of item 1 and is real:**
tier 3 sees the spec, so unlike tier 2 it can be swayed by how good the spec looks. That is accepted
deliberately, because tier 2's spec-blindness is what made the first attempt measure the wrong thing.

The prompt, fixed here:

```
You are reading one spec and judging whether specific key points survive in it.

--- BEGIN SPEC ---
<the contents of generated-2/<id>.md>
--- END SPEC ---

Below are numbered rows from a frozen key-point ledger. For each row decide `present`
under this definition, which is the governing definition and not a summary of one:

<item 9's two blockquotes, verbatim>

--- BEGIN ROWS ---
<n>. <ledger row item text>
--- END ROWS ---

You have not been shown anyone else's judgement of these rows and you are not reviewing
one. Answer from the spec alone.

Return exactly one line and nothing else, a JSON array of one object per row in order:
[{"n":1,"present":true,"reason":"<one sentence>"}]
```

Its output lands in `docs/skill-evidence/spec-length/escalation-2/<id>.json`:

```json
{
  "spec_id": "031cc4",
  "ledger": "tui-dc-picker",
  "rows": [ { "id": "tui-dc-picker-05", "present": false, "reason": "<one sentence>" } ]
}
```

`rows` carries **exactly** the rows item 8a flagged for that generation, once each, in ledger order.
A row in `escalation-2/<id>.json` that tier 2 did not flag is a hard error, and so is a flagged row
missing from it — `spec_length_2_final_disposition_is_the_recorded_join` checks both.

**The final disposition of every row, stated once and machine-derivable:** `present` is **tier 3's
call where an `escalation-2` record exists for that `(id, row)`, and tier 1's otherwise.** There is
no third source, no manual override, and no row whose disposition is decided by a human reading. The
same test refuses an override with no record behind it.

**The tier-3 overturn rate is reported**, defined in item 1's limitation 8 as
`overturned / escalated`, per arm and overall.

---

## 9. The `present` definition

Pre-registered, and handed to every scorer and every escalator verbatim. **The first blockquote is
inherited verbatim from `PROTOCOL.md` item 9** and is checked as such by
`spec_length_2_protocol_inherits_its_verbatim_blocks`:

> `present` is `true` when the row's content is recoverable **and actionable** from the generated
> spec alone: an implementer reading only this spec would build what the row states, without
> stopping to ask. A paraphrase counts, and may be evidenced by up to three spans. A mention that
> drops the row's operative detail — the exact name, the exact bound, the exact exclusion — does
> not.

**One clarification is added, and it is the only change to the `present` standard in this attempt.**
It is a second blockquote rather than an edit to the first, so the inherited text stays inherited and
a reader can see exactly what was added:

> A **fixture-internal cross-reference** — a section number such as `§7.3`, a pointer such as "see §0
> step 2", or a fixture table's own row numbering — is **not operative detail**. A spec that
> renumbers under compression has not dropped the decision; judge the content the reference points
> at, not the number. This applies identically to every arm and every row.

**Why this is pre-registered rather than left to judgement.** `RESULTS.md` §7.3 and the reading of
the record in `spec.md` §2 both found generated specs citing `§9.3` or `§6.3` where the ledger row
says `§7.3` — the specs renumbered under compression. Treating a renumbered cross-reference as a
dropped operative detail would eliminate arms for compressing, which is the behaviour being measured,
and it would do so unevenly: an arm that compresses more renumbers more. Fixing this before any new
measurement is what makes it a rule rather than a result.

**Note the boundary.** `§7.3` inside a *ledger row's own text* is a fixture-internal reference and is
covered by this clarification. An exact **name**, an exact **bound** and an exact **exclusion** are
operative detail and are not covered by it — the first blockquote still governs those, unchanged.

**Both blockquotes are handed to tier 1 and to tier 3 together**, in that order, wherever item 10 or
item 8b writes *"item 9's definition"* or *"item 9's two blockquotes"*.

---

## 10. The tier-1 scorer prompt template, and the sharding rule

Inherited from `PROTOCOL.md` item 10 with the path substitutions of item 15 and **three changes,
each of which follows from item 8 and none of which is new policy**: the span cap in the template's
rules rises from 3 to 5; a self-containment rule is added to the same list; and the concatenation
paragraph loses its reference to 8a invalidation, which no longer exists. The sharding boundaries are
**unchanged** — the same 30 shard dispatches, the same row ranges.

**The scorer is handed only** the generated spec file, its fixture's ledger rows for its shard, and
the item-8 schema plus the item-9 definition. **Never `blind-map-2.json`, never
`docs/skill-evidence/arms/`, never this file, never another generated spec, never another arm's
output.**

**The sharding rule, because a 91-row JSON reply is where a scorer truncates.** A scorer is given
**at most 60 ledger rows**. Shard boundaries are fixed here and are **never adjusted per arm**:

| ledger | rows | shards |
|---|---|---|
| `skill-stickiness` | 91 | `-01`–`-46`, `-47`–`-91` |
| `tiered-review` | 84 | `-01`–`-42`, `-43`–`-84` |
| `tui-dc-picker` | 55 | one shard, `-01`–`-55` |

Each shard writes `docs/skill-evidence/spec-length/retention-2/parts/<id>-<k>.json`, `k` being the
1-based shard number. **A shard file has the same closed shape as item 8** — `spec_id`, `ledger`,
`rows` — with `rows` covering exactly its own shard's ids, in ledger order. The phase agent then
**concatenates the shards' `rows` into `retention-2/<id>.json`** in ledger order; item 8's
completeness rule is what proves the concatenation lost nothing. Shards under `parts/` are working
material and are deliberately unchecked by any test; the assembled file is the evidence.

**Concatenation is assembly, not repair.** A shard that is malformed, or that fails the item-8
mechanical check, is **re-run whole** and never patched. **A tier-2 result is not on that list any
more**: under item 8a an `establishes: false` flags a row for tier 3 and invalidates nothing, so
there is nothing to re-run. `R6`'s closed list in item 12 is the authority on what may be re-run, and
this sentence must agree with it.

The template:

```
You are scoring one document against a frozen key-point ledger. Read the document in full:

    <abs path to generated-2/<id>.md>

Below are <k> ledger rows. For each row decide `present` under this definition, which is the
governing definition and not a summary of one:

<item 9's two blockquotes, verbatim>

--- BEGIN LEDGER ROWS ---
| id | kind | item |
|---|---|---|
<the shard's rows, verbatim from the ledger>
--- END LEDGER ROWS ---

Write <abs path to retention-2/parts/<id>-<k>.json> containing exactly this object and nothing
else:

{ "spec_id": "<id>", "ledger": "<fixture>",
  "rows": [ { "id": "<row id>", "present": true, "quotes": ["<span>"] } ] }

Rules, all of them hard:
  - `rows` carries every id above, exactly once, in the order given. No extras.
  - `present: true` requires 1 to 5 non-empty spans in `quotes`, each an EXACT verbatim
    substring of the document, which together establish the row.
  - `present: false` requires `quotes: []`.
  - EVERY span must be SELF-CONTAINED: it must begin at the start of a sentence, table
    cell, list item, heading or line-block, and end at the end of one. Never clip a span
    at a line break in the middle of a phrase. If the text you want spans two wrapped
    lines, quote the whole sentence, not the two halves.
  - Every span is copied from the document. Never from the ledger row.
  - No span may appear under more than one row.
  - No other keys, anywhere. No nulls.

Marking a row present that is not there is the one failure that corrupts this measurement. If
you are unsure, `false` is the honest answer.

Return exactly one line and nothing else:
{"spec_id":"<id>","shard":<k>,"wrote":"<path>","ok":true}
```

---

## 11. The blinding limitation, in this experiment's own terms

**Inherited from `PROTOCOL.md` item 11, unchanged, and then extended by three additions this attempt
introduces.** The inherited text first:

The scoring here is **label-blind, not arm-blind**, and the channel is **vocabulary and section
shape**: a generated spec's length, section headings and phrasing still correlate with its arm, and
an arm that says "decision record" can push that exact phrasing into every spec it generates.

**Do not substitute `skills/writing-skills/references/scoring-rubric.md`'s stated limitation for
this one.** That paragraph was written about a different leak channel — a `cites_section: true`
verdict identifying an armored transcript — and quoting it here would describe a channel this
experiment does not have while leaving the one it does have unstated. State the channel that
applies. **Do not describe this scoring as fully blind anywhere.**

**Three additions, all of them channels this attempt either opens or inherits with its eyes open.**

**(a) Tier 3 sees the generated spec.** Item 8b hands the escalator the whole spec, so unlike item
8a's adjudicator it can be swayed by how good the spec looks — a well-written spec may earn the
benefit of the doubt on a row a plainer one would not. This is limitation 9 of item 1 and it is
accepted deliberately: item 8a's spec-blindness is what made the first attempt measure span-citation
craft instead of retention. The tier-1 spans are withheld from tier 3 precisely so that the one thing
it cannot be swayed by is the scorer's own evidence.

**(b) Every dispatched subagent runs with a working directory inside this worktree, so it could read
`blind-map-2.json` or recover an arm from `git log`.** `RESULTS.md` §7.6 deviation 7 recorded exactly
this for the first attempt's scorers: the harness gives no way to set a subagent's working directory,
the prescribed control (dispatch outside the worktree, hand over a byte-for-byte copy) was run and
was **incomplete**, and the channel was logged as **open**. **This attempt inherits it unchanged and
logs it again on the same terms** — as open, not as controlled. Nothing suggested any first-attempt
dispatch used it, and nothing will suggest it here either, but *"was told only to read the spec"* is
an instruction and not a control. **Every task in this run re-discloses it in `RESULTS-2.md`; it is
not silently repeated.**

**(c) The first attempt's arm assignment is recoverable from this branch's history.** `RESULTS.md` §5
deviation 4 records that a commit message published the salt and the ordering rule, which gives up
the three-way arm partition, and deviation 5 records a second, cheaper disclosure of the same
assignment. That is **immaterial to this attempt's gate**, which uses a disjoint id pool (item 6) and
a fresh assignment recorded only in `blind-map-2.json` (item 7) — but it is **not** immaterial to
item 12a's calibration pass, which runs on the first attempt's own generations and whose arms are
therefore recoverable by anyone who goes looking. Item 12a scores **per row, not per arm**, and
`blind-map.json` is not opened; that is the mitigation, and it is a mitigation rather than a closure.
The write-up carries this as a footnote because this run documents every channel it knows about.

**And the rule that keeps the arm out of the scoring chain, stated here because item 11 is where what
may be claimed about the result lives:** no context that has opened `blind-map-2.json` may author or
dispatch tier 1, tier 2, tier 3 or the unblinding join, and none may write the write-up. The map
lands in a commit whose diff shows the whole assignment, so **reviewing that commit means checking
the map's shape, not reading its cells.**

**Do not describe this scoring as fully blind anywhere.**

---

## 12. The decision rules

**These decide the outcome. None of them may be revised after the first probe of item 5 is
dispatched.**

**`R1`, `R1a`, `R3`, `R3a`, `R4`, `R4a`, `R5`, `R5a`, `R6a` and `R7` are inherited verbatim from
`PROTOCOL.md` item 12**, and `spec_length_2_protocol_inherits_its_verbatim_blocks` checks each of
them against `PROTOCOL.md` itself rather than taking this sentence's word for it. **`R2` is inherited
in substance but not verbatim** — `R8` resolves its denominator, and it is the one decision rule this
file rewrites. **`R6` narrows.** **`R8` is added.** Item 15 lists all three changes in one place.

**Every rule below reads the *final disposition* of a row, defined in item 8b:** tier 3's call where
an `escalation-2` record exists for that `(id, row)`, tier 1's otherwise. Where an inherited rule
says a generation "retains" or "drops" a row, that is the final disposition and never tier 1's call
on its own.

- **R1 — the all-generations rule.** An arm retains a row only if **every generation that was scored
  against that row retains it** — which is the two samples of that row's own fixture, since a
  generation is only ever scored against its own fixture's ledger (item 3). Equivalently, and this
  is the form to implement: **an arm's dropped set is the union of its six per-generation dropped
  sets**, and its retention is `230 −` the size of that union. Chosen deliberately over a
  majority-of-samples rule: a majority rule would let an arm lose a decision in a real spec and still
  pass. *(This is an AND across generations. An earlier draft said "all six of its generations retain
  it", which read literally would require the four generations of the other two fixtures to retain a
  row they never judged — no row would ever be retained. The two sentences above are the operative
  ones.)*
- **R1a — per-generation retention is recorded alongside the union.** 18 per-generation counts, not
  just 3 union counts. Without them a null cannot be read: a row that 5 of 6 generations retain is
  sampling noise, and a row no generation retains is the instruction. Same data, and only R1a makes
  the failure diagnosable by whoever picks this up next.

- **R2 — the gate.** An arm that retains **every row of the discriminating set** clears. **Any** drop
  eliminates. No partial credit, no weighting by `kind`. The discriminating set is `230 −` the rows
  `R8` excludes, and both readings — `N/230` and `N/|discriminating set|` — are reported everywhere.
  This is `PROTOCOL.md`'s `R2` with its denominator resolved by `R8`, and it is the one decision rule
  this file does not inherit verbatim.

  **Why the denominator had to be resolved rather than left at `230/230`.** `R8` excludes a row that
  **every** generation of its fixture dropped. Under a literal `230/230` gate such a row has already
  eliminated every arm, so the carve-out could never change an outcome and would be a reporting
  annotation on a settled null. `spec.md` §3.2 adopts `R8` precisely so that *"a row any generation
  retains stays in the gate in full force"* while a row no arm could carry stops deciding the
  outcome; that is only true if `R2` reads the adjusted denominator. `R7`'s *"cleared `R2`"* and
  `R3a`'s ship / no-ship both read it too.

  **`R1`'s `230 −` arithmetic is unaffected and is the raw reading.** `R1` computes an arm's dropped
  set as the union over its six generations and its retention as `230 −` that union's size; that
  number is reported as `N/230` exactly as `R1` states it. `R2` then asks a different question of the
  same data — whether the arm dropped any row of the *discriminating* set — and it is `R2`, never
  `R1`, that decides clearance.

- **R3 — length is compared only among arms that clear R2.** Metric: `wc -l` and `wc -c` of each
  generated spec. Report per-arm mean lines over its 6 generations, per-fixture means, and the
  fixture's own length as the reference point (**791 / 463 / 414**). Report each **arm file's** own
  length too (`S0` 2 lines / 177 B, `S1` 15 lines / 1151 B) — a longer instruction costs context in
  every brainstorm phase forever, and it is the cost this rewrite already paid.
- **R3a — "shorter" means the generated spec, and the tie-break is fixed here.** The governing
  metric is **mean generated-spec lines across the arm's six generations** — that is the thing the
  human complained about. Ties break on mean bytes, then on the arm file's own line count. A shorter
  *instruction* that produces *longer* specs has not won anything. And if the arm with the lowest
  mean is `S1`, **no candidate beat the control**: ship nothing and record it.

  **The comparison T7 records is provisional; the winner is decided only after T8 and T9.** T7
  applies R3a over the arms that cleared R2, but T8 and T9 run *after* T7 and either can be fatal
  (R7). So the ordering, pre-registered here because otherwise it is settled once the results are
  in — apply these three steps in order:

  1. **The survivors** are the CANDIDATES (`S2`, `S3`) that cleared R2 *and* T8 *and* T9. If there
     are none, the outcome is the null under R7 and nothing ships.
  2. **The control still has to be beaten, under the full ordering and not just on lines.** If `S1`
     cleared R2, rank it against the lowest surviving candidate by **the same three-key order this
     rule already fixes: mean lines, then mean bytes, then the arm file's own line count.** If `S1`
     ranks first under that order, **no candidate beat the control**: ship nothing and record it,
     exactly as the sentence above says. The survivors clearing every gate does not by itself make
     one of them shorter than what already ships. **An exact tie on mean lines is resolved by bytes
     and then by the arm file, never by defaulting to the control** — the three-key order is the
     rule, and step 2 does not get a shorter one of its own. (If `S1` did **not** clear R2, it is
     not in R3's compared set at all and R4a governs the comparison instead.)
  3. **Otherwise the lowest-mean survivor wins.** If the provisional leader was eliminated by T8 or
     T9, **the next-lowest surviving candidate takes its place** — it is not discarded along with
     the leader, and the run does not fall to a null merely because the shortest survivor was not
     the shortest R2-clearer.

  T7 labels its own table **"provisional — T8 and T9 have not run"** whenever any candidate cleared
  R2, so no reader mistakes it for the verdict.
- **R4 — the control is not exempt, and a universal null is the likely outcome.** `S1`'s retention
  is the **instrument reading**: it says whether the gate is clearable at all under D2. R1 requires
  **460** row-judgements — 2 samples × 230 rows — to come back clean for **one** arm to pass, and
  **1380** across all three; at any realistic per-row
  fidelity the probable result is that **no arm clears, control included**. That is anticipated, not
  a failure of execution. The gate still binds at 230/230 for everyone, and T10 must state plainly
  when the null is attributable to the instrument rather than to the candidates.
- **R4a — the asymmetric outcome: a candidate clears R2 and the control `S1` does not.** R4 covers
  the symmetric null, and R7 covers *"`S1` clears, no candidate does"*. This rule covers the third
  case, which the file's own limitation 7 makes reachable — the residual scoring risk runs toward a
  **false pass**, so a candidate clearing a gate the shipped text failed is exactly the shape a
  leaky instrument produces. It is pre-registered here rather than decided when it happens.
  - **The candidate is not eliminated by the control's failure.** R2 is a per-arm gate, and a
    control that drops a row may simply have dropped it. T8 and T9 still run on the candidate under
    R7, unchanged; they are the independent fatal checks and neither is weakened here.
  - **But R3's length comparison then contains no control.** *"Shorter than what ships"* is not
    established by this run, because `S1`'s generations are outside the cleared set. T10 may still
    report the candidate's mean against `S1`'s mean, and when it does it carries the same
    **"descriptive, not a pass"** label R5 puts on a retention count below 230 — this bullet is what
    licenses that label for a *length* comparison, since R5's own text is scoped to retention.
  - **T10 must name every row `S1` dropped and every generation that dropped it**, beside the
    statement that the candidate retained it. That list is the only thing that lets the next reader
    tell a genuinely better candidate from a noisy instrument, and **T10 must say which it believes
    and why.** It may report either, and it may not omit the question.
  - The paste is permitted and is **not** automatic: it proceeds only if T8 and T9 are both clean.
- **R5 — retention counts below 230 are descriptive, never a pass.** They are recorded so the null
  is informative, and they are labelled *"descriptive, not a pass"* wherever they appear.
- **R5a — the copy check.** A generation whose length is **≥ 95% of its fixture's** (791 / 463 /
  414) has substantially copied its input rather than compressed it. That is **not** a failure — it
  is the correct null-ward behaviour under D2, and limitation 1 says a probe that copies its input
  scores full retention at full length. But a high-retention verdict on such a generation says
  nothing about the instruction, so `RESULTS.md` **flags every generation over that threshold**, and
  the write-up may not read a flagged generation as a win. Stated here rather than left to T7
  because it changes how a retention pass is read, and this file is where the rules that decide the
  outcome live.

- **R6 — no re-runs to chase a result.** One generation round per arm. A re-run is permitted only for
  a **protocol** failure, and the list is closed: **the probe wrote no file; a shard or a verdict is
  malformed (which covers a transmission verdict under item 14 and a gap file under item 14a exactly
  as it covers a retention verdict); a verdict fails the item-8 mechanical check.** Never for an
  unwanted outcome, and **every re-run is logged in `RESULTS-2.md` with its reason**.

  **This is `PROTOCOL.md`'s `R6` with one trigger removed: *"a verdict fails item 8a's relevance
  adjudication"* is gone**, because nothing is invalidated by tier 2 any more (item 8a). Removing a
  re-run trigger narrows the rule — it takes away a licence to re-run, it does not grant one — and it
  is the only change. The item-8 mechanical check now includes item 8's self-containment rule, so a
  verdict quoting clipped spans is still re-run whole.

- **R6a — you may never silently accept a verdict you believe is wrong.** If a verdict is
  schema-valid, passes adjudication, and still looks wrong on inspection, that is not a licence to
  re-run and not a licence to accept: **record the specific doubt in `RESULTS.md` against that
  verdict id and carry it into the write-up.** A recorded doubt is evidence; a quiet re-roll until
  the number looks right is the thing this whole file exists to prevent.
- **R7 — T8 and T9 run only on a CANDIDATE arm (`S2` or `S3`) that clears R2.** The word *candidate*
  is load-bearing: `S1` clearing on its own is the instrument reading, not a shippable outcome,
  because the control is what already ships. R4 makes *"`S1` clears, no candidate does"* a live and
  reachable result, and in that case T8 is pointless — nothing is being proposed — and T9 is
  **undefined**, because its whole question is *what does the control answer that the arm does not*,
  which has no meaning when the arm **is** the control. So: **if no candidate clears R2, T8 and T9
  are both skipped**, whatever `S1` did, and T10 records the outcome under R3a as *no candidate beat
  the control — ship nothing*. Any unrecovered row in T8, and any gap found in T9, is **fatal** to
  that arm.

- **R8 — the universal-drop carve-out.** A ledger row that **every generation scored against it**
  drops under the final disposition is excluded from the gate denominator and reported separately.
  **That is the six generations of the row's own fixture — 3 arms × 2 samples — not all 18**, because
  a generation is only ever scored against its own fixture's ledger (item 3). The criterion is a
  function of the union across all arms and is therefore symmetric under permutation of the arm
  labels, so it cannot be steered toward an arm. Both numbers are always reported: the raw `N/230`
  and the gate reading `N/|discriminating set|`. A row any generation retains stays in the gate in
  full force.

  **`R8` is a post-hoc adoption and this file says so plainly rather than presenting it as if it had
  always been there.** It was decided after the first attempt failed (`spec.md` §3.2, reviewer turn 1,
  `q_r8` = adopt) and before any new measurement was taken. What makes that legitimate is not the
  timing — it is that the criterion **cannot be aimed**: it reads only the final dispositions and the
  id→fixture map, never an arm, so permuting the arm labels leaves the excluded set identical while
  the per-arm retention counts permute with them.
  `cli/tests/skills_valid.rs::spec_length_2_r8_exclusion_is_arm_symmetric` executes exactly that
  permutation rather than asserting the property in prose. **The write-up states the post-hoc
  adoption and this argument together, and does not bury either.**

  **A row that NO generation judged is not excluded — it is an error.** That state means a fixture
  was never scored, and silently excluding its rows would shrink the denominator for rows nobody
  looked at. The exclusion function returns it as a failure, not as an exclusion.

  **The alternative that was rejected: a hand-authored "row admissibility mask".** A human deciding
  which frozen ledger rows count is exactly the shape of a redesign chosen to favour an outcome, and
  the reading of the record in `spec.md` §2 found that the two most suspect rows —
  `skill-stickiness-55` and `-65` — are not badly written rows at all. What failed was the instrument
  around them.

  > **`R8`'s denominator above is a correction to `spec.md` §3.2's wording, and it is a clarification
  > of its intent, not a change to it.** `spec.md` says *"every one of the 18 generations drops"*.
  > Read literally that excludes nothing ever: 12 of the 18 never judge any given row, so no row can
  > be dropped by all 18. This is the identical slip `PROTOCOL.md`'s `R1` records and corrects for its
  > own union rule (*"an earlier draft said 'all six of its generations retain it', which read
  > literally would require the four generations of the other two fixtures to retain a row they never
  > judged"*). The intent — *a row no arm can carry, control included* — is preserved exactly, and
  > the symmetry argument is unaffected. The operative form is the one stated above, and this
  > paragraph is the reason it is stated that way.

**A null is a valid, publishable result.** *"No arm is shorter without loss"* answers the question
that was asked. It is pre-registered here as an outcome precisely so it cannot later be treated as a
failure to be re-run away, and so nobody weakens R2, R5 or R7 to avoid reporting it.

---

## 12a. The calibration pass

Numbered out of sequence for the same reason 8a and 14a are: items 1–15 are cited by number
elsewhere, and this pass was added by `spec.md` §3.5 after the numbering was fixed.

**This is the first measurement taken under `PROTOCOL-2.md`, and it runs before any new generation
exists.** It tests the diagnosis in `spec.md` §2 with a measurement instead of a reading.

**What it runs on.** The **24 flagged rows** of the first attempt's operative six adjudication
passes, recomputed from `docs/skill-evidence/spec-length/invalidated/adjudication/*.json` joined to
`invalidated/<id>.json`. The operative pass is **attempt 2 for `4a73ef` and attempt 1 for `aa3199`,
`d25798`, `80d9a2`, `db3e2d` and `87e5a5`**; the 24 are the `establishes: false` pairs across those
six — `4a73ef` 4, `aa3199` 5, `d25798` 3, `80d9a2` 5, `db3e2d` 2, `87e5a5` 5 — out of 106 pairs
adjudicated (18 + 18 + 17 + 18 + 18 + 17), which reproduces `RESULTS.md` §7.3's 24-of-106 exactly.
**`4a73ef`'s attempt 1 is not in the set**: it was superseded by the `R6` re-run `RESULTS.md` §7.4
records, and counting both attempts would double-count one generation.
`cli/tests/skills_valid.rs::spec_length_2_calibration_is_the_recorded_flagged_set` recomputes that
set and requires `calibration-2.json` to be **exactly** it — so the calibration set cannot be trimmed
to the rows that confirm the diagnosis.

**What it does.** Six dispatches, one per generation, each the **tier-3 instrument of item 8b**
applied unchanged: the generated spec, the flagged rows' ledger text, item 9's two blockquotes, the
tier-1 spans withheld. Output is `docs/skill-evidence/spec-length/calibration-2.json`:

```json
{
  "instrument": "PROTOCOL-2 item 8b (tier 3)",
  "records": [
    { "spec_id": "db3e2d", "attempt": "1", "ledger": "skill-stickiness",
      "rows": [ { "id": "skill-stickiness-55", "present": true, "reason": "<one sentence>" } ] }
  ]
}
```

Six records, 24 rows in total.

**Four properties that make it safe to run, each of which is a constraint and not a hope.**

1. **It runs against `invalidated/`, whose files are already invalidated and which no gate reads.**
   It cannot promote anything into a verdict. Nothing in `retention-2/`, `adjudication-2/`,
   `escalation-2/` or `RESULTS-2.md` may cite it as evidence about an arm.
2. **It is scored per row, not per arm.** All six generations are `skill-stickiness` and they span
   arms; `blind-map.json` is not opened, and the pass reports 24 row-level calls with no arm
   attached to any of them.
3. **It changes no rule.** The instrument is frozen by this file before it runs, and item 12a is
   itself a governed item. Its window is window 2 from its first dispatch onward: a governed item may
   be clarified, never weakened, and every edit is logged in `RESULTS-2.md`.
4. **Its outcome is reported either way.** If tier 3 finds most of the 24 flagged rows recoverable
   from their specs, `spec.md` §2's diagnosis is confirmed — item 8a was measuring span-citation
   craft under a withheld spec. **If it does not, `PROTOCOL-2.md` still binds, unchanged, and the
   write-up says the diagnosis was wrong.** A calibration pass that can only confirm is not a
   calibration pass.

**Ordering, and it is checked.** `calibration-2.json`'s introducing commit precedes `generated-2/`'s
— that is `spec_length_2_protocol_precedes_every_generation`'s second half, and it is this item's own
ordering claim executed rather than asserted.

**Its known channel is item 11 addition (c):** the arms of these six generations are recoverable from
this branch's history. Scoring per row and never opening `blind-map.json` is the mitigation, and it
is a mitigation rather than a closure.

---

## 13. The candidate-authoring constraints — inherited, and discharged

`PROTOCOL.md` item 13 binds the task that authors candidate arms. **No arm is authored, edited or
re-frozen by this run** (item 2), so every constraint in it is discharged by there being nothing for
it to bind:

- `S2` and `S3` were authored under it, and both were checked against it at the time — including the
  correction `FREEZE.md`'s recorded breach describes, where `S3` was found to violate item 13's
  *"states the three things `S1` asks for"* and was corrected before any spec was generated.
- `skills/pipeline/phase-prompts/brainstorm.md` is **not edited** by this run either. A paste into it
  happens only if a candidate clears every gate, which `R7` and `R3a` already govern, and it would be
  the work of the write-up task and not of this protocol.

**This item is retained rather than deleted** so that the item numbering matches `PROTOCOL.md`'s and
a reader can diff the two files item by item. Retaining it costs nothing; renumbering would break
every cross-reference in both files.

---

## 14. The transmission sample

**Inherited from `PROTOCOL.md` item 14 with the path substitutions of item 15 and nothing else** —
the 60 row indices, the question phrasing, the blind question-writer, the definition of `recovered`,
and both prompts are its own. `transmission/` becomes `transmission-2/` and `generated/` becomes
`generated-2/`; every sentence, rule and return line is `PROTOCOL.md`'s. It runs under `R7` only, on
a candidate arm that cleared `R2`.

Fixed here so it cannot be tuned. **Exactly 20 rows per fixture, 60 in total**, selected by this
command — which is the definition, so run it rather than computing by hand:

```
seq 0 19 | awk -v n=<N> '{printf "%d\n", 1 + int($1*(n-1)/19 + 0.5)}'
```

It yields 20 distinct 1-based row indices spanning `-01` to `-<N>` inclusive. Confirmed at 20
distinct indices for all three fixtures, and written out so T8 reads them rather than re-deriving
them:

- `skill-stickiness` (N=91): `1 6 10 15 20 25 29 34 39 44 48 53 58 63 67 72 77 82 86 91`
- `tiered-review` (N=84): `1 5 10 14 18 23 27 32 36 40 45 49 53 58 62 67 71 75 80 84`
- `tui-dc-picker` (N=55): `1 4 7 10 12 15 18 21 24 27 29 32 35 38 41 44 46 49 52 55`

**Scope: sample 1 only**, three fixtures per surviving candidate arm — 3 probe runs and 3
adjudicator runs, not 6. Sample 2 is deliberately not transmission-tested; that is limitation 4 of
item 1.

**The question phrasing, written once here and identical across arms.** Each of the 20 questions is
the row's subject **with the row's answer removed**:

```
    What does this spec say about <the row's subject, with the row's answer removed>?
```

**Who fills that blank, and when — because the template alone does not pin the questions.** Turning
a ledger row into a subject-without-its-answer is a judgement call, one per row, 60 in total, and
T8 runs *after* T7 has unblinded. Leaving it to T8 would put the same non-blind hand on the
questions that item 14a exists to keep off T9's prompts. So:

- **An independent question-writer subagent composes all 60**, handed **only the sampled ledger
  rows** — never a generated spec, never an arm, never `blind-map-2.json`, never a retention verdict.
  It cannot favour an arm because it has never seen one.
- Its output is committed to `docs/skill-evidence/spec-length/transmission-2/questions/<fixture>.json`
  as `[{"id": "<ledger id>", "question": "<text>"}]`, covering that fixture's 20 sampled ids in
  order. **The same 20 questions are then used for every arm**, which is what makes the comparison
  between arms a comparison at all.
- **It may run at any point once the ledger is frozen** — it is blind whenever it runs, so the
  ordering that matters is only that the file is **committed before T8 dispatches its first probe**.
  T8 reads it and does not edit it; a question that looks wrong to T8 is recorded as a doubt under
  R6a, not rewritten.

Its prompt, fixed here:

```
Below are rows from a frozen key-point ledger. Each states a decision, interface, constraint or
scope boundary that a spec was supposed to carry.

For each row, write ONE question that asks what a spec says about that row's SUBJECT, with the
row's ANSWER removed. The question must be answerable only by someone who has the answer, and
must not contain the answer itself — no exact name, bound or exclusion that the row supplies.

Example shape, for a row reading "The picker has three levels: project -> config -> version":
    What does this spec say about how the picker's browsing hierarchy is structured?
Not: "What does this spec say about the picker's three levels?" -- that leaks the answer.

--- BEGIN ROWS ---
<id>: <row item text>
--- END ROWS ---

Return exactly one line and nothing else, a JSON array of one object per row in order:
[{"id":"<id>","question":"<text>"}]
```

**`recovered` is defined here, because it is a fatal criterion and an undefined fatal criterion is
one invented at the moment it is least neutral** — T8 runs after T7 has unblinded:

> `recovered` is `true` when the probe's answer states the row's content **with its operative
> detail** — the exact name, the exact bound, the exact exclusion — so that an implementer holding
> only that answer would build what the row states, without stopping to ask. An answer on the right
> topic that drops the operative detail is `false`, and so is an answer that says the spec does not
> address it.

This is the same standard as item 9's `present` and item 8a's *establish*, applied to a probe's
answer instead of to a quoted span, and it is worded to match them deliberately.

**The probe prompt**, fixed here and identical across arms — it is handed **only**
`generated-2/<id>.md` and the 20 questions, never the ledger, because being handed the answer is the
whole failure this test exists to catch:

```
Below is a spec. Read it, then answer the questions that follow using ONLY what it says.

--- BEGIN SPEC ---
<the contents of generated-2/<id>.md>
--- END SPEC ---

--- BEGIN QUESTIONS ---
<n>. What does this spec say about <the row's subject, with the row's answer removed>?
--- END QUESTIONS ---

Answer each question in your own words, from the spec alone. Do not guess, do not fill gaps
from what you know about software, and do not consult any other file. If the spec does not
address a question, say exactly: "the spec does not address this".

Return exactly one line and nothing else, a JSON array of one object per question in order:
[{"n":1,"answer":"<your answer>"}]
```

**The adjudicator prompt**, likewise fixed — it is handed the 20 `(ledger row, probe answer)` pairs
and **nothing else: no spec, no arm label, no other row**:

```
For each numbered pair below you are given a key-point row from a frozen ledger, and an answer
someone gave after reading a document you have not seen. Decide, for each pair independently:

    does the answer state this row, with its operative detail?

"Operative detail" is the exact name, the exact bound, the exact exclusion. An answer on the
right topic that drops it is a no, and so is an answer that says the document does not address
the point.

You are not judging the document, which you have not seen, and not judging whether the row is
a good row. Only whether this answer states this row.

--- BEGIN PAIRS ---
<n>. ROW: <ledger row item text>
    ANSWER: <the probe's answer>
--- END PAIRS ---

Return exactly one line and nothing else, a JSON array of one object per pair in order:
[{"n":1,"recovered":true},{"n":2,"recovered":false}]
```

The phase agent records both and never overwrites an adjudication it disagrees with — a disagreement
is recorded under R6a.

Verdicts land in `docs/skill-evidence/spec-length/transmission-2/<id>.json`:

```json
{
  "spec_id": "7f3a1c",
  "answers": [ { "id": "tui-dc-picker-03", "recovered": true, "answer": "<the agent's own words>" } ]
}
```

`answers` covers exactly the 20 sampled ids for that fixture, in order.

---

## 14a. The gap-finder and pairing adjudicator

**Inherited from `PROTOCOL.md` item 14a with the path substitutions of item 15 and nothing else** —
`gaps/` becomes `gaps-2/` and `generated/` becomes `generated-2/`. Both prompts, the `A`/`B` rule and
the join are its own. It runs under `R7` only. Its constraint on item 6 — that no property of an id
may be a function of the arm it is assigned to — is restated in item 6 and checked by
`spec_length_2_id_assignment_does_not_track_the_arm`.

Numbered out of sequence for the same reason 8a is: it was added during T2's own review, and items
1–14 are cited by number elsewhere. **It exists because `R7` makes a T9 gap fatal, and T9's prompts
were pinned nowhere** — not here, and only as prose in the plan. (T8's were incomplete for the same
reason and in the same review round; item 14 now carries them, so **every fatal instrument in this
run is pinned in this file**.) T9 runs after T7 has unblinded, so whoever writes its prompts then knows which arm is
which and authored two of the three. An instrument authored after the results are visible is the
thing this file exists to prevent, and R7's fatality is what made it worth fixing rather than noting.

**Scope, mirroring T8 exactly:** per surviving candidate arm, 3 fixtures × **sample 1 only** = 3
gap-finder runs + 3 pairing-adjudicator runs. Output is
`docs/skill-evidence/spec-length/gaps-2/<id>.md`, `<id>` being the **candidate's** generation id.

**The gap-finder** (foreground, blind) is handed the generated spec and the fixture's task line —
**never the fixture spec, never the ledger, never the control's generated spec**, since with the
control in hand it would be answering by comparison instead of by reading:

```
You are about to implement this task:

    <TASK LINE for the fixture, from item 4>

The only thing you have been given is the spec below. Read it as the implementer: assume it is
all you get, and that nobody is available to answer follow-up questions.

--- BEGIN SPEC ---
<the contents of generated-2/<id>.md>
--- END SPEC ---

List every question you would still have to ask before you could build this. A question counts
only if the spec does not answer it and you could not proceed without the answer; do not list
things you would simply decide yourself, and do not list questions about how to implement a
decision the spec already made.

Return exactly one line and nothing else, a JSON array of question strings in the order you
would need them answered:
["<question>", "<question>"]
```

**The pairing adjudicator** (foreground, blind to which spec is which) is handed that question list
and **both** generated specs relabelled `A` and `B`, with the arm identities withheld:

```
Below are two specs, A and B, written for the same task, and a list of questions an implementer
had after reading one of them. You are not told which spec the questions came from, and you are
not being asked which spec is better.

For each question, answer twice, independently: does spec A answer it, and does spec B answer
it? "Answers it" means an implementer reading that spec would not need to ask — a topic
mentioned without the operative detail does not count.

--- BEGIN SPEC A ---
<contents>
--- END SPEC A ---
--- BEGIN SPEC B ---
<contents>
--- END SPEC B ---

--- BEGIN QUESTIONS ---
<n>. <question>
--- END QUESTIONS ---

Return exactly one line and nothing else, a JSON array of one object per question in order:
[{"n":1,"a_answers":true,"b_answers":false}]
```

**Which spec is `A` is fixed by the ids, not chosen**, so position cannot carry a hint and nobody
picks it once the answers are visible: the two ids are compared as lowercase hex strings, and **the
lexicographically smaller id is `A`**. That guarantee rests on T4's assignment being independent of
the ids' values, so it is pinned here as a constraint on item 6: **T4 may not use any property of an
id — its value, its lexicographic rank, or its position in the pool listing — as a function of the
arm it is assigned to.** An assignment that gave one arm the smaller id in all three fixtures would
put that arm in position `A` every time and hand the adjudicator back the correlation this rule
removes. The control generation paired against is `S1`'s generation
for the **same fixture and same sample** — which exists whether or not `S1` itself cleared R2,
because the pairing reads the spec, not the verdict.

**The join is the phase agent's, and only afterwards.** A question the **control answers and the
candidate does not** is a **gap**, and a gap is fatal to that arm under R7. A question neither
answers is not a gap; a question the candidate answers and the control does not is recorded as a
point in the candidate's favour and changes no verdict.

`gaps-2/<id>.md` carries, in this order: the arm, fixture and sample; the gap-finder's questions
verbatim; the adjudicator's per-question `a_answers`/`b_answers` with the `A`/`B` labels resolved to
arm names **after** the join; the resulting gap list; and any disagreement recorded under R6a. As
everywhere else in this run, a disagreement with an adjudication is **recorded, never overwritten**.

---

## 15. The relationship to `PROTOCOL.md`

**`PROTOCOL-2.md` supersedes `PROTOCOL.md` for this run only.** `PROTOCOL.md` governs the first
attempt, which is closed; this file governs the second. Neither reaches into the other.

**`PROTOCOL.md` is not edited, and neither is any other first-attempt artifact.** The untouchable set
is `PROTOCOL.md`, `FREEZE.md`, `ledger/`, `fixtures/`, `arms/spec-length/*` including `S0.md`,
`generated/`, `invalidated/`, `RESULTS.md` and `blind-map.json`. Every task of this run ends by
checking that its own diff names none of them.

**`RESULTS.md`'s null stands and stays true.** It records that the first attempt's instrument left
retention undefined, and nothing here revises that; this file is a second attempt, not a correction
of the first attempt's record. `RESULTS-2.md` is a **new** file; `RESULTS.md` is not extended.

**`RESULTS.md` §7.4's reversible judgement is moot, and the five files are deliberately not re-run.**
§7.4 records five verdict files left un-re-run after `4a73ef`'s `R6` re-run failed, and flags that
judgement as reversible by a later task. It is moot here because **no file is invalidated by tier 2
any more** (item 8a), so there is nothing for a re-run to restore. The five stay as they are, in
`invalidated/`, and this sentence is the deliberate statement of that rather than an omission.

### Every place this file departs from `PROTOCOL.md`, in one list

Nothing below is a silent rewording. Where an inherited item could not be carried across unchanged,
the substitution is named here.

| # | what changed | why |
|---|---|---|
| 1 | **Path substitutions**, applied to items 5, 8, 10, 14 and 14a: `generated/` → `generated-2/`, `retention/` → `retention-2/`, `blind-map.json` → `blind-map-2.json`, `transmission/` → `transmission-2/`, `gaps/` → `gaps-2/`. **And a reading rule for the inherited rules of item 12, which were not rewritten:** where `R5a`, `R6a` or any other inherited rule says `RESULTS.md`, it means **`RESULTS-2.md`** for this run. `PROTOCOL.md`'s own `RESULTS.md` is not written to by this run at all. | A v2 record must be unmistakable for a v1 one. The rules of item 12 are inherited **verbatim** and checked as such, so the substitution is stated here rather than made in the text. |
| 2 | **Item 1's limitation 7 is replaced**, not inherited. The superseded text is quoted in full below. | It is the one limitation the redesign falsified: it says a caught row invalidates all 91, which item 8a no longer does. Inheriting it verbatim would have put a false statement in a frozen file. |
| 3 | **Item 1 gains limitations 8 through 13** — six additions carried from `spec.md` §6. | The redesign introduces costs the first attempt did not have: tier 3 sees the spec, `R8` is post-hoc, the overturn rate is an extrapolation beyond the sampled rows. |
| 4 | **The three windows are re-pinned.** `PROTOCOL.md`'s boundaries name tasks of the first attempt's decomposition ("before T3's first commit", "until T4 dispatches its first probe"). This run's boundaries are **the first calibration probe of item 12a** and **the first probe of item 5**. | The rule is inherited unchanged in substance; only the events it names are re-pointed at events this run actually has. Re-using task names would have made the windows unfalsifiable. |
| 5 | **`R2` is inherited in substance but not verbatim**: its denominator is the discriminating set, resolved by `R8`. | Item 12's `R2` states the reason in full. Without it `R8` is inert. |
| 6 | **`R6` narrows**: *"a verdict fails item 8a's relevance adjudication"* is removed from its closed re-run list. | Nothing is invalidated by tier 2 any more. Removing a re-run trigger takes away a licence; it does not grant one. |
| 7 | **`R8` is added.** | `spec.md` §3.2, reviewer turn 1, `q_r8` = adopt. Its denominator is the six generations of the row's own fixture, which is a clarification of `spec.md` §3.2's *"all 18"* wording; item 12's `R8` records the reason. |
| 8 | **Item 8's span cap rises from 3 to 5, and spans must be self-contained.** Item 10's template carries both. | `skill-stickiness-65` needs four citations against a three-span cap; the clipped spans of `skill-stickiness-55` are the self-containment case. Both are on disk. |
| 9 | **Item 8a's prompt gains one substitution**, *"one to three spans"* → *"one to five spans"*, following item 8's cap. **Its `SPANS:` placeholder line is left at three**, deliberately, because it is a join format and not a cap. | Any further edit to a frozen prompt would be an unpinned change. |
| 10 | **Item 8a's disposition changes** — an `establishes: false` flags a row for tier 3 instead of invalidating the verdict file — and **item 8b is new.** | This is the redesign. `RESULTS.md` §7 is the record of what the old disposition produced. |
| 11 | **Item 9 gains a second blockquote** (the fixture-internal-cross-reference clarification). The inherited blockquote is unchanged. | `spec.md` §3.3. Pre-registered before any new measurement, applied identically to every arm. |
| 12 | **Item 6's pool is new and disjoint**, and item 7 adds the no-salt-field procedure. **Item 7a (`fixture-map-2.json`) is new.** | `q_regen` = regenerate; and the scoring chain needs id→fixture without id→arm. |
| 13 | **Item 12a is new.** | `spec.md` §3.5. |
| 14 | **Item 13 is discharged**, not deleted. | No arm is authored this run; the numbering is kept so the two files diff item by item. |
| 15 | **Item 11 gains three additions** (tier 3 sees the spec; the subagent-cwd / `git log` channel, re-disclosed; the first attempt's recoverable arm map, which bears on item 12a). | Every channel this run knows about is written down. `RESULTS.md` §7.6 deviation 7 and §5 deviations 4 and 5 are the inherited ones. |
| 16 | **Seven item *headings* are reworded**: 7 (`blind-map.json` → `blind-map-2.json`), 8 and 10 (both gain *"tier-1"*), 8a (gains *"Tier 2"* and *"redesigned"*), 13 (*"binding on T3"* → *"inherited, and discharged"*), 14 (*"The T8 transmission sample"* → *"The transmission sample"*) and 14a (*"T9's instruments"* → *"The gap-finder and pairing adjudicator"*). **No item number changes and no body text changes with them.** | Headings that name a first-attempt task id would be false here — this run has no T8 or T9 in those roles (see the role table below). The numbers are what cross-references bind to, and they are untouched. |

**The first attempt's task names appear inside the rules this file inherits verbatim, and they are
not this run's task names.** `R3a`, `R4`, `R4a`, `R5a` and `R7` say T3, T4, T7, T8, T9 and T10. Read
them as roles, not as task ids:

| as written in an inherited rule | the role it means, in this run |
|---|---|
| T3 | whoever authors and freezes the arms — **discharged**; the arms are already frozen (item 2) |
| T4 | whoever dispatches the 18 probes of item 5 and writes `blind-map-2.json` |
| T7 | whoever unblinds, applies the gate, and writes `RESULTS-2.md` |
| T8 | whoever runs item 14's transmission test |
| T9 | whoever runs item 14a's gap test |
| T10 | whoever writes `docs/skill-evidence/spec-length.md`'s second section |

Nothing turns on the mapping except readability: each rule's own sentence says what the role does.

### The superseded text of limitation 7, quoted in full

`PROTOCOL.md` item 1's limitation 7 reads as follows. **It is quoted here so that the substitution is
visible rather than merely asserted, and it is NOT operative in this file** — item 1's limitation 7
above is what governs. It is fenced rather than blockquoted so that the bytes are exactly
`PROTOCOL.md`'s and a reader can diff the two by eye.

```
7. **Only 20% of `present: true` rows are relevance-adjudicated.** Item 8a samples every 5th such
   row; the other 80% are held to the mechanical check alone, which proves a cited span is really
   *in* the spec but not that it is *about* the row. So a scorer citing real-but-irrelevant text has
   a per-row chance of being caught, not a certainty — mitigated, not closed. Two things blunt it:
   whole-file invalidation means one caught row re-runs all 91 (or 84, or 55), so the expected cost
   of padding is high; and under R2 a *false* `present: true` can only ever inflate retention, so
   the residual risk runs toward a **false pass**, never toward a false elimination. Given that the
   likely outcome is a null (limitation 6), the risk that actually matters here is the one that is
   least likely to bite. Say so in the write-up rather than describing the adjudication as complete.
```

**What that text got wrong, precisely.** Its two mitigations were *"whole-file invalidation means one
caught row re-runs all 91 … so the expected cost of padding is high"* and *"under R2 a false
`present: true` can only ever inflate retention"*. The **first no longer exists**: item 8a invalidates
nothing. The second survives and is carried into the replacement. The replacement therefore says the
residual risk runs **harder** toward a false pass than it did in the first attempt, and names the two
things that bound it without closing it.

### What this file inherits verbatim, and what proves it

`cli/tests/skills_valid.rs::spec_length_2_protocol_inherits_its_verbatim_blocks` extracts the
following from `PROTOCOL.md` and requires each to appear in this file byte-for-byte:

- **item 1's limitations 1 through 6** — limitation 7 is excluded by name, for the reason in row 2 of
  the table above, and its superseded text is quoted in the fenced block immediately above so that it
  is still checkable against `PROTOCOL.md`;
- **item 4's three task lines**;
- **item 9's `present` blockquote** — the first of item 9's two;
- **the text of `R1`, `R1a`, `R3`, `R3a`, `R4`, `R4a`, `R5`, `R5a`, `R6a` and `R7`.** `R2` is excluded
  by name (row 5), and so are `R6` (row 6) and `R8` (row 7), which is new.

A near-match fails that check on purpose. If a block below drifts, **`PROTOCOL.md` is the authority
and this file is what is wrong** — `PROTOCOL.md` is a first-attempt artifact and is never edited to
make a check here pass.
