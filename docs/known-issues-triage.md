# known-issues.md triage — 2026-08-06

**This is a dated SNAPSHOT, and `known-issues.md` has moved since. Every number and every
`L<n>` below describes the file as it stood on 2026-08-06 — do not read them as current.**
Snapshot state: `docs/known-issues.md` on `drovr/brainstorm-rework` (base:
`drovr/skill-stickiness`, current with main), **4107 lines, 87 `##` sections**.

Method: every section's heading, `**Status:**` and `**Severity:**` line read; the 12 sections
carrying neither were read in full. Sections were **not** read end-to-end — classification is
from stated severity and self-description, so a few calls may move on closer reading.

## How to use the `L<n>` references

They are **line numbers into the 2026-08-06 snapshot**, not into the file you have. They were
already drifting when this was written, and the 2026-08-07 change below moved everything after
the old `L494` up by ~300 lines. Resolve an `L<n>` by its quoted description against the
current **headings** — which is the repo rule for citing `known-issues.md` anyway. Treat an
`L<n>` as an id, never as a location.

## What changed since the snapshot

**2026-08-07 — fixed entries are now deleted, not annotated.** `known-issues.md` went
87 → **79** sections (4268 → 3961 lines as of that change). Seven entries whose defect was
fixed were removed outright; three more that were marked FIXED were **kept and retitled**
because each still carries live open content (the cross-run-state bug *class* with open
follow-ups, the panel's still-open *partial*-commit hazard, and the workspace-repair contract
with its unguarded double-provision race). Two lessons were lifted out of deleted entries into
a new **`## Lessons kept from retired issues`** section, which replaced `## Resolved`.

Consequences for the counts below, which have **not** been recomputed: class **B — already
fixed** is by construction now near-empty and is no longer a category this file's policy allows
to accumulate; the `meta` row's `Resolved` is now that Lessons section; and the class-A total
and severity split lost the retired `questions.json` Submit entry (class A, high). Re-run the
triage rather than adjusting these by hand — a snapshot patched in place is worse than a stale
one honestly labelled.

## Counts (as of the 2026-08-06 snapshot)

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

## Two the brainstorm work reaches — one now retired, one only half-covered

- **Submit dies when `questions.json` is not a bare array** (high) — **retired**. The
  interactive-brainstorm work deleted `questions.json`, the `GET questions` route and the
  questions panel outright, so the entry is **gone from `known-issues.md` by deletion, not by a
  fix**: there is no longer a shape the payload can be wrong in. Do not look for a patch that
  closed it.
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
