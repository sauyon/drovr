# Tier-1 scoring notes — second attempt

`plan.md` T5a/T5b/T5c say a boundary-rejected span is *"a malformed shard → re-run whole under `R6`,
logged for `RESULTS-2.md`"*. `RESULTS-2.md` is not written until T8, so the log has to live
somewhere in between. **This is that file.** T5a opened it; T5b and T5c append to it; T8 reads it.

**Nothing structural points at this file** — not `plan.md`, not `PROTOCOL-2.md`, not `FREEZE-2.md`,
and no test opens it. It survives only because each tier-1 handoff names it. Stated here so a reader
does not mistake it for a checked artifact. (`plan.md` is likewise not in this repository; it lives
at `~/.local/share/drovr/runs/spec-length-ab2/plan.md`.)

**This file records procedure and counts. It records no arm**, and its author never opened
`blind-map-2.json`. See §6 for what its author *did* come to know, which is not nothing.

**THE RUN'S TIER-1 OUTCOME SO FAR, ACROSS TWO OF THREE FIXTURES: THREE VERDICTS FILED OF TWELVE
GENERATIONS ATTEMPTED.** `6e7393` and `e085f2` (`skill-stickiness`, §T5a below) and `48527b`
(`tiered-review`, §9). The other nine are **missing data**, not zero retention — every one of them
was given up to six corrected attempts per shard under the extended cap of §2b. **§8 and §9 are
T5b's record; §T5a–§7 are T5a's and are not rewritten.**

## T5a's outcome, up front

**Two of six verdicts filed. Four not filed.**

| | |
|---|---|
| filed, passing `spec_length_2_retention_verdicts_are_complete_and_quoted` | `6e7393` (91/91 present, 126 spans), `e085f2` (90/91, 120 spans) |
| **not filed** — cap exhausted, still failing the mechanical check | `66530f`, `e790f5`, `fd2c24`, `fe4059` |

**The four are MISSING DATA, not low retention scores.** Nothing about what an arm retained can be
read off a file the mechanical gate rejected. **T8 must not treat an absent verdict as a zero, and
T6 has no input for those four ids.** §5 is the escalation.

**And the two that ARE filed should not be read as retention signal either — v1 said so first.**
Across all 56 shards of this task the present rate is **2543 of 2548 row-judgements — 99.8%**, and
the two filed verdicts sit exactly on that ceiling (`6e7393` 91/91, `e085f2` 90/91). `RESULTS.md`
§7.5 already recorded the same shape and the same doubt: *"The retention numbers are implausibly
high, and T5 does not believe them. Four of six generations scored 91/91 … exactly the shape a
lenient scorer produces."* **This attempt reproduces it, and this file inherits the doubt.** The
gate these two verdicts cleared asserts **well-formedness, never judgement** — item 8 says so in
terms — so "filed" means "correctly shaped", not "correct". Whether the retention is real is tier
2's and tier 3's question (items 8a, 8b), and **T6 and T7 now carry the entire discriminating load
of this measurement.**

**An earlier version of this file concluded that T5a was BLOCKED by a defect in frozen
`PROTOCOL-2.md`. That conclusion is WITHDRAWN.** It was wrong, and it was wrong because of this
task's own tooling — §3 is the whole story, kept rather than deleted because the mistake is the most
useful thing here.

---

## 1. The calibration probe

T2 handed forward one open question above all others: **item 8's span self-containment rule refuses
84.9% of the first attempt's real spans — 957 of 1127.** A rule that refuses nearly everything by
construction is precisely the defect `RESULTS.md`'s null was about, so T2, T3 and T4 each carried the
same instruction forward: **T5 scores ONE generation first and looks at its refusal rate before
dispatching the other 17.**

That is what ran, before any other generation was dispatched: `66530f` (first id-lexicographically of
the six `skill-stickiness` ids), both shards, 91 of 91 rows, 128 spans, **13 refused — 10.2%**.

**10.2%, not 84.9%.** T2 bounded its own figure correctly: those 1127 spans were written under
`PROTOCOL.md`, which had **no boundary rule at all**, so 84.9% measured v1's quoting habit rather
than the rule's severity.

**But 10.2% was read as reassurance, and that was the error.** It licensed dispatching the other five
generations under a prompt that was itself defective (§3). The probe did its job — it surfaced a
number — and the number was interpreted without asking what produced it.

---

## 2. The re-run policy, fixed before the first re-run was dispatched

`R6` permits a re-run when *"a verdict fails the item-8 mechanical check"* and requires every re-run
to be logged. It sets **no cap**, and an uncapped loop is a slow way to arrive at `R6`'s own
prohibition — re-running until the output is acceptable.

1. **At most two re-runs per shard — three attempts in all.**
2. **The cap applies identically to every shard**, whatever any attempt returns. Not re-decided per
   generation, not re-decided per arm.
3. **A re-run re-runs the shard WHOLE**, with the byte-identical prompt. Nothing patched, no span
   edited, and the prompt never adjusted to steer a scorer away from a refused shape.
4. **A shard still failing after its third attempt is reported as a failing verdict, not fixed.**
5. **Every attempt is logged**, including the ones that succeed.

