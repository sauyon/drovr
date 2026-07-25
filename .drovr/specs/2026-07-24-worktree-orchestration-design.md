# Worktree orchestration — design

**Status:** APPROVED — decisions locked 2026-07-24
**Date:** 2026-07-24
**Goal:** Bake real git-worktree isolation into a drovr run, so a run's phases
edit an isolated checkout on their own branch and never touch the invoking
checkout. Closes the unbuilt Phase-2 stretch item `drovr:worktrees` — but as CLI
orchestration, not just a discipline skill.

## Why

Drovr's isolation model today is *single-writer + clean-context*: one agent edits
at a time, against `project_dir` (the invoking cwd or `--dir`). But every phase of
a run — brainstorm, plan, implement, review — and the human's own inline work all
edit the **same working tree**. A run in flight blocks the human from touching the
repo, and a half-finished run leaves uncommitted changes strewn across the real
checkout.

Git worktrees give each run a physically separate checkout on its own branch,
sharing the object store. The invoking checkout stays clean and usable. The
plumbing already anticipates this: `config.rs:294` injects a workspace-root guard
telling each phase agent to treat its `project_dir` as the absolute root and
**ignore an outer checkout if this is a worktree**. This design makes drovr
actually create that worktree.

## What already keys off `project_dir` (why this is mostly plumbing)

A single value threads the whole system. Point it at a worktree and the rest
follows:

- `cmd_new` (`main.rs:305`) resolves `project_dir`, then roots the herdr
  workspace there (`workspace_create`, `main.rs:327`) — all phase panes `cd` into it.
- `config.rs:284` passes `project_dir` as the agent's `--workspace` / adds the
  workspace-root guard system prompt.
- `head_sha(&state.project_dir)` (`main.rs:830`) and code-review's `base_sha`
  (`code_review.rs:121`) resolve git state **inside** `project_dir`.

So the core change is: **`drovr new --worktree` creates a worktree and sets
`project_dir` to it.** Downstream code is largely untouched.

## The load-bearing dependency: phases don't commit

Confirmed by `code_review.rs:158` — reviewers see `git diff {base}..{head}`
**plus the current working tree**. Drovr never commits; all phase output lives as
uncommitted working-tree changes against a base SHA.

Consequence for worktrees: without commits, a run's worktree holds only
uncommitted changes. That breaks two things:

1. **`git worktree remove` refuses a dirty tree** (needs `--force`, which discards
   the work).
2. The user's sketch of "merge branch" has nothing to merge — a branch with zero
   commits carries no work back.

**This design therefore requires establishing commit discipline** (Decision 4).
This is the one part that is *not* just plumbing.

## Design

### D1. Opt-in, with a config default *(recommended)*

`drovr new` runs today in any directory, git repo or not. Default-on worktrees
would break non-git and dirty-repo flows. So:

- New flag: `drovr new <run> --worktree` (short `--wt`).
- Config default: `worktree = true|false` in drovr config, overridable per-run by
  the flag.
- If `--worktree` is set but `project_dir` is **not** a git repo → hard error with
  a clear message (do not silently fall back — the user asked for isolation).

### D2. Worktree location — `.drovr/wt/<run>` inside the repo *(per your sketch)*

```
<repo>/.drovr/wt/<run>        # the worktree checkout
```

- Co-located and discoverable; `git worktree list` shows it.
- **Requires** adding `.drovr/wt/` to the repo's `.gitignore` (or drovr writes a
  local exclude via `.git/info/exclude` to avoid mutating the tracked `.gitignore`
  — see Decision 2).

*Alternative considered:* under the run dir
(`~/.local/share/drovr/runs/<run>/worktree`) — zero repo pollution, symmetric with
run state, no gitignore needed, but hidden and less discoverable. **Flagged for
reviewer (Decision 2).**

### D3. Branch `drovr/<run>` off the invoking HEAD

- `git worktree add .drovr/wt/<run> -b drovr/<run>` from the invoking checkout's
  current HEAD.
- Dirty invoking tree is fine: `worktree add` checks out HEAD clean; the human's
  uncommitted changes stay in the main checkout, untouched.
- Branch collision (`drovr/<run>` exists) → hard error naming the branch; the run
  name is already required-unique, so this only happens on leftover state.

### D4. Commit discipline — required, smallest viable form

For the branch to carry work, phase output must be committed. Options, smallest
first:

- **(4a) Final squash commit on compress *(recommended)*.** Leave the
  intra-phase flow exactly as it is (working-tree diffs, uncommitted). When a run
  reaches a completion boundary, drovr runs `git add -A && git commit` in the
  worktree with a generated message. One commit per run keeps code-review's
  working-tree diff model **completely unchanged mid-run**.
- (4b) Commit per implement task. Richer history, but changes the code-review base
  model (base becomes last commit, not run start) — more invasive.
- (4c) Require phase agents to commit (phase-prompt change). Pushes policy into
  prompts; least deterministic.

