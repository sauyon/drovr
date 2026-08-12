# RESULTS-2 — the spec-length A/B, second attempt

**The outcome, in one line: no arm clears `R2`, the control included, and no candidate beat the
control. Nothing ships.**

This is a **null about the arms**, not about the instrument, and that distinction is what separates
this attempt from the first one. `RESULTS.md`'s null was undefined retention — every verdict file of
the one fixture that was scored had been invalidated, and no arm had a retention count at all. Here
all eighteen generations carry a filed tier-1 verdict, all 1380 row-judgements have a defined final
disposition, and the arms are separated by 23 rows. The gate was clearable in principle and no arm
cleared it.

**`RESULTS.md` is not extended and no first-attempt artifact is touched.** Its null stands as the
record of what the first instrument produced.

---

## 1. What the gate says

`R2` is a gate at **full** retention over the discriminating set. **`R8` excludes nothing** — no
ledger row was dropped by all six generations of its own fixture — so the discriminating set is the
full **230**, and `N/230` and `N/|discriminating set|` coincide for every arm. Both are reported
anyway, because `R8` requires both everywhere and a reader should not have to infer that they are
equal.

**`R8` was adopted post-hoc, and that is stated here where a reader first meets it rather than left
to the write-up.** It was decided after the first attempt failed and before any new measurement was
taken. What makes that legitimate is not the timing but that the criterion **cannot be aimed**: it
reads only the final dispositions and the id→fixture map, never an arm, so permuting the arm labels
leaves the excluded set identical while the per-arm counts permute with them.
`spec_length_2_r8_exclusion_is_arm_symmetric` executes that permutation rather than asserting it —
**though only half of it does work**: the test's first half locks a type signature, which discovers
nothing, and its second permutes a synthetic four-row fixture rather than this run's own
dispositions. `plan.md` T11 pins that caveat and it is repeated here so `R8` is not oversold at the
point a reader meets it.

**Here the question is moot in the most reassuring way available: the exclusion set is empty, so
`R8` changed no arm's denominator at all.**

**Before the numbers below are compared to each other, the floor under them:** §12 records an
inter-dispatch noise floor of **one to three rows per generation** between two dispatches of the
same document. `R2` is a full-retention gate, so the floor does not corrupt the gate itself — an arm
clears only by retaining every row — but **it bounds every score *comparison* in this file**,
including the 23-row spread cited above. It rests on three generations, which is thin.

| arm | dropped rows | retention / 230 | retention / discriminating |
|---|---|---|---|
| S1 | 11 | 219 / 230 (descriptive, not a pass) | 219 / 230 (descriptive, not a pass) |
| S2 | 1 | 229 / 230 (descriptive, not a pass) | 229 / 230 (descriptive, not a pass) |
| S3 | 24 | 206 / 230 (descriptive, not a pass) | 206 / 230 (descriptive, not a pass) |

**Every one of these is labelled *descriptive, not a pass* under `R5`, and none of them is a
near-miss to be talked up.** `R2` admits no partial credit and no weighting by `kind`: any drop
eliminates. `S2` dropped one row and is eliminated exactly as `S3`, which dropped 24.

**The rows each arm dropped, in full** — `R1`'s union across that arm's six generations:

- **S1** (11): `skill-stickiness-05`, `skill-stickiness-56`, `tiered-review-04`, `tiered-review-09`, `tiered-review-11`, `tiered-review-22`, `tiered-review-23`, `tiered-review-31`, `tiered-review-41`, `tiered-review-84`, `tui-dc-picker-01`
- **S2** (1): `tiered-review-84`
- **S3** (24): `skill-stickiness-05`, `skill-stickiness-08`, `skill-stickiness-62`, `skill-stickiness-68`, `tiered-review-03`, `tiered-review-04`, `tiered-review-07`, `tiered-review-09`, `tiered-review-10`, `tiered-review-11`, `tiered-review-12`, `tiered-review-13`, `tiered-review-23`, `tiered-review-25`, `tiered-review-27`, `tiered-review-34`, `tiered-review-35`, `tiered-review-41`, `tui-dc-picker-01`, `tui-dc-picker-04`, `tui-dc-picker-05`, `tui-dc-picker-08`, `tui-dc-picker-24`, `tui-dc-picker-53`

**`R8`'s universal-drop set is empty**, so the table it requires has a header and no rows:

| row | fixture | retained by / 6 |
|---|---|---|

That emptiness is a finding, not an absence of one. `R8` exists to stop a row *no arm can carry*
from deciding the outcome; every row of all three ledgers was carried by at least one generation, so
no row is unciteable under the third instrument and **the null cannot be attributed to an
impossible row**. Under the second instrument four `tiered-review` configuration rows and
`skill-stickiness-46` were uncitable by construction for every arm (`PROTOCOL-3.md` §1); under this
one, `tiered-review-67`, `-68`, `-70`, `-71` and `skill-stickiness-46` are all retained by at least
one generation.

### The one row that decides `S2`, and the two the tier-3 pass overturned

**`S2`'s single dropped row is `tiered-review-84`**, and `S1` dropped it too — only `S3` retained
it. It is the sole reason `S2` does not clear.

**Tier 3 overturned 2 of the 98 rows escalated to it** (2.0%): `26d7a2`/`tui-dc-picker-24` and
`2c4295`/`tiered-review-22`. Both went `present: true` → `present: false`, so both *created* a drop
rather than rescuing one. `tiered-review-22` is `S1`'s; `tui-dc-picker-24` is `S3`'s. **Neither
touches `S2`**, so `S2`'s elimination does not rest on a tier-3 call.

---

## 2. Per generation — `R1a`

`R1a` requires the 18 per-generation counts beside the 3 union counts, because without them a null
cannot be read: a row 5 of 6 generations retain is noise, and a row none retains is the instruction.

| id | arm | fixture | sample | tier-1 present / N | tier-2 sampled | tier-2 false | tier-3 present | final present / N |
|---|---|---|---|---|---|---|---|---|
| `031cc4` | S3 | tui-dc-picker | 1 | 53 / 55 | 10 | 2 | 3 | 53 / 55 |
| `054872` | S3 | tiered-review | 1 | 76 / 84 | 15 | 2 | 8 | 76 / 84 |
| `08ae18` | S1 | tui-dc-picker | 2 | 54 / 55 | 10 | 3 | 3 | 54 / 55 |
| `26d7a2` | S3 | tui-dc-picker | 2 | 50 / 55 | 10 | 4 | 8 | 49 / 55 |
| `2c4295` | S1 | tiered-review | 2 | 81 / 84 | 16 | 4 | 5 | 80 / 84 |
| `2d2629` | S3 | tiered-review | 2 | 72 / 84 | 14 | 6 | 23 | 72 / 84 |
| `47173f` | S2 | tui-dc-picker | 2 | 55 / 55 | 11 | 2 | 2 | 55 / 55 |
| `48527b` | S1 | tiered-review | 1 | 78 / 84 | 15 | 4 | 13 | 78 / 84 |
| `66530f` | S2 | skill-stickiness | 2 | 91 / 91 | 18 | 1 | 3 | 91 / 91 |
| `6e7393` | S1 | skill-stickiness | 1 | 90 / 91 | 18 | 4 | 4 | 90 / 91 |
| `a9fcf9` | S2 | tui-dc-picker | 1 | 55 / 55 | 11 | 3 | 3 | 55 / 55 |
| `b2b8cf` | S1 | tui-dc-picker | 1 | 54 / 55 | 10 | 0 | 0 | 54 / 55 |
| `b49ff1` | S2 | tiered-review | 2 | 83 / 84 | 16 | 3 | 3 | 83 / 84 |
| `e085f2` | S3 | skill-stickiness | 2 | 89 / 91 | 17 | 2 | 2 | 89 / 91 |
| `e790f5` | S1 | skill-stickiness | 2 | 90 / 91 | 18 | 3 | 3 | 90 / 91 |
| `fd230c` | S2 | tiered-review | 1 | 83 / 84 | 16 | 3 | 3 | 83 / 84 |
| `fd2c24` | S2 | skill-stickiness | 1 | 91 / 91 | 18 | 1 | 3 | 91 / 91 |
| `fe4059` | S3 | skill-stickiness | 1 | 89 / 91 | 17 | 4 | 7 | 89 / 91 |

