# PROTOCOL-3 — pre-registration for the spec-length A/B's tier-1 instrument, third revision

**This file is an amendment, not a replacement.** `PROTOCOL-2.md` governs this run in full and is
**not edited**. This file changes exactly four things in it — item 8's span self-containment rule,
item 8's *disposition*, item 10's scorer template, and `R6`'s trigger list — adds one limitation and
one re-scoring instruction, and inherits everything else by reference. Where this file is silent,
`PROTOCOL-2.md` is the rule; where it speaks, it supersedes `PROTOCOL-2.md` **for the tier-1 pass
that writes `retention-3/` and for nothing else**.

**It is committed before the first dispatch it governs.** No `retention-3/` shard, no
`retention-3/<id>.json`, and no measurement under these rules exists at the commit that introduces
this file. That ordering is the whole warrant for calling this a pre-registration rather than a
description, and it is checkable in `git log` rather than taken on trust —
`spec_length_3_protocol_precedes_every_retention_3_record` executes it.

---

## 0. Why there is a third revision at all, and why iterating here is legitimate

The second attempt's tier-1 pass **filed 3 verdicts out of the 12 generations it attempted**, each
given up to six corrected attempts per shard, by scorers handed the frozen rule verbatim. Nine
generations have **no verdict at all**. That is not a measurement of what any arm retained; it is a
measurement of **whether a scorer can satisfy item 8**, and §1 below shows that for a specific and
identifiable class of ledger row the answer was *no, by construction, for every arm*.

Repeatedly revising an instrument and re-running is normally the exact failure pre-registration
exists to prevent. **It is sound here, and only here, because the work is blind.** The author of this
file has not opened `blind-map-2.json`, cannot see which generation belongs to which arm, and has
made no comparison between generations. Every rule below is a function of **markup** and of
**disposition** — never of a generation's identity, length, or content — and is applied identically
to all eighteen. An instrument change made under those conditions cannot be steered toward a result,
because its author cannot see the result.

**Three obligations follow, and they are binding on `RESULTS-2.md` and the write-up:**

1. **The revision count and the reason for each revision are on the record.** This is the **third**
   tier-1 instrument: `PROTOCOL.md` item 8, `PROTOCOL-2.md` item 8, and this file. "We fixed it
   until data appeared" is honest only with the count and the reasons attached, and §7 carries both.
2. **The moment arms become visible, iteration stops.** No further tier-1 instrument revision may be
   authored by, or on the advice of, any context that has read `blind-map-2.json` or seen a join.
3. **This file is never amended to sanction something already done.** The revision table below is
   append-only and every row names the commit that made the change and the window it was made in.

---

## 1. The diagnosis this revision answers

Derived mechanically from the committed record — the `corrected-round*.txt` reports under
`retention-2/parts/superseded/refusal-reports/`, which are the **one** implementation's own output,
joined against `generated-2/`. Nothing here re-implements item 8's predicate; a second implementation
would give a second answer to what the rule refuses, and `SCORING-2-NOTES.md` §3b's decision to keep
exactly one still binds.

**178 boundary refusals are recorded across the thirteen corrected rounds.** Classified by where the
refused span sits in its own generated spec:

| where the refused span sits | refusals | share |
|---|---|---|
| **entirely inside a fenced code block** | **81** | 45.5% |
| the preceding line ends in `*` (a bold-closed bullet or heading) | 42 | 23.6% |
| the preceding line ends in `"`, `'`, `` ` ``, `)` or `]` | 27 | 15.2% |
| other prose | 26 | 14.6% |
| straddles a fence line | 2 | 1.1% |

**Only the first row is claimed as a cause.** For a span lying wholly inside a fenced code block the
attribution is unambiguous and needs no second implementation to establish: item 8's rule is written
entirely in terms of prose sentence structure — a terminator in `.!?:;`, a blank or block-opening
neighbouring line, a table pipe — and **a line of a configuration block satisfies none of them on
either end**. The rows in the middle three bands are *consistent with* the two blind spots
`SCORING-2-NOTES.md` §3b already recorded by hand, and this file does not attribute them
individually, because doing so requires evaluating item 8's predicates outside the committed check.

**The unambiguous half of the diagnosis, stated as a defect rather than a rate: four ledger rows are
uncitable by construction.**

| ledger row | what it asks for | times refused |
|---|---|---|
| `tiered-review-70` | `cheap_agent` is `opencode` and `cheap_model` is `ko-ag/qwen3.6-35b-abliterated` | 20 |
| `tiered-review-67` | `enabled` defaults to `false` and stays off for the whole run | 12 |
| `tiered-review-68` | `mode` is an enum of `explorer`, `angle`, `file`, `change` | 12 |
| `tiered-review-71` | `timeout_ms` is 120000 | 12 |

Each is a **configuration-key row**. The evidence a spec offers for it is a line of a TOML block —
`timeout_ms  = 120000       # expiry takes the degrade path of §2`. That line is complete, unclipped,
and establishes the row; it ends in a digit or a comment, its neighbours are other config lines, and
**item 8 refuses it in every generation that renders the configuration as a code block**. There are
32 fenced blocks across the eighteen generations, so this is not a corner. `skill-stickiness-46`,
refused 38 times and the single most-refused row in the corpus, asks whether announcement strings
follow a template — evidence that is likewise a fixed-width string, not a sentence.

