# Tier-1 scoring notes — third instrument

The running record of the tier-1 pass `PROTOCOL-3.md` governs. It is the **`R6` re-run log and the
`PROTOCOL-3.md` amendment log** until `RESULTS-2.md` exists, and unlike `SCORING-2-NOTES.md` it is
**not orphaned**: `spec_length_3_protocol_stops_moving_after_the_first_verdict` reads this file for
the SHA of every commit that amends `PROTOCOL-3.md`, so a run that deletes or empties it fails.

**Read §3.1, §5.1 and §5.2 before quoting any number out of this file.** Each bounds what the numbers
mean: a shard overwritten under the assembler, a scorer prompt that carried more than the
pre-registration sanctioned and had to be corrected and the whole pass re-run, and a
one-to-three-row noise floor between two dispatches of the same document.

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

**This is the second pass under `PROTOCOL-3.md`.** The first was run under a scorer prompt that
carried framing §5 did not sanction; it was discarded and re-run, and §5.1 is the whole account.
Every figure below is the **corrected-prompt** pass unless it says otherwise.

---

## 2. The dispatch log

**32 dispatches, 32 shard files, 30 of them live.** Counted off the files on disk, not off this
section's prose.

| | |
|---|---|
| shard dispatches, first pass | 30 — one per shard of all eighteen generations |
| `R6` re-runs for a class-A failure | 2 (`2d2629`, both shards — §3) |
| dispatches that wrote no file | **0** |
| **total dispatches** | **32** |
| shard files | **32** — 30 under `retention-3/parts/`, 2 under `parts/superseded/` |
| the discarded first pass, preserved whole | **50 files** under `parts/superseded/deviating-prompt/` |

**Every dispatch was `sonnet`, `general-purpose`, foreground**, per `plan.md` §0's model pin.

### 2.1 Shard independence — hashed as they landed, and the log is committed

`SCORING-2-NOTES.md` §9.6 found one shard byte-identical to its own earlier attempt among 150 files
and could not tell a verbatim re-emission from a cached completion, so "six attempts" meant six
**dispatches** rather than six independent samples. §9.5 item 7(b) told this pass to hash its shards
as they land.

`tools/assemble-retention-3.py` prints a SHA-256 for every shard it reads, and **that output is
committed at `retention-3/parts/assemble-log.txt`.** It is committed because §3 cites a shard hash as
evidence, and **a hash that exists only in a transcript is the author's word** — which is what a
reviewer said of an earlier draft of §3.

**All shard files have distinct SHA-256s** — live, superseded, and the discarded pass, checked over
the whole set at once. **That is not proof of independence** and is not claimed as such: it rules out
the one observable §9.6 found and nothing more. Until something stronger is shown, **an attempt is
still a dispatch, not an independent sample.**

---

## 3. The one `R6` re-run, and what it says about class A

**`2d2629` failed class A on its first attempt and was re-run whole.** **Twenty-nine spans**, all in
shard 2, were **not verbatim substrings of the generated spec** — a whole shard of invented text.
That is fabrication, the one thing `PROTOCOL-3.md` §3 keeps fatal, and it is the shape a span takes
when a scorer writes from memory rather than copying.

**Both shards were re-dispatched, not just the offending one.** `R6`'s remedy is a re-run of the
**verdict**, and the verdict is the assembled file; re-running only shard 2 would file a verdict half
of which came from a superseded attempt. The failing pair is preserved at
`retention-3/parts/superseded/2d2629-{1,2}-attempt1.json`, **and this time it really is the graded
pair** — hashes `d08134c3…` and `ae9a990c…`, both recorded in `assemble-log.txt`.

**This is the load-bearing observation of the redesign, and it cuts against the redesign's own
convenience.** The class split was not a way of making everything pass. On a first pass through
eighteen generations the check still refused a verdict outright, on the one ground that matters.
Seventeen filed with class-B problems present and unhidden; the eighteenth was refused for inventing
text. **A gate that refuses fabrication and files everything else is doing the job item 8 was written
to do**; a gate that also discarded eighty-two sound rows because one span started mid-phrase was
not.

**And it happened in both passes, on different generations** — `fd2c24` under the deviating prompt,
`2d2629` under the corrected one. The fatal class is not decorative.

