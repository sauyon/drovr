# Invalidated scoring output — not verdicts, and no gate reads them

Every file in this directory is a `skill-stickiness` retention scoring pass that **failed
`PROTOCOL.md` item 8a's relevance adjudication** and is therefore invalidated in whole. Item 8a is
unambiguous: *"Any `establishes: false` invalidates the entire verdict file."*

They are kept because they are the evidence behind T5's escalation, and because 546 row-judgements
plus seven adjudications should not be thrown away merely because they cannot be used as verdicts.

**They are deliberately not in `retention/`, and `retention/` deliberately does not exist.** A file
in `retention/` is a verdict `T7` joins and `R2` gates on. These are not verdicts. This is the same
reasoning that put T4's feasibility reading in `feasibility/` rather than under `retention/parts/`
(`RESULTS.md` deviation 3), applied to a stronger case: T4's reading merely *had not been*
adjudicated, and these *have been, and failed*.

`cli/tests/skills_valid.rs::spec_length_retention_verdicts_are_complete_and_quoted` reads the
immediate `*.json` children of `retention/` and will not see any of this. Neither will
`spec_length_blind_map_precedes_every_retention_verdict`.

## What is here

| file | what it is |
|---|---|
| `<id>.json` | an assembled verdict, item-8 well-formed, item-8a **failed** |
| `parts/<id>-<k>.json` | the two scorer shards it was concatenated from (item 10) |
| `4a73ef.attempt-1.json` | the first scoring pass of `4a73ef`, replaced by the `R6` whole re-run |
| `parts/4a73ef-<k>.attempt-1.json` | that first pass's shards |

`4a73ef.json` is **attempt 2** — the `R6` re-run. It failed item 8a as well, on four of eighteen
sampled rows against attempt 1's six, with sixteen of the eighteen sampled rows carrying different
spans. That is what establishes the failure as structural rather than one scorer's slip.

## Why they failed, in one paragraph

Item 9 defines `present` as *recoverable and actionable from the generated spec alone*. Item 8a asks
a blind adjudicator whether **1–3 spans, with the spec withheld**, establish the row. Those are
different standards, and the second is strictly harder: a row that is genuinely present and
actionable in a 585-line spec can still be un-establishable from three quoted fragments. 26 of 106
adjudicated rows (25%) came back `establishes: false`, and because item 8a's stride is fixed, the
same hard rows are sampled in every file — `skill-stickiness-55` and `-65` each failed in **4 of the
4** files that sampled them. With whole-file invalidation over ~18 sampled rows, a 25% per-row
failure rate makes a passing file a `0.75^18` ≈ 0.6% event. The instrument cannot clear its own gate.

See `../RESULTS.md` §7 for the full record, the `R6` re-run log, the `R6a` doubts, and the
escalation. **Nothing here may be promoted into `retention/` without that escalation being
resolved** — an invalidated verdict filed as a verdict is the one thing item 8a exists to prevent.
