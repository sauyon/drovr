# relay

`relay` is a CLI tool for managing multi-phase AI agent workflows. It
orchestrates a fixed sequence of phases (brainstorm → plan → implement →
review), routes each phase to a Claude agent pane via
[herdr](https://github.com/sauyon/herdr), compresses finished phases into
handoff docs, and runs a local HTTP server for the human review loop.

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
cd relay/cli
cargo build
# binary: target/debug/relay
```

Add `target/debug` to your `PATH` or copy the binary to a location on your
`PATH`.

## Commands

### Porcelain

| Command | Description |
|---|---|
| `relay new <name> [--task <text>]` | Create a new run with 4 seeded phases. Requires the herdr claude integration. |
| `relay list` | List all runs with phase progress and current phase. |
| `relay status <name>` | Print each phase, its status, and the resume point. |
| `relay attach <name>` | Attach to the current phase's agent pane. |
| `relay resurrect <name>` | Reload a stopped run and print the resume point. |
| `relay serve <name> [--host H] [--port P]` | Start the review HTTP server (default `127.0.0.1:8791`). Blocks until killed. The server has no authentication; only bind a Tailscale host on a trusted tailnet. |
| `relay cleanup <name> [--purge]` | Stop herdr sessions. With `--purge`, remove the run directory. |

### Plumbing

| Command | Description |
|---|---|
| `relay phase start <run> <phase> [--seed <path>]` | Spawn a claude agent pane for the phase. |
| `relay phase send <run> <phase> <text>` | Send text to a running phase pane. |
| `relay phase wait <run> <phase> [--timeout-ms N]` | Poll until the phase agent is done (default 30 s). |
| `relay phase compress <run> <phase>` | Read the phase transcript and write `<phase>-HANDOFF.md` via `claude -p`. |
| `relay collect <run> <phase>` | Print the handoff doc for a finished phase. |
| `relay review summary <run> <text>` | POST summary text to the running review server, flipping state to `ready`. |

## Run directory and state contracts

Each run lives in `$XDG_DATA_HOME/relay/runs/<name>/` (defaults to
`~/.local/share/relay/runs/<name>/`).

### `state.json`

Written on `relay new`; updated by phase commands.

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

Written by `relay phase compress`. A compressed summary of the phase's agent
transcript (objective + key decisions + artifacts) suitable for seeding the
next phase.

### Review server files

The review server (`relay serve`) reads and writes these files in the run dir:

| File | Written by | Purpose |
|---|---|---|
| `review.addr` | `relay serve` | Bound `host:port`; read by `relay review summary`. |
| `spec.md` | agent (implement phase) | The spec document shown in the browser UI. |
| `prior.md` | server on each submit | Snapshot of the previous spec version for diffing. |
| `feedback.json` | server on submit | Human feedback JSON for the current turn. |
| `summary.txt` | server on POST `/summary` | Agent summary text. |
| `questions.json` | agent | MC questions for the reviewer (optional). |
| `approved` | server on approve | Marker file written when the spec is approved. |

## Review loop flow

```
relay serve <name>
```

1. Open `http://127.0.0.1:8791` in a browser. State starts as `idle`.
2. Read the spec, leave annotations, answer questions, and choose
   **Request changes** or **Approve**.
3. **Request changes** → server writes `feedback.json`, state becomes
   `waiting`. The agent reads the feedback, edits the spec, and then calls:
   ```
   relay review summary <name> "<what changed>"
   ```
   State becomes `ready`; refresh the browser to see the new spec.
4. Repeat until you choose **Approve** → state becomes `approved`.

## Skills

The `skills/` directory holds three superpowers-style skills that DRIVE this CLI. They are
the intended interface for agents — the CLI is the mechanism, the skills are the discipline.

| Skill | Use when |
|---|---|
| `relay:using-relay` | Orientation: prerequisites, the single-writer rule, and choosing handoff vs pipeline. |
| `relay:handoff` | Carry finished work across one phase boundary to a fresh agent (start → **inject seed** → wait → compress → collect). |
| `relay:pipeline` | Run a whole change through brainstorm → plan → implement → review with a human spec gate. |

**The load-bearing contract:** `relay phase start` spawns a plain `claude` and only records
the seed *path* — it does **not** inject the briefing. The skill injects it via
`relay phase send`. At the spec gate, the agent must run `relay review summary <run> "<text>"`
after **every** edit to `spec.md`, and `relay phase compress` writes exactly
`<phase>-HANDOFF.md` (the filename `relay collect` reads).

## Running tests

```
cargo test          # all 59 tests (unit + integration + e2e)
cargo test --test e2e   # e2e smoke only
```

The e2e test requires `herdr`, `claude`, and the herdr claude integration hook.
It creates an isolated run in a temp directory and removes it on completion. If
prerequisites are absent it prints a skip message and exits cleanly.
