# Tier-1 scoring notes — second attempt

`plan.md` T5a/T5b/T5c say a boundary-rejected span is *"a malformed shard → re-run whole under `R6`,
logged for `RESULTS-2.md`"*. `RESULTS-2.md` is not written until T8, so the log has to live
somewhere in between. **This is that file.** T5a opened it; T5b and T5c append to it; T8 reads it.

**Nothing structural points at this file** — not `plan.md`, not `PROTOCOL-2.md`, not `FREEZE-2.md`,
and no test opens it. It survives only because each tier-1 handoff names it. Stated here so a reader
does not mistake it for a checked artifact. (`plan.md` is likewise not in this repository; it lives
at `~/.local/share/drovr/runs/spec-length-ab2/plan.md`, the path `PROTOCOL-2.md` §1 discloses.)

**This file records procedure and counts. It records no arm**, and its author never opened
`blind-map-2.json`. See §5 for what its author *did* come to know, which is not nothing.

**T5a did not produce verdicts. It is blocked**, and §4 is the finding. `retention-2/` deliberately
holds no `<id>.json`.

---

## 1. The calibration probe, and what it settled

T2 handed forward one open question above all others: **`PROTOCOL-2.md` item 8's span
self-containment rule refuses 84.9% of the first attempt's real spans — 957 of 1127.** A rule that
refuses nearly everything by construction is precisely the defect `RESULTS.md`'s null was about, so
T2, T3 and T4 each carried the same instruction forward: **T5 scores ONE generation first and looks
at its refusal rate before dispatching the other 17.**

That is what ran, before any other generation was dispatched.

| | |
|---|---|
| generation | `66530f` (first id-lexicographically of the six `skill-stickiness` ids) |
| shards | 2, per item 10's fixed table — `-01`–`-46`, `-47`–`-91` |
| rows scored | 91 of 91 |
| spans written | 128 |
| **spans refused by item 8's boundary rule** | **13 — 10.2%** |
| rows carrying a refused span | 11 (`-02`, `-25`, `-43`, `-44`, `-45`, `-46`, `-47`, `-55`, `-59`, `-75`, `-83`) |

**10.2%, not 84.9%.** T2 bounded its own figure correctly: those 1127 spans were written under
`PROTOCOL.md`, which had **no boundary rule at all**, so 84.9% measured v1's quoting habit rather
than the rule's severity.

**What it does NOT show, and an earlier draft of this section claimed it did.** It is not that "a
scorer told the rule mostly obeys it": **item 10's frozen template does not carry item 8's mechanical
rule.** It carries a prose paraphrase — *"must begin at the start of a sentence, table cell, list
item, heading or line-block, and end at the end of one"* — and nothing more. `tools/build-tier1-prompts.py`
inserts item 9's two blockquotes and the shard's ledger rows; item 8's boundary blockquote is never
extracted and never reaches a scorer. So the drop from 84.9% to 10.2% is a difference between two
corpora and two quoting habits, **not** evidence that instruction fixed anything. §4 is what the
probe actually pointed at, and reading 10.2% as reassurance is the error that let five more
generations be dispatched before the real problem was visible.

### The refusal shapes

Item 8's worked example is a span **clipped at a hard wrap**, leaving one fragment ending mid-phrase
and one beginning mid-phrase (`invalidated/db3e2d.json`). **None of the 13 is that.** Every one is a
complete unit — a whole bullet, a whole numbered item, a whole line-block row, or a whole sentence.

**No left/right split is published here.** Attributing each of the 13 to the end that refused it
requires evaluating item 8's predicates outside the committed Rust check, and this run keeps exactly
one implementation of that rule (§4a). What follows is hand-verified against the prose rule, span by
span, for two spans only, and is offered as mechanism rather than proportion.

**Left-end refusals are real and are the counter-intuitive half.** A span can be a flawless complete
sentence and still be refused because of the line **above** it. Verified: `66530f.md:353`, the whole
bullet `` - `systematic-debugging` — *"…reproducing before fixing."* ``, is refused because line 352
ends in `*`. Item 8's block-start clause requires the preceding line to be blank, a heading, a table
row, or to end in `.!?:;`; a bullet closing inside an emphasis run ends in none of those. **So in a
list of such bullets each line disqualifies its neighbour**, which is why refusals cluster.

