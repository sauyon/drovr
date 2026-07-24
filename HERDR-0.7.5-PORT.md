# herdr 0.7.5 port + review-gate blocking — status & TODO

Branch `herdr-0.7.5-port`. Captures a port of drovr's herdr integration from herdr
0.7.3's CLI to herdr 0.7.5's Unix-socket JSON-RPC API, plus a new `drovr review wait`
gate primitive. **Not deployed** (drovr is a nix flake input — shipping means push +
bump the dotfiles flake input + `home-manager switch`).

## Why

herdr 0.7.5 changed the agent CLI out from under drovr 0.1.0: `agent send` was removed,
`agent start` now takes `--kind`/`--pane` (into an existing pane) instead of `--cwd`, and
`workspace create` flags changed. Under 0.7.5 the old `SystemHerdr` (which shelled those
commands) is broken, so `drovr phase send`/`phase start` fail. This port fixes that.

## What changed (done, built, unit-tested — `cargo test`: 72 + 1 e2e green)

- **`cli/src/herdr.rs` — `SystemHerdr` rewritten to talk the socket** (`$HERDR_SOCKET_PATH`,
  newline-delimited JSON-RPC). The `Herdr` trait, `FakeHerdr`, and all trait-level tests are
  unchanged, so `phase.rs`/`main.rs` are untouched.
  - `workspace_create` → `workspace.create`; `workspace_close` → `workspace.close`.
  - `agent_start` → `pane.split` (get a pane) then `agent.start {kind,pane_id,args}`.
    Includes a **retry-with-backoff**: `pane.split` returns before the new pane's shell is
    ready, so an immediate `agent.start` fails with "not an available shell" — retried until
    the shell settles (verified: the retry succeeds).
  - `agent_send` → `agent.prompt` (types **and** submits natively — the old
    `PASTE_SETTLE` carriage-return hack is gone).
  - `agent_read` → `agent.read {source:"recent"}`.
  - `integration_present` / `session_stop` kept as `herdr` CLI shell-outs (unchanged in 0.7.5).
- **`cli/src/review.rs` + `cli/src/main.rs` — new `drovr review wait <run>`**: a driver-side
  primitive that blocks (no timeout by default) on the run-dir gate state until the reviewer
  acts. Exit `0` + `approved` → gate passed; exit `2` + `request-changes` + the `feedback.json`
  body → revise and loop. Replaces the driver's busy-poll loop. (`--timeout-ms` is a test-only
  escape hatch.)

## Validated live against the running herdr 0.7.5 server

`workspace.create`/`close`, `pane.split`, `agent.start` (with the race retry), `agent.prompt`,
`agent.read` all work end-to-end (spawned a real pane + claude agent). herdr's **`blocked`**
state detection is confirmed: an agent showing a native selection prompt (e.g. an
AskUserQuestion form) is detected as `blocked` via the manifest rule `live_blocked_form`
(observed 101/120 samples during a live form).

## KNOWN BLOCKER — spawned-agent auth env (needs a herdr-level fix)

drovr-spawned agents come up on the wrong claude profile: a stale
`CLAUDE_CONFIG_DIR` (pointing at a throwaway account) inherited from the **herdr server's
own launch environment**. On 0.7.5 drovr cannot override it:

- 0.7.5 removed `agent start --env` (what 0.7.3 drovr used to inject the caller's profile).
- `env` on `workspace.create` and `pane.split` only *adds* new vars — it does **not** override
  a `CLAUDE_CONFIG_DIR` already present in the inherited env. (Both tried; agent still wrong.)
- Exporting `CLAUDE_CONFIG_DIR` into the pane's interactive shell *before* `agent.start` also
  fails — `agent.start` launches claude with the server env, not the shell's env. (Tried.)

**Resolution is at the herdr layer, not in drovr:** the herdr server must be launched with the
intended `CLAUDE_CONFIG_DIR` (so panes inherit the right profile), OR herdr needs a way to set
per-agent/per-pane env that overrides the inherited value. Until then, agents spawned by the
ported drovr on a server with a stale profile can't authenticate. Existing panes spawned before
a server upgrade are unaffected.

## TODO (deferred)

1. **Agent auth env** (above) — the gating issue for any real run on 0.7.5.
2. **Option C — blocked-on-review skill wiring** (design agreed, not yet written):
   - `skills/pipeline/phase-prompts/brainstorm.md`: after `drovr review summary`, the agent
     parks at a native blocking prompt (so herdr shows `blocked`) instead of going idle.
   - `skills/pipeline/SKILL.md`: the driver runs `drovr review wait`, and on return **unblocks**
     the agent's prompt via `herdr agent send-keys`/`pane.send_input` before delivering the
     decision. The blocked *detection* is verified; the driver-*unblock* half is NOT yet
     validated live (was blocked by the auth issue — couldn't get a working agent to render a
     form). Add a `drovr phase send-keys` (or similar) wrapper if the driver needs it.
3. **Deploy**: push `github:sauyon/drovr`, bump the dotfiles flake input, `home-manager switch`.
