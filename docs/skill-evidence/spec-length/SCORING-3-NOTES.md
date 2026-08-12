# Tier-1 scoring notes — third instrument

The running record of the tier-1 pass `PROTOCOL-3.md` governs. It is the **`R6` re-run log and the
`PROTOCOL-3.md` amendment log** until `RESULTS-2.md` exists, and unlike `SCORING-2-NOTES.md` it is
**not orphaned**: `spec_length_3_protocol_stops_moving_after_the_first_verdict` reads this file for
the SHA of every commit that amends `PROTOCOL-3.md`, so a run that deletes or empties it fails.

`SCORING-2-NOTES.md` remains the record of the **second** instrument's pass and is not edited here.
Nothing in this file re-interprets `retention-2/`.

---

## 1. The outcome, up front

**Eighteen of eighteen generations produced a filed verdict.** The second instrument filed 3 from the
12 it attempted; nine generations were missing data. There are now none.

| | second instrument (`PROTOCOL-2.md`) | third (`PROTOCOL-3.md`) |
|---|---|---|
| generations attempted | 12 | **18** |
| verdicts filed | **3** | **18** |
| attempts allowed per shard | 6 | 3 |
| attempts actually used | up to 6, cap exhausted on 9 ids | **1 for seventeen ids, 2 for one** |

**The cap was never binding.** `PROTOCOL-3.md` §5 offers three attempts per shard uniformly; the most
any id used is two. §2b's raised cap was the second instrument's response to the same problem and it
bought almost nothing — the difference here is the instrument, not the budget.

---

## 2. The dispatch log

**34 dispatches, 32 shard files, 30 of them live.** Every figure below is counted off the files on
disk, not off this section's own prose.

| | |
|---|---|
| shard dispatches, first pass | 30 — one per shard of all eighteen generations |
| dispatches that crashed mid-stream writing **no file** | **2** (`fd2c24-2` in the first pass, `fd2c24-1` in the re-run) |
| re-dispatches for a crash | 2 |
| `R6` re-runs for a class-A failure | 2 (`fd2c24`, both shards — see §3) |
| **total dispatches** | **34** |
| shard files on disk | **32** — 30 under `retention-3/parts/`, 2 under `parts/superseded/` |

**A dispatch that crashes without writing a file does not consume a scoring attempt**, and this is
`PROTOCOL-2.md`'s `R6` as `PROTOCOL-3.md` §3 inherits it: *"the probe wrote no file"* is a trigger
**separate** from a verdict failing the item-8 check. Both crashes were connection failures mid-response;
neither produced a verdict, so there was nothing for the check to fail. `SCORING-2-NOTES.md` §9.4
disclosed the same thing about the one verdict that passed under the second instrument, and the same
disclosure is owed here.

**Every dispatch was `sonnet`, `general-purpose`, foreground**, per `plan.md` §0's model pin.

### 2.1 Shard independence — hashed as they landed

`SCORING-2-NOTES.md` §9.6 found one shard byte-identical to its own earlier attempt among 150 files
and could not tell a verbatim re-emission from a cached completion, so "six attempts" meant six
**dispatches** rather than six independent samples. §9.5 item 7(b) told this pass to hash its shards
as they land, and `tools/assemble-retention-3.py` prints a SHA-256 for every shard it reads.

**All 32 shard files have distinct SHA-256s** — live and superseded together, checked over the whole
set at once. **That is not proof of independence** and is not claimed as such: it rules out the one
observable §9.6 found and nothing more. Until something stronger is shown, **an attempt is still a
dispatch, not an independent sample.**

---

## 3. The one `R6` re-run, and what it says about class A

**`fd2c24` failed class A on its first attempt and was re-run whole.** Two spans — for
`skill-stickiness-67` and `skill-stickiness-88`, both in shard 2 — were **not verbatim substrings of
the generated spec**. That is fabrication, the one thing `PROTOCOL-3.md` §3 keeps fatal, and it is
the shape a span takes when a scorer writes it from memory rather than copying it.

**Both shards were re-dispatched, not just the offending one.** `R6`'s remedy is a re-run of the
**verdict**, and the verdict is the assembled file; re-running only shard 2 would have filed a
verdict half of which was produced under a superseded attempt. The failing pair is preserved at
`retention-3/parts/superseded/fd2c24-{1,2}-attempt1.json`.

**This is the load-bearing observation of the whole redesign, and it cuts against the redesign's own
convenience.** The class split was not a way of making everything pass: the check still refused a
verdict, on the one ground that matters, on the first pass through eighteen generations. Seventeen
verdicts filed with class-B problems present and unhidden; the eighteenth was refused for inventing
text. **A gate that refuses fabrication and files everything else is doing the job item 8 was written
to do**; a gate that also discarded eighty-three sound rows because one span started mid-phrase was
not.

---

## 4. What filed, per generation

Derived from `retention-3/*.json` and from the class-B flags
`spec_length_3_class_b_flags_are_reported_and_never_fatal` prints. **Nothing in this table is typed
from a previous draft.**