`tier-2 sampled` is item 8a's stride sample — every 5th `present: true` row, `floor(n/5)` of `n`.
**No generation sampled empty**, so item 8a's empty-sample case did not arise. `tier-3 present` is
out of that generation's escalated set, which is the union of its tier-2 `establishes: false` rows
and its `PROTOCOL-3.md` §3 class-B rows; `b2b8cf` had neither and correctly has no `escalation-2`
file at all.

**The final disposition is a derived quantity and nobody is asked to trust this table.**
`spec_length_2_gate_arithmetic_matches_the_records` reparses every cell above by exact header string
and recomputes it from `retention-3/`, `adjudication-2/`, `escalation-2/`, `blind-map-2.json` and the
frozen ledgers.

---

## 3. `R3` — length, and why there is no length comparison

**`R3` compares length only among arms that clear `R2`. No arm cleared, so no comparison is
licensed, and none is made.** The figures below are descriptive and decide nothing.

| arm | mean lines | mean bytes | skill-stickiness | tiered-review | tui-dc-picker |
|---|---|---|---|---|---|
| S1 | 395.2 | 23659.5 | 560.5 | 322.5 | 302.5 |
| S2 | 493.0 | 30208.7 | 716.5 | 369.0 | 393.5 |
| S3 | 312.3 | 19041.5 | 421.0 | 260.0 | 256.0 |
| *fixture* | — | — | 791 | 463 | 414 |

**Stated plainly because it is the finding a reader would otherwise take from the table: `S2`, the
moderate trim, produced the LONGEST specs of the three arms** — a mean of 493.0 lines against the
control's 395.2, on every one of the three fixtures. `R3a`'s own words are that *"a shorter
instruction that produces longer specs has not won anything"*. Had `S2` cleared `R2`, that is what
would have decided it.

`S3`, the aggressive minimum, produced the shortest specs (312.3 mean lines) and dropped the most
rows (24). That is the direction the experiment was built to detect, and it is the only ordering in
this run that behaves as the hypothesis predicted — on an arm that did not clear the gate.

**The arm files' own lengths**, which `R3` also requires: `S0` 2 lines / 177 B (historical baseline,
never generated from, never shipped), `S1` 15 lines / 191 words / 1151 B, `S2` 11 lines / 114 words
/ 705 B, `S3` 4 lines / 59 words / 354 B.

### `R5a` — the copy check, and the reason `S2`'s 229/230 may not be read as a near-win

| id | arm | fixture | lines | bytes | % of fixture lines | `R5a` |
|---|---|---|---|---|---|---|
| `031cc4` | S3 | tui-dc-picker | 286 | 15960 | 69.1% |  |
| `054872` | S3 | tiered-review | 272 | 17521 | 58.7% |  |
| `08ae18` | S1 | tui-dc-picker | 288 | 16043 | 69.6% |  |
| `26d7a2` | S3 | tui-dc-picker | 226 | 13131 | 54.6% |  |
| `2c4295` | S1 | tiered-review | 328 | 19195 | 70.8% |  |
| `2d2629` | S3 | tiered-review | 248 | 15991 | 53.6% |  |
| `47173f` | S2 | tui-dc-picker | 377 | 21808 | 91.1% |  |
| `48527b` | S1 | tiered-review | 317 | 18071 | 68.5% |  |
| `66530f` | S2 | skill-stickiness | 642 | 41306 | 81.2% |  |
| `6e7393` | S1 | skill-stickiness | 541 | 34094 | 68.4% |  |
| `a9fcf9` | S2 | tui-dc-picker | 410 | 24225 | 99.0% | **FLAGGED** |
| `b2b8cf` | S1 | tui-dc-picker | 317 | 17908 | 76.6% |  |
| `b49ff1` | S2 | tiered-review | 336 | 18827 | 72.6% |  |
| `e085f2` | S3 | skill-stickiness | 418 | 25828 | 52.8% |  |
| `e790f5` | S1 | skill-stickiness | 580 | 36646 | 73.3% |  |
| `fd230c` | S2 | tiered-review | 402 | 22731 | 86.8% |  |
| `fd2c24` | S2 | skill-stickiness | 791 | 52355 | 100.0% | **FLAGGED** |
| `fe4059` | S3 | skill-stickiness | 424 | 25818 | 53.6% |  |

**Two generations are over `R5a`'s 95% threshold, and both are `S2`.** `a9fcf9` is 99.0% of its
fixture's length; `fd2c24` is **100.0%** — it is byte-identical to
`fixtures/skill-stickiness.spec.md`, the same blob `79525341f6c4699417fc1f8b6b20d84b8ddaacad`
(`GENERATION-2-NOTES.md` §3).

**`R5a` forbids reading either as a win, and the arithmetic shows why that matters here rather than
in the abstract.** `fd2c24` scores 91/91 — full retention on `skill-stickiness` — for the trivial
reason that it *is* the fixture. A document that copied its input retains everything at full length
and tells you nothing about the instruction that produced it; limitation 1 says exactly this. `S2`'s
229/230 is therefore built on six generations, **two of which substantially copied their input and
one of which copied it exactly**. Its one-row miss is not evidence that a moderate trim nearly
survives; it is largely evidence that two of its six probes did not compress.

`fd2c24`'s free full retention is **not** read as evidence for `S2`, per
`GENERATION-2-NOTES.md`'s open-items table.

---

## 4. `R7` and `R3a` — what happens next, and what ships

**`R7`: `plan.md` T9 (the item-14 transmission test) and T10 (the item-14a gap test) are both
SKIPPED.** `R7` runs them only on a **candidate** arm (`S2` or `S3`) that clears `R2`. Neither did.
`S1`'s own result does not change that — and `S1` did not clear either.

**`R3a`: nothing ships**, and the phrase *"no candidate beat the control"* is the pre-registered
wording for this exact case — but it arrives via `R7`, not via `R3a` step 2, and the difference is
worth one sentence.

**`R3a` step 1 is what governs the arithmetic:** *"The survivors are the CANDIDATES (`S2`, `S3`)
that cleared R2 and T8 and T9. If there are none, the outcome is the null under `R7` and nothing
ships."* There are none. **`R7` then supplies the wording:** *"if no candidate clears R2, T8 and T9
are both skipped, whatever `S1` did, and T10 records the outcome under `R3a` as **no candidate beat
the control — ship nothing***. That is why this file's opening line uses that clause.

**What it must not be read to mean.** `R3a` step 2 — which does say *"no candidate beat the control:
ship nothing and record it"* — is **not** the route here: its stated precondition is *"If `S1`
cleared R2"*, and `S1` did not. So nothing in this run weighed a candidate against the control and
found it wanting; **no candidate was ever in `R3`'s compared set at all**, because that set is empty.
"No candidate beat the control" is the pre-registered *label for the null*, not a finding about a
comparison that happened.

**`R4`'s symmetric null is what happened, and it was pre-registered as the likely outcome.** `R4`
says the control is not exempt, that `R1` requires 460 clean row-judgements for one arm to pass and
1380 across all three, and that *"at any realistic per-row fidelity the probable result is that no
arm clears, control included"*. It did. `R4a` — a candidate clearing where the control does not —
did not arise.