### 3.1 A shard was overwritten by its own still-running scorer, after it had been graded

**Found by review, not by me**, in the discarded first pass. It is the most transferable thing in
this file.

The scorer for `fd2c24-2` was re-dispatched after an earlier one crashed. A polling loop watched for
`retention-3/parts/fd2c24-2.json` to appear, saw it, and treated it as final; the verdict was
assembled and gated on those bytes. **The subagent was still running.** It then ran its own
verification pass — its completion message says *"All 45 quotes verified as exact substrings"* —
found the fabricated span itself, and **rewrote the same path** before its completion notification
arrived. The graded bytes were gone by the time they were moved aside, so an earlier draft of §3
cited, as the record of a fabrication, a file containing none.

**The generalisable rule: file-exists is not file-final while the writer is still running.** Wait for
the writer to *finish*, not for its output to *appear*. A polling loop over output paths is the wrong
primitive when the producer can revise in place.

**Three things bound the damage.**

- **Only that one shard was affected.** Every verdict of that pass equalled the concatenation of its
  own shards, checked mechanically.
- **The affected verdict was re-run anyway**, and the whole pass was later discarded for an unrelated
  reason (§5.1), so nothing filed rests on the lost bytes.
- **It is now guarded.** `spec_length_3_every_verdict_is_its_shards_concatenated` compares every
  `retention-3/<id>.json` with its own `parts/<id>-<k>.json` and with the ledger. Nothing did that
  before — the two are separate files and no check had compared them, which is why an overwrite could
  be silent. `PROTOCOL-2.md` item 10 already said the assembled file must be the shards concatenated;
  **nothing executed it.**

**What the guard does NOT prove**, stated because a reviewer raised it: it establishes referential
consistency **at commit time**, not provenance. A shard overwritten *and* re-assembled before the
commit would still pass. The corrected pass therefore also told every scorer to write its output only
after finishing verification, and the committed `assemble-log.txt` pins the hash of what was graded.

---

## 4. What filed, per generation

Derived from `retention-3/*.json` and from the class-B flags
`spec_length_3_class_b_flags_are_reported_and_never_fatal` prints. **Nothing here is typed from a
previous draft** — an earlier version of this table survived a checker fix that changed two of its
flag counts, and should not have.

| id | fixture | rows | `present` | spans | class-B flags |
|---|---|---|---|---|---|
| `66530f` | `skill-stickiness` | 91 | 91 | 128 | 2 |
| `6e7393` | `skill-stickiness` | 91 | 90 | 119 | 0 |
| `e085f2` | `skill-stickiness` | 91 | 89 | 114 | 0 |
| `e790f5` | `skill-stickiness` | 91 | 90 | 139 | 0 |
| `fd2c24` | `skill-stickiness` | 91 | 91 | 127 | 2 |
| `fe4059` | `skill-stickiness` | 91 | 89 | 127 | 4 |
| `054872` | `tiered-review` | 84 | 76 | 89 | 9 |
| `2c4295` | `tiered-review` | 84 | 81 | 99 | 2 |
| `2d2629` | `tiered-review` | 84 | 72 | 82 | 20 |
| `48527b` | `tiered-review` | 84 | 78 | 96 | 11 |
| `b49ff1` | `tiered-review` | 84 | 83 | 103 | 0 |
| `fd230c` | `tiered-review` | 84 | 83 | 105 | 0 |
| `031cc4` | `tui-dc-picker` | 55 | 53 | 58 | 1 |
| `08ae18` | `tui-dc-picker` | 55 | 54 | 57 | 0 |
| `26d7a2` | `tui-dc-picker` | 55 | 50 | 54 | 8 |
| `47173f` | `tui-dc-picker` | 55 | 55 | 65 | 0 |
| `a9fcf9` | `tui-dc-picker` | 55 | 55 | 60 | 0 |
| `b2b8cf` | `tui-dc-picker` | 55 | 54 | 66 | 0 |
| **total** | | **1380** | **1334** | **1688** | **59** |

**Read the totals off the table.** `SCORING-2-NOTES.md` §9.7 records that every figure that turned
out wrong in that file was a summary line typed by hand while the tables underneath were right, and
§10 below records this file repeating it.