**A rule that refuses the only admissible evidence for a row refuses it for every arm equally.** That
is what makes this a defect in the instrument and not a finding about any generation, and it is also
why fixing it cannot favour an arm.

**The second failure mode is untouched by any of that, and is the dominant one on `tiered-review`.**
141 shared-span violations are recorded — one span cited for two rows — concentrated in **24 distinct
row pairs** across 38 rows, recurring across independent scorers and independent generations.
`SCORING-2-NOTES.md` §8.3 tested a ledger-entailment explanation for these and **withdrew it**; this
file does not re-form it, and **does not need to**. §3's disposition is deliberately agnostic between
"the scorer was lazy" and "the two rows are hard to evidence separately", because it routes the row
to a judge either way. That agnosticism is the design, not an evasion of the question.

**One caution about the withdrawal, recorded and not acted on.** §8.3 tested the hypothesis against
the two *filed* verdicts — which are, by construction, the verdicts that contained no shared span.
That is a selection effect and it makes the withdrawal weaker than its wording suggests. It is
recorded here so a later reader does not treat the question as closed; **nothing in this file rests
on it either way.**

---

## 2. Item 8's span self-containment rule, revised

`PROTOCOL-2.md` item 8's rule is inherited **whole and unchanged**, and three clauses are added to
it. Nothing is removed, and no span that item 8 accepts today is refused under this file — the change
is one-directional, and `retention_3_check_accepts_everything_v2_accepted` proves it over the corpus
rather than asserting it.

**The inherited rule, unchanged:**

> **Some occurrence of the span in the generated spec must begin on a boundary and end on a
> boundary.** A span may occur more than once; one clean occurrence is enough, because the scorer is
> quoting the document and not an offset into it.

**Clause F — fenced code.** A **fenced code block** is the region strictly between a line whose first
non-whitespace characters are ` ``` ` and the next such line; the two fence lines themselves are not
inside it. If **every line an occurrence of the span touches lies inside one and the same fenced code
block**, then that occurrence:

- **begins on a boundary** when it starts at the first non-whitespace character of its line;
- **ends on a boundary** when it ends at the end of a line, or at the last character before the
  closing fence.

*Why this cannot re-admit what the rule exists to catch.* The failure item 8 was written against is a
span clipped across a **hard wrap in prose**. Fenced code is not hard-wrapped prose: its line breaks
are the author's, each line is a unit, and a whole line of a config block is self-contained in
exactly the sense the rule means. Clause F requires whole lines on both ends, so a mid-line clip
inside a code block is still refused. The worked example the rule was built on —
`invalidated/db3e2d.json`'s two spans for `skill-stickiness-55` — is prose, is not inside a fence,
and is **still refused**; `retention_3_check_still_refuses_the_db3e2d_clip` keeps that true.

**Clause M — a marked line opens its own block.** In the block-start clause, an occurrence that
starts at the first non-marker character of its line **begins on a boundary** whenever that line's
prefix contains at least one actual marker — a run of `#`, a `-`, `*` or `+` bullet, a `>`, a `|`, or
`<digits>.` — **regardless of what the preceding line is**.

