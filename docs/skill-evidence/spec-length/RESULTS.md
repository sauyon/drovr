# RESULTS — the spec-length A/B

**Run:** `spec-length-ab` · **Branch:** `drovr/spec-length-ab`

**This file was opened by T4, not T7, and holds only what T4 owns: the raw measurements.**
`PROTOCOL.md`'s preamble anticipated T7 creating it, and the plan lists it as a T7 interface — but
the plan's own T4 procedure directs the generated specs' `wc` numbers and the feasibility reading
here, and `PROTOCOL.md`'s window-2 revisions were owed a log in a file that did not yet exist. T4
opening it resolves both. **T7 owns every analysis below the raw tables** — the per-arm unions under
`R1`, the gate verdict under `R2`, the length comparison under `R3`/`R3a`, the dropped-row lists, the
instrument reading under `R4`, and the re-run log under `R6`.

**No arm appears anywhere in this file.** The per-generation table below carries `id`, `fixture`,
`sample`, lines and bytes, and deliberately **not** the arm. `blind-map.json` holds the assignment
and is not read by T5 or T6 at all. An arm column here would unblind the scoring for anyone who
opened this file before T7, which is exactly the leak the map exists to prevent. **T7 adds the arm
column when it unblinds, and not before.**

**This file is never shown to a scorer or an adjudicator, on the same footing as
`blind-map.json`.** Withholding the arm column is not sufficient on its own: §3 tabulates all 18
lengths grouped by fixture, and **length is precisely the leak channel `PROTOCOL.md` item 11 names
as this experiment's real one**. A scorer holding this table can rank the six generations of its own
fixture and form a prior on the arm under exactly the hypothesis being tested. Item 10 already lists
what a scorer may be handed — the spec, its shard's ledger rows, the item-8 schema and the item-9
definition — and this file is not on it. That list is the rule; this paragraph exists so nobody
reaches for `RESULTS.md` thinking the arm column was the whole risk.

---

## 1. `PROTOCOL.md` revisions requiring a log here

`PROTOCOL.md`'s window 2 — from T3's first commit until T4 dispatched its first probe — permits a
governed item to be **clarified, never weakened**, and requires every edit to be logged here with its
reason and its commit. Two such revisions were made, both by T3, both inside window 2. This section
is that log; it is also recorded in `PROTOCOL.md`'s own revision table, which is where the
obligation was written down.

| commit | reason |
|---|---|
| `3096339409700082c6624c7fe28863f4375d8d8c` | Filled item 2's `S2`/`S3` size cells, which read "recorded by T3" and had nowhere to resolve to — item 13 requires each arm's line and byte count to be reported, and no column in `FREEZE.md` or `MANIFEST.md` can hold a size. Added a **fourth** deviation to item 2 (T3's in-place rewrite of `S3`'s `FREEZE.md` row); the section had said "Three, all recorded here", which would have told T10 its list was complete when it was not. Relabelled the `c041300` row with its real SHA. Item 2 **clarified, not weakened**. |
| `730120a74d1225d9fdec560853125a9b2dda1958` | Corrected the row above, which had claimed "no governed item touched". That was false: `PROTOCOL.md` line 23 defines the governed items as items 1–14a, **all sixteen**, and item 2 is one of them. Nothing about the edit changed — no rule, threshold or gate moved, and items 3–14a stayed byte-identical — only its description. Also corrected a stale "the three deviations" pointer in `FREEZE.md`'s breach section. |

**Window 2 closed when T4 dispatched its first probe.** From that moment `PROTOCOL.md` may not be
revised at all, and any edit whatsoever is a deviation. **T4 made none.**

---

## 2. The freeze, re-verified at the gate

`FREEZE.md:179-182` requires the freeze to be confirmed by hand **at the moment it is relied on**,
not merely at some point since. T4 re-ran `git hash-object --no-filters` over all ten rows — the
three fixtures, the three ledgers, `S0`, and `S1`/`S2`/`S3` — **before dispatching any probe**. All
ten matched. Nothing in the ledger, the fixtures or the arms moved.

---

## 3. Raw per-generation measurements

18 generations, one round (`R6`), taken as produced. `%` is lines as a fraction of that
generation's own fixture (`R3`'s reference points: `skill-stickiness` 791, `tiered-review` 463,
`tui-dc-picker` 414).

