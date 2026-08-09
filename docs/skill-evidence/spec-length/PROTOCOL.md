# PROTOCOL — pre-registration for the spec-length A/B

**Run:** `spec-length-ab` · **Branch:** `drovr/spec-length-ab` · **Written by T2, 2026-08-08.**

This file is the **pre-registration**. It writes down every rule that decides the outcome, and it
is committed **before any candidate arm text exists and before any spec is generated or scored** —
that ordering is the whole claim, and it is checkable: `git log` must show this file's commit
preceding the commits that introduce `docs/skill-evidence/arms/spec-length/S2.md` and `S3.md`, and
preceding the generation commit. No test parses this file. Its force is the ordering, plus the fact
that T10's write-up has to be checkable against it line by line.

**This file is authoritative over the plan on every rule it states.** The plan
(`~/.local/share/drovr/runs/spec-length-ab/plan.md`) decomposed the work and named most of these
rules first, but it is a working document that is not itself pre-registered, and its T7 and T10
sections enumerate the rules **by name in prose** — a list that does not include `R4a` or `R5a`,
both added here during T2's own review. **T7 and T10 apply the complete rule set in item 12, not
the subset the plan's prose happens to name.** Where the two differ on a rule, this file governs
and the difference is recorded in `RESULTS.md`. This clause exists because a literal executor of
the plan's T10 branch could otherwise paste a winning candidate and report an unqualified *"shorter
without loss"* in exactly the case `R4a` was written to qualify.

**What is protected: every item in this file.** The **governed items** are **items 1 through 14a,
all sixteen, without exception**. Item 12 is not special and is not the boundary: R1 and R2 are
*defined in terms of* item 9's `present` test, item 8's schema, item 8a's sample and item 10's
instrument; the id pool (item 6) and the probe template (item 5) determine what is measured at all;
and item 11 governs what may be claimed about the result. Loosening any of them moves the outcome as
surely as rewriting R2 would. This is stated as "everything" rather than as a list because an
earlier draft of this clause protected item 12 alone, and the next draft protected a nine-item
subset that still left the probe template and the id pool outside it — each time, the carve-out was
the bug.

**What may change, and when.** Three windows, and there is no fourth:

1. **Before T3's first commit** — while no arm text and no generated spec exist. Corrections are
   legitimate and need no log entry beyond the revision table below. **This is the only window in
   which a rule may be added, and the only one in which a governed item may be weakened.**
2. **From T3's first commit until T4 dispatches its first probe** — arm text exists but no result
   does. Corrections are still legitimate, because nothing has been measured, but **no governed item
   may be weakened — only clarified**, and every edit is logged in
   `docs/skill-evidence/spec-length/RESULTS.md` with its reason and its commit.
3. **After T4 dispatches its first probe** — **no governed item may be revised at all**, and any
   edit to this file whatsoever is a protocol deviation, logged in `RESULTS.md` with its reason and
   its commit.

A rule chosen once a result is visible is not a rule; it is the result wearing a rule's clothes.

**Every edit in every window appends a row to the revision table below**, in the same commit that
makes the edit. Windows 2 and 3 log to `RESULTS.md` **as well**, not instead: `RESULTS.md` does not
exist until T7, so a reader before then would otherwise have no record at all, and a reader after it
should not have to open a second file to learn that this one moved.

**This file was revised four times after its first commit, all inside window 1**, and says so here
rather than leaving `git log -p` as the only witness. Count the rows below `7cfd07a`: that number
and this sentence must always agree, and both must agree with
`git log --oneline -- docs/skill-evidence/spec-length/PROTOCOL.md`.