Recommend **4a**. It is the least-invasive way to make the branch real. **Flagged
(Decision 4).**

### D5. Completion & cleanup — prune worktree, keep branch, DO NOT auto-merge

Your sketch said "merge branch." I recommend **against** drovr performing the
merge:

- Auto-merge is an outward-facing, hard-to-reverse action into the human's real
  branch, and can conflict. Per the working discipline (escalate external side
  effects; never act on the shared branch unprompted), drovr should **stop at a
  reviewable branch**, not merge it.
- Instead, on `drovr cleanup <run>`:
  1. `workspace_close` (existing — kills panes).
  2. If dirty and uncommitted and not `--purge` → **refuse**, tell the user (don't
     silently `--force` away work).
  3. `git worktree remove` (force only under `--purge`).
  4. **Keep the branch** `drovr/<run>`. Print it plus a suggested
     `git merge drovr/<run>` / PR command so the human drives the merge.
- `--purge` additionally deletes the branch and the run dir (current `--purge`
  behavior, extended).

**Flagged (Decision 1):** auto-merge vs. hand-back-a-branch. I recommend hand-back.

### D6. State — two new `RunState` fields

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub worktree_path: Option<String>,   // .drovr/wt/<run>, absolute
#[serde(default, skip_serializing_if = "Option::is_none")]
pub worktree_branch: Option<String>, // drovr/<run>
```

`#[serde(default)]` → every existing run loads unchanged (both `None` → behaves
exactly as today, no worktree). Cleanup prunes only when `worktree_path.is_some()`.

## Edge cases

| Case | Behavior |
|---|---|
| `--worktree` in a non-git dir | Hard error, no fallback. |
| `project_dir` is itself a worktree | `worktree add` still works (shared `.git`); allowed. |
| Branch `drovr/<run>` exists | Hard error naming it. |
| Dirty invoking tree | Fine — HEAD checked out clean; human's changes untouched. |
| Worktree dirty at cleanup (4a failed to commit) | Refuse remove unless `--purge`; never silently discard. |
| Old run (`worktree_path: None`) | No worktree logic runs — identical to today. |

## Scope / non-goals

- **Not** changing the intra-phase working-tree diff model (Decision 4a preserves it).
- **Not** auto-merging or auto-PRing (Decision 5).
- **Not** default-on (Decision 1 — opt-in + config).
- The discipline skill `drovr:worktrees` (documenting this for the human) can be a
  thin follow-up once the CLI exists; this spec is the CLI.

## Rough plan (post-gate)

1. `RunState` fields + serde defaults; unit test old-state load. (~S)
2. Worktree create in `cmd_new` behind `--worktree`/config; gitignore/exclude
   handling; error paths. (~M)
3. Final-commit-on-compress (4a) in the compress/completion path. (~M)
4. Cleanup: prune worktree, keep branch, `--purge` deletes branch; dirty-refusal. (~M)
5. Config flag plumbing + `drovr:code-review` panel unaffected (verify base still
   resolves in the worktree). (~S)
6. Tests: e2e worktree lifecycle (create → phase edits isolated → commit → prune),
   non-git error, dirty-cleanup refusal. (~M)

Estimate: ~2–3 focused days including tests.

## Implementation & review outcome (2026-07-24)

Implemented test-first across 5 tasks; new module `cli/src/worktree.rs` holds the
git helpers. Two independent read-only reviewers (git-correctness + integration)
ran adversarial passes. Findings triaged with impact-scaled judgement:

- **Fixed:** `--purge` is now robust to a vanished/corrupt worktree (warns and
  still removes the run dir instead of wedging); a missing worktree is tolerated;
  `create()` canonicalizes the repo up front (no post-add strand), pre-checks a
  leftover worktree dir with a clear message, and only writes `.git/info/exclude`
  after a successful `worktree add` (a failed create leaves zero trace).
- **Not actioned (verified):** phase-op `project_dir` existence checks (pre-existing
  for all runs, not introduced here); branch/path invariant (holds by construction
  in `create`); detached HEAD (works correctly); unborn HEAD (rare; git error is
  surfaced with context). The canonicalize-orphan finding was walked back by the
  reviewer itself as a near-impossible FS race.

Tests: 195 unit + e2e lifecycle (keep-branch, `--purge`, vanished-worktree). All green.

## Locked decisions (approved 2026-07-24)

1. **Merge policy — hand back branch.** Cleanup prunes the worktree, keeps
   `drovr/<run>`, prints a suggested merge/PR command. Drovr never merges.
2. **Location — `.drovr/wt/<run>` in-repo.** Ignored via `.git/info/exclude`
   (drovr appends `.drovr/wt/` to the local exclude on first worktree create; the
   tracked `.gitignore` is never mutated).
3. **Default — opt-in `--worktree` + config `worktree = true`.** Off by default;
   config makes it a standing default; the flag overrides per-run.
4. **Commit discipline — final commit on compress (4a).** Intra-run flow stays
   working-tree-based (code-review unchanged); one squash commit at the completion
   boundary.