| id | fixture | sample | lines | bytes | % of fixture |
|---|---|---|---|---|---|
| `14855d` | tiered-review | 1 | 324 | 18724 | 70.0% |
| `1c6368` | tiered-review | 1 | 300 | 17670 | 64.8% |
| `28529c` | tui-dc-picker | 2 | 350 | 19423 | 84.5% |
| `350ba8` | tiered-review | 1 | 384 | 21980 | 82.9% |
| `37e173` | tui-dc-picker | 2 | 363 | 20934 | 87.7% |
| `3cfbc8` | tui-dc-picker | 1 | 339 | 18848 | 81.9% |
| `4a73ef` | skill-stickiness | 1 | 585 | 38270 | 74.0% |
| `4fdca8` | tui-dc-picker | 1 | 347 | 20405 | 83.8% |
| `80d9a2` | skill-stickiness | 2 | 621 | 38776 | 78.5% |
| `87e5a5` | skill-stickiness | 2 | 455 | 28238 | 57.5% |
| `88fb62` | tiered-review | 2 | 378 | 21559 | 81.6% |
| `aa3199` | skill-stickiness | 1 | 706 | 46801 | 89.3% |
| `b0d2fc` | tiered-review | 2 | 397 | 22709 | 85.7% |
| `bbd141` | tui-dc-picker | 2 | 197 | 11330 | 47.6% |
| `d25798` | skill-stickiness | 1 | 528 | 34055 | 66.8% |
| `db3e2d` | skill-stickiness | 2 | 693 | 45077 | 87.6% |
| `f8729b` | tui-dc-picker | 1 | 374 | 21619 | 90.3% |
| `fe9c04` | tiered-review | 2 | 345 | 19927 | 74.5% |

`lines` is `wc -l` and `bytes` is `wc -c` on `generated/<id>.md`. Every number re-derivable with
`wc -l -c docs/skill-evidence/spec-length/generated/*.md`.

### The copy check (`R5a`)

**No generation is flagged.** `R5a` flags any generation at **≥ 95%** of its fixture's length as
having substantially copied its input rather than compressed it. The highest reading is `f8729b` at
**90.3%**, and the lowest is `bbd141` at **47.6%**. Nothing crosses the threshold, so no generation
carries the "says nothing about the instruction" caveat `R5a` attaches — but the top of the range
sits close enough to it that T7 should re-derive rather than inherit this line.

---

## 4. The feasibility reading

> **Feasibility reading, n = 1. 54 of 55 rows present.** Generation `f8729b`, scored against the
> 55-row `tui-dc-picker` ledger by a single foreground scorer on `PROTOCOL.md` item 10's template —
> the same instrument T5 and T6 will use. The one row scored absent is `tui-dc-picker-01` (the
> explicit-`count` constraint). Verdict at `feasibility/f8729b-1.json`.

**What it is for.** `R4` pre-registers a universal null as the *likely* outcome: `R1` needs 460
row-judgements clean for one arm to pass, and at any realistic per-row fidelity the expected result
is that no arm clears. This reading costs one scorer run and says, before T5 and T6 spend 36 more,
whether 230/230 is reachable at all under D2. **It is recorded because it was taken, not because it
was reassuring** — the instruction to write it down either way is what makes it evidence.

**What it is not.** It is **n = 1** of 18, one fixture of three, and the smallest ledger of the
three. It is **not** a retention verdict: it has not been through item 8a's relevance adjudication,
it lives outside `retention/` precisely so nothing can mistake it for one, and no gate reads it.
Under `R5` it is **descriptive, never a pass** — and note that under `R2` a 54/55 generation would
**eliminate** its arm, since the gate is 230/230 with no partial credit.

**T5's verdict for `f8729b` governs, and supersedes this reading.** `f8729b` is a `tui-dc-picker`
generation, so T6 scores it again on the same instrument as part of the real measurement. When the
two disagree — on `tui-dc-picker-01` or on any other row — **the T6 verdict is the one that counts,
and this reading is superseded rather than reconciled with it.** The disagreement is recorded under
`R6a` against `f8729b` and carried into the write-up; it is not grounds for a re-run under `R6`,
which is a closed list this is not on. Pinned here because the reading was published *before* the
scoring, and a rule written afterwards to settle a conflict is a rule chosen with the answer visible.

