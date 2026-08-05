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
| 16 | **Unaided control (`tdd`) — not a §7.3 row** | 4 | 27 | 4 (this stage only) | n/a |

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

> **RESOLVED 2026-08-05 by the user, who authorized further measurement at this phase's
> discretion.** The call taken is **option 1: the *Arm B on held-out* row's ceiling rises from 20
> to 21**, so Tasks 17–20 have the 16 runs they need. Option 2 was rejected because accepting a
> null would discard a whole skill's arm B measurement to pay for a protocol bug. Option 3 was
> rejected because rewriting the retry rule would retroactively edit a spent row and weaken the
> guard that stops a bad probe being re-rolled until it says something better.
>
> **This is a deviation from frozen `spec.md` §7.3, recorded as one rather than folded in
> silently.** Tasks 17–20 must still read this table before starting and halt with a null rather
> than extend any *other* row.

**Task 16's unaided control (4 runs, 2026-08-05) is not a `spec.md` §7.3 stage.** §7.3's budget
table has no unaided row — the gap Task 6 recorded as its first limitation — and the user
authorized these runs after `ab-tdd` had shipped. They enter **no pre-registered bar** and do not
change the ab-tdd verdict. 2 held-out scenarios × 2 samples, no skill text of any arm in the
prompt, blind-scored like every other stage. Result: **3 of 4 compliant unaided**, against 4/4
for each of A, A′ and B. Full record in `docs/skill-evidence/tdd.md` under *Unaided control*.

**It is scoped to `tdd`.** The other four skills still have no unaided condition, and this result
does not transfer — discriminating power belongs to each scenario pair, and `tdd`'s two differed
sharply (`tdd-2` produced the only failure; `tdd-3` was saturated at 2 of 2 even unaided).
**Cumulative is now 27**, and the run's arithmetic totals ≈123 against a table §7.3 itself writes
as "≈122".

Two transcripts (`code-review/42a94a.md`, `code-review/d7006e.md`) needed their `## Scenario`
block repaired after the fact — the probes abridged their copy of it. **This cost no runs**:
the block was restored from the checked-in `code-review-1.md`, and the `## Response` blocks
were left byte-identical. See `docs/skill-evidence/code-review.md` for the full record.