**The same-line rule has the same blind spot.** Verified: `-25` span 2 begins at column 66, directly
after `…no skill applies." `. It is the start of a sentence, but item 8 requires the span to be
immediately preceded by `. `, `! ` or `? `, and what precedes it is `." ` — a full stop behind a
closing quotation mark. Refused. An earlier draft of this file called this span a genuine mid-phrase
clip; it is not, it is the delimiter problem again on a third clause.

---

## 2. The re-run policy, fixed before the first re-run was dispatched

`R6` permits a re-run when *"a verdict fails the item-8 mechanical check"* and requires every re-run
to be logged with its reason. It sets **no cap**, and an uncapped re-run loop against a rule that
refuses a routine markup shape is a slow way to arrive at `R6`'s own prohibition — re-running until
the output is acceptable. `RESULTS.md` §7.4 is the precedent that a re-run can itself fail.

**So a cap is fixed here, and it was fixed before any re-run had been dispatched or seen.**

1. **At most two re-runs per shard — three attempts in all.**
2. **The cap applies identically to every shard of all eighteen generations**, whatever any attempt
   returns. It is not re-decided per generation and not re-decided per arm.
3. **A re-run re-runs the shard WHOLE**, with the byte-identical prompt. Nothing is patched, no span
   is edited, and the prompt is never adjusted to steer the scorer away from the refused shape —
   that would be amending a frozen instrument mid-measurement.
4. **A shard still failing after its third attempt is reported as a failing verdict, not fixed** —
   recorded here and in `RESULTS-2.md` with its refused spans, exactly as `RESULTS.md` §7.4 recorded
   five verdict files left un-re-run. **A verdict the instrument refused is missing data, not a low
   retention score**, and T8 must treat it as such: nothing about what the arm retained can be read
   off a file the mechanical gate rejected.
5. **Every attempt is logged in §3**, including the ones that succeed.

**What is checkable about "fixed before", stated precisely, because the strong version is false.**
Commit `0b4cf46` adds §1–§2 with an **empty** §3 and only the two round-0 shards; commit `d8d28a3`
adds every re-run artifact. **That ordering between two commits is checkable.** The ordering between
the commit and the *dispatch* is not: a subagent dispatch leaves no trace in the repository, and
commit timestamps are author-settable. So the honest claim is: the repository carries the strongest
ordering evidence it can, and that the dispatch followed the commit remains the author's word. An
earlier draft of this section said "checkable rather than a matter of trust", which claimed exactly
the part that is trust.

**Why a cap is arm-symmetric.** Scorers are blind — item 10 hands them one generated spec, their own
shard's ledger rows, and items 8 and 9, and nothing else. The cap is a fixed number applied to every
shard before any of them ran. No arm can carry a row a different arm cannot.

### 2a. A delivery defect in this task's own tooling, and the corrected re-run

**Pre-registered here before the corrected prompt was dispatched, for the same reason §2 was.**

**Item 10 does not say the scorer gets the template. It says what the scorer gets, and item 8 is on
the list:**

> The scorer is handed **only** the generated spec file, its fixture's ledger rows for its shard, and
> **the item-8 schema** plus the item-9 definition.

`plan.md` T5a says the same in fewer words — *"items 8 + 9"*. **Rounds 0 through 2 delivered item 9
and not item 8.** `tools/build-tier1-prompts.py` substituted item 9's two blockquotes through the
template's explicit placeholder, and for item 8 the scorer got only the template's one-sentence
paraphrase — *"begin at the start of a sentence, table cell, list item, heading or line-block"* —
which is **not** the character-level predicate the gate runs. Item 8's self-containment blockquote
reached no scorer in 26 dispatches.

**That is this task's tooling, not the frozen protocol**, and T4's handoff records the lesson it
breaks: *"when one artifact looks wrong, the first question is whether the tooling did it, because
that question is about all of them."* §4 as first written asked the wrong question. It concluded from
thirteen attempts that the instrument is unusable, when all thirteen were draws from **one**
distribution: the same under-delivered prompt, held byte-identical by §2.3 precisely so re-runs would
measure variance. The hypothesis that would actually settle it — *does a scorer told the real rule
satisfy it?* — was never run.

