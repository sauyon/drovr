---
name: worktrees
description: Use when deciding whether to isolate a drovr run in its own git worktree, and how to hand its branch back — the discipline behind `drovr new --worktree`
---

# Worktrees

## Why

A drovr run edits `project_dir` across every phase. By default that is the
invoking checkout, so a run in flight blocks you from touching the repo and
leaves uncommitted changes strewn across it. A **git worktree** gives the run its
own checkout on its own branch, sharing the object store — the invoking checkout
stays clean and usable. This is the physical form of the single-writer rule:
**one run, one worktree, one writer.**

## When to isolate

Reach for `--worktree` when:

- You want to keep working in the main checkout while the run proceeds.
- The run is large or long enough that half-finished, uncommitted state in your
  real tree would be a hazard.
- Multiple runs may touch the same repo — each gets its own branch, no collisions.

Skip it for a quick inline change you will finish and commit yourself, or in a
non-git directory (it is a hard error there — isolation needs a repo).

## The flow

```
drovr new <run> --worktree        # .drovr/wt/<run> on branch drovr/<run>; project_dir points there
  ...phases run isolated in the worktree, never the outer checkout...
drovr cleanup <run>               # commits the run's work, prunes the worktree, KEEPS the branch
```

- Set a standing default with `worktree = true` in config; `--no-worktree`
  overrides per-run.
- `.drovr/wt/` is ignored via `.git/info/exclude` — the tracked `.gitignore` is
  never touched.

## Handing the branch back — drovr never merges

Cleanup leaves you a reviewable branch `drovr/<run>` and prints the merge command.
**You** drive the merge — drovr does not act on your shared branch. Review it like
any other branch, then `git merge drovr/<run>` (or open a PR).

`drovr cleanup <run> --purge` force-removes the worktree, deletes the branch, and
drops the run dir — use it to discard a run whose work you don't want.

## Bind checklists to tracked task state

Isolating a run and handing its branch back are both multi-step flows.

> When a skill or briefing gives you a numbered checklist, create **one tracked item per step**
> using whatever task tool this harness exposes — `TodoWrite`, or `TaskCreate`/`TaskUpdate` —
> before you start step 1. Mark each in-progress when you start it and complete when its
> evidence is in hand. If the harness exposes no task tool, write the checklist to
> `~/.local/share/drovr/runs/<run>/checklist.md` when inside a run, or `CHECKLIST.md` at the
> repo root otherwise, and tick items there. An untracked checklist decays with the context
> window; that decay is the exact failure drovr exists to fight.

## Discipline

- **One writer per worktree.** Fan-out investigation still goes to read-only
  explorers, never parallel writers — the worktree isolates the run from *other
  checkouts*, not the single-writer rule from itself.
- **Don't hand-edit the worktree from outside the run** while a phase is live; the
  phase agent is that worktree's writer.
- **Uncommitted work is safe:** cleanup commits before pruning, and git refuses to
  remove a dirty worktree without `--purge` — it will never silently discard work.