**Note what a 54/55 would mean if it held.** Under `R1` the arm's dropped set is the union over its
six generations, and under `R2` any drop eliminates — so a single absent row on one generation is
enough to eliminate that arm at 229/230. This reading therefore already sketches an outcome for one
arm. It decides nothing: it is n=1, unadjudicated, and superseded by T6 per the paragraph above.

**The scorer was blind; the phase agent was not.** See deviation 1 below.

**The arm is deliberately not named here.** It is in `blind-map.json` and T7 reads it there.

---

## 5. Deviations recorded by T4

Six, none of them a `PROTOCOL.md` edit. **T10 repeats these alongside the four in `PROTOCOL.md`
item 2**, which makes ten in the write-up's complete list. Deviation 4 is the serious one and is an
**escalation, not a closed item**.

1. **The feasibility generation could not be chosen "without consulting `blind-map.json`", because
   the agent that chose it had just written that file.** The plan asks T4 to pick one
   `tui-dc-picker` generation while staying blind to its arm, and then to record the reading as
   *"arm unknown"*. That is not achievable as specified: the same task assigns all 18 ids and takes
   the reading, so the phase agent holds the whole map by construction. Recording it as *"arm
   unknown"* would have been a false label on a real number.

   **What was done instead, and why it is not a free hand.** The selection rule was fixed and
   written down *before* it was evaluated, and it is a function of the id alone: order the six
   `tui-dc-picker` ids by `sha256("<id>|spec-length-ab/T4/feasibility")` and take the first. It was
   run **once**; there was no second salt and no redraw. Reproducible:

   ```
   for id in 28529c 37e173 3cfbc8 4fdca8 bbd141 f8729b; do
     printf '%s  %s\n' "$(printf '%s|%s' "$id" "spec-length-ab/T4/feasibility" | sha256sum | cut -d' ' -f1)" "$id"
   done | sort | head -1
   ```

   **The scorer itself was fully blind** — it received the generated spec and the ledger rows, and
   nothing else. What is compromised is the *choice* of which generation to read, not the reading.
   The residual risk is that a phase agent holding the map could have chosen a generation it
   expected to score well; the fixed, pre-declared, single-shot rule is what bounds it, and this
   paragraph is the rest of the answer.

2. **`RESULTS.md` was opened by T4, not T7.** The plan lists it as a T7 interface while directing T4
   to write into it twice. See this file's own preamble. T7 extends it; nothing here is T7's to
   re-derive except as its verification requires.

3. **`feasibility/` is a new directory the plan did not name.** The plan says to score the
   feasibility generation "exactly as T5/T6 will", which would have put the file under
   `retention/parts/`. It is deliberately **not** there: a file in `retention/` is a verdict T7
   joins and `R2` gates on, and this one has had no item-8a adjudication. Keeping it in its own
   directory is what stops a feasibility reading from being counted as a retention verdict.
   `spec_length_retention_verdicts_are_complete_and_quoted` (T5) reads the immediate `*.json`
   children of `retention/` and will not see it.