**What is checkable about "fixed before", precisely, because the strong version is false.** Commit
`0b4cf46` adds §1–§2 with an empty log and only the two round-0 shards; later commits add the rest.
**That ordering between commits is checkable.** The ordering between a commit and a *dispatch* is
not: a subagent dispatch leaves no trace in the repository, and commit timestamps are author-settable.
So the repository carries the strongest ordering evidence it can, and that the dispatch followed the
commit remains the author's word.

### 2a. The delivery defect, and the corrected prompt

**Pre-registered in commit `ed583e9`, which adds §2a and the builder change and zero shard files;
all 30 corrected shard files arrive in a later commit. That commit ordering is checkable. As in §2,
that the *dispatch* followed the commit remains the author's word.**

**Item 10 does not say the scorer gets the template. It says what the scorer gets, and item 8 is on
the list:**

> The scorer is handed **only** the generated spec file, its fixture's ledger rows for its shard, and
> **the item-8 schema** plus the item-9 definition.

`plan.md` T5a says the same in fewer words — *"items 8 + 9"*. **The first 26 dispatches delivered item
9 and not item 8.** `tools/build-tier1-prompts.py` substituted item 9's two blockquotes through the
template's explicit placeholder; for item 8 the scorer got only the template's one-sentence paraphrase
— *"begin at the start of a sentence, table cell, list item, heading or line-block"* — which is **not**
the character-level predicate the gate runs. Item 8's self-containment blockquote reached no scorer.

**The correction, and why it is not steering.** Item 8 is handed over verbatim and entire, appended
after the template. **The template itself is unedited and still goes out byte-identical as a prefix**
— verified by comparing the built files. §2.3 forbids adjusting a prompt to steer a scorer away from
a refused shape; delivering a frozen rule that item 10 says the scorer receives invents no guidance,
and the same bytes go to every shard of every generation, so it is arm-symmetric by §2's own argument.

**The cap restarts at two re-runs per shard for the corrected prompt**, because a corrected delivery
is a different dispatch from the one §2.1 capped, and `R6` licenses each re-run independently on its
own trigger.

**Three disclosures about what the correction actually put in front of a scorer**, so a reader does
not have to find them in the source:

1. **The appended block is item 8 entire — 5614 bytes — not only its schema.** Item 10 says "the
   item-8 schema" and also says the scorer never gets "this file"; the two clauses are in tension and
   this resolves toward the enumerated handover. It means the scorer also saw item 8's rationale, its
   sharding subsection, the gate test's own name, and pointers to two v1 artifacts. Item 9's
   blockquotes were already excerpts of the same file, but the surface is wider than before and §6's
   item-11(b) disclosure should be read with that in mind.
2. **Two authored lines were added** — `--- BEGIN THE ITEM-8 SCHEMA, WHICH GOVERNS AND IS NOT A
   SUMMARY ---` and its closing marker. They are not frozen text. They are arm-symmetric and steer
   toward no refused shape, but "invents no guidance" is not literally true of them.
3. **"Byte-identical" means the built prefix matches what the 26 defective dispatches emitted** — not
   that it matches the frozen template, which of course carries substituted paths and rows. What a
   reader can check without the prompts (uncommitted, §7.7) is the `ed583e9` diff, which is the
   stronger evidence anyway: it is purely additive and touches no substitution.

**The correction did not move the measurement, only the span shape — and that is checkable.** Present
rate across the 26 defective shards is **1181 / 1183 = 99.8%**; across the 26 superseded corrected
shards it is **1181 / 1183 = 99.8%**. Spans per present row go 1.45 → 1.43. If the appended rule had
coached scorers toward finding more content, this is where it would show, and it does not.

### 2b. The cap was extended AFTER four ids had missed — a post-hoc amendment, on the driver's decision

**Opened by T5b before it dispatched anything.** Everything below §2b in this file that describes a
dispatch was written after this subsection was committed; the commit that adds §2b adds no shard
file, exactly as `ed583e9` did for §2a. **That commit ordering is checkable. As in §2 and §2a, that
the dispatch followed the commit remains the author's word.**

**Say the shape of this plainly, because it is the thing a reader should be suspicious of.** §2's cap
was fixed before any re-run ran, and §4 records that it was honoured: after round 6 the four ids
`66530f`, `e790f5`, `fd2c24` and `fe4059` had each spent three corrected attempts per shard and were
reported as failing rather than re-run. **The cap is now being raised, and it is being raised by a
decision taken after seeing that exactly those four missed.** That is post-hoc. §5 escalated it for
that reason, and the decision came back from the human driver, on the record, as option 1: **extend
the re-run budget, uniformly, and re-run.**

**The amendment.**

> Under the corrected prompt the cap is **at most five re-runs per shard — six attempts in all**,
> replacing §2.1's two re-runs / three attempts. §2.2–§2.5 are unchanged and still bind: the cap
> applies identically to every shard whatever any attempt returns, a re-run re-runs the shard whole
> with the byte-identical prompt, a shard still failing after its last attempt is reported failing
> and not fixed, and every attempt is logged.

**Three properties are what keep it from being result-shaped, and all three are load-bearing.**

1. **It is uniform across all three fixtures and all eighteen generations**, not granted to the four
   that missed. Every `tiered-review` and `tui-dc-picker` generation has the same six attempts per
   shard available from its first dispatch. A cap raised for the shards that failed and no others
   would be a cap fitted to a result; a cap raised for the instrument is a change to the instrument.
