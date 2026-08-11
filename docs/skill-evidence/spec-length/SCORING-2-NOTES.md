# Tier-1 scoring notes — second attempt

`plan.md` T5a/T5b/T5c say a boundary-rejected span is *"a malformed shard → re-run whole under `R6`,
logged for `RESULTS-2.md`"*. `RESULTS-2.md` is not written until T8, so the log has to live
somewhere in between. **This is that file.** T5a opened it; T5b and T5c append to it; T8 reads it.

**Nothing structural points at this file** — not `plan.md`, not `PROTOCOL-2.md`, not `FREEZE-2.md`,
and no test opens it. It survives only because each tier-1 handoff names it. Stated here so a reader
does not mistake it for a checked artifact.

**This file records procedure and counts. It records no arm**, and its author never opened
`blind-map-2.json`. See §4 for what its author *did* come to know, which is not nothing.

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

**The refusal rate is 10.2%, not 84.9%, and the difference is not luck.** T2 bounded its own figure
correctly: those 1127 spans were written under `PROTOCOL.md`, which had **no boundary rule at all**,
so 84.9% measured v1's quoting habit rather than the rule's severity. Item 10 puts the rule into the
v2 scorer prompt, and a scorer told the rule mostly obeys it.

**The count is the authoritative check's own, not a re-implementation.** It is read off the failure
report of `cli/tests/skills_valid.rs::spec_length_2_retention_verdicts_are_complete_and_quoted`,
which names every offending span in one pass. No second boundary checker was written, precisely so
there is no second answer to the question of what the rule refuses.

### What the surviving 10.2% actually looks like — and it is not v1's failure

The rule was written against a span **clipped at a hard wrap**, leaving one fragment ending
mid-phrase and one beginning mid-phrase. `invalidated/db3e2d.json` is item 8's worked example of it.

**None of the 13 has that shape.** Every one of them is a *complete* unit — a whole bullet, a whole
numbered item, a whole line-block — refused at its right-hand end on a markup delimiter:

| shape | count | example ending |
|---|---|---|
| sentence-final `.` followed by a closing `**` | 6 | `…marked \`[tier 4]\`.**` |
| sentence-final `.` followed by a closing `*"` | 3 | `…before claiming done."*` |
| fixed-width line-block row, no terminal punctuation | 4 | `ONLY: tdd, systematic-debugging` |

Item 8 requires a span's last character to be one of `.!?:;`, or end-of-line with the next line blank
or opening a new block, or be followed by `|`. **A bullet whose sentence ends inside an emphasis run
closes on `**`, not on the full stop, and is refused** — which is exactly the third strictness T2
found while writing the check and asserted as a refusal rather than loosening. Bold-led and
bold-closed bullets are everywhere in this corpus.

**This is recorded, not repaired.** `PROTOCOL-2.md` is frozen and item 10's template is frozen; the
scorer prompt cannot be given an extra hint about emphasis runs, and the check cannot be relaxed. A
wrong protocol is a finding to report, not a file to adjust. **The finding, stated plainly: item 8's
right-hand boundary condition does not recognise a closing emphasis delimiter, so it refuses
well-formed spans at a rate this corpus makes routine.** It is arm-symmetric — nothing about it
favours a long arm or a short one — so it does not bias the comparison; it costs re-runs.

---

## 2. The re-run policy, fixed before the first re-run was dispatched

`R6` permits a re-run when *"a verdict fails the item-8 mechanical check"* and requires every re-run
to be logged with its reason. It sets **no cap**, and an uncapped re-run loop against a rule that
refuses a routine markup shape is a slow way to arrive at `R6`'s own prohibition — re-running until
the output is acceptable. `RESULTS.md` §7.4 is the precedent that a re-run can itself fail.

**So a cap is fixed here, and it is fixed before any re-run has been dispatched or seen.** The commit
that adds this section precedes the first re-run dispatch, which is what makes that claim checkable
rather than a matter of trust.

1. **At most two re-runs per shard — three attempts in all.**
2. **The cap applies identically to every shard of all eighteen generations**, whatever any attempt
   returns. It is not re-decided per generation and not re-decided per arm.
3. **A re-run re-runs the shard WHOLE**, with the byte-identical prompt. Nothing is patched, no span
   is edited, and the prompt is never adjusted to steer the scorer away from the refused shape —
   that would be amending a frozen instrument mid-measurement.
4. **A shard still failing after its third attempt is reported as a failing verdict, not fixed** —
   recorded here and in `RESULTS-2.md` with its refused spans, exactly as `RESULTS.md` §7.4 recorded
   five verdict files left un-re-run. Its generation is then **not** at full retention by
   construction, and **T8 must treat it as missing data rather than as a result**, because a verdict
   the instrument refused says nothing about what the arm retained.
5. **Every attempt is logged in §3**, including the ones that succeed.

**Why a cap is arm-symmetric.** Scorers are blind — item 10 hands them one generated spec, their own
shard's ledger rows, and items 8 and 9, and nothing else. The cap is a fixed number applied to every
shard before any of them ran. No arm can carry a row a different arm cannot.

**Superseded attempts are kept.** A failed shard's output is moved to
`retention-2/parts/superseded/<id>-<k>-attempt<n>.json` rather than overwritten, so the log in §3 can
be checked against the files it describes. `parts/` is item 8's sanctioned working-material directory
and no test recurses into it.

---

## 3. Re-run log

Every `R6` re-run of a tier-1 shard in this attempt, in dispatch order. **Reason is always the
protocol trigger, never an outcome.**

| # | shard | attempt | trigger | outcome |
|---|---|---|---|---|
| — | — | — | — | *(appended as re-runs are dispatched)* |

---

## 4. What this file's author knew, and what that costs

Stated because it weakens a claim this task would otherwise make.

**T5a's author never opened `blind-map-2.json`.** Sharding was done from `fixture-map-2.json`, which
is arm-free, exactly as `plan.md` P2 intends.

**But it did print `wc -l`/`wc -c` for the six `skill-stickiness` generations in one table**, against
T4's explicit handoff instruction to *"print aggregates, not per-item sizes, if you ever want to stay
blind to your own draw."* Six generations of one fixture are three arms of two samples each, so a
size-ordered pairing is a **plausible guess at the arm partition** — not a read of it, but not
nothing either. **The figures themselves leak nothing new**: `GENERATION-2-NOTES.md` §1 already
publishes them and `wc -l generated-2/*.md` reproduces them in one command. What was avoidable was
holding them in the context that dispatches scoring.

**What stands between that inference and the measurement**, in order: the scorers are blind and were
handed nothing but their own spec and rows; every prompt was machine-built from `PROTOCOL-2.md` by
`tools/build-tier1-prompts.py` with no per-item transcription; the re-run cap in §2 was fixed before
any re-run outcome was known; and the pass/fail authority is a committed test, not a judgement.

**T5b and T5c should not repeat it.** `wc` the six files as a total, or not at all.