**What `R4` also requires — and the task number in its text is NOT `plan.md`'s, and is not this
file's either.** `R4` says *"T10 must state plainly when the null is attributable to the instrument
rather than to the candidates"*. That `T10` is `PROTOCOL-2.md`'s own numbering, and its role table
maps it to **"whoever writes `docs/skill-evidence/spec-length.md`'s second section"** — which is
`plan.md`'s **T11**, the write-up. It is *not* `plan.md`'s T10, the item-14a gap test skipped three
paragraphs above; the same table maps this file's author to `T7`, *"whoever unblinds, applies the
gate, and writes `RESULTS-2.md`"*. Three documents number these tasks differently, exactly as
`GENERATION-2-NOTES.md` §1 warns.

**So the obligation is the write-up's, and it is answered here anyway rather than deferred.** The
data to answer it exists nowhere else, and leaving it for T11 would risk its being read as skipped
along with the gap test. **T11 still owes it in its own voice; what follows is this file's answer,
not a discharge of T11's duty.** The honest answer is **partly, and the split is visible.**

- **Not attributable to the instrument:** the instrument produced a defined disposition for every
  one of the 1380 cells, excluded no row as unciteable, and separated the arms by 23 rows
  (206 / 219 / 229). An instrument that could not discriminate would not produce that spread.
- **Attributable to the instrument, and not dismissible:** `S1` — *the shipped text* — dropped 11
  rows. The control failing its own gate is the instrument reading, and it says the gate as
  specified is not clearable at the fidelity this probe design achieves. `PROTOCOL-3.md` §4's
  limitation 7a compounds this from the other side: the tier-1 mechanical check no longer eliminates
  a verdict, so the residual scoring risk runs toward a **false pass**, and the retention counts
  above are more likely too high than too low.

**So the defensible reading is narrow and is stated as such:** under this instrument, at 230/230,
neither candidate trim survives, and neither does the text that already ships. **The question "can
`brainstorm.md`'s spec-authoring instruction be shortened without losing key points" is not answered
by a clean pass for any arm; what this run establishes is a ranking under a gate nothing cleared,
with `S3` clearly worse than `S1` (24 drops vs 11) and `S2` not readable as better because two of
its six generations copied their input.**

---

## 5. The instrument's three revisions, and the dispatch count each took

`PROTOCOL-3.md` §7 item 1 requires this and does not accept a citation in its place. **There were
three tier-1 instruments.**

| revision | what it changed | outcome |
|---|---|---|
| `PROTOCOL.md` item 8 | the original: 1–3 spans, no self-containment rule | **~1% of verdict files passable by construction**; every verdict of the one scored fixture invalidated; `RESULTS.md`'s null was about the instrument |
| `PROTOCOL-2.md` item 8 | 1–5 spans; a span self-containment rule added | **3 verdicts filed from 12 generations attempted**, under a cap of six attempts per shard. Nine generations have no verdict — missing data, not low retention |
| `PROTOCOL-3.md` §2/§3 | clauses F (fenced code), M (marked lines), E (trailing emphasis); the class-A / class-B split | **18 verdicts from 18 generations** |

**"We fixed it until data appeared" is only honest with the count and the reasons attached, so here
they are.** The third revision took **66 tier-1 scorer dispatches** — **34** for a first pass that
was **discarded in full**, and **32** for the pass that stands.

**Two accountings exist and they differ by two, so both are given rather than one being chosen.**
The *files on disk* are 64: the discarded pass left 32 shard files
(`retention-3/parts/superseded/deviating-prompt/` holds 50 — 30 `-shard.json`, 2
`fd2c24-{1,2}-attempt1.json` and 18 assembled verdicts) and the standing pass 32 (30 under
`retention-3/parts/`, 2 under `parts/superseded/`). The *dispatch* counts are 34 and 32: commit
`9c4860e` records 34 for the discarded pass against its 32 files and **does not itemise the
difference** — see §8 deviation 2, which withdraws an earlier inference about what those two extra
dispatches were; `SCORING-3-NOTES.md` §2 counts the standing pass at 32 dispatches / 32 files with
**zero** that wrote no file. **66 is the dispatch total and 64 the file total.** An earlier draft of this paragraph said 62, which is neither and
matches no record — it was `30 + 32`, the discarded pass's *shard* count added to the standing
pass's *dispatch* count.

The discard is `SCORING-3-NOTES.md` §5.1:
review found the first pass had been dispatched under a prompt that wrapped the frozen sections in
framing the pre-registration did not sanction, the framing measurably moved scorer behaviour
(distinct shared spans 28 → 18, class-B flags 82 → 59), and the whole pass was re-run rather than
kept. Tier 2 took **18** dispatches and tier 3 **17**, plus **one** `R6` re-run in tier 3.

**What that comparison could NOT establish, in `SCORING-3-NOTES.md` §5.1's own form — because the
flattering half of it travels more easily than the honest half.** The framing did **not** visibly
move `present`: 1326 → 1334, eight rows across eighteen generations. But eight rows is **inside**
§5.2's one-to-three-rows-per-generation noise floor, which over a corpus this size would allow a
swing of roughly ±18 to ±54. So — and this is the only form in which it should be quoted — **this
comparison could not have detected an inflation of `present` smaller than its own noise floor, and
the observed difference is smaller than that floor. It is not evidence that the framing left
`present` alone; it is evidence that the experiment was not powered to say.**

**And the two passes are not perfectly matched** — a different generation was re-run under `R6` in
each — **so read the shared-span rows as the finding and the `present` row as the absence of one.**