2. **A passing verdict is still never re-run.** `R6` licenses a re-run on a failure trigger and a
   cleared verdict has none. Uniformity here means equal *opportunity*, never re-rolling a result
   already in hand: **`e085f2` and `6e7393` stand exactly as filed** and no attempt is dispatched
   against either.
3. **It is disclosed as post-hoc, here and in `RESULTS-2.md`.** T8 must carry this forward and must
   not present the six-attempt cap as though it had been the plan. It was not.

**What the extra attempts can and cannot buy, stated before they are spent.** §5 already weighed
this and the weighing does not change because the answer came back yes: the trend the extension
banks on is a trend in **gate-passing**, on an instrument whose retention output is saturated at
99.8% (§7.4). More filed verdicts buy T6 well-formed *inputs*. They buy no additional retention
signal, and a verdict that clears on attempt six is not better evidence than one that cleared on
attempt one — it is the same well-formedness assertion, reached later. **Nothing in `RESULTS-2.md`
may read the post-extension pass count as a result about spec length.**

**And one of the three options §5 offered was NOT taken.** Option 3 — treat separately the two ids
that failed on shared spans alone, with zero boundary refusals — is not adopted. Those two get the
same six attempts as everything else, because a remedy applied only to the shards with one failure
mode is the result-shaped thing property 1 exists to forbid. **The shared-span non-compliance
remains an open finding about scorer behaviour** (§3, §7) and is not treated as a protocol defect.

---

## 3. What the correction changed — the finding of this task

**56 shard dispatches in all: 26 under the defective prompt, 30 under the corrected one.**

| | defective prompt | corrected prompt |
|---|---|---|
| shard-pair attempts | 13 | 15 |
| spans written | 1709 | 1946 |
| **boundary refusals** | **207 — 12.1%** | **43 — 2.2%** |
| verdicts that cleared the gate | **0 of 13** | **2 of 15** |

**The boundary refusal rate falls sharply when the scorer is handed the rule the gate runs.** That is
the finding, and it retires the earlier one.

**How large the fall is depends on which comparison you make, and the pooled 12.1% → 2.2% is the
least defensible one** — it pools four corrected rounds against three defective ones of different
composition. The **matched** comparisons, which are the ones §2a pre-registered:

| matched pair | defective | corrected | ratio |
|---|---|---|---|
| the `66530f` probe — same id, same two shards | 13 / 128 = 10.2% | 7 / 128 = 5.5% | **1.9×** |
| the all-six round | 93 / 785 = 11.8% | 22 / 780 = 2.8% | **4.2×** |

**And something the prompt cannot explain is also happening**: within the corrected prompt the rate
falls monotonically round on round — 5.5%, 2.8%, 1.7%, 0.8% — while the prompt is fixed. Selection
pressure should push the *other* way, since each round retires the shards that cleared and leaves the
harder ones. **This is unexplained and is not claimed as an effect of the correction.** The honest
statement of the finding is directional: handing over item 8 moved the rate down by at least the 1.9×
of the matched probe, and it is what turned zero passing verdicts into two.

**Per round, under the corrected prompt:**

| round | shards | spans | boundary | shared-span | verdicts cleared |
|---|---|---|---|---|---|
| 1 | `66530f` only | 128 | 7 | 0 | — |
| 2 | all six | 780 | 22 | 6 | **`e085f2`** |
| 3 | the five still failing | 641 | 11 | 6 | **`6e7393`** |
| 4 | the three with budget left | 397 | 3 | 6 | — |

**One claim from this task's own commit message needs correcting here.** `43d7715` says duplicate-span
violations "largely disappear" under the corrected prompt. **They do not.** Under the defective prompt
there were 10 across 13 attempts (0.8 per attempt); under the corrected prompt 18 across 15 (1.2 per
attempt). The rule — *"no span may appear under more than one row"* — is stated in the template either
way, so the correction was never going to touch it. **Two of the four unfiled verdicts failed on
shared spans alone, with zero boundary refusals**, which is plain scorer non-compliance and not a
protocol defect at all.

### 3a. Why the earlier BLOCKED finding was wrong, kept as the record

The withdrawn version of this file concluded that item 8's rule made every verdict unpassable, and
supported it with thirteen failed attempts and an admissibility measurement. Both were real. The
conclusion was still wrong, for a reason worth writing down:

**Thirteen attempts were thirteen draws from ONE distribution.** §2.3 held the prompt byte-identical
across re-runs precisely so that re-runs would measure variance — which meant the re-run budget could
never test the one hypothesis that mattered. *Does a scorer told the actual rule satisfy it?* was
never asked until a reviewer asked it.

**T4's handoff records the lesson this broke**, and it is worth quoting because it was available the
whole time: *"when one artifact looks wrong, the first question is whether the tooling did it, because
that question is about all of them."* Thirteen artifacts looked wrong. The question was asked of the
protocol and not of the tool.