| id | fixture | rows | `present` | spans | class-B flags |
|---|---|---|---|---|---|
| `66530f` | `skill-stickiness` | 91 | 90 | 129 | 4 |
| `6e7393` | `skill-stickiness` | 91 | 90 | 132 | 0 |
| `e085f2` | `skill-stickiness` | 91 | 89 | 121 | 4 |
| `e790f5` | `skill-stickiness` | 91 | 91 | 137 | 4 |
| `fd2c24` | `skill-stickiness` | 91 | 91 | 124 | 0 |
| `fe4059` | `skill-stickiness` | 91 | 90 | 118 | 8 |
| `054872` | `tiered-review` | 84 | 74 | 86 | 14 |
| `2c4295` | `tiered-review` | 84 | 81 | 94 | 0 |
| `2d2629` | `tiered-review` | 84 | 68 | 81 | 13 |
| `48527b` | `tiered-review` | 84 | 77 | 90 | 12 |
| `b49ff1` | `tiered-review` | 84 | 83 | 105 | 6 |
| `fd230c` | `tiered-review` | 84 | 83 | 106 | 4 |
| `031cc4` | `tui-dc-picker` | 55 | 53 | 59 | 0 |
| `08ae18` | `tui-dc-picker` | 55 | 54 | 62 | 2 |
| `26d7a2` | `tui-dc-picker` | 55 | 48 | 55 | 6 |
| `47173f` | `tui-dc-picker` | 55 | 55 | 64 | 0 |
| `a9fcf9` | `tui-dc-picker` | 55 | 55 | 61 | 0 |
| `b2b8cf` | `tui-dc-picker` | 55 | 54 | 62 | 0 |
| **total** | | **1380** | **1326** | **1686** | **77** |

**Read the totals off the table.** `SCORING-2-NOTES.md` §9.7 records that every figure that turned
out wrong in that file was a summary line typed by hand while the tables underneath were right.

**Present rates by fixture, and they have real spread:** `skill-stickiness` 89–91 of 91;
`tiered-review` 68–83 of 84; `tui-dc-picker` 48–55 of 55. **Four of the eighteen retain every row**
— `47173f`, `a9fcf9`, `e790f5`, `fd2c24`. **This is the first tier-1 output in either attempt that
looks like a measurement rather than a saturation or a blank**, and `SCORING-2-NOTES.md` §7 item 4 /
§9.5a item 10's 99.8%-ceiling doubt should be re-read against it — narrowed further, still not
withdrawn, because a fourth of the corpus at a perfect score is exactly what that doubt is about.

**None of this is evidence about spec length.** This file's author cannot see arms, has not opened
`blind-map-2.json`, and has made no comparison between generations beyond the arm-free table above.
Six generations of a fixture are three arms of two, and the spread could be the fixture, the ledger,
the arms or the scorers. **The join is T6's and T8's.**

---

## 5. The class-B flags — 77 of them, and none fatal

| kind | flagged row-instances |
|---|---|
| a span cited for more than one row | **62** |
| a span with no occurrence beginning and ending on a boundary | **15** |
| **total** | **77** |

**A shared span contributes two row-instances**, one per row that cites it, because
`PROTOCOL-3.md` §3 flags both: which of the two the span really establishes is a judgement and tier
3 is where judgement lives. So the 62 are 31 shared spans.

**Seven of eighteen verdicts carry no class-B flag at all**, and the eleven that do carry between 2
and 14. Under the second instrument every one of those eleven would have been discarded whole, and
the seven clean ones would have been the entire yield.

**The shared-span mode is still dominant and still unexplained**, exactly as `SCORING-2-NOTES.md`
§9.3 item 2 found for `tiered-review`. §8.3's ledger-entailment hypothesis was tested and withdrawn
there, and **this file does not re-form it**. `PROTOCOL-3.md` §3's disposition is deliberately
agnostic between "the scorer was lazy" and "the two rows are hard to evidence separately" — it routes
the row to a judge either way, which is why the pass did not need the question answered first.

**One caution `PROTOCOL-3.md` §1 records and this file repeats**: §8.3 tested its hypothesis against
the two *filed* verdicts, which by construction were the ones containing no shared span. That is a
selection effect and it makes the withdrawal weaker than its wording suggests. Nothing here rests on
it either way, and **the 31 shared spans are now on disk with their rows named**, so a later task can
test the question against a sample that was not selected for lacking them.

---

## 6. What the revision cleared of the second instrument's record

`retention_3_clears_the_uncitable_config_rows_of_the_v2_record`, a committed test rather than a
scratch script, reads the 178 boundary refusals out of the second instrument's own
`corrected-round*.txt` reports and re-runs each through the third instrument's rule:

**154 of 178 recorded v2 boundary refusals are accepted under `PROTOCOL-3.md` §2.** 24 survive, and
under §3 those are class-B flags rather than verdict kills.