**The reason this iteration is not result-shaped is that it was blind**, and that is the whole
warrant. Every instrument revision was authored by a context that had not opened
`blind-map-2.json`, could not see which generation belonged to which arm, and made no comparison
between generations. Each rule is a function of markup and of disposition, never of a generation's
identity, length, or content, and each was applied identically to all eighteen. `PROTOCOL-3.md` §0
states this and §0's obligation 2 — *the moment arms become visible, iteration stops* — was
honoured: **tiers 2 and 3 were complete and committed before anything in this run read the blind
map** (commit `bb1ca14`, this file's is later).

### The uncitable-row diagnosis — `PROTOCOL-3.md` §7 item 5

The clearest single piece of evidence in either attempt that a null can be an artefact of an
instrument: under the v2 rule, **four `tiered-review` configuration rows (`-67`, `-68`, `-70`,
`-71`) and `skill-stickiness-46` could not be evidenced by any arm.** Their evidence is a line of a
fenced config block or a fixed-width string, and item 8's rule was written entirely in prose-sentence
terms — a config line satisfies none of its clauses on either end. A rule that refuses the only
admissible evidence for a row refuses it for every arm equally. Under the third instrument all five
are retained by at least one generation.

### Limitation 7a, beside limitation 7 — `PROTOCOL-3.md` §7 item 3

**Limitation 7 first, because "beside limitation 7" is unreadable without it.** From `PROTOCOL-2.md`
item 1 — this is the operative text, which replaced `PROTOCOL.md`'s and is not the superseded version
that file quotes for comparison:

> **7. Only ~20% of `present: true` rows are relevance-adjudicated, and the redesign makes an
> adjudication failure *less* consequential than it was.** Item 8a samples every 5th such row; the
> other ~80% are held to the mechanical check alone, which proves a cited span is really *in* the
> spec but not that it is *about* the row. Under the first attempt a single caught row invalidated
> all 91, so the expected cost of a lenient scorer was high. **Under this protocol a caught row is
> escalated to tier 3, which sees the spec and may restore it, and nothing is invalidated.** That is
> a deliberate loosening: it is what makes retention *defined*, and it is also what makes the
> residual scoring risk run harder toward a **false pass** than it did in the first attempt. Two
> things bound it, and neither closes it: tier 3 answers item 9's own question with more context
> than a 46-row batch scorer had, and under `R2` a false `present: true` can only ever inflate
> retention, never eliminate an arm. **The write-up states this as a loosening, reports the tier-3
> overturn rate, and does not describe the adjudication as complete.**

**In this run 260 of 1334 `present: true` rows were sampled — 19.5%.** The other 80.5% were held to
the mechanical check alone. Now 7a, quoted from `PROTOCOL-3.md` §4 and not softened:

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

**And the sentence `PROTOCOL-3.md` §4 requires be carried in its own words:** *if every arm comes out
at full retention under this instrument, the correct reading is that the instrument did not
discriminate, not that the arms are equivalent.* **That is not what happened** — no arm came out at
full retention and the spread is 23 rows — so the clause does not bind this write-up. It is quoted
because §4 requires it to appear where a reader meets the results, and because the reader is
entitled to see that the pre-registered escape hatch was not needed.

---

## 6. Class-B flag rate and tier-3 overturn rate — `PROTOCOL-3.md` §7 item 4

Both are required per generation. The class-B flag count is the number of `(row, problem)` flags the
item-8 checker raised on that verdict; the distinct-row count is smaller where one row carries two
problems, and the distinct set is what tier 3 received.

| id | arm | class-B flags | distinct rows flagged | tier-2 false | escalated (union) | overturned |
|---|---|---|---|---|---|---|
| `031cc4` | S3 | 1 | 1 | 2 | 3 | 0 |
| `054872` | S3 | 9 | 7 | 2 | 8 | 0 |
| `08ae18` | S1 | 0 | 0 | 3 | 3 | 0 |
| `26d7a2` | S3 | 8 | 5 | 4 | 9 | 1 |
| `2c4295` | S1 | 2 | 2 | 4 | 6 | 1 |
| `2d2629` | S3 | 20 | 19 | 6 | 23 | 0 |
| `47173f` | S2 | 0 | 0 | 2 | 2 | 0 |
| `48527b` | S1 | 11 | 9 | 4 | 13 | 0 |
| `66530f` | S2 | 2 | 2 | 1 | 3 | 0 |
| `6e7393` | S1 | 0 | 0 | 4 | 4 | 0 |
| `a9fcf9` | S2 | 0 | 0 | 3 | 3 | 0 |
| `b2b8cf` | S1 | 0 | 0 | 0 | 0 | 0 |
| `b49ff1` | S2 | 0 | 0 | 3 | 3 | 0 |
| `e085f2` | S3 | 0 | 0 | 2 | 2 | 0 |
| `e790f5` | S1 | 0 | 0 | 3 | 3 | 0 |
| `fd230c` | S2 | 0 | 0 | 3 | 3 | 0 |
| `fd2c24` | S2 | 2 | 2 | 1 | 3 | 0 |
| `fe4059` | S3 | 4 | 4 | 4 | 7 | 0 |
| **total** | | **59** | **51** | **51** | **98** | **2** |

**The overturn rate is `overturned / escalated` = 2 / 98 (2.0%)**, and `rescued = 96`.

**Per generation** — required by `PROTOCOL-3.md` §7 item 4 — **the table's own `escalated` and
`overturned` columns ARE the rate**, stated as a fraction rather than a percentage because on
denominators of 2 to 23 a percentage would imply a precision the counts do not carry. Two
generations have a non-zero rate: **`26d7a2` 1/9 and `2c4295` 1/6.** The other fifteen escalating
generations are **0/n**, and `b2b8cf` escalated nothing, so its rate is undefined rather than zero.

**Per arm, which `PROTOCOL-2.md` item 1 limitation 8 requires alongside the overall figure:**

| arm | escalated | overturned | overturn rate |
|---|---|---|---|
| S1 | 29 | 1 | 3.4% |
| S2 | 17 | 0 | 0.0% |
| S3 | 52 | 1 | 1.9% |
| **overall** | **98** | **2** | **2.0%** |

**One definitional note, because two documents define `escalated` differently and the denominator
above follows the frozen one.** `plan.md` T7 defines `escalated` as *"rows carrying a tier-3 record
(equivalently, rows tier 2 marked `establishes: false`)"*. Under `PROTOCOL-3.md` §3 that
parenthetical equivalence no longer holds — the escalated set is tier 2's rows **union** the class-B
rows — so the two halves of `plan.md`'s definition disagree by 47 rows. **The denominator here is
rows carrying a tier-3 record (98), which is the first half and the one item 8b's join actually
keys on.** Had the second half been used the rate would read 2 / 51 (3.9%).

**What a LOW overturn rate means, and it is not "tier 1 was right".** `plan.md` T7 spells out the
inference for a high rate; the low one has its own reading and it is less comfortable. Tier 3 saw
the spec and agreed with tier 1 on 96 of 98 rows it had been told were doubtful — including all 51
rows flagged for a *span* problem, where the span was withheld from it and it judged the spec
afresh. Two readings fit:

1. The class-B flags were mostly span-citation artefacts, exactly as `PROTOCOL-3.md` §3 supposed
   when it made them row-local rather than fatal, and tier 3 corrected for them.
2. Tier 3, which sees the spec and is *"swayed by how good the spec looks"* (item 8b's own stated
   cost, limitation 9), is a lenient judge, and 96 rescues is what leniency looks like.

**This run cannot separate them**, and the honest note is that reading 2 compounds limitation 7a in
the same direction — toward a false pass and inflated retention. **The one piece of evidence that
bears on it is the calibration pass**, below, which found 23 of 24 independently-flagged rows
recoverable — a comparable rescue rate on a corpus chosen before any of this was measured.

---

## 7. The calibration reading — item 12a

Item 12a's pass ran **before any new generation existed**, applying the tier-3 instrument unchanged
to the **24 flagged rows** of the first attempt's operative six adjudication passes.

**Result: 23 of 24 rows judged `present: true`.** Per generation: `4a73ef` 4/4, `80d9a2` 4/5,
`87e5a5` 5/5, `aa3199` 5/5, `d25798` 3/3, `db3e2d` 2/2.

**That confirms `spec.md` §2's diagnosis.** Item 8a was measuring span-citation craft under a
withheld spec, not retention: 23 of the 24 rows a spec-blind adjudicator called unestablished were
recoverable from the spec by a reader who could see it. Item 12a required the outcome be reported
either way, and had it come back the other way `PROTOCOL-2.md` would have bound unchanged with the
diagnosis recorded as wrong.

**Its known channel, restated:** the arms of those six generations are recoverable from this
branch's history (item 11 addition (c)). The mitigation is that the pass is scored per row with no
arm attached and never opened `blind-map.json` — a mitigation, not a closure. Nothing in this file
cites `calibration-2.json` as evidence about an arm.

---

## 8. Deviations

0. **Plan tasks T6, T7 and T8 ran in ONE phase context, not three.** `plan.md` §2 specifies
   *"Thirteen tasks. Each is one clean-context implement-phase agent"*, and its decomposition puts
   tier 2, tier 3, and unblind-and-gate in separate contexts. This run's driver superseded that and
   directed one agent to run all three. **Why it matters, stated as the strongest version of the
   objection:** under the plan's decomposition the separation between *scoring* and *seeing the
   arms* was **structural** — the tier-2 and tier-3 agents could not have read `blind-map-2.json`
   because they did not exist yet when it was still unread. Collapsed into one context, that
   separation becomes an **ordering claim by the agent that made it**, and `plan.md` §0 and
   `PROTOCOL-2.md` item 11 both forbid a context that has opened the blind map from dispatching any
   tier.

   **What still checks it, and it is weaker than structure but is not nothing.** The ordering is in
   `git log` rather than in prose: tiers 2 and 3 were complete and committed at `bb1ca14`, and the
   first read of `blind-map-2.json` and every artifact derived from it — this file included — landed
   at `92fea48`, a later commit. `adjudication-2/` and `escalation-2/` are byte-identical across
   that boundary. **A reader who does not take the agent's word for it can verify that the records
   existed before the unblinding commit; what no record can show is what a single context knew at
   what moment.** `RESULTS.md` §5 deviation 2 is the precedent for logging exactly this kind of
   collapsed separation rather than leaving it implicit.
1. **The `wc` / `R5a` figures were published early.** `GENERATION-2-NOTES.md` committed the
   per-generation line and byte counts, the fixture percentages and both `R5a` flags at the
   generation task, before tiers 1–3 ran. Logged here because that file's open-items table requires
   it. **What it discloses is bounded:** the table carries no arm, and
   `wc -l generated-2/*.md` reproduces it in one command from files that were already committed. It
   could not have steered a scorer, which is never shown a length figure — but it was published
   ahead of the write-up that owns it, and that is the deviation.
2. **`R6` re-runs in tier 1 — the standing pass and the discarded one. (Tier 3's is item 3.)** The
   one that matters is **`2d2629`, tier 1, for fabrication.** `R6` requires *every* re-run to be
   logged here with its reason, and this is the one that matters most. On its first attempt
   `2d2629` failed the **class-A** check: **twenty-nine spans, all in shard 2, were not verbatim
   substrings of the generated spec** — a whole shard of invented text. Fabrication is the one thing
   `PROTOCOL-3.md` §3 keeps fatal, and **both** shards were re-dispatched rather than only the
   offending one, because `R6`'s remedy is a re-run of the *verdict* and the verdict is the
   assembled file. The failing pair is preserved at
   `retention-3/parts/superseded/2d2629-{1,2}-attempt1.json`, hashes `d08134c3…` and `ae9a990c…`,
   both in `parts/assemble-log.txt`.

   **This cuts against the redesign's own convenience and is reported for that reason.** The class
   A/B split was not a way of making everything pass: on a first pass through eighteen generations
   the check still refused a verdict outright, on the one ground that matters. Seventeen filed with
   class-B problems present and unhidden; the eighteenth was refused for inventing text. **The
   honest count is one clean catch in eighteen, not two** — a class-A refusal also occurred on
   `fd2c24` in the discarded pass, and it is not counted, because `SCORING-3-NOTES.md` §3.1 shows
   that catch was read from bytes a still-running scorer then overwrote, in a pass that no longer
   stands. One instance is thinner evidence than "twice" would have been.

   **The full `R6` count, because `R6` says *every* re-run and an earlier draft of this item logged
   only the tier-3 one.** In the **standing** tier-1 pass there is exactly **one**: `2d2629`, above
   — `SCORING-3-NOTES.md` §2's table records 32 dispatches, 30 shards plus those 2 re-runs, and
   **zero dispatches that wrote no file**.

   **In the discarded pass the log is incomplete, and saying so is the honest form.** Two of its
   re-runs are on the record: **`fd2c24`'s two shards, re-dispatched whole after a class-A refusal
   for two fabricated spans** (commit `9c4860e`), and separately **one dispatch of `fd2c24-2` that
   crashed and was re-dispatched** (`SCORING-3-NOTES.md` §3.1, *"re-dispatched after an earlier one
   crashed"*) — a probe that wrote no file, `R6`'s first trigger. **Beyond that the accounting does
   not close: `9c4860e` records 34 dispatches against 32 shard files and itemises neither the
   difference nor a reason for each.** An earlier draft of this bullet asserted "two dispatches that
   wrote no file" as though the record said so; **it does not — that was an inference from the
   34-versus-32 gap, and it is withdrawn.** What can be said is that the discarded pass took more
   dispatches than it left files, that at least one of those was a crash, and that **`R6`'s
   log-every-re-run duty was not fully met by that pass and cannot now be met retrospectively.** No
   figure in this file rests on that pass; the gap is recorded because `R6` does not exempt a
   discarded pass from the log.
3. **One `R6` re-run, in tier 3.** `fe4059`'s first tier-3 dispatch **wrote no file** — `R6`'s first
   trigger verbatim — returning its answer as reply text instead. The dispatch was re-run whole
   against the byte-identical prompt and the re-run's reply is what `escalation-2/fe4059.json`
   holds. **The returned text was not harvested**: lifting it out of a transcript would have
   produced a record whose provenance is a channel no rule here sanctions. Full hashes in
   `TIER-23-PROVENANCE.md`. This is **not** a re-run in `SCORING-3-NOTES.md` §8a's sense — that
   bound is on tier-1 dispatches and excludes `R6` re-runs explicitly.
4. **Tier 1's whole first pass was discarded and re-run** — 34 dispatches — under
   `SCORING-3-NOTES.md` §5.1, for a prompt that added framing §5 did not sanction. Recorded there in
   full; named here because a discarded pass is a deviation a reader of results is entitled to meet.
5. **The tier-1 prompt carries two precedence sentences beyond the frozen text.** `PROTOCOL-3.md` §5
   says §2 and §3 go into the scorer prompt *verbatim*; the dispatched prompt is that text plus two
   sentences naming which of three contradicting frozen texts governs. `SCORING-3-NOTES.md` §5.1
   defends this as an argument, **not a measurement**, and this run's own finding is that added
   sentences move scorers. It is open.
6. **Two defects in the frozen `PROTOCOL-3.md` were recorded and NOT fixed**
   (`SCORING-3-NOTES.md` §9.1), because editing a frozen file would force a rewritten `FREEZE-3.md`
   hash row — the exact breach `FREEZE.md`'s closing section records as the thing not to repeat — or
   a second row for one path, which `spec_length_3_freeze_rows_still_hash_to_their_files` rejects. A
   tool implementing a frozen rule may be fixed to match it; the frozen rule may not move.

   **§9.1's conclusion, which is the transferable part and is owed here rather than the mechanism
   alone: a protocol that says a window-2 edit is permitted-with-disclosure, frozen by a record that
   makes any edit unrecordable, has one of the two rules doing nothing.** `PROTOCOL-3.md` §6's
   window 2 is a dead letter. Here that fails safe — the file cannot move — but **a later
   pre-registration should resolve the contradiction deliberately rather than inherit it**, by
   either freezing the protocol's *content* separately from its amendment log or dropping the
   window-2 permission it cannot honour.
7. **Two changes were made to committed checks, and they run in OPPOSITE directions. Both are
   disclosed, because an earlier draft of this item named only the first and called the net effect
   "strictly harder", which is false of the second.**
   - **(a) The re-point, which tightens.** The tier-2, tier-3 and gate checks now read their tier-1
     verdicts from `retention-3/` rather than `retention-2/`. No frozen text moved —
     `PROTOCOL-3.md` §5 replaced the tier-1 pass those items name, so `retention-3/` *is* the tier-1
     record — and the check recomputes **18** stride samples where only 3 were ever possible before.
   - **(b) The widened flagged set, which LOOSENS one direction of one check.**
     `spec_length_2_final_disposition_is_the_recorded_join` previously required
     `escalation-2`'s rows to be exactly tier 2's `establishes: false` rows. It now requires them to
     be that set **union** `PROTOCOL-3.md` §3's class-B rows. In the *missing-escalation* direction
     that is a tightening — **47** more rows must now be answered by tier 3 or the check fails. In
     the `escalated.difference(&flagged)` direction it is a **loosening**: escalating a class-B row
     was a hard error under the old check and is now required, so **47 of this run's 98 escalations
     (48.0%)** would have failed the old form. **The change is mandated by frozen `PROTOCOL-3.md` §3**, which flags
     a class-B row "exactly as an `establishes: false` flags a row under item 8a" and escalates it
     under item 8b — the old check simply predated §3 — but "strictly harder" was the wrong summary
     and is withdrawn.

---

## 8a. Revision log — every amendment to a governed file, by SHA

`spec_length_2_protocol_stops_moving_before_the_first_probe` requires every commit that touched
`PROTOCOL-2.md` beyond its first to be **named by SHA here**, and it executes that rather than
trusting this section to exist. The rule is deliberately stricter than any window requires: it does
not classify a commit by window, because a check that had to decide when the first probe happened
would be a check with a judgement call in it.

**`PROTOCOL-2.md` has two commits. One amendment.**

| commit | window | what it changed |
|---|---|---|
| `ea011be` | — | the first commit; the file itself. Not an amendment, and not required to be listed. |
| `128a5f2d4af90a157f5557795395a5f94d99ed5d` | **1** | **Four claims the file made about itself, corrected. No governed rule was added, weakened or clarified.** (a) every `spec_length_2_*` citation was in the present tense and none of those tests existed yet — the preamble now says to read each name as an obligation on the next task rather than a report, `R8`'s arm-symmetry included. (b) Item 15's departure table claimed completeness and was not complete; item 6's id-lexicographic-rewrite constraint is a new rule, now row 12a. (c) A citation to `RESULTS.md` §5 deviation 5 corrected to deviation 4. (d) The preamble now states that the amendment log covers every commit beyond the first regardless of window, and item 15 warns that a seven-limitation `.contains()` would pass on the fenced copy of the superseded limitation 7. |

**This was a window-1 correction — made before any probe of item 5 was dispatched, when even a
weakening would have been legal — and it weakened nothing.** It is logged because the check does not
care about the distinction, which is the right way round.

**`FREEZE-2.md`'s row for `PROTOCOL-2.md` was corrected in the following commit**, having recorded
`ea011be`'s hash for a file that had moved. That is `FREEZE.md`'s own recorded lesson repeating —
the freeze happened before the checking finished — and it was disclosed rather than quietly
re-hashed.

**`PROTOCOL-3.md` has one commit and no amendments**, checked separately by
`spec_length_3_protocol_stops_moving_after_the_first_verdict` against `SCORING-3-NOTES.md`.

---

## 8b. The two open file-access channels, re-disclosed

`PROTOCOL-2.md` item 11 and `plan.md` §0 both require these to be **re-disclosed in `RESULTS-2.md`,
not silently repeated**, and item 11 adds that every task in the run re-discloses them. They are
logged **as open, not as controlled.**

**(a) Every dispatched subagent runs with a working directory inside this worktree, so it *could*
read `blind-map-2.json` or recover an arm from `git log`.** `RESULTS.md` §7.6 deviation 7 recorded
exactly this for the first attempt's scorers: the harness gives no way to set a subagent's working
directory, the prescribed control — dispatch outside the worktree, hand over a byte-for-byte copy —
was run and was **incomplete**, and the channel was logged as open. This attempt inherits it
unchanged and on the same terms.

**This task's own 36 dispatches inherit it too, and that is said plainly rather than left to
inference.** 18 tier-2 adjudicators and **18** tier-3 dispatches — 17 that produced an
`escalation-2` file plus `fe4059`'s first, which wrote none and was re-run (deviation 3) — each ran
inside this worktree. **36, not 35: 17 is the count of escalation FILES, and a dispatch that wrote
no file still ran**, which is the same distinction §5 insists on when it separates 66 dispatches
from 64 files. The dispatch that produced nothing is exactly the kind this section exists to
disclose. The
mitigations were real but are instructions, not controls: their prompts were written **outside** the
repository, each in its own directory so a subagent could not list its siblings; each was told to
read one named file and no other, and not to search the repository; and **the tier-2 builder
asserts** that no prompt contains its generation's id, `generated-2`, `blind-map-2` or
`fixture-map-2`.

**The tier-3 builder has no equivalent assertion, and that asymmetry is disclosed rather than
glossed.** All four guards *could* have been written there and were not — including the
`generated-2` one: item 8b's template holds that string only inside the placeholder
`<the contents of generated-2/<id>.md>`, which the builder substitutes away, so such a guard would
have passed rather than been meaningless. (An earlier draft of this paragraph claimed the opposite
and was wrong.) Tier 3 is the tier item 8b already exempts from spec-blindness (limitation 9), so
the marginal risk is small; it is not zero, and **the gap is a fact about this task's tooling, not a
reasoned exemption.**

Nothing suggests any dispatch used the channel. **But *"was told only to read the prompt"* is an
instruction and not a control**, exactly as the first attempt's *"was told only to read the spec"*
was, and the channel stays open.

**(b) The calibration corpus's own arm map is recoverable from this branch's history**
(`RESULTS.md` §5 deviation 4; `PROTOCOL-2.md` item 11 addition (c)). It is immaterial to this attempt, which uses a
different id pool and never re-scores those generations for a verdict — item 12a's pass is scored
per row with no arm attached and never opened `blind-map.json` — but it is carried because this run
documents every channel it knows about. **A mitigation, not a closure.**

**Nothing in this run is described as fully blind.** Item 11 also carries the inherited
vocabulary/section-shape channel, tier 3's spec-visibility caveat (limitation 9 — tier 3 sees the
spec and so can be swayed by how good it looks), and §10's title-phrasing channel, which this file
adds to the list.

---

## 9. Freeze re-verification

`FREEZE-3.md` names one path, `PROTOCOL-3.md`, at `959e3f03…`, frozen at `2bee0ee4…`, and
`spec_length_3_freeze_rows_still_hash_to_their_files` re-hashes it on every test run — and also
checks that no path it names was already frozen by `FREEZE.md` or `FREEZE-2.md`. `FREEZE-2.md`'s and
`FREEZE.md`'s rows are re-verified by their own checks. All green at this commit; the full-suite
figures are in the task report.

**`FREEZE-3.md` deliberately does not freeze `retention-3/`.** Freezing an output would make an `R6`
re-run a freeze breach rather than the remedy it is.

**What none of these checks does, and §12's carry of `SCORING-2-NOTES.md` §9.5a item 12, question 3, records it as open: nothing verifies that the
freeze records are COMPLETE.** Every row that exists is re-hashed to its file; no check asks whether
an artifact that should carry a row is missing one. A freeze record can be wrong by omission, and
omission is the one failure mode nothing here looks for.

**Pre-registration ordering, executed rather than asserted:**
`spec_length_3_protocol_precedes_every_retention_3_record` requires `PROTOCOL-3.md`'s commit to
precede every `retention-3/` commit, and
`spec_length_3_protocol_stops_moving_after_the_first_verdict` requires every commit touching it to be
an ancestor of `retention-3/`'s introducing commit, with every amendment beyond the first named by
SHA in `SCORING-3-NOTES.md`. **`PROTOCOL-3.md` has one commit and no amendments.**

---

## 10. The title-phrasing channel, joined to the arms

`GENERATION-2-NOTES.md` §1 found that the generated specs' `# ` titles are **no longer arm-invariant**
— the first attempt's structural guarantee survives only for `tui-dc-picker` — and deliberately
deferred the join to this task rather than writing a second, weaker record of the secret before the
unblind. **T8's half is discharged here; the write-up (`plan.md` T11) owes it in its own voice, since
`GENERATION-2-NOTES.md` and `SCORING-2-NOTES.md` §9.5a item 12 both owe it to T8 *and* the
write-up.**

| fixture | distinct titles | arms carrying each |
|---|---|---|
| `tui-dc-picker` | 1 | all six: S1×2, S2×2, S3×2 |
| `skill-stickiness` | 2 | `# Spec: skill stickiness` ×5 — S1×2, S2×1, S3×2; `# Spec: Skill Stickiness` ×1 — **S2** |
| `tiered-review` | 3 | `# Tiered (cascade) code review for drovr` ×4 — S1×2, S2×2; `… — spec` ×1 — **S3**; `# Spec: Tiered …` ×1 — **S3** |

**The answer is mixed, and the half that is uncomfortable is stated first.** On `tiered-review` the
phrasing **separates `S3` perfectly**: both variant titles are `S3`'s two generations, and the
majority title is exactly `S1`'s and `S2`'s four. On six generations that is not a small
coincidence. On `skill-stickiness` it does **not** separate: the lone variant is `S2`'s, and `S2`'s
other generation carries the majority title, so no arm is picked out. On `tui-dc-picker` there is
one title and nothing to separate.

**So: no consistent title↔arm mapping across the run, but a complete separation within one fixture.**
That belongs beside item 11's other channels as a **limitation on the blinding**, not as a closed
question — and the reason it did not bite here is structural rather than lucky: **tier-1 scoring is
sharded one generation at a time, so no scorer ever held two titles of one fixture**, and tier 2
never sees the document at all. The one stage that would have held two titles of one fixture side by
side is item 14a's pairing adjudicator — **and `R7` skipped it.**

---

## 11. Recorded doubts — `R6a`

`R6a` forbids silently accepting a verdict believed wrong, and forbids re-rolling one. Two doubts
are recorded rather than acted on.

1. **`fd2c24`'s 91/91 is not a measurement of `S2`.** The generation is byte-identical to its
   fixture, so full retention is arithmetically forced. The verdict is correct about the document
   and says nothing about the instruction. It is not re-run — `R6`'s trigger list is closed and does
   not include this — and it is not quietly dropped from `S2`'s union either, because dropping it
   would be a hand-authored exclusion of exactly the shape `R8`'s rejected alternative describes.
   It stands, flagged, and `S2`'s figure is read with it.
2. **`2d2629` carried 20 class-B flags — a third of the run's total — and tier 3 rescued all 23 of
   its escalated rows.** A verdict that filed on that many flags and then lost none of them at tier
   3 is the shape limitation 7a warns about. There is no rule under which it fails, and it did not
   change `S3`'s outcome (`S3` was eliminated 24 times over), but a reader comparing arms on
   `tiered-review` should know the arm with the worst spread also produced the most heavily flagged
   verdict in the corpus.

---

## 12. What `SCORING-2-NOTES.md` and `SCORING-3-NOTES.md` still carry

`PROTOCOL-3.md` §7 item 6 requires that this file **adds to** rather than replaces those lists — and
§7's preamble says an item is *"not discharged by citing this file — each must appear where a reader
of the results will meet it"*. **An earlier draft of this section discharged four obligations by
naming them. They are carried below instead.**

**Three dispositions, not two.** `SCORING-3-NOTES.md` §8 item 8 grades each: items **1, 4, 7(b) and
§9.5a 14 are discharged**; items **2 and 3 are superseded** by `PROTOCOL-3.md` §5's own cap and
sharding and are *"not a debt"*; the rest are open. An earlier draft's binary "stands except
discharged" list wrongly re-opened 2 and 3.

**§9.5 item 7(a) — the duplicate-shard finding, owed here in as many words.** In the second
attempt's tier-1 pass **one shard's attempt 4 was byte-identical to its attempt 2** — the only
duplicate among 150 shard files. So *"six attempts per shard"* meant **six dispatches, not six
independent samples**, and a verbatim re-emission could not be told from a cached completion. **This
run's answer was to hash every shard as it landed** (§9.5 item 7(b), discharged):
`retention-3/parts/assemble-log.txt` and `parts/prompt-hashes.txt` are committed, all shard files
have distinct SHA-256s, and `TIER-23-PROVENANCE.md` does the same for tiers 2 and 3. **That rules
out the one observable and nothing more — an attempt is still a dispatch, not an independent
sample**, and no figure in this file may be read as resting on independent repeats.

**§9.5 item 8 — the cap extension AND its failure, together, because reporting one without the
other misdescribes the run in the flattering direction.** The second attempt's tier-1 pass extended
its per-shard attempt cap from three to six, post hoc, for four ids that had not filed. **It
produced no verdict for any of the four.** The extended cap is why "3 of 12" is the right
denominator to remember and not "3 of 12 on a stingy budget": the budget was raised and the yield
did not move. That is evidence about the instrument, not about the arms.

**§9.5a item 10 — the present-rate doubt, which no tier-1 number may be published without.**

**Whose 99.8% it is, stated first, because this file has already got it wrong once and
`SCORING-2-NOTES.md` §9.3 records an earlier draft of *that* file getting it wrong in the same
way.** The **99.8%** figure is the **second** attempt's: §9.3's *"Every `skill-stickiness` round of
this run sat at a 99.8% present rate (2543 of 2548)"*. **It is not the first attempt's, and `99.8%`
appears nowhere in `RESULTS.md`.** What v1 recorded is the same *shape* and the same *doubt*
qualitatively — §7.5's *"implausibly high … four of six generations scored 91/91"* — and §9.3
forbids characterising v1's rate **at all, "not as higher, lower, or close"**, because such a phrase
is a quantitative claim wearing a qualitative coat. **No v1 aggregate is derived here, so no
comparison to v1 is drawn.**

**The doubt itself, which stands undischarged:** a tier-1 pass that marks essentially everything
present is measuring something other than retention, and §9.3 narrowed the observation to
`skill-stickiness` without withdrawing it.

**This run's rates, with the populations kept apart — because the comparison that suggests itself is
between two different denominators, and an earlier draft made it.** 99.8% is `skill-stickiness`
**only**, across all of that task's shard dispatches. This run's overall tier-1 rate is
**1334/1380 = 96.7%**, which pools three fixtures over one filed verdict per generation and is
therefore **not** the same population. Per fixture, which is:

| fixture | tier-1 present | rate |
|---|---|---|
| `skill-stickiness` | 540 / 546 | **98.9%** |
| `tui-dc-picker` | 321 / 330 | 97.3% |
| `tiered-review` | 473 / 504 | 93.8% |

**The closest comparable figure is `skill-stickiness` at 98.9% against 99.8% — a gap of about five
rows over 546, inside this file's own one-to-three-rows-per-generation noise floor.** It is
*closer*, not like-for-like: **one population difference remains**, since 99.8% is a rate over all
of that task's shard dispatches, including superseded and re-run ones, while 540/546 is one filed
verdict per generation. No arithmetic here removes that, so the comparison is indicative and nothing
is concluded from its sign. What can be said without it: **a 98.9% present rate on the fixture that
raised the doubt does not retire the doubt.** A rate that high
on the fixture that raised it is the same doubt limitation 7a states from the other direction, and
the two should be read together. `tiered-review`'s 93.8% is the fixture where tier 1
discriminated **most** — all three show real spread (`SCORING-3-NOTES.md` §4) — and it also carries
**42 of the run's 59 class-B flags**.

**§9.5 item 9 belongs beside it, because it points the same way.** That tier-1 well-formedness was
not reliably reachable — **3 verdicts filed from 12 generations, each given up to six corrected
attempts per shard, by scorers handed the frozen rule verbatim** — is itself evidence about the
instrument: **the tier-1 gate was measuring scorer compliance, and the discriminating load
accordingly sits on tiers 2 and 3 and the `R2` gate**, exactly as `PROTOCOL-3.md` §4's own sentence
puts it.

**§9.5a item 12 is four questions, not one, and "nothing else keeps [them] alive" is its own
wording.** All four, with their state:

1. **The title-variance channel** — **answered in §10 of this file, which discharges T8's half.**
   §9.5a item 12 and `GENERATION-2-NOTES.md` both owe it to *T8 and the write-up*, so **T11 still
   owes it in its own voice**, exactly as it owes `R4`'s attribution question.
2. **Whether re-dispatching two transmission questions was right** (*"decide before T9; the revert is
   `git revert 6268365`"*) — **still open**, and now moot for the outcome, since `R7` skipped the
   transmission test entirely. It remains open as a record question and the revert is still the
   remedy if a later attempt runs that test.
3. **The 20 `FREEZE-2.md` rows are not checked for completeness** — **still open**. §9 above
   re-verifies that every row that *exists* hashes to its file; nothing checks that every artifact
   that *should* have a row has one. A freeze record can be complete-and-wrong only by omission, and
   omission is exactly what no check here looks for.
4. **The leak check tells a reader to "re-run under `R6`" for a case `R6`'s closed trigger list does
   not cover** — **still open**. A generated spec that leaks is not one of `R6`'s three triggers, so
   the instruction is unactionable as written; `GENERATION-2-NOTES.md` reached the same conclusion
   independently ("`R6` does not license a re-run either — its trigger list is closed"). Repairing
   it would mean editing a frozen file, so it is recorded, not fixed.

**§9.5a item 13 — a frozen-instrument inconsistency, and this run's third departure from item 10's
template.** Item 9's first blockquote — handed verbatim to every scorer, **including all 96 of
`SCORING-2-NOTES.md`'s T5b's dispatches** — says a paraphrase *"may be evidenced by up to three spans"*, while
the same prompt's item-8 rules say **1 to 5**. Scorers followed the rules. With `SCORING-2-NOTES.md`
§2a this is the **second place item 10's template contradicts the items it claims to carry**, and
the two belong together as one finding about item 10. **`PROTOCOL-3.md` §5 answered it by dropping
the contradicting clause from the dispatched tier-1 prompt** — pre-registered, and asserted in
`tools/build-tier1-prompts-3.py` so that a drift in the frozen text fails the build rather than
silently shipping both numbers. **That drop is a third departure of the dispatched prompt from item
10's frozen template**, alongside the two precedence sentences deviation 5 records; unlike those, it
is pre-registered rather than residual. **Item 8b's tier-3 prompt is NOT amended**, so the clause is
still handed to escalators verbatim — inert there, since an escalator is never shown a span.

**§9.5 item 6 — `FREEZE-2.md` gained no row in this run.** It stands closed at its 20 rows (21 with
T1's `PROTOCOL-2.md` row). The third freeze record is a separate file, `FREEZE-3.md`, with one row;
nothing was appended to `FREEZE-2.md` and nothing in it was rewritten.

### `SCORING-3-NOTES.md` §3.1 — file-exists is not file-final

**Owed here by §8 item 4, and carried rather than cited because it is the most transferable finding
in the run.** In the discarded first pass, a polling loop watched for a shard file to appear, saw
it, and graded those bytes. **The subagent was still running.** It then ran its own verification
pass, found a fabricated span itself, and **rewrote the same path** before its completion
notification arrived — so an earlier draft of `SCORING-3-NOTES.md` §3 cited, as the record of a
fabrication, a file containing none. **The rule: wait for the writer to *finish*, not for its output
to *appear*.** It is now guarded by
`spec_length_3_every_verdict_is_its_shards_concatenated` — but **that guard proves referential
consistency at commit time, not provenance**: a shard overwritten *and* re-assembled before the
commit would still pass. This task's tier-2 and tier-3 passes were therefore held until every writer
had signalled completion, and their outputs were re-hashed after a wait to confirm they had stopped
moving.

### `SCORING-3-NOTES.md` §8a — the accusation, and what this file can and cannot say to it

**§8a is on the carry list deliberately: it is the strongest objection to the whole run, and a
write-up discharging a checklist mechanically must not be able to omit it.** Stated as a hostile
reader would:

> **Every revision of the tier-1 check has loosened it. None has ever tightened it.** The span cap
> went 3 → 5; a self-containment rule was added that refused valid evidence; that rule was then
> widened three ways and its two commonest failures made non-fatal. **The yield went 0 → 3 of 12 →
> 18 of 18.** That is the exact shape of an instrument tuned until the data appeared.

**What is true in it.** The direction is real and one-way on the gate, and §8a concedes the core
charge outright: **the accusation is about reactivity across revisions — each instrument was
designed in response to the previous one's poor yield — and nothing rebuts that, because it is
true.** What the pre-commit ordering rules out is only the narrower and more damning charge, that a
rule was chosen after seeing the numbers it would produce; `git log` settles that one.

**What bounds it, with this run's own figures substituted for §8a's.**

1. **Fabrication was never loosened and it still fires** — `2d2629`, twenty-nine invented spans,
   refused outright (deviation 2). **One clean catch in eighteen, not two.**
2. **One gate-relevant tightening shipped in the same diff as the loosenings** — clause F's
   leading-whitespace rule, narrowed after review found it over-accepting. The suite's growth from
   109 to 121 checks is **mostly bookkeeping and provenance**, and citing it as evidence of a
   tighter gate would conflate two things.
3. **The instrument still discriminates, and this is the strongest of the four because it is a fact
   about the output rather than the author's intentions.** **28 of 230 rows are dropped by at least
   one generation** — §8a's figure was 26, computed on tier-1 dispositions before tier 3 ran;
   recomputed on **final** dispositions it is 28, tier 3 having overturned two rows to `false`. Four
   of eighteen generations retain everything (`47173f`, `66530f`, `a9fcf9`, `fd2c24`), and final
   present rates run from 72/84 to 55/55. An instrument tuned to pass would not leave that spread.

**§8a's unanswered question is explicitly assigned to this file: *"Is 18 of 18 the right yield, or
merely a reachable one?"*** There is no external standard for how many verdicts a correct tier-1
pass should produce, so the question cannot be answered directly. **What this run adds is the
downstream check §8a nominated**, and the answer is better than the accusation predicts: tier 2
sampled 19.5% of present rows and returned 51 `establishes: false`; tier 3 judged 98 rows with the
spec in hand; and **`R2` eliminated all three arms, control included.** A tier-1 pass tuned to make
arms pass would have made an arm pass. **That is not proof the yield is right — a uniformly lenient
tier 1 inflates every arm equally and could still leave a spread — but it is the one piece of
evidence §8a said would bear on it, and it points away from the accusation rather than toward it.**

**The bound on further iteration, restated because it now binds the next agent rather than this
one.** A **re-run** is any tier-1 scorer dispatch against a generation that already has a
`retention-3/` verdict or a superseded shard on disk, other than an `R6` re-run triggered by a
class-A failure. **By that count one has happened** — §5.1's corrected-prompt pass. **A second must
be escalated to the human driver rather than taken by an agent.** Nothing enforces this; its only
teeth are that the count is derivable from `git log` and `retention-3/parts/superseded/`.

**§5.2's inter-dispatch noise floor — one to three rows per generation between
two dispatches of the same document — bounds every count in this file.** `R2` is a full-retention
gate, so the floor does not corrupt the gate itself; it does mean the 219 / 229 / 206 spread should
not be read to single-row precision. **It rests on three generations, which is thin.**

`TIER-23-PROVENANCE.md` holds the SHA-256 of every tier-2 and tier-3 prompt as dispatched and every
reply as read.

---

## 13. What this run does not establish

- **That `S2` or `S3` is safe to ship.** Neither cleared. Nothing ships.
- **That a shorter instruction cannot work.** The gate eliminated the shipped text too, so this run
  did not test candidates against a bar the control could clear.
- **That the retention counts are accurate to a row.** Limitation 7a runs them high; §5.2's noise
  floor runs them ±1–3 per generation.
- **That the arms are equivalent.** They are separated by 23 rows and the ordering is `S2` 229,
  `S1` 219, `S3` 206 — but `S2`'s figure is contaminated by two copy-flagged generations and may not
  be read as a win.
- **That blinding held perfectly.** §10's title channel separates `S3` completely within one
  fixture. No stage that could exploit it ran.