**A withdrawn supporting measurement, and what survives of it.** The earlier version measured that
only 24.5% of these documents' distinct non-blank lines are admissible spans, and — excluding table
rows and headings — 47–58 "citable prose lines" per document against 91 rows, concluding the pool was
smaller than the requirement. **That conclusion is withdrawn too.** Three faults, each found in review:
the excluded table rows are *not* furniture (scorers cite them roughly 180 times across this corpus,
and in these specs a table row is often the operative content); the whole-line population misses how
scorers actually quote (**1134 of 1709 spans are multi-line**, so a span pairs any admissible start
with any admissible end and the real pool is far larger); and the `all` column's own numbers —
100–124 admissible lines against 91 rows — never supported a shortfall in the first place. The raw
output is kept at `parts/superseded/refusal-reports/admissibility.txt` and the numbers in it are
correct; **it is the reading of them that was wrong.** What survives is narrow and still useful: the
rule is markup-sensitive, and a corpus of bold-closed bullets and fixed-width line-blocks makes it
bite hardest.

### 3b. What remains true about item 8's rule

Verified by hand against the frozen prose, and not withdrawn:

- **A span can be a complete, correct sentence and still be refused because of the line above it.**
  `generated-2/66530f.md:353` is a whole bullet refused because line 352 ends in `*`; item 8's
  block-start clause needs the preceding line blank, a heading, a table row, or ending in `.!?:;`.
  In a list of bold-closed bullets each line disqualifies its neighbour.
- **The same-line clause has the same blind spot.** A span beginning after `…applies." ` fails,
  because the rule requires `. `, `! ` or `? ` immediately before it and a closing quotation mark
  intervenes.
- **It is navigable once you know it** — end the span at the `.` inside the emphasis run, or start it
  where a clean boundary exists — which is exactly what the corrected prompt let scorers do, and why
  the rate fell fivefold.

**No left/right split of the 207 is published.** Attributing each refusal to an end requires
evaluating item 8's predicates outside the committed Rust check, and this run keeps exactly one
implementation so there is never a second answer to what the rule refuses.

---

## 4. The full dispatch log

Every attempt, in order. **Reason is always the protocol trigger, never an outcome.** Every shard of
every attempt is at `retention-2/parts/superseded/<id>-<k>[-corrected]-attempt<n>.json`; the raw
failure report of every round is at `.../refusal-reports/`.

| round | prompt | shards | attempt | trigger | outcome |
|---|---|---|---|---|---|
| 0 | defective | `66530f` ×2 | 1 | first run — §1 calibration probe | failed |
| 1 | defective | `66530f` ×2 | 2 | `R6` — failed the item-8 check | failed |
| 1 | defective | other five ×2 | 1 | first run | all five failed |
| 2 | defective | `66530f` ×2 | 3 | `R6` — same trigger | failed |
| 2 | defective | other five ×2 | 2 | `R6` — same trigger | all five failed |
| — | | | | **delivery defect found; §2a pre-registered** | |
| 3 | corrected | `66530f` ×2 | c1 | corrected delivery, probe first | failed (7, was 13) |
| 4 | corrected | all six ×2 | c1/c2 | `R6` | **`e085f2` cleared** |
| 5 | corrected | five ×2 | c2/c3 | `R6` | **`6e7393` cleared** |
| 6 | corrected | three ×2 | c3 | `R6` | none cleared; **cap exhausted for all four** |

**A passing verdict is never re-run.** `R6` licenses a re-run on a failure trigger; a verdict that
cleared the gate has no trigger, and re-rolling one would be the thing `R6` exists to forbid.

**The cap was honoured, not extended.** After round 6, `66530f`, `e790f5`, `fd2c24` and `fe4059` had
each used their two corrected re-runs. Extending the cap *after seeing which four missed* is exactly
the decision pre-registration exists to prevent, so it was not extended. Re-opening that budget is a
**driver** call, not this task's — §5.

**The driver re-opened it. §2b is the amendment and §8 is the log of what the extra attempts did.**
Rounds 0–6 above are T5a's complete record under the original cap and are not rewritten.

---

## 5. Escalation — what only the driver can decide

**The run cannot proceed to T6 for four of six `skill-stickiness` ids.** T6 reads
`retention-2/<id>.json` and computes a stride-5 sample over it; for `66530f`, `e790f5`, `fd2c24` and
`fe4059` there is nothing to read. Three options, none of which is T5a's to take:

1. **Re-open the re-run budget** for the four, under the corrected prompt. Two verdicts cleared in
   two rounds and `R6` itself sets no cap; the only thing in the way is this task's own
   pre-registration, which the driver can lift explicitly and on the record. **Weigh it against the
   ceiling**: that trend is in *gate-passing*, on an instrument whose retention output is saturated
   at 99.8%, so more filed verdicts buy well-formed inputs for T6 and not more retention signal.
2. **Accept a scoped result** over the ids that cleared, and carry four ids as missing data into
   `RESULTS-2.md`. Honest, and much weaker: two of six is not a fixture.
3. **Treat the shared-span failures separately.** Two of the four failed *only* on shared spans, with
   zero boundary refusals — a different defect with a different remedy.

**T5b and T5c should not start until this is decided**, because whatever is decided applies to them:
they will run the same instrument over `tiered-review` and `tui-dc-picker`, and they should run the
**corrected** builder from the first dispatch rather than rediscovering §2a.

### 5a. Resolved — the human driver chose option 1