**Present rates by fixture, and they have real spread:** `skill-stickiness` 89–91 of 91;
`tiered-review` 72–83 of 84; `tui-dc-picker` 50–55 of 55. **Four of the eighteen retain every row** —
`47173f`, `66530f`, `a9fcf9`, `fd2c24`. **This is the first tier-1 output in either attempt that looks
like a measurement rather than a saturation or a blank**, and `SCORING-2-NOTES.md` §7 item 4 / §9.5a
item 10's 99.8%-ceiling doubt should be re-read against it — narrowed further, still not withdrawn,
because a fifth of the corpus at a perfect score is what that doubt is about.

**None of this is evidence about spec length.** This file's author cannot see arms, has not opened
`blind-map-2.json`, and has made no comparison between generations beyond the arm-free table above.
Six generations of a fixture are three arms of two, and the spread could be the fixture, the ledger,
the arms or the scorers. **The join is T6's and T8's.**

---

## 5. The class-B flags — 59 of them, and none fatal

| kind | flagged row-instances |
|---|---|
| a span cited for more than one row | **44** |
| a span with no occurrence beginning and ending on a boundary | **15** |
| **total** | **59** |

**A shared span contributes more than one row-instance**, because `PROTOCOL-3.md` §3 flags every row
that cites it: which of them the span really establishes is a judgement, and tier 3 is where judgement
lives. **The arithmetic is not a halving** — the check keeps the first citing row in a map and emits a
**pair** of flags each time a later row repeats the quote, so a span cited by `N` rows contributes
`2(N-1)`:

| rows citing one span | such spans | row-instances |
|---|---|---|
| 2 | 15 | 30 |
| 3 | 2 | 8 |
| 4 | 1 | 6 |
| **total** | **18** | **44** |

**18 distinct shared spans**, and the worst carries four ledger rows. **Nine of eighteen verdicts
carry no class-B flag at all**; the nine that do carry between 1 and 20. Under the second instrument
every one of those nine would have been discarded whole, and the nine clean ones would have been the
entire yield.

**§8.3's withdrawn ledger-entailment hypothesis is not re-formed here**, and `PROTOCOL-3.md` §3's
disposition does not need it: it routes a shared row to a judge whether the cause is scorer laziness
or genuinely inseparable ledger rows. **One caution, recorded and not acted on:** §8.3 tested that
hypothesis against the two *filed* v2 verdicts, which by construction contained no shared span. That
is a selection effect and it makes the withdrawal weaker than its wording suggests. **The 18 shared
spans are on disk with their rows named**, so a later task can test the question against a sample that
was not selected for lacking them.

### 5.1 The first pass was run under a prompt the pre-registration did not sanction, and was discarded

**A deviation from `PROTOCOL-3.md` §5, found by review, and the reason this pass ran twice.**

§5 pre-registers that the prompt carries *"§2's three clauses and §3's two classes written into it
verbatim"*. The first build of `tools/build-tier1-prompts-3.py` appended both sections verbatim **and
wrapped them in framing of its own**. The §3 wrapper is the one that mattered:

> …a boundary refusal and a shared span are class B, they flag one row each and are never fatal, and
> there is therefore NO reason to hide one, to drop a row you cannot cite cleanly, or to reuse a span
> you have already used. Cite the best span you can and let the check flag what it flags.

**Why that is not a formality.** `SCORING-2-NOTES.md` §2a is this run's own finding that **prompt
wording moves scorer behaviour measurably**. Telling scorers in the same breath that a shared span is
not fatal is an intervention on one of the two things the pass then reported — and *"no reason to …
drop a row you cannot cite cleanly"* points at `present`, the **primary** metric. Two scorers cited §3
in their own summaries as licence for a span they knew was shared.

**Why it was re-run rather than merely disclosed.** An earlier draft of this section argued that
`PROTOCOL-3.md` §6 window 2 and §0's warrant forbade a re-run. **That was a category error and a
reviewer named it:** those provisions govern revising `PROTOCOL-3.md`'s *rules*. The prompt
**builder** is a tool, is not frozen by `FREEZE-3.md`, and correcting it does not revise a rule — it
brings the implementation **into** compliance with frozen §5. The data had been collected under a
prompt that violated the pre-registration; re-running under a compliant one is the conservative
action, not a fourth instrument revision. The builder now appends §2 and §3 with delimiters and a
one-line precedence statement, and nothing else.