*Why.* The preceding-line test exists to distinguish a block start from the continuation of a wrapped
sentence. **A line that carries a marker is a block start; it is never a wrapped continuation**, so
the test has nothing to do there. As frozen, item 8 asked it anyway, which produced the pathology
`SCORING-2-NOTES.md` §3b recorded: in a list of bold-closed bullets **each bullet disqualifies its
neighbour**, because the preceding line ends in `*` rather than in `.!?:;`. Clause M leaves the
unmarked case — a span starting at the first non-whitespace character of a line with no marker at all
— **exactly as frozen**, because that is the case where wrapped-continuation is a real risk and the
preceding-line test is doing real work.

**Clause E — trailing emphasis and closers are transparent to the terminator test.** Wherever this
rule tests for a terminator in `.!?:;` — on the preceding line in the block-start clause, on the
span's own final characters in the end clause, and on the same-line `. `/`! `/`? ` clause — a
trailing run of `*`, `_`, `` ` ``, `"`, `'`, `)`, `]` and `}` is **removed first**, and the test
applies to what remains.

*Why.* `…the rule applies."` and `**…the rule applies.**` are sentences that end. The terminator is
the sentence's; the quotation mark and the emphasis run are markup wrapped around it, and a rule that
cannot see past them is testing the markup rather than the sentence. §3b recorded this as the
same-line clause's blind spot. **Clause E cannot re-admit a clip**: a left half ends mid-phrase, so
it has no terminator to uncover, and a right half starts mid-phrase, so it is neither at a marker nor
after a sentence end. Stripping closers reveals terminators that are there; it does not invent them.

**Clause F, M and E are functions of markup alone.** None reads a generation's identity, length,
arm, or content. Each is evaluated identically for all eighteen generations by one implementation,
`cli/tests/skills_valid.rs`, which remains the only implementation of this rule in the run.

**What this rule still does not catch, restated so no reader takes a green check for relevance.** A
span lifted cleanly out of a container is **accepted**: a well-formed sentence that is a bullet under
an *Out of scope* heading begins and ends on boundaries. That case is tier 2's and tier 3's, exactly
as `PROTOCOL-2.md` item 8 says, and clause F widens it — a config line under a heading reading
*"rejected alternative"* is now citable and is likewise tier 3's to catch.

---

## 3. Item 8's disposition, revised — the change this revision is really about

`PROTOCOL-2.md` diagnosed the first attempt's failure precisely: item 8a ended *"Any
`establishes: false` invalidates the entire verdict file"*, one flagged row destroyed all 91, and
retention came out undefined. **It fixed that in item 8a and left the identical disposition standing
one item away, in item 8.** The second attempt's data says item 8 is now the binding one — 9 of 12
generations produced no verdict, and the verdicts that failed typically failed on **2 to 8 rows out
of 84**.

So item 8's mechanical check divides into two classes, and they have different consequences.

**Class A — fatal. The verdict is re-run whole under `R6`, never patched.** Everything that means
the file is not a readable record of a scoring pass:

- a schema violation — an extra key, a missing key, a wrong type, a `null`;
- `rows` not carrying every ledger id of its fixture exactly once in ledger order;
- a `spec_id` outside item 6's pool, or one disagreeing with the file's own stem;
- `present: true` with an empty `quotes`, `present: false` with a non-empty one, or more than five
  spans;
- **a span that is not a verbatim substring of the generated spec at all.**

The last is **fabrication, and nothing in this revision softens it.** A scorer that invents text is
the failure every other rule here is downstream of, and it stays a whole-file kill.

**Class B — row-local, and never fatal.** Both of these:

- a span that **is** in the spec but no occurrence of which begins and ends on a boundary under §2;
- a span cited for **more than one row** within a verdict.

A row with a class-B problem is **flagged**, exactly as an `establishes: false` flags a row under
`PROTOCOL-2.md` item 8a, and is **escalated to tier 3 under item 8b, whose call governs it**. **A
shared span flags every row that cites it, not one of them** — which row the span really establishes
is a judgement, and tier 3 is where judgement lives; picking one mechanically would be the check
deciding a question it has no means to answer. **The rest of the verdict is unaffected, and the file
is filed.**

**`R6`'s trigger list is amended accordingly**, and this is the only decision rule this file touches.
It now reads, in full:

> the probe wrote no file; a shard or a verdict is malformed (which covers a transmission verdict
> under item 14 and a gap file under item 14a exactly as it covers a retention verdict); **a verdict
> fails the item-8 class-A check**.

*"A verdict fails the item-8 mechanical check"* is replaced by the class-A form. Never for an
unwanted outcome; every re-run logged. `R1`, `R1a`, `R2`, `R3`, `R3a`, `R4`, `R4a`, `R5`, `R5a`,
`R6a`, `R7` and `R8` are **untouched** — in particular `R2` stays a gate at full retention on the
`R8`-adjusted denominator, and `R8` stays arm-symmetric.

**A flagged row is not a re-run trigger and not a scoring failure.** It is an input to tier 3. A
verdict may file with any number of flags, and §5 requires the count to be reported per generation
precisely so that a verdict which filed on many of them is visible as such.

---

## 4. Limitation 7a — the loosening, stated in limitation 7's own register

`PROTOCOL-2.md` item 1's limitation 7 records that the item-8a redesign made an adjudication failure
less consequential and pushed the residual risk toward a false pass. **This revision does the same
thing to item 8, and the two compound.** Limitation 7a, which the write-up carries beside limitation
7 and does not soften:

> **7a. The tier-1 mechanical check no longer eliminates a verdict, and that is a second deliberate
> loosening in the same direction as limitation 7.** Under `PROTOCOL-2.md` a single unciteable span
> discarded all 84 or 91 rows of a verdict; under this protocol it flags one row and the other rows
> stand. That is what makes retention *measurable at all* — under the previous disposition 3 of 12
> attempted generations produced a verdict and the other 9 are **missing data, not low retention**.
> It is also a further shift of the residual scoring risk toward a **false pass**, on top of the
> shift limitation 7 already recorded, and the two compound: a lenient span now costs one flagged
> row rather than a whole verdict, and under `R2` a false `present: true` can only ever inflate
> retention, never eliminate an arm. **Three things bound it and none of them closes it:**
> fabrication is still fatal under class A; every class-B row is escalated to tier 3, which sees the
> spec and answers item 9's question with more context than a batch scorer had; and the class-B flag
> count is reported per generation, so a verdict that filed on many flags cannot be read as one that
> filed on none. **The write-up states this as a loosening, reports the class-B flag rate and the
> tier-3 overturn rate per generation, and does not describe tier 1 as a gate.**

**And the honest form of the risk, which limitation 7a does not let a reader miss:** the mechanical
check was the only part of the instrument that could refuse a verdict cheaply. Having made it
row-local, **the discriminating load now sits entirely on tier 2, tier 3, and the `R2` gate.** If
every arm comes out at full retention under this instrument, the correct reading is *the instrument
did not discriminate*, not *the arms are equivalent* — and `RESULTS-2.md` must say so in those words
if that is what happens.

---

## 5. The re-scoring pass this file governs

**All eighteen generations are re-scored, uniformly, into `retention-3/`.** Not the fifteen that lack
a v2 verdict — **all eighteen**.

*Why the three that already filed are re-scored too.* `6e7393`, `e085f2` and `48527b` cleared the v2
instrument. Keeping them would mean three generations measured by one instrument and fifteen by
another, and the experiment's whole question is a **comparison between arms**. An arm cannot be
compared with another arm on a measurement made under a different rule, and which three generations
happen to have a v2 verdict is not arm-symmetric — it is whatever survived a rule that refused config
lines. **Uniformity is the point**, and it is the same argument `SCORING-2-NOTES.md` §2b made for
offering the extended cap to every generation rather than only to the ones that had missed.

**`retention-2/` is not edited, not deleted, and not re-interpreted.** It stands as the record of what
the v2 instrument produced, and `RESULTS-2.md` reports both passes. **No v2 shard is re-graded under
these rules.** A shard produced under the old prompt, judged a failure, and then re-admitted by a new
rule is precisely the retroactive sanctioning this run has twice refused; every verdict in
`retention-3/` comes from a dispatch made after this file was committed.

**Mechanics, pre-registered here rather than chosen later:**