**Decided by the human, after being shown that four ids missed and which four**, and passed to T5b
through the phase brief. **Option 1: re-open the re-run budget, uniformly, and re-run.** Option 2
(accept a scoped two-of-six result) and option 3 (treat the shared-span failures separately) were
not taken. §2b is the amendment, with the three properties that make it legitimate and the
disclosure obligation it puts on `RESULTS-2.md`. **T5b executed it; T5c inherits the same cap.**

---

## 6. What this file's author knew, and what that costs

**T5a's author never opened `blind-map-2.json`.** Sharding came from `fixture-map-2.json`, which is
arm-free. No committed tool of this task reads the blind map.

**But it printed `wc -l`/`wc -c` for the six `skill-stickiness` generations in one table**, against
T4's explicit instruction to *"print aggregates, not per-item sizes, if you ever want to stay blind to
your own draw."* Six generations of one fixture are three arms of two samples each, so a size-ordered
pairing is a **plausible guess at the arm partition** — not a read of it, but not nothing. The figures
leak nothing new (`GENERATION-2-NOTES.md` §2 already publishes per-id `wc` figures for all eighteen);
what was avoidable was holding them in the context that dispatches scoring. **T5b and T5c should not
repeat it.**

**What stands between that inference and the measurement:** scorers are blind and were handed nothing
but their own spec, their rows, and items 8 and 9; every prompt was machine-built from `PROTOCOL-2.md`
with nothing transcribed by hand; the caps were pre-registered before their outcomes were known; a
passing verdict was never re-run; and pass/fail authority is a committed test, not a judgement.

**Item 11(b)'s re-disclosure, owed by every task in this run.** T5a dispatched 56 scorer runs whose
prompts carry an absolute path into this worktree, so each could in principle have read `generated-2/`,
`PROTOCOL-2.md` or the git history. Prompts were written outside the repository, one per directory, so
no scorer could list its siblings. **This was not audited against the transcripts** — T4 scanned all 18
of its probe transcripts for `blind-map` access and found none; T5a ran no equivalent scan. That is a
gap, not a clean bill, and it grew with the dispatch count.

### A frozen artifact was overwritten in the working tree, and restored

Both harnesses in `tools/` delete `retention-2/` and one overwrites a `generated-2/*.md`, by design —
they are meant to run only inside a throwaway clone. A first version of their guard computed the
repository root as `__file__.parents[3]`, which is `docs/`. The guard never fired, and while it was
being **tested** both scripts ran against the live worktree: `generated-2/66530f.md` was overwritten
and `retention-2/` deleted entire.

**Everything committed was restored, and that is checkable:** `git hash-object` of the restored
`generated-2/66530f.md` is `774dd6c26f5e0c3bcffca68ebe4d3888aa12839e`, the blob `FREEZE-2.md:60`
records, and `spec_length_2_freeze_rows_still_hash_to_their_files` passes.

**One uncommitted file WAS lost** — `refusal-reports/admissibility.txt`, written before the incident
and not yet committed, which `git checkout` could not bring back. It was regenerated and is committed
now, but an earlier draft's "nothing was lost" was wrong about it. **A blob-hash check proves the
frozen material survived and says nothing about working material.**

**The guard now finds the root by walking up to `.git`.** It refuses the worktree root, a
subdirectory of it, and any path that is not a checkout root — all three exercised by hand, none by a
committed test. **It does not refuse a sibling git worktree of the same repository**, whose root also
carries `.git`; that is a known remaining hole.

---

## 7. What T5b, T5c and T8 inherit

**§7 was written by T5a. T5b has since run: items 1–3 below are DISCHARGED, and §9.5 is the current
list for T5c.** Item 1 (use the corrected builder) was followed; item 2 (wait for the §5 decision)
was resolved by §5a and executed as §2b; item 3 (the admissibility tool is hardcoded) is fixed —
the tool now takes a required fixture argument. **Items 4–8 are still open and are re-listed with
T5b's own additions in §9.5.** Nothing here is deleted, because the list is also the record of what
each task was told.

1. **Use the corrected `tools/build-tier1-prompts.py`** — it hands over item 8. Do not rebuild prompts
   from the template alone; §2a is why.
2. **Wait for the driver's decision in §5** before dispatching, because it governs your budget too.
3. **`tools/measure-span-admissibility.py` is hardcoded to `skill-stickiness`** (its id list, ledger
   path and row-count assertion). It needs a fixture argument before T5b or T5c can use it. Given §3a,
   treat what it measures with care: it counts whole lines, and most real spans are multi-line.
4. **The 99.8% present rate is an undischarged doubt, not a result** — `RESULTS.md` §7.5's, now
   reproduced. It belongs in `RESULTS-2.md` beside the tier-2 and tier-3 outcomes, and no tier-1
   number should be published without it.
5. **Undischarged obligations owed to T8, carried from `GENERATION-2-NOTES.md`'s six-row table —
   four are still open:** flag both `R5a` generations and read neither as a win; do not read
   `fd2c24`'s retention as evidence; log the early publication of the `wc`/`R5a` figures as a
   deviation; and — after the join — report whether generation titles track the arm.