**The correction, and why it is not steering.** Item 8 is now handed over verbatim and entire,
appended after the template. **The template itself is unedited and still goes out byte-identical** —
this adds to the handover, it does not rewrite frozen text. §2.3 forbids adjusting the prompt to
steer a scorer away from the refused shape; delivering a frozen rule that item 10 says the scorer
receives is the opposite of inventing a hint, and the same bytes go to every shard of every
generation, so it is arm-symmetric by §2's own argument.

**The budget, fixed before the outcome is known.** One generation is probed first — `66530f`, as in
§1, so the two are comparable — and the other five are dispatched only if the corrected prompt
materially changes the refusal rate. **The §2.1 cap restarts at two re-runs per shard for the
corrected prompt**, because a corrected delivery is a different dispatch from the one §2.1 capped;
`R6` licenses the re-run independently, its trigger being a verdict that failed the item-8 mechanical
check. Attempts under the old prompt stay in the §3 log and are not deleted.

**What this costs, said plainly:** if the corrected prompt clears the gate, then §4's blocked finding
was a finding about this task's tooling wearing the protocol's clothes, and the honest record is that
it took two review rounds to ask whether the tool did it.

**Superseded attempts are kept.** Every shard of every attempt is at
`retention-2/parts/superseded/<id>-<k>-attempt<n>.json`, and the raw failure report of every round —
naming every refused span — is at `retention-2/parts/superseded/refusal-reports/`. `parts/` is item
8's sanctioned working-material directory and no test recurses into it.

---

## 3. Re-run log

Rounds map to attempts unevenly because `66530f` ran once alone as the §1 calibration probe. **Rows 1
and 3 are first runs, not re-runs**; they are here so the log is the whole dispatch record.

| round | shards | attempt | trigger | outcome |
|---|---|---|---|---|
| 0 | `66530f` ×2 | 1 | first run — the §1 calibration probe | **failed** the item-8 check |
| 1 | `66530f` ×2 | 2 | `R6` — verdict failed the item-8 mechanical check | **failed** |
| 1 | the other five ×2 each | 1 | first run | **all five failed** |
| 2 | `66530f` ×2 | 3 | `R6` — same trigger; **cap of §2.1 now exhausted** | **failed** |
| 2 | the other five ×2 each | 2 | `R6` — same trigger | **all five failed** |

**Thirteen shard-pair attempts, 26 shard dispatches. 1709 spans written. Zero passing verdicts.**

| generation | attempt 1 | attempt 2 | attempt 3 |
|---|---|---|---|
| `66530f` | 13 / 128 | 16 / 134 | 14 / 132 |
| `6e7393` | 16 / 125 | 19 / 129 | — |
| `e085f2` | 20 / 123 | 21 / 121 | — |
| `e790f5` | **8 / 137** | 15 / 146 | — |
| `fd2c24` | 17 / 138 | 19 / 141 | — |
| `fe4059` | 16 / 128 | 13 / 127 | — |

*(boundary refusals / spans written. A verdict passes only at **zero**.)*
**Totals: 207 refusals / 1709 spans = 12.1%.** Per attempt the rate runs **5.8% to 17.4%** — a 3×
spread, so it is **not** flat, and an earlier draft of this file called it flat. What survives
without that word: the best single attempt in thirteen still carried 8 refusals where a pass requires
0.

### 3a. A second, independent failure the boundary rule does not explain

**Ten of the thirteen attempts also violated item 8's no-shared-span rule** — *"no span may be cited
for more than one row"* — which item 10's prompt states verbatim to every scorer:

| round | verdicts carrying a shared span |
|---|---|
| 1 | `6e7393` ×1, `fd2c24` ×2, `fe4059` ×2 |
| 2 | `66530f` ×1, `6e7393` ×1, `e085f2` ×1, `e790f5` ×1, `fe4059` ×1 |

