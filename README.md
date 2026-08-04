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

### Pane reaping

```toml
# Close a phase's herdr pane once the run has provably moved past it. ON by default.
reap_finished_panes = true
```

Without this, every phase's pane lived until `drovr cleanup` and a long run
accumulated a tab per phase plus one per reviewer.

**Reaping is triggered by supersession, never by completion.** A pane outlives
`drovr phase done` — the implement↔review loop re-enters the same pane with
`drovr phase send` and no `phase start`, and reaping on completion would kill
that loop on its first iteration. The three triggers are the moments a run has
provably moved past a phase:

| Trigger | Reaps |
|---|---|
| `drovr phase start <run> <phase>`, after its own launch succeeds | every **other** `Done` phase's pane (never a reviewer's — those belong to a panel that may still be in flight) |
| `drovr code-review run`, after the findings are merged | the panel's own reviewer panes |
| `drovr phase reap <run> <phase>` | the phase you named |

Reaping closes a **pane**, not a tab: a phase's tab may also hold a pane the
human split into it, and drovr closes only what it can prove is its own. In the
ordinary case — drovr's pane alone in its tab — the tab goes with it.

`reap_finished_panes = false` restores the pre-reaping behaviour: every pane
stays until `drovr cleanup`, and nothing else changes. It turns off the two
**automatic** triggers (and the retired-pane sweep at those triggers). It does
**not** turn off `drovr phase reap`, which is a command rather than a policy and
stays available either way — including its sweep.

Reaping a phase does not change its status; it says something about the pane,
not about whether the work finished. Bring it back with `drovr phase rehydrate`.

### Resuming an agent's session

`drovr phase rehydrate` relaunches a phase and asks the backend to resume the
agent's recorded session, so the conversation comes back rather than a fresh
agent reading the notes. How a backend spells that is per-agent, and the two
shapes are mutually exclusive:

```toml
[agents.claude]
command = "claude"
resume_flag = "--resume"          # <command> … --resume '<id>'   (where other flags go)

[agents.codex]
command = "codex"
resume_subcommand = "resume"      # <command> resume '<id>' …     (immediately after)
```

Built-ins: `claude` and `cursor` get `resume_flag = "--resume"`. **codex gets
neither** — `codex resume <id>` is the expected shape but its argument ordering
was never verified, and an unverified guess composes a wrong command line where
absence merely reseeds. Opt in explicitly as above. An empty value is rejected
at config load: a bare `--resume` opens claude's interactive session picker and
parks the pane forever.

A backend with no resume surface is not an error — a rehydrate of a phase that
ran under it degrades to a **reseed** (fresh agent, seed re-sent).

### Reflex

The `session-start` hook injects the `drovr:using-drovr` router skill as the
always-on reflex for human-facing sessions (it no-ops inside a drovr-spawned
phase). The hook delegates rendering to `drovr reflex`, so the reflex is shaped
by the `[reflex]` table — with no `[reflex]` table the built-in reflex is
injected unchanged:

```toml
[reflex]
# Master switch. false suppresses the reflex entirely for human sessions.
enabled = true
# Optional: replace the framing text before the skill body inside the
# <EXTREMELY_IMPORTANT> wrapper. Absent → the built-in framing.
preamble = "You are running drovr. Apply the discipline below."

# Per-discipline toggles, keyed by the section names tagged in the router skill
# (skills/using-drovr/SKILL.md). A section omitted here stays enabled;
# set one to false to drop it from the injected reflex.
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
| `drovr serve [--host H] [--port P]` | Start the always-on review server (default `127.0.0.1:8791`); serves **every** run plus a session-list landing page. Blocks until killed, and is auto-started on demand by `drovr review …`, so you rarely run it by hand. Exactly one server may serve a data dir: while one holds the `server.pid` lock, this exits 1 and points at it (rather than starting a second server and stealing `server.addr` from it). The server has no authentication; only bind a Tailscale host on a trusted tailnet. |
| `drovr cleanup <name> [--purge]` | Close the panes drovr opened for the run (phase panes, reviewer panes, the workspace root pane) and prune its worktree. Panes you opened yourself in the run's workspace are left alone, and the workspace only closes when nothing but drovr's panes were in it. With `--purge`, also remove the run directory and delete the branch. |

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
| `drovr phase done <run> <phase>` | Run by the phase agent as its final action; refuses until the agent has authored `<phase>-HANDOFF.md`, then drops the completion marker. Must run from inside the phase's own pane — the marker is stamped with the `$DROVR_PASS` token that pane was launched under. From anywhere else it refuses and prints the command that would work. |
| `drovr phase rehydrate <run> <phase>` | Bring back a phase whose pane is gone, resuming its recorded agent session where the backend offers one. Exit 0 = the pane is back **and** the agent has this phase's context; 2 = the pane is back but the agent was **not** given its context (act on it, never read it as success); 1 = refused or failed. |
| `drovr phase reap <run> <phase>` | Close a phase's pane and release the phase from it, on demand. Exit 0 = the pane is gone and the phase no longer records it (including when there was nothing to reap, so re-running is safe); 2 = the pane is still there and the phase still holds it — herdr would not close it, or could not be reached; 1 = refused or failed. |
| `drovr collect <run> <phase>` | Print the handoff doc for a finished phase. |
| `drovr review summary <run> <text>` | POST summary text to the always-on review server (auto-starting it if needed), flipping that run's state to `ready`. |
| `drovr review wait <run> [--timeout-ms N]` | Block until the reviewer acts, then exit (default 30 min). Exit 0 = approved, 3 = changes requested, 2 = timeout (re-run to resume), 1 = error. |
| `drovr reflex --skill <path>` | Render the SessionStart reflex JSON from `<path>`, shaped by `[reflex]` config. Run by the `session-start` hook; prints nothing when the reflex is disabled. |

### Reaping and rehydrating a pane

`drovr phase reap` is the manual form of the automatic reaping above, and it is
also the supported way to clear a phase that records a pane herdr no longer has
— the state that otherwise makes `phase rehydrate` refuse with "still holds
pane" forever, with nothing able to clear it.

It additionally **sweeps the run's retired panes**: panes drovr opened that no
phase points at any more, which a reviewer replaced mid-panel leaves behind.
They belong to no phase, so this is the only command that reaches them short of
`drovr cleanup`. The sweep is best-effort and reports itself; **it does not
affect the exit code**, which is about the phase you named. `drovr phase reap`
runs it regardless of `reap_finished_panes` — the config gates the two automatic
triggers, not the command.

`drovr phase rehydrate` is the way back. It opens a fresh tab in the run's
project directory, under the profile the phase originally ran with, and asks the
backend to resume the recorded session:

- **`Resumed`** means the *session came back* — herdr reported that session id
  on the new pane. It is not a claim that an agent started working; it is the
  claim the ⟳ button makes, and it is only made on positive evidence.
- **`Reseeded`** means no session was recoverable, so a fresh agent was launched
  and the phase's seed re-sent. The artifacts come back; the conversation does
  not.
- **`Incomplete`** (exit 2) is the only outcome that is not a success: the pane
  is back, but the agent in it was not confirmed to have this phase's context.
  It names which of five things went wrong — the agent never reported ready; the
  resume came up carrying a *different* session (the pane is surrendered, since
  a different session is positive evidence the record does not describe it);
  herdr never reported *any* session id (the pane is kept — nothing seen is not
  evidence); a fresh agent is up and the phase has no seed to give it; or the
  seed could not be delivered.

It **refuses a reviewer phase**, and that refusal is the design: a reviewer
delivers its findings through drovr's MCP findings server, which is handed over
on the command line at launch and cannot be re-attached to a resumed session. A
resumed reviewer would have no `submit_findings` tool, so it could never
deliver. A panel is **re-run, not rehydrated** — `drovr code-review run` resumes
a panel in flight, so it costs only the angles actually lost.

In `drovr serve`, a reaped phase renders dimmed in the agent tree with a **⟳**
button. The button appears exactly where a click will work (it is gated on the
same predicate the CLI refuses on), and its tooltip says which of the two things
it promises: resume this phase's session, or launch a fresh agent and re-send
the seed.

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

Phases carry more than the four keys above once they have run — notably `pass`
(the token identifying the current pass over the phase, exported to its agent as
`$DROVR_PASS` and stamped into `<phase>.done`) and `reaped` (drovr closed this
phase's pane; the status is untouched, so this says nothing about whether the
work finished).

Two run-level keys are worth knowing about:

| Key | Meaning |
|---|---|
| `review_phases` | Reviewer phases (`review:<task>:<iter>:<angle>`), kept out of `phases` so they never pollute pipeline progress. |
| `retired_panes` | Panes drovr opened that no longer belong to any phase — a reviewer replaced mid-panel leaves one. `drovr cleanup` closes exactly the panes drovr can prove are its own, and this is how a pane stays provably drovr's after its phase lets go of it. **The list shrinks as well as grows:** the retired-pane sweep drops an entry once the pane behind it is provably gone. It is not an append-only audit log — an entry that outlives its pane proves nothing, and herdr reissues pane ids. |

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
| `server.pid` | `drovr serve` | Daemon pid, and the file the single-server lock is taken on: the running server holds an exclusive lock on it, so a second `drovr serve` refuses (on any port) instead of stealing `server.addr`. The kernel releases the lock however the server exits, so a crashed server never wedges the next start — the pid inside is only for humans (`kill $(cat server.pid)`). Nothing else is checked: a server holding no lock (a build predating it, or one whose lock file was deleted) is not detected. |

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

### Other per-run files

| File | Written by | Purpose |
|---|---|---|
| `<phase>.done` | `drovr phase done` | Completion marker, stamped with the pass token of the agent that wrote it. `drovr phase wait` accepts it only if that token matches the phase's current `pass`, which is what stops a previous pass's still-live agent from completing the current one. |
| `<phase>-HANDOFF.md` | the finishing phase agent | See above; `drovr collect` reads it. |
| `run.lock` | `drovr phase rehydrate` / `reap` | The exclusive lock serializing the commands that move a run's panes around. Rehydrate and reap are the same read-modify-write over `pane_id` in opposite directions, so they share one lock — reaping a phase a rehydrate is bringing back would end with a live pane nothing records. Advisory and kernel-held, so a crashed holder leaves nothing stale; a second command **fails rather than queues**, since the holder may be inside a 30 s readiness wait. (Named `rehydrate.lock` while rehydrate was its only holder.) |

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
| `drovr:handoff` | Carry finished work across one phase boundary to a fresh agent (start **briefed** → wait → collect; the phase agent authors its own handoff before `phase done`). |
| `drovr:pipeline` | Run a whole change through brainstorm → plan → implement → review with a human spec gate. |

**The load-bearing contract:** drovr composes every brief and injects it —
`drovr phase start <run> <phase> --context …` for a phase, `drovr code-review run/brief
… --context …` for a reviewer. You supply only the context; `drovr phase brief` prints
exactly what an agent will be told. Never author the frame yourself. At the spec gate, the agent must run `drovr review summary <run> "<text>"`
after **every** edit to `spec.md`, and the finishing phase agent authors exactly
`<phase>-HANDOFF.md` (the filename `drovr collect` reads) before `drovr phase done`.

## Running tests

```
cargo test              # unit + integration + e2e
cargo test --test e2e   # e2e smoke only
```

The e2e test requires `herdr`, `claude`, and the herdr claude integration hook.
It creates an isolated run in a temp directory and removes it on completion. If
prerequisites are absent it prints a skip message and exits cleanly.

### The lint gate is PARITY, not zero

`cargo clippy --all-targets -- -D warnings` does **not** pass on `main`, and
`cargo fmt --check` does not either. Neither can be a pass/fail gate as written,
and chasing them into the green is a separate, deliberate change — a
formatting-only commit conflicts with every branch in flight, so it needs a quiet
moment rather than a branch that happens to notice (see `docs/known-issues.md`).

**The gate a branch is actually held to: introduce no new finding.** Measure
before and after, and compare the *sets*, not just the counts — line numbers move
as code does, so compare by file and lint, and treat a finding whose lint class
already existed in that file as pre-existing.

**Measuring is subtler than it looks.** Cargo only re-emits warnings for targets
it actually recompiles, so a cached run silently under-reports — touching one
module can take the count from 8 to 4. And the same source-level warning is
reported once per compilation target (`src/*.rs` compiles as both `bin "drovr"`
and `bin "drovr" test`), which is what the `generated 1 warning (1 duplicate)`
summary lines are about. Force a full re-check and dedupe by source location:

```
cd cli
touch src/*.rs tests/*.rs        # or: CARGO_TARGET_DIR=$(mktemp -d)
cargo clippy --all-targets --message-format=short 2>&1 |
  grep ': warning: ' | sort -u
```

`--message-format=short` prints one `file:line:col: warning: …` per finding, so
`sort -u` collapses the per-target duplicates; the result is stable across cold
and warm builds. `cargo fmt` has no equivalent trick — do not run it, and see the
known-issues entry for why naming a single file reformats the whole crate.
