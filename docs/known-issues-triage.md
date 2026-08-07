# known-issues.md triage — 2026-08-06

Against `docs/known-issues.md` on `drovr/brainstorm-rework` (base: `drovr/skill-stickiness`,
current with main): **4107 lines, 87 `##` sections.**

Method: every section's heading, `**Status:**` and `**Severity:**` line read; the 12 sections
carrying neither were read in full. Sections were **not** read end-to-end — classification is
from stated severity and self-description, so a few calls may move on closer reading.

## Counts

| class | n | meaning |
|---|---|---|
| **A — real, open, fixable** | 56 | a defect with a fix |
| **B — already fixed** | 11 | marked FIXED / past tense; record only |
| **C — deliberate, documented, not a defect** | 12 | "working as designed", "accepted, not fixed" |
| **D — test flake / dev infra** | 6 | real, but not product bugs |
| **E — upstream, not drovr** | 1 | context-% computed against 200k |
| meta | 2 | `Resolved`, `Follow-ups` |

**Of the 56 open: 13 high, 18 medium, 25 low.**

## The finding that matters: 56 issues are ~6 themes

Fixing these one at a time is the expensive way. They cluster by root cause:

**1. The gate's completion signal is unreliable (7 issues, 5 high).**
`L2526` gate writes nothing when the run dir is gone yet reports approved · `L2551` review-state
cache never evicted, so a reused run name inherits the old verdict · `L2752` piping a `wait`
destroys its exit-code contract, timeout reads as approval · `L2938` stale `server.addr` +
occupied port deadlocks discovery permanently · `L3654` gate tells the driver to poll for
`ready`, losing a fast approval · `L1674` `review wait` fails if the server restarts mid-wait ·
`L2824` `review.state.json` is sticky — polling detects a condition, not a transition.
*Common shape: a wait/poll that cannot distinguish "approved" from "something went wrong".*

**2. drovr cannot tell whether an agent received its prompt (7 issues, 3 high).**
`L1370` `phase send` returns success with the prompt unsubmitted · `L3707` every cold `opencode`
reviewer pane swallows its seed · `L3994` the seed-delivery detector reports the **opposite** of
what happened on a cursor pane and kills the panel · `L1612` agents park on the "New MCP server"
prompt undetected · `L903` a read-only cursor reviewer parks at plan mode's gate · `L4059` an
auto-suggested prompt is indistinguishable from composer content · `L1131` `code-review run`
panel never completes.

**3. `state.json` has no concurrency control (4 issues, 1 high).**
`L2007` concurrent writers lose whole phases (verified) · `L2278` `cleanup` clobbers a concurrent
write · `L2469` `save_preserving_archived` rescues one field only · `L2695` (its explanation).
*All four name the same missing compare-and-swap / lockfile.*

**4. Cleanup / archive lifecycle leaks (6 issues, all low-medium).**
`L806` `cleanup` auto-commits whatever the worktree holds · `L2574` `drovr new` on an existing
name orphans the old workspace · `L2636` `--purge` leaves a destroyed workspace with
`archived: false` · `L3114` empty workspace left when herdr can't list panes · `L2623`
`code-review run` only checks `archived` at entry · `L2608` archive failures only reach the
browser console.

**5. Review-panel scope and trust (4 issues).**
`L86` reviewers judge an intermediate task against the whole run's goal · `L708` an author-run
panel is not a gate · `L1971` `code-review brief` names a tool the reviewer does not have ·
`L1289` `submit_findings` can be DEFERRED.

**6. Test-suite reliability (6, class D).**
`L388`, `L1061`, `L1782`, `L1810`, `L2666` flakes + `L2736`/`L1009` `ENV_LOCK` poisoning.
`L2736` has a written fix shape (`.lock().unwrap_or_else(|e| e.into_inner())` across 5 files) —
cheap, and it stops one failure becoming ~90.

## Two are already being resolved by the brainstorm work

- **`L424`** (high) — Submit dies when `questions.json` is not a bare array. The approved-design
  direction **deletes `questions.json`**, so this goes away rather than getting fixed.
- **`L3830`** — `feedback.json` overwritten every turn. `interview.jsonl` is append-only, which
  covers the interview channel; the gate's own `feedback.json` still has the defect.

## Cheap wins, independent of everything else

1. `L1747` + `L933` — `main` is not `cargo fmt` clean, and formatting one file reformats the
   crate. One `cargo fmt` commit on main retires both and removes a recurring merge hazard.
2. `L2736` — the `ENV_LOCK` sweep above.
3. `L357` — editing `cli/web/index.html` can silently test the OLD page. A build-freshness
   issue that has been wasting debugging cycles.

## Recommended order

Theme 1 (gate signal) first — it is the highest-severity cluster and it is the mechanism every
run depends on to advance. Then theme 3 (`state.json`), which theme 4 partly depends on. Theme 2
is the largest and most valuable but needs herdr-side investigation, so it wants its own run.
