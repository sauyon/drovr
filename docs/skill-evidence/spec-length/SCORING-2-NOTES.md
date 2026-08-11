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
numbered item, a whole line-block — refused over a markup delimiter:

| shape | count | example |
|---|---|---|
| sentence closing inside a `**…**` emphasis run | 6 | `…marked \`[tier 4]\`.**` |
| sentence closing inside a `*"…"*` quoted run | 3 | `…before claiming done."*` |
| fixed-width line-block row, no terminal punctuation | 4 | `ONLY: tdd, systematic-debugging` |

**The mechanism runs through BOTH ends, which a first reading of these misses.** Item 8 requires a
span's last character to be one of `.!?:;` (or end-of-line before a new block, or `|`) — so a bullet
whose sentence ends inside an emphasis run closes on `*`, not on the full stop. **And** it requires
a span that opens its own line to have a *preceding* line that is blank, a heading, a table row, or
ends in `.!?:;` — so one bullet ending `."*` also disqualifies the bullet **after** it. In a list of
such bullets each line breaks its neighbour. That is why the refusals cluster.

This is the third strictness T2 found while writing the check and asserted as a refusal rather than
loosening. Bold-led and bold-closed lines are everywhere in this corpus. §3a measures how far it
reaches.

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

Every `R6` re-run of a tier-1 shard in this attempt. **Reason is always the protocol trigger, never
an outcome.** Rounds map to attempts unevenly because `66530f` ran once alone as the §1 calibration
probe:

| round | shards dispatched | attempt no. | trigger | outcome |
|---|---|---|---|---|
| 0 | `66530f` ×2 | 1 | first run — the §1 calibration probe | **failed** the item-8 check, 13 refusals |
| 1 | `66530f` ×2 | 2 | `R6` — verdict failed the item-8 mechanical check | **failed**, 16 refusals |
| 1 | the other five ×2 each | 1 | first run | **all five failed**, 8–20 refusals each |
| 2 | `66530f` ×2 | 3 | `R6` — same trigger; **cap of §2.1 now exhausted** | **failed**, 14 refusals |
| 2 | the other five ×2 each | 2 | `R6` — same trigger | **all five failed**, 13–21 refusals each |

**Fourteen shard-pair attempts. 1581 spans written, 194 refused (12.3%). Zero passing verdicts.**

| generation | attempt 1 | attempt 2 | attempt 3 |
|---|---|---|---|
| `66530f` | 13 / 128 | 16 / 134 | 14 / 132 |
| `6e7393` | 16 / 125 | 19 / 129 | — |
| `e085f2` | 20 / 123 | 21 / 121 | — |
| `e790f5` | 8 / 137 | 15 / 146 | — |
| `fd2c24` | 17 / 138 | 19 / 141 | — |
| `fe4059` | 16 / 128 | 13 / 127 | — |

*(refused spans / spans written. A verdict passes only at **zero**.)*

**The re-runs do not converge, and they were never going to.** The refusal rate is flat across
attempts and across generations; the best single attempt in the whole set still carried 8 refusals
where a pass requires 0. Each raw failure report — naming every refused span — is committed under
`retention-2/parts/superseded/refusal-reports/`, and every shard of every attempt is beside it.

**The last permitted re-run was not spent, and here is why that is not outcome-shaping.** §2.1 allows
two re-runs per shard; `66530f` used both and is terminal under §2.4. T5a's verification requires
**all six** verdicts to pass, so no result from a third attempt on the other five could change this
task's outcome. And the asymmetry that matters: stopping early here leaves every verdict **failing**,
which is the outcome least favourable to producing any measurement at all. A stopping rule can only
be accused of chasing a result when stopping helps; this one costs.

---

## 3a. The finding: the tier-1 instrument cannot be cleared on this corpus

**Item 10's frozen prompt instructs the scorer to produce spans that item 8's frozen rule refuses.**
The prompt says a span *"must begin at the start of a sentence, table cell, list item, heading or
line-block, and end at the end of one"*. Every refused span above satisfies that description. Item 8's
mechanical rule then applies a different test — one written in terms of raw characters:

- a span **begins** on a boundary only if the **preceding line** is blank, a heading, a table row, or
  ends in one of `.!?:;`;
- a span **ends** on a boundary only if its **last character** is one of `.!?:;`, or it ends at
  end-of-line with the next line blank or opening a new block, or it is followed by `|`.

**Neither end sees through a closing markup delimiter.** A sentence that ends `done."*` or
`[tier 4]`.**` terminates in `*`, not in `.`. That breaks the right-hand test for the span itself,
**and** the left-hand test for whatever span begins on the following line. In a corpus written in
bold-led bullets, quoted announcement strings and fixed-width line-blocks, that is not a rare shape.

**What the instrument admits, measured.** Citing every distinct non-blank line of each generated spec
as a candidate span and running the authoritative check over the result:

| generation | distinct non-blank lines | admissible | admitted |
|---|---|---|---|
| `66530f` | 517 | 121 | 23.4% |
| `6e7393` | 431 | 100 | 23.2% |
| `e085f2` | 346 | 101 | 29.2% |
| `e790f5` | 463 | 124 | 26.8% |
| `fd2c24` | 636 | 123 | 19.3% |
| `fe4059` | 341 | 100 | 29.3% |
| **total** | **2734** | **669** | **24.5%** |

**Three of every four complete lines of a generated spec cannot be quoted at all.** A verdict must
evidence 91 rows, and only 100–124 admissible lines exist per document — so the scorer must not
merely find admissible spans, it must find admissible spans that also happen to be the ones
evidencing the specific rows. Admissibility and relevance are close to orthogonal, and the margin
between them is about zero.

**Two honest bounds on that 24.5%.** It measures **whole lines**, which is the strategy item 10's
prompt tells the scorer to use (*"quote the whole sentence"*); admissible sub-line spans exist — a
sentence preceded by `. ` on the same line qualifies — so 24.5% is not a census of every admissible
substring. And it is a property of this corpus's markup, not a claim about prose in general.

**This is the first attempt's failure, relocated.** `RESULTS.md`'s null was *about the instrument*:
frozen item 8a made every verdict ~1% passable by construction, so no arm could pass and the control
could not either. **The redesign fixed 8a and did not touch this.** Item 8's self-containment rule is
new to this attempt, and it now makes every verdict **0%** passable at the mechanical gate — a
stricter failure than the one it replaced, at an earlier tier.

**It is arm-symmetric, and that is the one consolation.** The admitted rate varies from 19.3% to
29.3% with no relation to any arm, and the refusal rate is flat across all fourteen attempts. So it
does not bias a comparison between arms. **It prevents there being a comparison at all.**

**Nothing was adjusted to get past it.** `PROTOCOL-2.md` is frozen and stays frozen; item 10's
template was delivered byte-identical on every attempt; the check was not weakened; no span was
patched. Per the driver's standing constraint, a wrong protocol is a finding to report, not a file to
adjust. **T5a therefore reports a blocked task rather than a scored one**, and `retention-2/` is left
holding no verdict, because a verdict the instrument refused is not evidence of retention and must
not sit there as though it were.

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