**What changed when the framing was removed** — both passes over the same eighteen documents, both
recomputed under the same checker:

| | deviating prompt | corrected prompt |
|---|---|---|
| verdicts filed | 18 | 18 |
| distinct shared spans | **28** | **18** |
| shared-span row-instances | **62** | **44** |
| boundary flags | 20 | 15 |
| class-B flags total | 82 | 59 |
| rows `present` | 1326 / 1380 | 1334 / 1380 |

**The framing did move behaviour, in the direction the reviewer predicted for shared spans**:
removing it cut distinct shared spans by more than a third. **It did not visibly inflate `present`** —
the corrected pass scores eight rows *higher*, not lower — so the predicted inflation of the primary
metric is **not observed**, and this file does not claim it was. The two passes are not perfectly
matched (a different generation was re-run in each under `R6`), so read the shared-span rows as the
finding and the `present` row as the absence of one.

**`RESULTS-2.md` must carry this section**, because it is simultaneously the strongest evidence in
either attempt that scorer prompts move scorer behaviour, and an instance of the instrument's own
author doing it by accident.

### 5.2 The generations scored under both instruments — an inter-dispatch noise floor

`6e7393`, `e085f2` and `48527b` have a `retention-2/` verdict as well. Comparing the two **for one
generation against itself** is arm-blind and is the only such comparison in this file. Measured on
the discarded first pass:

| id | v2 `present` | v3 `present` | rows that flipped |
|---|---|---|---|
| `6e7393` | 91/91 | 90/91 | 1 — `-05` true→false |
| `e085f2` | 90/91 | 89/91 | 3 — `-05`, `-08` true→false; `-56` false→true |
| `48527b` | 80/84 | 77/84 | 3 — `-11`, `-23`, `-47` true→false |

**One to three rows per generation move between two dispatches of the same document**, and
`skill-stickiness-05` flipped the same way in two independent generations. The span rule only widened,
so **this is not the rule** — it is scorer judgement on `present`, which is item 9's question and was
never mechanical. The corrected pass moves several of these again, which is the same phenomenon.

**`RESULTS-2.md` must state this before quoting any per-generation retention difference.** A gap of
one to three rows between two arms is inside it. **`R2` is a full-retention gate, so the noise floor
does not corrupt the gate itself** — an arm clears only by retaining every row — but it bounds what
any *comparison of scores* could mean.

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
that ordering.

---

## 7. `R8` excludes nothing on these dispositions

`R8` carves a ledger row out of the gate denominator when **every generation scored against it** —
the six of its own fixture — drops it. Computed over the tier-1 dispositions:

| fixture | rows | universally dropped | dropped by at least one |
|---|---|---|---|
| `skill-stickiness` | 91 | **0** | 5 |
| `tiered-review` | 84 | **0** | 16 |
| `tui-dc-picker` | 55 | **0** | 5 |
| total | 230 | **0** | **26** |

**So the discriminating set is the full 230 and `R2`'s two readings coincide.** Twenty-six rows are
dropped by at least one generation; those are the rows that can separate arms at all.

**This is a tier-1 figure and it is not final.** Tier 3 governs every class-B row and can overturn a
`present`, so the universal-drop set must be recomputed from the **final dispositions** at T6, which
is what `universally_dropped` in the suite already does. **`R8` remains a post-hoc adoption that must
be disclosed as one** — that it fires on nothing here does not retire the disclosure obligation.

---

## 8. What T6 and later inherit

1. **`retention-3/` holds eighteen verdicts of eighteen generations.** `plan.md`'s T6 — *"18
   dispatches, one per generation"* — **can now run as written**, which it could not after T5b.
2. **Tier 2 and tier 3 read `retention-3/`, not `retention-2/`.** Item 8a's stride sample is
   recomputed from the `present: true` rows of the **v3** verdicts.
