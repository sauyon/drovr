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

## T5a's outcome, up front

**Two of six verdicts filed. Four not filed.**

| | |
|---|---|
| filed, passing `spec_length_2_retention_verdicts_are_complete_and_quoted` | `6e7393` (91/91 present, 126 spans), `e085f2` (90/91, 120 spans) |
| **not filed** — cap exhausted, still failing the mechanical check | `66530f`, `e790f5`, `fd2c24`, `fe4059` |

**The four are MISSING DATA, not low retention scores.** Nothing about what an arm retained can be
read off a file the mechanical gate rejected. **T8 must not treat an absent verdict as a zero, and
T6 has no input for those four ids.** §5 is the escalation.

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

**Pre-registered in commit `ed583e9` before the corrected prompt was dispatched.**

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

---

## 3. What the correction changed — the finding of this task

**54 shard dispatches in all: 26 under the defective prompt, 28 under the corrected one.**

| | defective prompt | corrected prompt |
|---|---|---|
| shard-pair attempts | 13 | 15 |
| spans written | 1709 | 1946 |
| **boundary refusals** | **207 — 12.1%** | **43 — 2.2%** |
| verdicts that cleared the gate | **0 of 13** | **2 of 15** |

**The boundary refusal rate falls by a factor of five when the scorer is handed the rule the gate
runs.** That is the finding, and it retires the earlier one.

**Per round, under the corrected prompt:**

| round | shards | spans | boundary | shared-span | verdicts cleared |
|---|---|---|---|---|---|
| 1 | `66530f` only | 128 | 7 | 0 | — |
| 2 | all six | 780 | 22 | 6 | **`e085f2`** |
| 3 | the five still failing | 641 | 11 | 6 | **`6e7393`** |
| 4 | the three with budget left | 397 | 3 | 6 | — |

**One claim from this task's own commit message needs correcting here.** `74e4b0f` says duplicate-span
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

---

## 5. Escalation — what only the driver can decide

**The run cannot proceed to T6 for four of six `skill-stickiness` ids.** T6 reads
`retention-2/<id>.json` and computes a stride-5 sample over it; for `66530f`, `e790f5`, `fd2c24` and
`fe4059` there is nothing to read. Three options, none of which is T5a's to take:

1. **Re-open the re-run budget** for the four, under the corrected prompt. The trend supports it —
   two verdicts cleared in two rounds — and `R6` itself sets no cap; the only thing in the way is
   this task's own pre-registration, which the driver can lift explicitly and on the record.
2. **Accept a scoped result** over the ids that cleared, and carry four ids as missing data into
   `RESULTS-2.md`. Honest, and much weaker: two of six is not a fixture.
3. **Treat the shared-span failures separately.** Two of the four failed *only* on shared spans, with
   zero boundary refusals — a different defect with a different remedy.

**T5b and T5c should not start until this is decided**, because whatever is decided applies to them:
they will run the same instrument over `tiered-review` and `tui-dc-picker`, and they should run the
**corrected** builder from the first dispatch rather than rediscovering §2a.

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

**Item 11(b)'s re-disclosure, owed by every task in this run.** T5a dispatched 54 scorer runs whose
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

1. **Use the corrected `tools/build-tier1-prompts.py`** — it hands over item 8. Do not rebuild prompts
   from the template alone; §2a is why.
2. **Wait for the driver's decision in §5** before dispatching, because it governs your budget too.
3. **`tools/measure-span-admissibility.py` is hardcoded to `skill-stickiness`** (its id list, ledger
   path and row-count assertion). It needs a fixture argument before T5b or T5c can use it. Given §3a,
   treat what it measures with care: it counts whole lines, and most real spans are multi-line.
4. **Undischarged obligations owed to T8, carried from `GENERATION-2-NOTES.md`'s six-row table —
   four are still open:** flag both `R5a` generations and read neither as a win; do not read
   `fd2c24`'s retention as evidence; log the early publication of the `wc`/`R5a` figures as a
   deviation; and — after the join — report whether generation titles track the arm.
5. **Still-open questions from T4's handoff, which nothing else keeps alive:** the title-variance
   channel (owed by T8 and the write-up); whether re-dispatching two transmission questions was right
   (decide before T9; the revert is `git revert 6268365`); the 20 freeze rows are not checked for
   completeness; and the leak check tells a reader to "re-run under `R6`" for a case `R6`'s closed
   trigger list does not cover.
6. **A frozen-instrument inconsistency found here.** Item 9's first blockquote — handed verbatim to
   every scorer — says a paraphrase *"may be evidenced by up to three spans"*, while the same prompt's
   rules say **1 to 5**. Scorers followed the rules. With §2a this is the *second* place item 10's
   template and the items it claims to carry disagree; they belong together as one finding about
   item 10.
7. **The built prompts are not committed.** "Byte-identical on every attempt" rests on the builder
   being deterministic, which is checkable by re-running it, not on a diffable artifact.