6. **Still-open questions from T4's handoff, which nothing else keeps alive:** the title-variance
   channel (owed by T8 and the write-up); whether re-dispatching two transmission questions was right
   (decide before T9; the revert is `git revert 6268365`); the 20 freeze rows are not checked for
   completeness; and the leak check tells a reader to "re-run under `R6`" for a case `R6`'s closed
   trigger list does not cover.
7. **A frozen-instrument inconsistency found here.** Item 9's first blockquote — handed verbatim to
   every scorer — says a paraphrase *"may be evidenced by up to three spans"*, while the same prompt's
   rules say **1 to 5**. Scorers followed the rules. With §2a this is the *second* place item 10's
   template and the items it claims to carry disagree; they belong together as one finding about
   item 10.
8. **The built prompts are not committed.** "Byte-identical on every attempt" rests on the builder
   being deterministic, which is checkable by re-running it, not on a diffable artifact.

---

## 8. T5b — the extended cap, spent on T5a's four outstanding ids

**Written by T5b (`implement-task-6`), which the driver instructed to finish T5a's four
outstanding `skill-stickiness` ids under §2b's extended cap before starting `tiered-review`.**

### 8.1 The outcome, up front

**The extension bought nothing. Three further attempts per shard for each of the four ids —
`66530f`, `e790f5`, `fd2c24`, `fe4059` — and NOT ONE verdict cleared the gate.** All four remain
what §5 called them: **missing data, not low retention.** `retention-2/` still holds exactly two
`skill-stickiness` verdicts, `6e7393` and `e085f2`, unchanged and never re-run.

| round | prompt | shards | attempt | trigger | outcome |
|---|---|---|---|---|---|
| 5 | corrected | the four ×2 | c4 | `R6` — failed the item-8 check, under §2b's raised cap | none cleared |
| 6 | corrected | the four ×2 | c5 | `R6` — same trigger | none cleared |
| 7 | corrected | the four ×2 | c6 | `R6` — same trigger; **cap exhausted for all four** | none cleared |

**Problems per round, for the four ids only** (each round's raw gate output is at
`parts/superseded/refusal-reports/corrected-round{5,6,7}.txt`; round 5's was reproduced rather than
captured live, and its file says so):

| round | attempt | boundary | shared-span | total |
|---|---|---|---|---|
| 5 | c4 | 7 | 12 | 19 |
| 6 | c5 | 20 | 4 | 24 |
| 7 | c6 | 26 | 6 | 32 |

**It does not converge, and the direction is the wrong one.** Under a fixed prompt three more
draws produced more refusals, not fewer. Read against §3's monotonic *fall* across the earlier
corrected rounds — 5.5%, 2.8%, 1.7%, 0.8% — which §3 already recorded as unexplained, **the fall
now looks less like a trend and more like the noise §3 was careful not to claim as an effect.**
Two rounds moving one way and three moving the other, on a fixed instrument, is what variance
looks like. **§3's directional claim about the item-8 correction is not affected**: that rests on
the matched `66530f` probe (1.9×) and on zero-passing-verdicts becoming two, neither of which this
touches.

### 8.2 What this says about the driver's decision, plainly

**The decision to extend was sound and the result was negative, and those are two different
things.** §2b pre-registered what the extra attempts could buy — well-formed inputs for T6, never
retention signal — and said it before they were spent. They bought neither. That is the outcome a
pre-registration is *for*: had the cap been raised without §2b, three losing rounds would now be
invisible and only a later winning round would have been written down.

**The honest reading is that T5a's original cap was set in a reasonable place.** §4's "the cap was
honoured, not extended" turned out to cost nothing, and the four ids look less like ids that
needed more attempts and more like ids whose documents these scorers do not score compliantly.
Six corrected attempts per shard — **twelve shard dispatches per id** — with zero passes, against
`6e7393` clearing on its first corrected attempt and `e085f2` on its second, is a large asymmetry
that attempt count did not close. **What distinguishes the two groups is not established here, and
this file offers no account of it.** Any account would be a claim about the arms, which this file's
author has no basis for and must not make.

**A length-based explanation was drafted here and is WITHDRAWN — review caught it, and it was wrong
twice over.** The draft read *"it is not document length (`fe4059` is the shortest of the six and
fails worst)"*. By the canonical metric this run uses everywhere else — lines, and lines as a
percentage of the fixture, `GENERATION-2-NOTES.md` §2 — **`fe4059` is not the shortest**: `e085f2`
is, at 418 lines / 52.8% against `fe4059`'s 424 / 53.6%. `fe4059` is smaller only by raw byte count,
by **10 bytes in ~25,800**, on a metric this experiment never uses. **And the corrected fact points
the opposite way from the claim it was supporting**: the genuinely shortest of the six is one of the
two that passed.

**Both of those are less bad than where the number came from.** The draft's "shortest" was read off a
per-generation size table this task printed by accident — §6 records T5a doing exactly this against
T4's explicit instruction, and §9's own tooling change exists partly to stop the admissibility script
reprinting it. **Sorting six generations of one fixture by size in a context that dispatches scoring
is the de-blinding, whatever conclusion is drawn from it**, and drawing a conclusion made it worse.
Nothing is claimed about length here, and T8 must not read one out of this subsection.

### 8.3 A hypothesis about the shared-span failures, checked and WITHDRAWN