3. **Tier 3's input is item 8a's flagged rows PLUS the 59 class-B rows this pass produced.** The
   class-B set is **recomputed from the verdict and the spec** by the suite — there is no field a
   scorer could write to forge one.
4. **`RESULTS-2.md` must carry `PROTOCOL-3.md` §7's seven items in full**, plus §3.1, §5.1 and §5.2 of
   this file, limitation 7a beside limitation 7, the class-B flag rate per generation, and the tier-3
   overturn rate per generation.
5. **Three tier-1 instruments and two passes of the third**, and the write-up names all of them.
   *"We fixed it until data appeared"* is honest only with the count and the reasons attached.
6. **Iteration stops at the join.** No further tier-1 instrument revision may be authored by, or on
   the advice of, any context that has read `blind-map-2.json` or seen a join.
7. **`FREEZE-3.md` freezes `PROTOCOL-3.md` and nothing else.** `retention-3/` is deliberately not
   frozen: freezing an output would make an `R6` re-run a freeze breach rather than the remedy.
8. **`SCORING-2-NOTES.md` §9.5 items 1–9 and §9.5a items 10–14, itemised.**

   | §9.5 / §9.5a item | state |
   |---|---|
   | 1 — the admissibility tool takes a fixture argument | **discharged** (noted; not needed this pass) |
   | 4 — nine of twelve generations are missing data | **discharged** — 18 of 18 filed |
   | 7(b) — hash the shards as they land | **discharged** — `assemble-log.txt` is committed; §2.1 |
   | 7(a) — `RESULTS-2.md` must carry the duplicate-shard finding | open, owed to T8 |
   | 2, 3 — the §2b cap and `tui-dc-picker`'s single shard | superseded by `PROTOCOL-3.md` §5's own cap and sharding; not a debt |
   | 5, 6, 8, 9 | open, all owed to `RESULTS-2.md` |
   | §9.5a 10–14 | **all open**, including item 12's decision that **blocks T9** (whether re-dispatching two transmission questions was right; the revert is `git revert 6268365`) |

   **This file adds to that list rather than shortening it**: §3.1, §5.1 and §5.2 are three new
   obligations on `RESULTS-2.md`.
9. **None of those obligations is test-enforced**, unlike the amendment log and the shard-concatenation
   check. They rely on the same unenforced-prose mechanism that produced most of §10 below. **A later
   task that wants them kept should make one of them executable rather than adding a tenth bullet.**

---

## 9. `PROTOCOL-3.md` amendment log

**Required by `spec_length_3_protocol_stops_moving_after_the_first_verdict`**, which asserts that
every commit touching `PROTOCOL-3.md` beyond the first is named by SHA here. The check does not
classify a commit by window: a window-1 amendment must be disclosed too.

| commit | window | what changed |
|---|---|---|
| *(the introducing commit — not listed; a row cannot name its own commit, and the check exempts `commits[0]`)* | 1 | §§0–8 as pre-registered |

**No amendment has been made.** `git log --oneline -- docs/skill-evidence/spec-length/PROTOCOL-3.md`
settles it, not this table.

### 9.1 Two defects in `PROTOCOL-3.md`, found by review AFTER the first dispatch, deliberately NOT fixed

Both are defects in how the frozen file **describes itself**. Neither is a rule error and neither
changes what was measured.

1. **§6's check table lists nine tests; twelve now exist.** Missing from it are
   **`retention_3_span_rule_sees_past_markup`** — the **only** test exercising clauses M and E in
   isolation, and they are §2's two most novel clauses —
   `retention_3_clears_the_uncitable_config_rows_of_the_v2_record` (§6 above), and
   `spec_length_3_every_verdict_is_its_shards_concatenated` (§3.1, which did not exist when
   `PROTOCOL-3.md` was written). **A reader following §6 alone would not find the test that proves
   clauses M and E do what §2 claims.**
2. **§2's prose names two tests that do not exist under those names**:
   `retention_3_check_accepts_everything_v2_accepted` and
   `retention_3_check_still_refuses_the_db3e2d_clip`. The real names use `span_rule`, which is what
   §6's own table says. **§6 is right and §2 is stale**, within one file.