4. **T4 published the id assignment a second time, in a commit message, and `PROTOCOL.md` item 6
   says it may be recorded *only* in `blind-map.json`. This is the most serious thing on this page
   and it cannot be undone.**

   **What happened.** Commit `b03cba02183fb0eaf3e3a9d31e2fb18b75c861d4`'s message states the
   derivation in full: the pool is ordered by `sha256("<id>|spec-length-ab/T4/draw-1")` ascending
   and assigned in that order to the triples in canonical arm-major order (`S1`, `S2`, `S3`; within
   an arm `skill-stickiness`, `tiered-review`, `tui-dc-picker`; within a fixture sample 1 then 2).
   That is a complete re-encoding of the map. T4's own review reconstructed all 18 cells from the
   commit message alone and got a byte-for-byte match.

   **Why it was written.** To make the draw *auditable* — so a reader could confirm the assignment
   was not steered toward an arm, rather than take T4's word. The reasoning was right and the
   channel was wrong: an auditor needs the derivation, and a scorer must not have it, so it belonged
   inside `blind-map.json` (or a sibling held to the same never-shown rule), not in `git log`.

   **The draw, since this is now the place it is recorded.** One attempt, salt
   `spec-length-ab/T4/draw-1`, no redraw. The declared acceptance criterion was item 14a's: at
   sample 1, across the three fixtures, neither `S1` nor a candidate may hold the lexicographically
   smaller id — position `A` in T9's pairing — in all three `S1`-vs-candidate pairings. Draw 1 came
   out **1–2 against one candidate and 1–2 against the other**, mixed in both, so the criterion was
   satisfied on the first attempt. **The redraw fallback was declared but never exercised, and that
   matters:** re-drawing *until* the arms come out balanced would make the assignment conditional on
   the arm–id correlation, which is itself a function of the arm and so the very thing item 14a
   forbids. Had it fired, the redraw would have been a deviation to record here, not a reset.
   `cli/tests/skills_valid.rs::spec_length_id_assignment_does_not_track_the_arm` pins the outcome.

   **What it costs.** The draw itself remains item-14a compliant: the hash ordering is genuinely
   independent of the arm, and no property of an id is a *function* of its arm. What is lost is the
   operational guarantee. For anyone holding the commit log, the arm is now computable from the id —
   so `blind-map.json` being withheld no longer withholds the assignment, and item 11's
   *"label-blind, not arm-blind"* understates the exposure for this run.

   **It is not repairable.** `FREEZE.md` forbids rewriting history on this branch, and T10 has to be
   able to check the ordering. Re-drawing under a fresh salt does not help either: the old id↔spec
   pairing is already in history under the old filenames, so a remap would leave two recoverable
   mappings instead of one. `R6` does not license regeneration for this — it is not a probe failure,
   and its re-run list is closed.

   **What is asked of the tasks after this one.**

   - **T5 and T6:** a scorer is handed the spec, its shard's ledger rows, and item 8/9's text —
     nothing else, and no reason to read the repository. Do not hand a scorer anything that invites
     it to consult `git log`, and do not paste the salt into a prompt or a commit message.
   - **T10:** report this as a stated limitation on the blinding, next to item 11's channel, and say
     plainly that a determined reader of this branch's history could have unblinded any generation.
   - **The driver:** whether this is severe enough to void the run and restart under an unpublished
     salt is **not T4's call**, and T4 has not made it. The run continues as specified; this entry
     is the escalation.

5. **The 18 generated files' mtimes were normalised to a single timestamp after they were written.**
   They had been written in three contiguous blocks of six with multi-minute gaps between them, and
   the blocks were the arms in order — `ls -l` recovered the exact three-way partition, which is
   the entire structure a between-arm comparison needs. Git does not track mtimes, so a fresh clone
   was never affected; but T5 and T6 score **in this worktree** and their scorer subagents can run
   `ls`. All 18 were set to `2026-08-08T00:00:00Z` with `touch -d`. **Content is untouched**: the
   `git hash-object` digest over all 18 files is identical before and after, and every `wc` number
   in §3 was taken before the change and re-verified after. Recorded so that uniform mtimes are not
   later misread as evidence that the specs were written in one batch.

6. **Every generated spec opens with a `# ` title, and item 5 says the file holds "only the spec
   body — no header".** All 18 begin with a markdown title naming the fixture's subject. Read
   literally that is non-compliant. It is **not** an arm channel: the title is byte-identical across
   all six generations of each fixture, so it carries no arm signal, and the fixture's subject is
   ordinary vocabulary throughout a spec about it. Nor is it the probes' fault — item 5's template
   never tells a probe this; the sentence sits in the surrounding commentary. And T4 could not have
   fixed it without editing the raw measurement, which is the one repair this run forbids. Recorded
   as a drafting gap in item 5 rather than smoothed over.

---

## 6. What T7 still owes this file

Left here so it is not rediscovered: the per-generation table with its **arm** column, the per-arm
retention under `R1` and per-generation counts under `R1a`, the dropped-row list per arm, the gate
verdict under `R2`, the length comparison under `R3`/`R3a` labelled *"provisional — T8 and T9 have
not run"*, the instrument reading under `R4` beside the feasibility reading above, and the re-run
log under `R6` with any doubts under `R6a`. **`R4a` and `R5a` bind T7 and are in neither of the
plan's T7 or T10 rule lists** — `PROTOCOL.md` item 12 is the complete set.