| commit | what changed |
|---|---|
| `7cfd07a` | first commit — items 1–14 plus 8a; rules `R1`, `R1a`, `R2`, `R3`, `R3a`, `R4`, `R5`, `R6`, `R6a`, `R7`. |
| `bdd6622` | `R4`'s row-judgement count corrected from 1380 to **460** per arm (1380 is the figure across all three arms). |
| `3dd0dcc` | **`R4a` and `R5a` added** — neither was in the first commit. `R1`'s primary sentence reworded (the original, read literally, required generations that never judged a row to retain it). Item 8a's sample offset pinned. |
| `6e33620` | item 3's "233" miscount explanation corrected to name the exact command that produces it; `R4a`'s cross-reference to `R5` loosened; item 8a's "20%" qualified against `floor(n/5)`; the amendment windows split from two to three; this table added. No rule added or weakened. |
| `5043dbf` | **`14a` added** — T9's gap-finder and pairing-adjudicator prompts and the `gaps/` schema, which `R7` makes fatal but which no file pinned. **`R3a` gained its post-T8/T9 fallback**, which was undefined. The protected set widened from item 12 alone to a nine-item list. This table made append-only and forward-looking. |
| the commit that added this row | **Item 14 gained a definition of `recovered` and literal probe and adjudicator prompts** — T8's fatal criterion was named but never defined, and its prompts were the only instrument prompts still unwritten, which made `5043dbf`'s claim that T9 was "the only" unpinned instrument false. **`R3a`'s fallback gained step 2**, the check against `S1`'s mean, without which it could crown a candidate the control had beaten. The protected set widened again, from nine items to **all sixteen**. Revision count corrected from three to four. |

**This file does not restate the freeze.** `docs/skill-evidence/spec-length/FREEZE.md` is the hash
record and `docs/skill-evidence/arms/MANIFEST.md` is the provenance record; both are authoritative
and neither is copied here.

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
7. **Only 20% of `present: true` rows are relevance-adjudicated.** Item 8a samples every 5th such
   row; the other 80% are held to the mechanical check alone, which proves a cited span is really
   *in* the spec but not that it is *about* the row. So a scorer citing real-but-irrelevant text has
   a per-row chance of being caught, not a certainty — mitigated, not closed. Two things blunt it:
   whole-file invalidation means one caught row re-runs all 91 (or 84, or 55), so the expected cost
   of padding is high; and under R2 a *false* `present: true` can only ever inflate retention, so
   the residual risk runs toward a **false pass**, never toward a false elimination. Given that the
   likely outcome is a null (limitation 6), the risk that actually matters here is the one that is
   least likely to bite. Say so in the write-up rather than describing the adjudication as complete.

---

## 2. The arms

| arm | role | path | `git hash-object --no-filters` | size |
|---|---|---|---|---|
| `S1` | **control**, frozen by T1 | `docs/skill-evidence/arms/spec-length/S1.md` | `bb0d5cdcf2903e9d47e705820911a2464c73ab22` | 15 lines / 191 words / 1151 bytes |
| `S2` | candidate — a moderate trim of `S1` | `docs/skill-evidence/arms/spec-length/S2.md` | recorded by T3 in `FREEZE.md` | recorded by T3 |
| `S3` | candidate — the aggressive minimum | `docs/skill-evidence/arms/spec-length/S3.md` | recorded by T3 in `FREEZE.md` | recorded by T3 |

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

Three, all recorded here because the artifacts that state them are frozen or append-only and so
cannot be corrected in place. T10 repeats all three in the write-up.

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

Arm-invariant except for the two substitutions. T4 dispatches 18 of these, foreground,
`subagent_type: general-purpose`, `model: sonnet`.