**8 of the 12 non-probe verdicts fail this rule as well as the boundary rule.** It is plain scorer
non-compliance with an instruction it was given, it has nothing to do with item 8's boundary
condition, and **repairing the boundary rule would not have produced passing verdicts in those
eight.** The round totals in the committed reports (`98 problem(s)`, `106 problem(s)`) are the
boundary refusals (93, 101) plus these ten. Any claim that T5a was blocked *solely* by the boundary
rule is false, and §4 is scoped accordingly.

---

## 4. The findings

### 4a. Item 10's prompt asks for spans that item 8's rule refuses

The prompt says a span *"must begin at the start of a sentence, table cell, list item, heading or
line-block, and end at the end of one"*. Item 8's mechanical rule then applies a different test,
written in raw characters:

- a span **begins** on a boundary only if the **preceding line** is blank, a heading, a table row, or
  ends in one of `.!?:;`;
- a span **ends** on a boundary only if its **last character** is one of `.!?:;`, or it ends at
  end-of-line with the next line blank or opening a new block, or it is followed by `|`.

Most refusals are one of two shapes, both of which satisfy the prompt and fail the rule: a line whose
sentence closes inside a `**…**` or `*"…"*` run **terminates in `*`, not `.`**; and a fixed-width
line-block row, or a line following a ``` ``` ``` fence, ends in no terminator at all. Either one
disqualifies the span **and** the span that opens on the next line.

**The proportions are not measured here, and deliberately.** Splitting 207 refusals into left-end and
right-end causes needs a second implementation of the boundary predicates, and this run keeps exactly
one — the committed Rust check — so that there is never a second answer to what the rule refuses.
§1 publishes no split for the same reason, and its two hand-verified spans illustrate the mechanism
rather than measure its share.

### 4b. What the rule admits, measured — and the number that matters

Citing every distinct non-blank line of each generated spec as a candidate span and handing the
verdict to the authoritative check (`tools/measure-span-admissibility.py`; raw output committed at
`retention-2/parts/superseded/refusal-reports/admissibility.txt`):

| generation | lines | admissible | | citable prose lines | admissible | |
|---|---|---|---|---|---|---|
| `66530f` | 517 | 121 | 23.4% | 450 | **55** | 12.2% |
| `6e7393` | 431 | 100 | 23.2% | 377 | **47** | 12.5% |
| `e085f2` | 346 | 101 | 29.2% | 295 | **52** | 17.6% |
| `e790f5` | 463 | 124 | 26.8% | 396 | **58** | 14.6% |
| `fd2c24` | 636 | 123 | 19.3% | 566 | **54** | 9.5% |
| `fe4059` | 341 | 100 | 29.3% | 295 | **55** | 18.6% |
| **total** | **2734** | **669** | **24.5%** | **2379** | **321** | **13.5%** |

**The `prose` column is the one to read.** 348 of the 669 admissible lines are furniture — every
whole table row, every `|---|---|`, every `---`, and all but one heading are admissible, and none is
something a scorer would cite as evidence of a ledger row. Excluding them:

> **Each document offers 47–58 admissible whole prose lines. A verdict must evidence 91 rows.**

That is the finding, and it is stronger than the ratio: the citable pool is **smaller than the
requirement**. A verdict marking most rows present therefore cannot be written from whole prose lines
at all — it must lean on sub-line spans and on structural lines, in a corpus where the rule refuses
7 of 8 prose lines outright.

**Bounds, because this figure will get quoted.** It counts **whole lines**, which is the strategy
item 10's prompt prescribes; admissible sub-line spans exist and are not counted, so the true pool is
larger than 47–58. Lines are **deduplicated**, so a line counts admissible if any one of its
occurrences is — an upper bound on per-position admissibility. And it is a property of this corpus's
markup, not of prose in general.

**On arm-symmetry — an earlier draft of this file overclaimed, and in a length experiment that
matters.** It said the admitted *rate* varies "with no relation to any arm". The rate in fact falls
monotonically with document length (`fd2c24`, the longest, is lowest at 9.5%; `fe4059` and `e085f2`,
the shortest, are highest), and **length is the arm-defining variable of this experiment**, so that
was close to the opposite of a safe claim — made, moreover, by an author who cannot see the arms. The
two claims the evidence does support:

1. **The check cannot select on arm by construction** — its inputs are a span and a document, and no
   arm label reaches it.
2. **The absolute admissible pool is nearly constant — 47 to 58 prose lines across a 1.9× range of
   document length.** The requirement (91 rows) is constant too. So the *shortfall* applies to every
   generation, long or short.

### 4c. The ceiling: tier 1 scored 99.8% of rows present

Across all 26 committed shards: **1181 rows `present: true`, 2 `present: false` — 99.83%.** Both
absent rows are in one shard (`e085f2-2`). Every generation retains essentially 91 of 91.

**This is a two-attempt pattern, not a v2 artifact.** The first attempt's verdicts under
`invalidated/` are **631 present of 637 — 99.06%**.

**What it means, stated carefully in both directions.** It does **not** mean the experiment is dead:
tier 1 is the first stage of a funnel, and items 8a and 8b exist precisely because a `present: true`
can rest on real-but-irrelevant text. Discrimination is *designed* to happen at tiers 2 and 3. But it
does mean **the entire discriminating power of this measurement now rests on T6 and T7**, because the
tier-1 stage separates nothing — and that a tier-1 verdict, once it passes the mechanical gate, will
say every arm retained everything. **T8 must not read a 91/91 tier-1 result as evidence that
shortening lost nothing.** That neither attempt of this experiment has remarked on a 99%+ tier-1
present rate is itself worth the write-up's attention.

### 4d. What this is, and is not, relative to `RESULTS.md`'s null

`RESULTS.md`'s null was about the instrument: frozen item 8a invalidated a whole verdict file on any
one of ~18 sampled rows failing, and at the **observed** 22.6% per-row failure rate that leaves
`0.774^18 ≈ 1.0%` of verdicts surviving. Two corrections to how an earlier draft of this file put it:
the 1% is **empirical and conditional**, not "by construction" — `RESULTS.md` notes that even a 5%
per-row rate would leave ≈40% — and §7.1's finding is that no arm had a **defined** retention count,
which is stronger than "no arm could pass".

**The redesign fixed 8a, as it set out to.** What blocks T5a is item 8's self-containment rule, which
is **new to this attempt**, together with the independent scorer failure in §3a. The parallel worth
drawing is structural rather than numerical: for the second time, the measurement is stopped by its
own instrument before any arm comparison is reached.

**What is NOT claimed:** that the instrument is 0% passable by construction. An earlier draft said
so. The per-shard refusal counts run 1 to 13 — `e790f5-2` attempt 1 refused **1 of 64 spans**, one
span away from a clean shard — and §2.1's third attempt for `e790f5` was never spent. Thirteen
attempts all failed and the citable pool is below the row count; that is what the evidence carries,
and impossibility is not.

### 4e. Nothing was adjusted to get past any of it

`PROTOCOL-2.md` stays frozen. Item 10's template was delivered byte-identical on every attempt. The
check was not weakened. No span was patched, no shard edited, no verdict assembled by repair. Per the
driver's standing constraint, a wrong protocol is a finding to report, not a file to adjust.
`retention-2/` is left holding no verdict.

**A consequence a reader should know: the suite is GREEN at this commit and the blocked state is
invisible to it.** `spec_length_2_retention_verdicts_are_complete_and_quoted` passes vacuously on an
empty `retention-2/` — the correct behaviour before scoring runs, and indistinguishable from it here.
Nothing in the test suite asserts that six verdicts ought to exist.

---

## 5. What this file's author knew, and what that costs

Stated because it weakens claims this task would otherwise make.

**T5a's author never opened `blind-map-2.json`.** Sharding was done from `fixture-map-2.json`, which
is arm-free, exactly as `plan.md` P2 intends. No committed tool of this task reads the blind map.

**But it did print `wc -l`/`wc -c` for the six `skill-stickiness` generations in one table**, against
T4's explicit handoff instruction to *"print aggregates, not per-item sizes, if you ever want to stay
blind to your own draw."* Six generations of one fixture are three arms of two samples each, so a
size-ordered pairing is a **plausible guess at the arm partition** — not a read of it, but not
nothing. **The figures leak nothing new**: `GENERATION-2-NOTES.md` §2's `R5a` table already publishes
per-id `wc` figures for all eighteen generations, and `wc -l generated-2/*.md` reproduces them in one
command. What was avoidable was holding them in the context that dispatches scoring. **And §4b's
table publishes a per-generation line count, which is a size proxy keyed by id** — the same class of
thing this paragraph warns against, published because the finding cannot be stated without it.

**What stands between that inference and the measurement**, in order: the scorers are blind and were
handed nothing but their own spec and rows; every prompt was machine-built from `PROTOCOL-2.md` by
`tools/build-tier1-prompts.py` with nothing transcribed by hand; the re-run cap in §2 was fixed
before any re-run outcome was known; and the pass/fail authority is a committed test, not a
judgement.

**T5b and T5c should not repeat it.** `wc` the six files as a total, or not at all.

### A frozen artifact was overwritten in the working tree, and restored

**Disclosed because it touched `generated-2/`, which this run treats as frozen.** Both harnesses in
`tools/` delete `retention-2/` and one of them overwrites a `generated-2/*.md`, by design — they are
meant to run only inside a throwaway `git clone --shared`. A first version of their safety guard
computed the repository root as `__file__.parents[3]`, which is `docs/`, not the root. The guard
therefore never fired, and while it was being *tested* both scripts ran against the live worktree:
`generated-2/66530f.md` was overwritten with the harness's synthetic document and
`retention-2/` was deleted entire, including `parts/superseded/`.

**Nothing was lost, and that is checkable rather than asserted.** Every affected path was committed
at `d8d28a3`; `git checkout` restored all of it, and `git hash-object` of the restored
`generated-2/66530f.md` is `774dd6c26f5e0c3bcffca68ebe4d3888aa12839e`, which is the blob
`FREEZE-2.md:60` records for that file. **The freeze row is the check that the restoration was exact,
and it passes.** `spec_length_2_freeze_rows_still_hash_to_their_files` passes over the whole table.

**The guard now finds the root by walking up to `.git`** instead of counting parents, and refuses the
worktree root, any subdirectory of it, and any path that is not a checkout root. All three refusals
are exercised. **The lesson worth carrying: a destructive tool's guard is itself a thing to test
before trusting, and `parents[n]` is a bad way to name a repository root.**

**Item 11(b)'s re-disclosure, which is owed by every task in this run.** T5a dispatched 26 scorer
runs whose prompts carry an absolute path into this worktree, so each scorer could in principle have
read `generated-2/`, `PROTOCOL-2.md` or the git history. Prompts were written outside the repository,
one per directory, so no scorer could list its siblings; each was told to read exactly one document.
**This was not audited against the transcripts** — T4 scanned all 18 of its probe transcripts for
`blind-map` access and found none; T5a ran no equivalent scan, and that is a gap, not a clean bill.

---

## 6. What T5b, T5c and T8 inherit

1. **Do not re-run T5a's shards hoping for a pass.** Thirteen attempts, zero passes. The cap in §2 is
   spent for `66530f` and the argument for not spending the rest is in the handoff.
2. **T5b and T5c will hit the same gate** on `tiered-review` and `tui-dc-picker` unless those corpora
   are markedly less marked-up. Measure admissibility with `tools/measure-span-admissibility.py`
   **before** dispatching 12 and 6 more scorer runs; it costs one clone and no probes.
3. **Two undischarged items inherited from `GENERATION-2-NOTES.md`, owed by T8**: flag both `R5a`
   generations and read neither as a win; and log the early publication of the `wc`/`R5a` figures as
   a deviation in `RESULTS-2.md`.
4. **A frozen-instrument inconsistency found here and reported nowhere else.** Item 9's first
   blockquote — handed verbatim to every scorer by item 10's template — says a paraphrase *"may be
   evidenced by up to three spans"*, while the same prompt's rules say **1 to 5**. Scorers followed
   the rules; 5-span rows appear throughout. It bears on §4 directly, since each extra span is
   another chance to trip the boundary rule.
5. **The built prompts are not committed.** "Byte-identical on every attempt" rests on
   `tools/build-tier1-prompts.py` being deterministic, which is checkable by re-running it, rather
   than on a diffable artifact.
