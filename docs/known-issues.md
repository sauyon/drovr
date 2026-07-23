# Known issues

## `drovr phase send` does not reliably submit large injections

**Severity:** high (the pipeline's own injection path is unreliable).
**Found:** 2026-07-21, dogfooding the drovr-v2 pipeline (every implement task hit it).

### Symptom

After `drovr phase send <run> <phase> "<large briefing>"`, the pasted text lands in
the target claude pane's input buffer but is **never submitted** — the agent sits idle
(`$0.00` cost, no activity) indefinitely. `drovr phase wait` then times out even though
the agent never started.

Observed with multi-KB briefings, but also reproduced with a short (~1 line) pointer
message — so it is **not strictly size-gated**; the submit carriage return is being lost.

### Root cause (hypothesis)

`SystemHerdr::agent_send` writes the text, waits `PASTE_SETTLE` (150 ms), then sends a
CR to submit. claude's TUI uses bracketed-paste; when the CR arrives before the paste
has fully "settled," it is absorbed into the paste instead of submitting. The 150 ms
constant (see `cli/src/herdr.rs`, `PASTE_SETTLE`) is too short for large / slow pastes,
and the failure is timing-dependent (racy), which is why a short message can miss too.

### Reliable workaround (used to drive the drovr-v2 run)

Follow every content send with a **second, empty submit**, with a short delay between:

```
drovr phase send <run> <phase> "<briefing>"
sleep 4
drovr phase send <run> <phase> ""      # bare CR flushes the buffered paste
```

The empty send carries no paste, so its CR submits cleanly. Sending the two
back-to-back (no delay) can still race — the empty CR can fire before the big paste
lands and submit nothing.

A lower-overhead variant that also worked: inject a **short pointer** message
("your briefing is in `<path>` — read it and execute") instead of a large paste, still
followed by the empty-submit flush.

### Fix ideas (for a future drovr change)

1. Have `agent_send` always send the submit CR as a **separate keystroke after a
   readback/confirmation** that the paste landed, rather than a fixed 150 ms sleep.
2. Make `PASTE_SETTLE` scale with payload size, or poll the pane until the buffer
   reflects the full paste before sending CR.
3. Prefer a file-pointer injection convention in the skills (write the briefing to the
   run dir, send a one-line pointer) to keep pastes tiny.
4. Add an e2e/integration test that asserts a large `agent_send` actually submits.

## Review server: diff baseline (`prior.md`) doesn't advance on agent revisions

**Severity:** medium (the "what changed this turn" diff is wrong when the agent revises more
than once between reviewer submits).
**Found:** 2026-07-22, after posting two `review summary` revisions for one reviewer turn.

### Symptom

The diff panel shows the change since the reviewer's **last submit**, not since the previous
revision. If the agent posts multiple `drovr review summary` revisions between reviewer
submits, they all diff against the same stale `prior.md`, so a later revision shows the
accumulated change (e.g. v1→v3) instead of just the latest (v2→v3).

### Root cause

`cli/src/review.rs` snapshots `prior.md` **only** in the POST `/submit` handler (on reviewer
submit). `drovr review summary` (a new agent revision) does not re-baseline `prior.md`, and
the agent overwrites `spec.md` before calling `review summary`, so the pre-revision version is
already lost.

### Fix

Re-baseline per revision: snapshot the outgoing `spec.md` → `prior.md` **before** the new
revision supersedes it. Options: (a) a `drovr review revise` command (or `review summary`
variant) that snapshots `spec.md`→`prior.md` then accepts the new spec; (b) the skill workflow
copies `spec.md`→`prior.md` immediately before writing the new `spec.md`. Goal: each turn's
diff = (previous revision) vs (this revision), regardless of how many revisions per submit.

## Phase agents in a nested git worktree edit the OUTER repo, not the worktree

**Severity:** high (breaks driving drovr against a worktree — the intended isolation model).
**Found:** 2026-07-23, dogfooding the pipeline to implement drovr:code-review.

### Symptom

`drovr new --dir <worktree>` + phase spawn: the phase agent's pane cwd is correctly the
worktree, and `plan.md` uses relative paths — yet the agent's edits (`cli/src/config.rs` etc.)
land in the **outer/main checkout**, not the worktree. `git status` in the worktree stays
clean; the outer repo's working tree goes dirty.

### Root cause

A linked git worktree shares the outer repo's `.git` (`git rev-parse --git-common-dir` →
`<main>/.git`). Claude Code anchors its workspace root to that common repo, so relative file
edits resolve against the outer checkout, not the worktree cwd. Nesting the worktree under the
main repo (`.claude/worktrees/`) makes it worse.

### Workaround (used to unblock)

Set the run's `project_dir` to the **repo root itself** on the target branch (check the branch
out at the main location), so there is no outer repo to stray into. Not real isolation.

### Fix ideas

1. drovr spawns phases in a **full clone** (independent `.git`), not a shared-`.git` worktree,
   for true isolation.
2. Inject an explicit absolute-project-root guard into the phase briefing/CLAUDE.md.
3. Investigate whether a non-nested (sibling) worktree + an explicit `--add-dir`/workspace-root
   hint to the spawned `claude` avoids the common-dir anchoring.
