# Run ledger

Every probe subagent this run spawns is counted here. **Append-only**: each probe-spawning
task adds its rows as its *last* evidence write, and rewrites nothing above.

`spec.md` §7.3 sets a **122-run hard ceiling** across the whole run, split into per-stage
ceilings. This table is the only mechanism tracking it. Tasks 16–21 read the cumulative
total *before* starting and **halt with a null** rather than cross a ceiling — a stage that
hits its ceiling records the null in `docs/skill-evidence/<skill>.md` and stops; it does not
silently extend.

**A retried run counts.** If a probe is re-dispatched because its first attempt failed,
returned nothing, or wrote a bad transcript, that is two runs against the ceiling, not one.
The `runs this stage` cell records what was actually spent, not what was planned.

| task | stage (§7.3 row) | runs this stage | cumulative | stage ceiling | ceiling hit? |
|---|---|---|---|---|---|
| 6 | RED / baseline on dev set | 10 | 10 | 10 | no |
| 16 | Arm A on held-out (`tdd`) | 4 | 14 | 20 | no |
| 16 | Arm A′ on held-out (`tdd`) | 4 | 18 | 16 | no |
| 16 | Arm B on held-out (`tdd`) | 5 | 23 | 20 | no |
| 16 | REFACTOR re-tests (`tdd`) | 0 | 23 | 20 | no |

**Task 6's 10 runs, in detail:** 5 skills × 1 `dev` scenario × 2 samples, arm A text, model
`sonnet`, foreground `general-purpose` subagents. **Zero retries** — all 10 probes returned a
confirmation and wrote a transcript on their first dispatch. The stage completed its planned
10 of 10, so `ceiling hit?` is `no` in the sense that matters: no work was cut short by the
ceiling and nothing was left unmeasured. There is no headroom left in this row.

**Task 16's 13 runs, in detail:** 3 arms × 2 held-out scenarios × 2 samples = 12 planned, plus
**1 retry**. Probe `79bd97` (arm B, `tdd-3`, sample 1) returned its answer as its final message
instead of writing its transcript file, producing no transcript; it was re-dispatched and the
retry succeeded. Per this table's own rule that a retried run counts, the arm B row reads **5**,
not 4, and the phase total is **13**, not 12. Model `sonnet`, `general-purpose` subagents,
per `plan.md` C5.

The **REFACTOR row is 0 by construction, not by omission.** Arm A scored 4 of 4, so `plan.md`'s
pre-registered branch **(a)** fired and evaluation stopped; REFACTOR is reachable only via branch
(d). `tdd` reverts to A′ with its ≤4 REFACTOR allotment unspent. See
`docs/skill-evidence/tdd.md`.

**Headroom after this phase: 99 runs of 122.** Per-stage: *Arm A on held-out* has 16 of 20 left
and needs 16 (4 skills × 4) — **exactly zero slack**. *Arm A′ on held-out* has 12 of 16 and needs
12 (3 discipline skills × 4) — **exactly zero slack**.

**The *Arm B on held-out* row is now over-subscribed, and this needs a run-level decision before
Task 17 starts.** Its ceiling is 20. Task 16 spent **5** of them (4 runs + the retry), leaving
**15**. Tasks 17–20 have 4 skills left and need **16**. That is a shortfall of **1 run, already
incurred** — not a margin that a future retry would erase. Even with zero further retries, the
last skill in that stage reaches its final run with the ceiling crossed, and §7.3's rule is
explicit: work **halts and records a null**, it does not silently extend.

The three ways out are all run-level calls and **none of them is taken here**: raise the arm B
row's ceiling (a change to frozen `spec.md` §7.3); accept a null on one skill's arm B and record
which; or rule that a protocol-failure retry producing no transcript is not a charged run (a
change to this table's stated rule, which would also retroactively rewrite the row above).
**Escalated in `ab-tdd-HANDOFF.md` rather than decided by this phase.** The ledger records what
was actually spent; it is not the place to resolve the shortfall by relabelling it.

Two transcripts (`code-review/42a94a.md`, `code-review/d7006e.md`) needed their `## Scenario`
block repaired after the fact — the probes abridged their copy of it. **This cost no runs**:
the block was restored from the checked-in `code-review-1.md`, and the `## Response` blocks
were left byte-identical. See `docs/skill-evidence/code-review.md` for the full record.