Round 5's shared-span refusals fell on strikingly repetitive ledger pairs, and the **same pairs
recurred across different scorers scoring different generations**. There are **nine**, extracted
from `corrected-round5.txt` rather than listed from memory:

`01`/`65`, `02`/`83`, `04`/`90`, `09`/`55`, `09`/`56`, `16`/`89`, `36`/`63`, `39`/`87`, `43`/`47`.

Several are semantically nested: row 89 is the `grep` that row 16 describes, row 90 is the overlap
check that row 04 requires. The hypothesis was that the ledger contains entailment pairs a
well-written spec states in one sentence, so item 8's no-shared-span rule makes both rows
un-evidenceable at once — an **instrument** defect, and a much more serious finding than scorer
non-compliance.

**It is false, and checking it cost two minutes.** Both filed verdicts were examined against all
nine pairs, and **neither shares a span on any of them**: `6e7393` evidences **both** members of
all nine with **disjoint** spans, and `e085f2` does the same for eight of the nine. `e085f2`'s one
exception is the pair `09`/`56`, where it marks **`skill-stickiness-56` `present: false`** — which
is that verdict's only `false` row in all 91. A second witness exists in these documents and two
scorers found it every time. **The pairs are attractors for the violation, not forcers of it**, and
§3's reading — scorer non-compliance with a rule the prompt states verbatim — stands unamended.

**A review of this task caught the list above short by two.** An earlier draft named seven pairs
while the surrounding sentences counted nine, which also left `e085f2`'s single `false` row
belonging to no named pair and so unverifiable against the files. The count of nine was right and
the list was wrong. It is now derived by regex from the round-5 report, like §9.3's totals, for the
same reason: **this is the second hand-transcription slip in this task's own writing**, and a
subsection whose entire argument is "a cheap mechanical check beats a plausible story" is the worst
possible place to keep a hand-typed list.

**Recorded because the check is the point, not the hypothesis.** §3a's whole lesson is that a
plausible story about a frozen instrument being broken deserves a cheap mechanical check before it
is believed, and the check here was "do the verdicts that already passed do the thing you claim is
impossible?" **Nothing in this subsection is a finding. It is a hypothesis that was tested and
died**, kept so the next reader does not re-form it.

### 8.4 What T5c and T8 must carry from this

1. **The four ids are still missing data.** `T6` has no input for `66530f`, `e790f5`, `fd2c24` and
   `fe4059`, and an absent verdict is **not** a zero. This is unchanged from §5; the extension did
   not change it.
2. **`RESULTS-2.md` must report the extension AND its failure**, together. Reporting a raised cap
   without reporting that it produced no verdict would misdescribe the run in the flattering
   direction. §2b's post-hoc disclosure obligation and this outcome are one item, not two.
3. **The negative result is itself evidence about the instrument** — that tier-1 well-formedness
   is not reliably reachable for four of six `skill-stickiness` generations, by scorers given the
   frozen rule verbatim and six attempts. That belongs in `RESULTS-2.md` beside the 99.8%
   present-rate doubt, and it points the same way: **the tier-1 gate is measuring something about
   scorer compliance, and the discriminating load still sits on T6 and T7.**
4. **`FREEZE-2.md` is still closed at this run's rows.** T5b added none.

---

## 9. T5b — `tiered-review`, the task's own fixture

### 9.1 The outcome

**One of six verdicts filed.** `retention-2/` now holds **three** verdicts in all.

| | |
|---|---|
| filed, passing the gate | **`48527b`** — 84 rows, 80 present, 99 spans, cleared on attempt c5 |
| **not filed** — cap exhausted, still failing | `054872`, `2c4295`, `2d2629`, `b49ff1`, `fd230c` |

**The five are MISSING DATA, not low retention**, exactly as §5 says of T5a's four. **T6 has no input
for them.** Across both fixtures this run has now attempted 12 generations and filed 3.

### 9.2 The dispatch log

Rounds continue §4's and §8.1's numbering. Every attempt is at
`parts/superseded/<id>-<k>-corrected-attempt<n>.json`; every round's raw gate output is at
`parts/superseded/refusal-reports/corrected-round<n>.txt`.

| round | shards | attempt | trigger | outcome |
|---|---|---|---|---|
| 6 | `054872` ×2 | c1 | first run — §1-style calibration probe | failed |
| 7 | `054872` ×2 | c2 | `R6` | failed |
| 8 | `054872` ×2; other five ×2 | c3; c1 | `R6`; first run | none cleared |
| 9 | all six ×2 | c4; c2 | `R6` | none cleared |
| 10 | all six ×2 | c5; c3 | `R6` | none cleared |
| 11 | all six ×2 | c6; c4 | `R6`; **`054872` cap exhausted** | none cleared |
| 12 | five ×2 | c5 | `R6` | **`48527b` cleared** |
| 13 | four ×2 | c6 | `R6`; **cap exhausted for all six** | none cleared |

**The calibration probe was run and it is why the other five were dispatched.** `054872` went first
(id-lexicographically), and its first corrected attempt refused **5 boundary spans of 94 — 5.3%**,
against **5.5% (7/128)** for the `skill-stickiness` corrected probe of round 3. The instrument does
not behave differently on this corpus, which is what the probe existed to establish. It was
dispatched alongside the `skill-stickiness` re-runs rather than after them, because the two fixtures
are independent and neither one's result gates the other's.