**Why not just edit it, given that §6 window 2 explicitly allows an edit as a logged deviation.**
Because that path is **foreclosed by a different mechanism**. `FREEZE-3.md` hashes `PROTOCOL-3.md`
and is append-only, so an edit forces either a **rewritten hash row** — the exact breach `FREEZE.md`'s
closing section records as the thing not to repeat — or a **second row for one path**, which
`spec_length_3_freeze_rows_still_hash_to_their_files` rejects.

**That tension is itself a finding for `RESULTS-2.md`:** a protocol that says a window-2 edit is
permitted-with-disclosure, frozen by a record that makes any edit unrecordable, has one of the two
rules doing nothing. Here that is the safe direction — the file cannot move — but a later revision
should resolve it deliberately rather than inherit it.

**Note the asymmetry with §5.1, which is deliberate.** The *prompt builder* was corrected and the pass
re-run; the *protocol* was not touched. **A tool that implements a frozen rule may be fixed to match
it. The frozen rule itself may not move.**

---

## 10. Corrections made to this file during review

**The table is the count** — `SCORING-2-NOTES.md` §9.7 records that a prose tally of review inside the
file under review is false one round later.

| # | what was wrong |
|---|---|
| 1 | **`spec_length_3_protocol_precedes_every_retention_3_record` had its `descends_from_in` arguments swapped, and the suite was red at `9c4860e` while that commit's message claimed it green.** See §10.1. |
| 2 | §5 said the shared-span row-instances were the total divided by two. They are `2(N-1)` per span, and some spans are cited by more than two rows. |
| 3 | §9.1 added: defects in `PROTOCOL-3.md`'s self-description, recorded rather than fixed because the file is frozen; and the reason the window-2 edit path is foreclosed. |
| 4 | **§3 cited a superseded shard as the record of a fabrication it did not contain.** The artifact had been overwritten by its own still-running scorer. §3.1 records the incident and the guard it caused. |
| 5 | **§5.1: the scorer prompt was not §2/§3 verbatim.** First disclosed and defended; then, on review, the defence was found to be a category error and **the whole pass was re-run** under a corrected builder. |
| 6 | §5.2 added: an inter-dispatch noise floor of one to three rows. |
| 7 | §8 item 8 said only §9.5 items 1 and 4 were discharged while §2.1 described discharging item 7(b). Replaced with a per-item table. |
| 8 | Clause F refused a whole fenced line in a **CRLF** document and accepted a span starting inside a line's leading whitespace. Both fixed, both now asserted. |
| 9 | **§4's and §5's flag counts were stale**: the clause-F fix in item 8 changed the boundary-refusal count for two generations, and the tables were not recomputed against the corrected checker. **A fix to a checker invalidates every number derived from it.** |
| 10 | §3 said the real span in `generated-2/fd2c24.md` was "the one-space form". It is the **zero**-space form; no one-space form exists in that file. |
| 11 | §2.1 cited a shard hash that existed only in a transcript. `assemble-log.txt` is now committed. |

### 10.1 The commit that claimed a green suite it had not run

**Commit `9c4860e`'s message ends "Whole crate green: 809 / 9 / 28 / 2 / 6 / 120 / 1". That was false
at that commit**, and it is corrected here rather than by rewriting the message.

The suite was run green *before* `retention-3/` was staged. At that moment
`spec_length_3_protocol_precedes_every_retention_3_record` was **vacuous** — its
`Introduced::NotCommitted` arm returned early, because the directory it orders against did not exist
in history. Committing the eighteen verdicts made that arm live, and it contained a swapped-argument
call. The suite was not re-run after the commit, so a passing run of one program was reported as a
passing run of a different one.

**The generalisable form:** *a test that is vacuous until an artifact exists is not verified by a run
made before that artifact exists.* This suite is full of such tests — nearly every `spec_length_*`
corpus check returns early on a missing directory — so the green run that matters is the one taken
**after** the artifact is committed.

**The bug was worse than a red test.** On a linear history the swapped call returns `Yes` in exactly
the case the check exists to catch — `retention-3/` committed *before* `PROTOCOL-3.md` — and `No` in
the compliant case. The underlying ordering was and is compliant, so **nothing about the
pre-registration is in doubt; the check that was supposed to prove it was.**
