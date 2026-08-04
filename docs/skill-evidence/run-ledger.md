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

**Task 6's 10 runs, in detail:** 5 skills × 1 `dev` scenario × 2 samples, arm A text, model
`sonnet`, foreground `general-purpose` subagents. **Zero retries** — all 10 probes returned a
confirmation and wrote a transcript on their first dispatch. The stage completed its planned
10 of 10, so `ceiling hit?` is `no` in the sense that matters: no work was cut short by the
ceiling and nothing was left unmeasured. There is no headroom left in this row.

Two transcripts (`code-review/42a94a.md`, `code-review/d7006e.md`) needed their `## Scenario`
block repaired after the fact — the probes abridged their copy of it. **This cost no runs**:
the block was restored from the checked-in `code-review-1.md`, and the `## Response` blocks
were left byte-identical. See `docs/skill-evidence/code-review.md` for the full record.