| | |
|---|---|
| output | `retention-3/<id>.json`, item 8's schema unchanged; shards at `retention-3/parts/<id>-<k>.json` |
| sharding | `PROTOCOL-2.md` item 10's boundaries unchanged — `skill-stickiness` `-01`–`-46` / `-47`–`-91`; `tiered-review` `-01`–`-42` / `-43`–`-84`; `tui-dc-picker` one shard `-01`–`-55`. **30 shard dispatches.** |
| prompt | `PROTOCOL-2.md` item 10's template, with §2's three clauses and §3's two classes written into it verbatim. The frozen text scorers get is the text the gate runs — that is `SCORING-2-NOTES.md` §2a's finding and it is not un-learned. |
| model | `sonnet`, foreground, `general-purpose`, as every dispatch in this run |
| attempts | **three per shard, uniform across all eighteen generations, from the first dispatch.** Class-A failures should be rare under §3; a cap that has to be spent is evidence the diagnosis was wrong, and §7 must say so if it is. |
| independence | **every shard file is SHA-256 hashed as it lands, and the hashes are recorded.** `SCORING-2-NOTES.md` §9.6 found one shard byte-identical to its own earlier attempt and could not tell a re-emission from a cached completion. Until that is shown otherwise, **an attempt is a dispatch, not an independent sample**, and a duplicate hash is reported rather than counted. |
| item 9's cap | item 9's first blockquote says a paraphrase *"may be evidenced by up to three spans"* while item 8 says one to five. `SCORING-2-NOTES.md` §9.5a item 13 records the inconsistency. **The prompt states 1–5 once and does not carry the contradicting clause**; this is a correction to item 10's template, logged here, and it changes no rule — item 8's cap was always the operative one and scorers always followed it. |

---

## 6. What is checked, and by what

Executed, not asserted. Every check below is in `cli/tests/skills_valid.rs`.

| check | what it proves |
|---|---|
| `spec_length_3_retention_verdicts_are_complete_and_quoted` | item 8's schema, completeness and **class-A** rules over `retention-3/`, under §2's revised span rule. **Vacuous on an empty or absent directory**, which is the correct state until the pass runs. |
| `spec_length_3_class_b_flags_are_reported_and_never_fatal` | over the real `retention-3/`: every class-B flag is surfaced with its row, and **no class-B problem fails the check**. §3's disposition, executed rather than described. |
| `retention_3_check_splits_class_a_from_class_b` | a fabricated span is **fatal**; a boundary-refused span and a shared span are **flagged and not fatal**; and a shared span flags **both** rows. |
| `retention_3_span_rule_accepts_everything_v2_accepted` | §2 is one-directional, over the three real v2-filed verdicts' spans as well as the unit fixtures: no span the v2 rule accepted is refused under v3. |
| `retention_3_span_rule_still_refuses_the_db3e2d_clip` | the worked example item 8 was written against is still refused, on **both** halves, under v3. |
| `retention_3_span_rule_admits_a_fenced_config_line` | clause F accepts a whole config line inside a fence — **and still refuses a mid-line clip inside the same fence**, so the clause is not a hole. |
| `spec_length_3_freeze_rows_still_hash_to_their_files` | every `FREEZE-3.md` row hashes to its file, and **no path it names is already frozen by `FREEZE.md` or by `FREEZE-2.md`**. The v2 check cross-references only `FREEZE.md`; that gap is not repeated here. |
| `spec_length_3_protocol_precedes_every_retention_3_record` | this file's commit precedes every `retention-3/` commit in `git log`. |
| `spec_length_3_protocol_stops_moving_after_the_first_verdict` | every commit touching this file is an ancestor of `retention-3/`'s introducing commit — window 2's rule, executed — **and** every such commit beyond the first is named by SHA in `SCORING-3-NOTES.md`. |

**The amendment log is `SCORING-3-NOTES.md`, not `RESULTS-2.md`, and that is deliberate.**
`PROTOCOL-2.md`'s equivalent check logs to `RESULTS-2.md`, which is the unblinding task's deliverable
and does not exist yet; until it does, that half of the check only prints a warning and asserts
nothing. Pointing this file's amendment log at an artifact that exists in the same commit makes the
obligation **enforced rather than pending** — and it gives `SCORING-3-NOTES.md` something structural
holding it alive, which `SCORING-2-NOTES.md` has never had. `RESULTS-2.md` inherits the log at T8
under §7; it does not replace it.

**The two windows this file has.** There is no third.

