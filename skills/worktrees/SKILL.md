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
stays clean and usable *for other work*. This is the physical form of the
single-writer rule: **one run, one worktree, one writer.**

That does **not** mean the driver stays behind in the invoking checkout. The
worktree is where the run happens, and the driver goes there too — see "The
flow" below.

## When to isolate

Reach for `--worktree` when:

- You want the main checkout left free for other work while the run proceeds.
  (Free for *other* work — the driver of the run itself moves into the worktree.)
- The run is large or long enough that half-finished, uncommitted state in your
  real tree would be a hazard.
- Multiple runs may touch the same repo — each gets its own branch, no collisions.

Skip it for a quick inline change you will finish and commit yourself, or in a
non-git directory (it is a hard error there — isolation needs a repo).

## The flow

```
drovr new <run> --worktree        # .drovr/wt/<run> on branch drovr/<run>; project_dir points there
EnterWorktree({path: ".drovr/wt/<run>"})   # ← YOU move too. Do this NEXT, before anything else.
  ...phases run isolated in the worktree, never the outer checkout...
drovr cleanup <run>               # commits the run's work, prunes the worktree, KEEPS the branch
```

- Set a standing default with `worktree = true` in config; `--no-worktree`
  overrides per-run.
- `.drovr/wt/` is ignored via `.git/info/exclude` — the tracked `.gitignore` is
  never touched.

### ⚠️ The `EnterWorktree` line is not optional — drovr cannot move you

`drovr new --worktree` **prints** the worktree path; it does not put you in it.
Nothing in drovr ever changes your directory, and nothing can: a subprocess
cannot change its parent's working directory. Moving is your job, and it is the
step drivers skip.

Skip it and you are still in the invoking checkout, so every bare `git status`,
`git log` and `git diff` you run answers about **main, not your run's branch** —
including other agents' uncommitted files, which you will read as your own. On a
repo with several runs in flight that is how one agent clobbers another.

`cd` will not do it: the Bash tool's cwd resets to the session's primary
directory after every call. Use the harness tool:

```
EnterWorktree({path: ".drovr/wt/<run>"})   # switches the SESSION's dir; persists
ExitWorktree({action: ...})                # when the run is done
```

Then stay there. Do not operate from the main checkout for the rest of the run.

## Handing the branch back — drovr never merges

Cleanup leaves you a reviewable branch `drovr/<run>` and prints the merge command.
**You** drive the merge — drovr does not act on your shared branch. Review it like
any other branch, then `git merge drovr/<run>` (or open a PR).

`ExitWorktree` **first**, then `drovr cleanup`, then merge — in that order. The
merge belongs in the checkout you are merging *into*, and cleanup prunes the
worktree out from under you if you are still standing in it.

`drovr cleanup <run> --purge` force-removes the worktree, deletes the branch, and
drops the run dir — use it to discard a run whose work you don't want.

## Discipline

- **Enter the worktree, and work only from there.** See "The `EnterWorktree`
  line is not optional" above. A driver still sitting in the invoking checkout
  is reading the wrong tree, silently, for the whole run.
- **One writer per worktree.** Fan-out investigation still goes to read-only
  explorers, never parallel writers — the worktree isolates the run from *other
  checkouts*, not the single-writer rule from itself.
- **The worktree belongs to the run's PHASE AGENTS. The driver drives and reads.**
  Do not hand-edit files there while a phase is live — not docs, not a `git
  merge`, not "just one fix". The phase agent is that worktree's writer, and a
  review panel may be reading those exact files: a merge started under a running
  panel puts conflict markers in front of the reviewers, and they will review
  them. If the driver must change something, wait for the phase to finish, or
  send the work to the phase agent.
- **Uncommitted work is safe:** cleanup commits before pruning, and git refuses to
  remove a dirty worktree without `--purge` — it will never silently discard work.
