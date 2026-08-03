# drovr

`drovr` is a CLI tool for managing multi-phase AI agent workflows. It
orchestrates a fixed sequence of phases (brainstorm → plan → implement →
review), routes each phase to a Claude agent pane via
[herdr](https://github.com/sauyon/herdr), has each finishing phase agent compress
its own work into a handoff doc, and runs an always-on local HTTP server (with a
browsable session list) for the human review loop.

Drovr leans on all three context-engineering levers
(anthropic.com/engineering/effective-context-engineering-for-ai-agents), not compaction alone:
**compaction** (the handoff docs), **note-taking / git** (phases re-read source from artifact
pointers rather than carrying a transcript forward), and **sub-agents** (read-only explorers
do fan-out investigation). Fresh, bounded contexts are what guard against Chroma's *context
rot* (trychroma.com/research/context-rot) — the output degradation that sets in as a window
fills.

## Prerequisites

- **herdr** — terminal-based AI agent session manager.
- **herdr claude integration** — install with `herdr integration install claude`.
  This hooks Claude Code's stop event so herdr can track when an agent is done.
- **claude** — Claude Code CLI (`claude`), on your `PATH`.
- **explore-mcp** (optional) — MCP server for file exploration; used by the
  brainstorm and plan phase prompts.

## Install / build

```
git clone <repo>
cd drovr/cli
cargo build
# binary: target/debug/drovr
```

Add `target/debug` to your `PATH` or copy the binary to a location on your
`PATH`.

## Configuration

Drovr loads `${XDG_CONFIG_HOME:-~/.config}/drovr/config.toml`. Built-in
definitions are provided for Claude, Cursor, and Codex; user definitions and
flags override them.

Automated review panels prefer Cursor's `agent` command when it is executable
on `PATH` and its herdr integration is installed, then fall back to the backend
that created the run. Pin a review backend independently when needed:

```toml
default_agent = "claude"
review_agent = "codex"
angles = ["correctness", "security", "error-handling", "type-design"]
# Default host `drovr serve` binds to when `--host` is omitted. Override it to
# reach the review UI from other devices — e.g. your Tailscale IP on a trusted
# tailnet (the server has no authentication).
serve_host = "127.0.0.1"
```

An explicit `review_agent` is honored without availability-based fallback, so
launch errors remain visible instead of silently selecting another backend.
Cursor reviewers default to `composer-2.5` to give Cursor callers an
independent model perspective. Override it in the agent map:

```toml
[agents.cursor]
command = "agent"
review_model = "gpt-5.6-terra-medium"
```

### Reflex

Two hooks, one `[reflex]` table.

The `session-start` hook injects the `drovr:using-drovr` router skill as the
always-on reflex for human-facing sessions (it no-ops inside a drovr-spawned
phase). The `user-prompt` hook injects a much smaller **per-turn gate card**
before every prompt, because a `SessionStart` injection scrolls out from under
the agent as the context fills and the discipline has to still be reachable at
turn 200. Both delegate to `drovr reflex`, so both are governed by the
`[reflex]` table — the SessionStart reflex is *shaped* by it, the gate is only
switched on and off. With no `[reflex]` table both are injected unchanged:

```toml
[reflex]
# Master switch. false suppresses both the SessionStart reflex and the
# per-turn gate.
enabled = true
# The per-turn gate card (UserPromptSubmit). Defaults to TRUE. Unlike the
# SessionStart reflex, it deliberately does NOT no-op inside a drovr-spawned
# phase — a phase is exactly where the discipline has to hold. It is skipped
# for one turn after the agent SUCCESSFULLY invokes a `drovr:*` skill, since a
# session already running the discipline does not need re-telling. A skill call
# that failed to load does not count, and still gets the card.
#
# Cost is cumulative, not a rate: the card is 547 bytes (budgeted at <=600) and
# each injection *stays* in the context window, so an unsuppressed 100-turn
# session carries ~55 KB by the end. The suppression rule is what keeps the
# common case to a handful of injections.
#
# This switch is GLOBAL, not per-project: config resolves to the single path
# ${XDG_CONFIG_HOME:-$HOME/.config}/drovr/config.toml, so `false` turns the gate
# off in every repo and `true` injects the card in every repo, drovr or not.
per_turn = true
# Optional: replace the framing text before the skill body inside the
# <EXTREMELY_IMPORTANT> wrapper. Absent → the built-in framing. Applies to the
# SessionStart reflex only; the gate card is a fixed const.
preamble = "You are running drovr. Apply the discipline below."

# Per-discipline toggles, keyed by the section names tagged in the router skill
# (skills/using-drovr/SKILL.md). A section omitted here stays enabled;
# set one to false to drop it from the injected reflex. SessionStart only.
[reflex.sections]
single-writer = true   # the single-writer / read-only-explorers principle
always-review = true   # the "always review before done" rule
methodology   = true   # routing to drovr:tdd / systematic-debugging / …
escalation    = true   # the phases / handoff escalation contract
```

## Commands

### Porcelain

| Command | Description |
|---|---|
| `drovr new <name> [--task <text>]` | Create a new run with 4 seeded phases. Requires the herdr claude integration. Isolates the run in a git worktree (`.drovr/wt/<run>` on branch `drovr/<run>`) **by default** — pass `--no-worktree` to edit the launch checkout in place, or set `worktree = false` in config. |
| `drovr list` | List all runs with phase progress and current phase. |
| `drovr status <name>` | Print each phase, its status, and the resume point. |
| `drovr attach <name>` | Attach to the current phase's agent pane. |
| `drovr resurrect <name>` | Reload a stopped run and print the resume point. |
| `drovr serve [--host H] [--port P]` | Start the always-on review server (default `127.0.0.1:8791`); serves **every** run plus a session-list landing page. Blocks until killed, and is auto-started on demand by `drovr review …`, so you rarely run it by hand. The server has no authentication; only bind a Tailscale host on a trusted tailnet. |
| `drovr cleanup <name> [--purge]` | Stop herdr sessions. With `--purge`, remove the run directory. |

### Review UI keyboard navigation

The review server's pages are drivable without a mouse, vim- or emacs-style. A
cursor moves over the session list on the landing page, and over the run's open
questions on a run page. Press <kbd>?</kbd> in the UI for the same table.

| Keys | Action |
|---|---|
| `j` `↓` `C-n`* · `k` `↑` `C-p` | next / previous row or question |
| `g` `M-<` · `G` `M->` | first / last |
| `C-v` `M-v` | page down / up |
| `Enter` `o` `l` `→` | open the session under the cursor |
| `1`–`9` | pick that option on the question under the cursor |
| `i` | type a custom answer on that question |
| `/` `C-s` | filter the session list |
| `h` `←` | back to the session list |
| `Esc` `C-g` | close the filter or help, or leave a text box |
| `?` | key help |

Keys never fire while you are typing in a text box — `Esc`/`C-g` steps out
first. \* `C-n` only reaches the page on macOS, where the browser's own modifier
is Cmd; Chrome and Firefox on Linux/Windows reserve `Ctrl+N` for a new window and
a page cannot intercept it. The bind is always registered — on Linux/Windows the
in-app help adds a footnote saying your browser will take it, and that footnote
is hidden on macOS where it does not apply.

### Plumbing

| Command | Description |
|---|---|
| `drovr phase start <run> <phase> [--seed <path>]` | Spawn a claude agent pane for the phase. |
| `drovr phase send <run> <phase> <text>` | Send text to a running phase pane. |
| `drovr phase wait <run> <phase> [--timeout-ms N]` | Poll until the phase agent is done (default 30 s). |
| `drovr phase done <run> <phase>` | Run by the phase agent as its final action; refuses until the agent has authored `<phase>-HANDOFF.md`, then drops the completion marker. |
| `drovr collect <run> <phase>` | Print the handoff doc for a finished phase. |
| `drovr review summary <run> <text>` | POST summary text to the always-on review server (auto-starting it if needed), flipping that run's state to `ready`. |
| `drovr review wait <run> [--timeout-ms N]` | Block until the reviewer acts, then exit (default 30 min). Exit 0 = approved, 3 = changes requested, 2 = timeout (re-run to resume), 1 = error. |
| `drovr reflex --skill <path>` | Render the SessionStart reflex JSON from `<path>`, shaped by `[reflex]` config. Run by the `session-start` hook; prints nothing when the reflex is disabled. |

## Run directory and state contracts

Each run lives in `$XDG_DATA_HOME/drovr/runs/<name>/` (defaults to
`~/.local/share/drovr/runs/<name>/`).

### `state.json`

Written on `drovr new`; updated by phase commands.

```json
{
  "name": "my-feature",
  "task": "implement OAuth login",
  "phases": [
    { "name": "brainstorm", "status": "Pending", "handoff_doc": null, "herdr_session": null, "pane_id": null },
    { "name": "plan",       "status": "Pending", "handoff_doc": null, "herdr_session": null, "pane_id": null },
    { "name": "implement",  "status": "Pending", "handoff_doc": null, "herdr_session": null, "pane_id": null },
    { "name": "review",     "status": "Pending", "handoff_doc": null, "herdr_session": null, "pane_id": null }
  ],
  "gate": "spec",
  "cursor": 0
}
```

Phase `status` values: `Pending`, `Running`, `Done`, `Failed`.

### `<phase>-HANDOFF.md`

Authored by the finishing phase agent itself, in-context, as its final action
(enforced by `drovr phase done`). A compressed summary of the phase's work
(objective + key decisions + artifacts, with git pointers) suitable for seeding
the next phase.

### Server discovery files

The always-on server writes two files in the drovr data dir (not per-run):

| File | Written by | Purpose |
|---|---|---|
| `server.addr` | `drovr serve` | Bound `host:port`; read by `drovr review summary`/`wait` and `ensure_server`. |
| `server.pid` | `drovr serve` | Daemon pid (liveness). |

### Per-run review files

The server reads and writes these files in each run dir:

| File | Written by | Purpose |
|---|---|---|
| `spec.md` | agent (brainstorm/spec gate) | The spec document shown in the browser UI. |
| `prior.md` | server on submit / per revision | Snapshot of the previous spec version for diffing. |
| `last_summarized.md` | server on POST summary | Rolling copy that re-baselines `prior.md` per revision. |
| `review.state.json` | server on state change | Durable `{state, turn}` — makes the server restart-safe. |
| `feedback.json` | server on submit | Human feedback JSON for the current turn. |
| `summary.txt` | server on POST summary | Agent summary text. |
| `questions.json` | agent | MC questions for the reviewer (optional). |
| `approved` | server on approve | Marker file written when the spec is approved. |

## Review loop flow

The server is always on (auto-started on demand by `drovr review …`). Open
`http://127.0.0.1:8791` to see the **session list**; every run appears with a
state badge. Click one to browse its spec, diffs, and code-review findings —
active gates are interactive, finished runs are browsable read-only. To keep it
supervised across logins/reboots, install the `systemd --user` unit at
`packaging/drovr.service` (`systemctl --user enable --now drovr`).

1. In a run's detail view, state starts as `idle`.
2. Read the spec, leave annotations, answer questions, and choose
   **Request changes** or **Approve**.
3. The driver posts a summary, then **waits** for the reviewer instead of
   busy-polling state:
   ```
   drovr review summary <name> "<what changed>"
   drovr review wait <name>   # blocks; run in the background
   ```
   `wait` blocks while state is `idle`/`ready` and exits when the reviewer acts.
   It is resumable: on timeout (exit 2) just re-run it — the on-disk markers are
   the source of truth, so no reviewer action is ever missed. The harness wakes
   the driver on the process exit; no hot poll loop is needed.
4. **Request changes** → server writes `feedback.json`, state becomes
   `waiting`, and `wait` exits **3**. The agent reads the feedback, edits the
   spec, calls `drovr review summary` (state → `ready`), and the driver runs
   `drovr review wait` again.
5. **Approve** → server writes the `approved` marker, state becomes `approved`,
   and `wait` exits **0**.

## Skills

The `skills/` directory holds three superpowers-style skills that DRIVE this CLI. They are
the intended interface for agents — the CLI is the mechanism, the skills are the discipline.

| Skill | Use when |
|---|---|
| `drovr:using-drovr` | Orientation: prerequisites, the single-writer rule, and choosing handoff vs pipeline. |
| `drovr:handoff` | Carry finished work across one phase boundary to a fresh agent (start → **inject seed** → wait → collect; the phase agent authors its own handoff before `phase done`). |
| `drovr:pipeline` | Run a whole change through brainstorm → plan → implement → review with a human spec gate. |

**The load-bearing contract:** `drovr phase start` spawns a plain `claude` and only records
the seed *path* — it does **not** inject the briefing. The skill injects it via
`drovr phase send`. At the spec gate, the agent must run `drovr review summary <run> "<text>"`
after **every** edit to `spec.md`, and the finishing phase agent authors exactly
`<phase>-HANDOFF.md` (the filename `drovr collect` reads) before `drovr phase done`.

## Running tests

```
cargo test          # all 59 tests (unit + integration + e2e)
cargo test --test e2e   # e2e smoke only
```

The e2e test requires `herdr`, `claude`, and the herdr claude integration hook.
It creates an isolated run in a temp directory and removes it on completion. If
prerequisites are absent it prints a skip message and exits cleanly.