```
You are the brainstorm phase of a drovr run for this task:

    <TASK LINE for the fixture>

Your investigation and your interview with the human are COMPLETE. Everything that was
decided is in <abs path to fixtures/<fixture>.spec.md>. Read it. It is your notes, not
your output.

Write the spec to <abs path to generated/<id>.md>, following this instruction exactly.
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

Eighteen opaque 6-hex tokens. **T4 assigns them to (arm, fixture, sample) triples in an order of its
own choosing and records the assignment only in `blind-map.json`**, so the listing order here maps
to nothing and cannot be read as an assignment.

```
87e5a5  88fb62  4a73ef  350ba8  80d9a2  b0d2fc
3cfbc8  37e173  f8729b  4fdca8  bbd141  db3e2d
14855d  28529c  d25798  fe9c04  1c6368  aa3199
```

---

## 7. `blind-map.json` schema

`docs/skill-evidence/spec-length/blind-map.json`, written by T4 **before any scoring**, committed
before the first scorer is dispatched. **Never shown to a scorer or an adjudicator**, and not read
by T5 or T6 at all — those tasks depend on its existence, not its contents.

```json
{ "7f3a1c": { "arm": "S1", "fixture": "tui-dc-picker", "sample": 1 } }
```

Eighteen entries, covering each (arm, fixture, sample) triple **exactly once**. It is joined to the
verdicts **only after every one of the 18 verdicts is recorded** (T7).

---

## 8. The retention verdict schema

`docs/skill-evidence/spec-length/retention/<id>.json`, one file per generated spec, assembled by the
phase agent from its scorer shards:

```json
{
  "spec_id": "7f3a1c",
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

**`quotes` is an array of 1–3 spans, not one string, and that is a fix for a real failure mode.**
Compression reorders: a row's operative detail can end up split across two non-adjacent sentences —
the name in one place, the bound in another. A single-span schema would force the scorer to either
cite a fragment that does not substantiate the row or mark it absent when it is plainly present,
biasing every arm toward false negatives on exactly the reorganised prose a shorter instruction
produces. So: `present: true` requires **1–3 non-empty spans, each a verbatim substring of the
generated spec, which together establish the row**. `present: false` requires `[]`.

Every span is lifted from the **generated spec**, never from the ledger. That prevents a scorer
inventing text; **it does not by itself prevent a scorer citing real-but-irrelevant text** — item 8a
is what covers that, and the two must not be confused.

**No span may be cited for more than one row within a verdict file.** A near-boilerplate sentence
pressed into service for six rows is the cheapest way to fake retention.

T5 lands `cli/tests/skills_valid.rs::spec_length_retention_verdicts_are_complete_and_quoted`, which
enforces every rule in this item over the immediate `*.json` children of `retention/`. It asserts
**well-formedness, never judgement** — nothing in it can say a verdict is *right*. **An empty
directory is a pass**, which is the correct state before T5.

---

## 8a. The relevance adjudication pass

The answer to *"the schema in item 8 can be satisfied while being wrong."* It is numbered out of
sequence because it was added during plan review and later tasks cite item numbers.

For each verdict file, an **independent blind adjudicator** subagent is given a pre-registered **20%
sample of that file's `present: true` rows** as `(ledger row text, the cited spans)` pairs, **and
nothing else: not the generated spec, not the arm, not the other rows**. Because it never sees the
spec, it cannot be swayed by how good the spec looks.

**The sample is fully determined, offset included, so two dispatches cannot select different rows.**
Take the file's `present: true` rows in ledger order and number them 1, 2, 3, … *among themselves*
(this index is over `present: true` rows only, not over ledger rows). The sample is **every row whose
index is divisible by 5** — the 5th, 10th, 15th, and so on. That is `floor(n/5)` rows out of `n`.

**"20%" is the nominal figure, and `floor(n/5)/n` is the real one** — they coincide only when `n` is
a multiple of 5, and otherwise the sample is slightly *under* a fifth (`n = 9` gives 1 row, 11%).
The mechanics above are what binds; the round number is shorthand, including where limitation 7 of
item 1 uses it. Do not adjust the stride or the offset to hit a true 20%. If
a verdict has fewer than five `present: true` rows the sample is **empty**: record that in
`RESULTS.md` against the verdict id and do **not** substitute a different sample, a different offset
or a smaller stride to manufacture one.

Its prompt, fixed here and identical for every verdict:

```
For each numbered pair below you are given a key-point row from a frozen ledger, and one to
three spans of text quoted from a document. Decide, for each pair independently:

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

**Any `establishes: false` invalidates the entire verdict file**, which is re-run whole under R6 —
never patched. **Only 20% of `present: true` rows are adjudicated**; that is limitation 7 of item 1,
and the pass is a mitigation, not a closure.

---

## 9. The `present` definition

Pre-registered, and handed to every scorer verbatim:

> `present` is `true` when the row's content is recoverable **and actionable** from the generated
> spec alone: an implementer reading only this spec would build what the row states, without
> stopping to ask. A paraphrase counts, and may be evidenced by up to three spans. A mention that
> drops the row's operative detail — the exact name, the exact bound, the exact exclusion — does
> not.

---

## 10. The scorer prompt template, and the sharding rule

**The scorer is handed only** the generated spec file, its fixture's ledger rows for its shard, and
the item-8 schema plus the item-9 definition. **Never `blind-map.json`, never
`docs/skill-evidence/arms/`, never this file, never another generated spec, never another arm's
output.**

**The sharding rule, because a 91-row JSON reply is where a scorer truncates.** A scorer is given
**at most 60 ledger rows**. Shard boundaries are fixed here and are **never adjusted per arm**:

| ledger | rows | shards |
|---|---|---|
| `skill-stickiness` | 91 | `-01`–`-46`, `-47`–`-91` |
| `tiered-review` | 84 | `-01`–`-42`, `-43`–`-84` |
| `tui-dc-picker` | 55 | one shard, `-01`–`-55` |

Each shard writes `docs/skill-evidence/spec-length/retention/parts/<id>-<k>.json`, `k` being the
1-based shard number. **A shard file has the same closed shape as item 8** — `spec_id`, `ledger`,
`rows` — with `rows` covering exactly its own shard's ids, in ledger order. The phase agent then
**concatenates the shards' `rows` into `retention/<id>.json`** in ledger order; item 8's
completeness rule is what proves the concatenation lost nothing. Shards under `parts/` are working
material and are deliberately unchecked by any test; the assembled file is the evidence.

**Concatenation is assembly, not repair.** A shard that is malformed, fails the mechanical check, or
fails 8a adjudication is **re-run whole** and never patched.

The template:

```
You are scoring one document against a frozen key-point ledger. Read the document in full:

    <abs path to generated/<id>.md>

Below are <k> ledger rows. For each row decide `present` under this definition, which is the
governing definition and not a summary of one:

<item 9's definition, verbatim>

--- BEGIN LEDGER ROWS ---
| id | kind | item |
|---|---|---|
<the shard's rows, verbatim from the ledger>
--- END LEDGER ROWS ---

Write <abs path to retention/parts/<id>-<k>.json> containing exactly this object and nothing
else:

{ "spec_id": "<id>", "ledger": "<fixture>",
  "rows": [ { "id": "<row id>", "present": true, "quotes": ["<span>"] } ] }

Rules, all of them hard:
  - `rows` carries every id above, exactly once, in the order given. No extras.
  - `present: true` requires 1 to 3 non-empty spans in `quotes`, each an EXACT verbatim
    substring of the document, which together establish the row.
  - `present: false` requires `quotes: []`.
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

The scoring here is **label-blind, not arm-blind**, and the channel is **vocabulary and section
shape**: a generated spec's length, section headings and phrasing still correlate with its arm, and
an arm that says "decision record" can push that exact phrasing into every spec it generates.

**Do not substitute `skills/writing-skills/references/scoring-rubric.md`'s stated limitation for
this one.** That paragraph was written about a different leak channel — a `cites_section: true`
verdict identifying an armored transcript — and quoting it here would describe a channel this
experiment does not have while leaving the one it does have unstated. State the channel that
applies. **Do not describe this scoring as fully blind anywhere.**

---

## 12. The decision rules

**These decide the outcome. None of them may be revised after T4 dispatches its first probe.**

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
- **R2 — the gate.** An arm at **230/230** clears. **Any** drop eliminates. No partial credit, no
  weighting by `kind`.
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
  2. **The control still has to be beaten.** If `S1` cleared R2 and its mean is **lower than or
     equal to** the lowest surviving candidate's, then **no candidate beat the control**: ship
     nothing and record it, exactly as the sentence above says. The survivors clearing every gate
     does not by itself make one of them shorter than what already ships. (If `S1` did **not** clear
     R2, it is not in R3's compared set at all and R4a governs the comparison instead.)
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
- **R6 — no re-runs to chase a result.** One generation round per arm. A re-run is permitted only
  for a **protocol** failure, and the list is closed: the probe wrote no file; a verdict is
  malformed; a verdict fails the item-8 mechanical check; a verdict fails item 8a's relevance
  adjudication. Never for an unwanted outcome, and **every re-run is logged in `RESULTS.md` with its
  reason**.
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

**A null is a valid, publishable result.** *"No arm is shorter without loss"* answers the question
that was asked. It is pre-registered here as an outcome precisely so it cannot later be treated as a
failure to be re-run away, and so nobody weakens R2, R5 or R7 to avoid reporting it.

---

## 13. The candidate-authoring constraints, binding on T3

- **`S2` is a moderate trim of `S1`; `S3` is the aggressive minimum** — the shortest text that still
  states the three things `S1` asks for. Each arm's line and byte count is reported.
- **Both obey locked decision 6**: no "Open questions" section, no TBD, no "decided during
  implementation". An arm that reintroduces open questions is not shippable regardless of how it
  scores.
- **Generic text only.** An arm may not name a fixture, a ledger row, or any fixture-specific term.
  Checkable, and checked:
  `grep -iE 'skill-stickiness|tiered-review|tui-dc-picker|deploy config|fieldTree'` over both arm
  files must return nothing. Writing the ledger's topics into the instruction is the arm gaming its
  own rubric.
- **No arm may contain the string `questions.json`** —
  `cli/tests/skills_valid.rs::no_phase_prompt_mentions_questions_json` scans every phase prompt for
  it by substring, and every arm is a candidate for pasting into `brainstorm.md` at T10.
- **No arm may share an 8-word verbatim run with the superpowers corpus.**
  `cli/tests/skills_valid.rs::no_verbatim_overlap_with_superpowers` walks every `*.md` under
  `skills/` and does compare on this machine. Both candidates are trims of `S1`, which is itself
  superpowers-descended prose, so a trim that keeps a long shipped phrase intact is the likely way
  to trip it — and it would trip at **T10**, long after the arm was frozen and measured. T3 checks
  it at authoring time by staging the arm text at a scratch `skills/**/*.md` path, running that
  test, then deleting the scratch file. The fix is to rewrite the offending run; the check is never
  weakened.
- **`S1` is not edited and `brainstorm.md` is not edited.** T3 creates two files and appends four
  rows, in T1's two-commit order: arms in one commit, the `FREEZE.md` and `MANIFEST.md` rows naming
  that commit in the next.
- **Both arms are authored with this file already visible**, which is limitation 3 of item 1 and is
  an external-validity limit, not an internal one: the ledger they are graded against was frozen
  before either existed.

---

## 14. The T8 transmission sample

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
`generated/<id>.md` and the 20 questions, never the ledger, because being handed the answer is the
whole failure this test exists to catch:

```
Below is a spec. Read it, then answer the questions that follow using ONLY what it says.

--- BEGIN SPEC ---
<the contents of generated/<id>.md>
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

Verdicts land in `docs/skill-evidence/spec-length/transmission/<id>.json`:

```json
{
  "spec_id": "7f3a1c",
  "answers": [ { "id": "tui-dc-picker-03", "recovered": true, "answer": "<the agent's own words>" } ]
}
```

`answers` covers exactly the 20 sampled ids for that fixture, in order.

---

## 14a. T9's instruments

Numbered out of sequence for the same reason 8a is: it was added during T2's own review, and items
1–14 are cited by number elsewhere. **It exists because `R7` makes a T9 gap fatal, and T9's prompts
were pinned nowhere** — not here, and only as prose in the plan. (T8's were incomplete for the same
reason and in the same review round; item 14 now carries them, so **every fatal instrument in this
run is pinned in this file**.) T9 runs after T7 has unblinded, so whoever writes its prompts then knows which arm is
which and authored two of the three. An instrument authored after the results are visible is the
thing this file exists to prevent, and R7's fatality is what made it worth fixing rather than noting.

**Scope, mirroring T8 exactly:** per surviving candidate arm, 3 fixtures × **sample 1 only** = 3
gap-finder runs + 3 pairing-adjudicator runs. Output is
`docs/skill-evidence/spec-length/gaps/<id>.md`, `<id>` being the **candidate's** generation id.

**The gap-finder** (foreground, blind) is handed the generated spec and the fixture's task line —
**never the fixture spec, never the ledger, never the control's generated spec**, since with the
control in hand it would be answering by comparison instead of by reading:

```
You are about to implement this task:

    <TASK LINE for the fixture, from item 4>

The only thing you have been given is the spec below. Read it as the implementer: assume it is
all you get, and that nobody is available to answer follow-up questions.

--- BEGIN SPEC ---
<the contents of generated/<id>.md>
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

`gaps/<id>.md` carries, in this order: the arm, fixture and sample; the gap-finder's questions
verbatim; the adjudicator's per-question `a_answers`/`b_answers` with the `A`/`B` labels resolved to
arm names **after** the join; the resulting gap list; and any disagreement recorded under R6a. As
everywhere else in this run, a disagreement with an adjudication is **recorded, never overwritten**.
