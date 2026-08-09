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

**The scorer was blind; the phase agent was not.** See deviation 1 below.

**The arm is deliberately not named here.** It is in `blind-map.json` and T7 reads it there.

---

## 5. Deviations recorded by T4

Three, none of them a `PROTOCOL.md` edit. **T10 repeats these alongside the four in `PROTOCOL.md`
item 2.**

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

---

## 6. What T7 still owes this file

Left here so it is not rediscovered: the per-generation table with its **arm** column, the per-arm
retention under `R1` and per-generation counts under `R1a`, the dropped-row list per arm, the gate
verdict under `R2`, the length comparison under `R3`/`R3a` labelled *"provisional — T8 and T9 have
not run"*, the instrument reading under `R4` beside the feasibility reading above, and the re-run
log under `R6` with any doubts under `R6a`. **`R4a` and `R5a` bind T7 and are in neither of the
plan's T7 or T10 rule lists** — `PROTOCOL.md` item 12 is the complete set.