The test asserts the **diagnosis**, not the rate: every refusal recorded against `tiered-review-67`,
`-68`, `-70` and `-71` — the four configuration-key rows `PROTOCOL-3.md` §1 names as uncitable by
construction — is cleared. It also asserts, for every refusal it parses, that the span really is in
the spec and really is refused by the v2 rule, so a drift between the reports and the check fails the
test rather than passing quietly.

**No v2 shard was re-graded.** Every verdict in `retention-3/` comes from a dispatch made after
`PROTOCOL-3.md` was committed; `spec_length_3_protocol_precedes_every_retention_3_record` executes
that ordering. Re-admitting a shard that was produced under the old prompt and judged a failure is
the retroactive sanctioning this run has refused twice, and it would have been the cheapest possible
way to fake this section.

---

## 7. `R8` excludes nothing on these dispositions

`R8` carves a ledger row out of the gate denominator when **every generation scored against it** —
the six of its own fixture — drops it. Computed over the tier-1 dispositions:

| fixture | rows | universally dropped |
|---|---|---|
| `skill-stickiness` | 91 | **0** |
| `tiered-review` | 84 | **0** |
| `tui-dc-picker` | 55 | **0** |
| total | 230 | **0** |

**So the discriminating set is the full 230 and `R2`'s two readings coincide.** Rows dropped by *at
least one* generation — the ones that actually discriminate — are 4 of 91, 22 of 84 and 7 of 55.

**This is a tier-1 figure and it is not final.** Tier 3 governs every class-B row and can overturn a
`present`, so the universal-drop set must be recomputed from the **final dispositions** at T6, which
is what `universally_dropped` in the suite already does. **`R8` remains a post-hoc adoption that must
be disclosed as one** — that it fires on nothing here does not retire the disclosure obligation, and
`plan.md` §0 discipline 2 is unaffected.

---

## 8. What T6 and later inherit

1. **`retention-3/` holds eighteen verdicts of eighteen generations.** `plan.md`'s T6 — *"18
   dispatches, one per generation"* — **can now run as written**, which it could not after T5b.
2. **Tier 2 and tier 3 read `retention-3/`, not `retention-2/`.** The item-8a stride sample is
   recomputed from the `present: true` rows of the **v3** verdicts.
3. **Tier 3's input is larger than item 8a alone.** It is item 8a's flagged rows **plus the 77
   class-B rows this pass produced**. `PROTOCOL-3.md` §3 makes the second set governed by tier 3, and
   the class-B set is **recomputed from the verdict and the spec** by the suite — there is no field a
   scorer could write to forge one.
4. **`RESULTS-2.md` must carry `PROTOCOL-3.md` §7's seven items in full**, including limitation 7a
   beside limitation 7, the class-B flag rate per generation, the tier-3 overturn rate per
   generation, and the sentence about what a uniformly-full-retention outcome would actually mean.
5. **Three tier-1 instruments, and the write-up names all three.** `PROTOCOL.md` item 8;
   `PROTOCOL-2.md` item 8 (1–5 spans, self-containment); `PROTOCOL-3.md` (clauses F/M/E, class A/B).
   *"We fixed it until data appeared"* is honest only with the count and the reasons attached.
6. **Iteration stops at the join.** No further tier-1 instrument revision may be authored by, or on
   the advice of, any context that has read `blind-map-2.json` or seen a join.
7. **`FREEZE-3.md` freezes `PROTOCOL-3.md` and nothing else.** `retention-3/` is deliberately not
   frozen: freezing an output would make an `R6` re-run a freeze breach rather than the remedy.
8. **Everything `SCORING-2-NOTES.md` §9.5 items 1–9 and §9.5a items 10–14 carry is still open.**
   This file discharges **items 1 and 4 of §9.5 only** — the missing-data problem is gone, and the
   admissibility tool was not needed. Every other row, including §9.5a item 12's decision that
   **blocks T9** (whether re-dispatching two transmission questions was right; the revert is
   `git revert 6268365`), is undischarged and carried forward.

---

## 9. `PROTOCOL-3.md` amendment log

**Required by `spec_length_3_protocol_stops_moving_after_the_first_verdict`**, which asserts that
every commit touching `PROTOCOL-3.md` beyond the first is named by SHA here. The check does not
classify a commit by window: a window-1 amendment must be disclosed too. That is a tightening and it
is cheaper than a check that would have to decide when the first dispatch happened.

| commit | window | what changed |
|---|---|---|
| *(the introducing commit — not listed; a row cannot name its own commit, and the check exempts `commits[0]`)* | 1 | §§0–8 as pre-registered |

**No amendment has been made.** If this table is still empty when `RESULTS-2.md` is written, that is
the claim that `PROTOCOL-3.md` was pre-registered once and not touched again — and `git log --oneline
-- docs/skill-evidence/spec-length/PROTOCOL-3.md` is what settles it, not this table.

---

## 10. Corrections made to this file during review

Consolidated here rather than scattered, and **the table is the count** — `SCORING-2-NOTES.md` §9.7
recorded that a prose tally of review inside the file under review is false one round later.

| # | what was wrong |
|---|---|