1. **Window 1 — until the first `retention-3/` dispatch.** Nothing has been measured under these
   rules. Writing the checker sits here on purpose: implementing §2 is how an ambiguity in it gets
   found, and finding it here costs nothing. Corrections need no log beyond the revision table.
2. **Window 2 — after the first dispatch.** No rule in this file may be revised at all. Any edit
   whatsoever is a deviation, logged in `RESULTS-2.md` with its reason and its commit.

---

## 7. What `RESULTS-2.md` and the write-up must carry from this file

Not optional, and not discharged by citing this file — each must appear where a reader of the results
will meet it.

1. **There were three tier-1 instruments**, and the write-up names all three, says what each changed,
   and says why. `PROTOCOL.md` item 8; `PROTOCOL-2.md` item 8 — 1–5 spans and the self-containment
   rule; this file — clauses F/M/E and the class-A/class-B split.
2. **The second revision's outcome, in full: 3 verdicts filed from 12 generations attempted**, under
   a cap of six attempts per shard. That is what motivated this one and it is reported as a result
   about the instrument, not omitted as a false start.
3. **Limitation 7a, beside limitation 7**, in the words of §4, including the sentence about what a
   uniformly-full-retention outcome would actually mean.
4. **The class-B flag count per generation**, and after tier 3 the overturn rate per generation.
5. **The uncitable-row diagnosis of §1** — that four `tiered-review` rows and `skill-stickiness-46`
   could not be evidenced under the v2 rule by any arm — because it is the clearest single piece of
   evidence in either attempt that a null can be an artefact of an instrument.
6. **Every obligation `SCORING-2-NOTES.md` §9.5 items 1–9 and §9.5a items 10–14 already carry.** This
   file discharges none of them and adds to them; it does not replace that list.
7. **That this instrument was revised while blind**, with §0's warrant stated plainly, and that
   iteration stopped at the join.

---

## 8. The relationship to `PROTOCOL-2.md` and `PROTOCOL.md`

`PROTOCOL-2.md` is **not edited by this file** and remains authoritative for this run on every item
except the four §0 names. `PROTOCOL.md` is not edited by either, `RESULTS.md`'s null stands
unchanged, and no first-attempt artifact is touched.

**Every place this file departs from `PROTOCOL-2.md`, in one list:**

| item | departure |
|---|---|
| item 8, span rule | clauses F, M and E added (§2). One-directional; nothing removed. |
| item 8, disposition | class A / class B split (§3). Class B is row-local and escalates to item 8b. |
| item 10, template | the revised rule and the two classes are written into the scorer prompt; item 9's contradicting *"up to three spans"* clause is dropped from it (§5). |
| item 12, `R6` | *"a verdict fails the item-8 mechanical check"* → *"a verdict fails the item-8 class-A check"*. No other decision rule changes. |
| item 1, limitations | limitation 7a added (§4). Limitation 7 is unchanged. |
| new | §5's re-scoring pass into `retention-3/`; §6's checks; §7's reporting obligations. |

---

## Revision table

**Append-only.** A row is never rewritten: correcting an entry means the record of what was
pre-registered changed, which is the thing that must not happen.

**A row cannot name its own commit, and this file does not pretend otherwise** — the same reason
`PROTOCOL-2.md`'s revision table gives. `git log --oneline -- docs/skill-evidence/spec-length/PROTOCOL-3.md`
resolves every commit here, oldest last, and
`spec_length_3_protocol_stops_moving_after_the_first_verdict` requires every commit beyond the first
to be named by SHA in `SCORING-3-NOTES.md`. **That check is what counts the amendments, not a human
reading this table** — `PROTOCOL.md`'s own revision count was wrong twice, both times by one.

**Every edit in either window appends a row here, in the same commit that makes the edit.** Window 2
logs to `SCORING-3-NOTES.md` **as well**, not instead — and because the check does not classify a
commit by window, a window-1 amendment must be disclosed there too. That is a tightening, and it is
cheaper than a check that would have to decide when the first dispatch happened.

| # | window | change |
|---|---|---|
| 1 | 1 | first commit — §§0–8: the diagnosis, clauses F/M/E, the class-A/class-B split, `R6`'s amended trigger list, limitation 7a, the `retention-3/` re-scoring pass, and the seven checks. No `retention-3/` record exists at this commit. |