**Counts, derived from the files on disk rather than from arithmetic.** `parts/superseded/` holds
**144** shard files, of which T5a left 52, so **T5b added 92**; with `48527b`'s two live shards that
is **94 shard files from 96 dispatches**. The two missing dispatches died mid-stream on API errors
and wrote nothing (round 12).

### 9.3 Two things `tiered-review` does differently, both recorded and neither claimed

1. **The 99.8% ceiling does not reproduce.** Every `skill-stickiness` round of this run and the last
   sat at a 99.8% present rate, and §7.4 carries that as an undischarged doubt. **`tiered-review`
   present rates run 73–84 of 84 — 87% to 100% — with real spread between generations and between
   attempts on the same generation.** That is what a measurement with some discriminating power
   looks like, and it is the first sign in either attempt that the instrument is not simply
   saturated. **It is not evidence about spec length**: this file's author cannot see arms, six
   generations is three arms of two, and the spread could be the fixture, the ledger, the arms or
   the scorers. **§7.4's doubt is narrowed to `skill-stickiness`, not withdrawn.**
2. **Shared-span violations dominate here; boundary refusals dominated there.** Over rounds 8–13,
   counting only the six `tiered-review` ids, the split is **77 boundary / 92 shared-span** of 169
   problems, where `skill-stickiness` round 7 was **26 boundary / 6 shared-span**. §3's shared-span
   finding — plain scorer non-compliance with a rule the prompt states verbatim — is the live
   failure mode for this fixture. **§8.3's withdrawn ledger-entailment hypothesis must not be
   re-formed from this**; it was tested against the filed verdicts and died.

   | round | boundary | shared-span | total |
   |---|---|---|---|
   | 8 | 12 | 18 | 30 |
   | 9 | 7 | 16 | 23 |
   | 10 | 25 | 11 | 36 |
   | 11 | 8 | 22 | 30 |
   | 12 | 14 | 12 | 26 |
   | 13 | 11 | 13 | 24 |
   | **total** | **77** | **92** | **169** |

   **The denominator is the six `tiered-review` ids only** — each round's raw file also scans the
   filed `skill-stickiness` verdicts, which contribute zero. The totals are summed from the
   `corrected-round{8..13}.txt` files mechanically, not by hand: a first draft of this subsection
   carried 46 / 51, which was simply wrong, and pooling populations by hand is the error reviewers
   caught in T5a three rounds running.

### 9.4 The one filed verdict, and why its crash-retry is disclosed

`48527b` cleared on attempt **c5**, in the same round in which **two dispatches died mid-stream on
API errors writing no file** — one of them `48527b`'s own shard 1. Both were re-dispatched with the
byte-identical prompt.

**A crashed dispatch does not consume a scoring attempt, and `48527b` used exactly five.** A dispatch
that writes no file produced no verdict, so there is nothing for the item-8 check to fail; `R6`'s
closed trigger list carries *"the probe wrote no file"* as a trigger **separate** from *"a verdict
fails the item-8 mechanical check"*, and §2/§2b cap scoring attempts rather than process crashes.
**This is checkable on disk and not merely asserted:** `parts/superseded/` holds exactly **four**
`48527b-1` and **four** `48527b-2` corrected attempts (c1–c4), and the fifth is the filed shard pair
under `parts/`. Four superseded plus one filed is five.

**Disclosed at length because it is the verdict that passed.** A crash-retry on the single generation
that cleared is precisely the thing a sceptical reader should want to check, and the answer should not
depend on taking this file's word for it.

One of the crashed agents reported that its temporary helper script *"was replaced by another process
mid-run"* — concurrently dispatched scorers writing scratch scripts to a shared path. **No shard file
was affected**: the collision was in agents' private scratch space, and each shard is written by
exactly one agent to its own path. Later dispatches were told to keep scratch files inside their own
prompt directory.

### 9.5 What T5c and T8 inherit from T5b

1. **`tools/measure-span-admissibility.py` now takes a required fixture argument** and prints
   aggregates only. T5c can use it directly. Do not read a shortfall out of it — §3a's three faults
   still apply, it counts whole lines, and most real spans are multi-line. The `tiered-review`
   measurement is at `parts/superseded/refusal-reports/admissibility-tiered-review.txt`.
2. **The extended cap (§2b) applies to `tui-dc-picker` too — six attempts per shard, uniformly**,
   from the first dispatch. It is not a budget T5c may raise further, and it is not one T5c may
   decline to offer: uniformity is what makes §2b legitimate.
3. **`tui-dc-picker` is ONE shard of 55 rows**, not two. A single scorer carries the whole verdict,
   so there is no shard-pair to combine and the gate is all-or-nothing per attempt.
4. **T6 has no input for nine of twelve generations so far.** Three filed: `6e7393`, `e085f2`,
   `48527b`. Nine are missing data. **An absent verdict is never a zero.**
5. **`RESULTS-2.md` must report the pass RATE as an instrument property.** Three verdicts filed from
   twelve generations, each given up to six corrected attempts per shard, is the headline fact about
   tier 1 in this attempt — and it is a fact about scorer compliance with item 8, not about what any
   arm retained.
6. **`FREEZE-2.md` is still closed at this run's rows.** T5b added none.
