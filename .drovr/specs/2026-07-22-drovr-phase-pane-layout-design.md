# Spec: phase panes stop opening as a split against an empty root shell

**Run:** fix-split-pane · **Status:** Draft — awaiting review

## Problem

`drovr new` calls `herdr workspace create`, which auto-creates a **root shell
pane** in tab `t1`. Every `phase_start` then runs `herdr agent start
--workspace <ws>` with **no `--tab`** (`cli/src/herdr.rs:184`), so herdr drops the
phase's `claude` pane into the active tab — **splitting it against that empty root
shell**. The user sees each phase as a split pane sitting next to a useless new
shell session. Confirmed live on the leftover run `w16`: tab `t1` holds `p1`
(empty shell, no agent) split against `p2` (the `claude` "plan" agent).

Two facts constrain the fix (both verified against live herdr):

1. A freshly created **tab also auto-spawns its own shell pane** — so "one tab per
   phase + `agent start --tab`" only relocates the same split into every tab.
2. `herdr agent send` / `agent read` operate purely by `pane_id` and work on a
   pane whose `claude` was launched via `herdr pane run` — they do **not** require
   an `agent start` registration.

## Approach (A: reuse the tab's shell pane)

Run `claude` **inside** an existing shell pane instead of splitting a new pane
beside it. Result per run: phase 1 uses the workspace's root pane; phases 2..N
each get their own tab and reuse that tab's shell pane. Zero splits, zero orphan
shells, zero mid-run pane closes (respects the existing "never close panes
mid-run" policy at `cli/src/herdr.rs:11`).

### Focus safety — first-class requirement

`herdr pane run` and `pane rename` have **no `--no-focus` flag**, and the focus
behavior of the pane/agent I/O commands is inconsistent. The fix MUST NOT move the
user's focus. Mechanism: **capture the focused workspace id before any pane
operation and restore it afterward** (`herdr workspace focus <prev>`), unless each
operation is proven focus-neutral during implementation. `workspace create` and
`tab create` continue to pass `--no-focus`.

## Interfaces / contracts

**`cli/src/herdr.rs` — `Herdr` trait:**

- `workspace_create(label, cwd, env) -> Workspace { id, root_pane }`
  Now returns the root pane id (parsed from `result.root_pane.pane_id`) and passes
  `--cwd` + auth `--env` at create time. Extract the existing auth-var logic in
  `build_agent_start_args` into an `auth_env_flags()` helper shared by workspace
  and tab creation.
- `tab_create(workspace, label, cwd) -> String` *(new)* — creates a `--no-focus`
  tab with `--env`; returns the tab's auto shell pane id.
- `pane_run(pane_id, command)` *(new)* — `herdr pane run <pane_id> <command>`.
- `pane_rename(pane_id, label)` *(new)* — cosmetic phase label on the pane.
- `focused_workspace() -> Option<String>` and `workspace_focus(id)` *(new)* —
  capture/restore focus around pane operations.
- **Removed:** `agent_start` (superseded) and the already-dead `session_stop`.
- **Unchanged:** `agent_send`, `agent_read`, `workspace_close`,
  `integration_present`. The proven `agent_send` CR / `PASTE_SETTLE` submit
  timing is untouched.

**`cli/src/run.rs` — `RunState`:**

- New field `root_pane: Option<String>` (`#[serde(default)]`), set by `drovr new`,
  consumed (taken) by the first `phase_start`.

**`cli/src/main.rs` — `drovr new`:**

- `workspace_create` now supplies `project_dir` as cwd and records both
  `workspace` and `root_pane` in `RunState`.

**`cli/src/phase.rs` — `phase_start`:**

- Capture focused workspace. Pick the target pane: `run.root_pane.take()` for the
  first phase, else `tab_create(ws, phase, project_dir)`. Then `pane_run(target,
  "claude")`, `pane_rename(target, phase)`, record `pane_id`, restore focus. If the
  phase already has a `pane_id` (restart/resume), reuse it rather than creating a
  new tab. Never closes a pane.

## Scope boundaries

**In scope:** the pane-layout change, focus preservation, and their tests.

**Out of scope (YAGNI):**
- No change to the review-server protocol, `agent_send` timing, or the handoff /
  compress / wait contracts.
- No mid-run pane closing; no parallel writers.
- No change to `drovr cleanup` (single `workspace_close` still tears everything
  down).
- Retrofitting old runs missing `root_pane` — they degrade gracefully (first phase
  falls back to `tab_create`, leaving the pre-existing empty root shell; acceptable
  for pre-fix runs).

## Testing strategy (TDD)

- `FakeHerdr` gains `tab_create` / `pane_run` / `pane_rename` / `focused_workspace`
  / `workspace_focus` recording and returns a `root_pane` from `workspace_create`.
- `phase_start` tests assert: first phase calls `pane_run` on the recorded
  `root_pane` (no `tab_create`); a subsequent phase calls `tab_create` then
  `pane_run` on the new tab's pane; **no** `agent_start` / `pane_close` ever; focus
  captured-then-restored around the operations; `pane_run` command is exactly
  `claude` (no inlined secrets).
- `auth_env_flags()` unit tests (moved from `build_agent_start_args`): included
  when set, omitted when unset, secrets never inlined into the `pane run` command.
- Parsing: `workspace_create` extracts `root_pane.pane_id`; `tab_create` extracts
  the new tab's pane id.
- The existing 59 tests must stay green (minus the removed `agent_start` tests,
  which are replaced by the `pane_run` equivalents).

## Open questions

1. Is capture/restore focus sufficient, or is there a race if the user navigates
   during `phase_start`? Accept the small race for now; the operation is brief.
2. Should `tab_create` label the tab with the phase name for readability? Proposed:
   yes (`--label <phase>` + `pane_rename`).
