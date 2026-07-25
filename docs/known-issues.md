# Known issues

## Review-server Submit button does nothing when `questions.json` is not a bare array

**Severity:** high (the human spec gate is unusable — the reviewer's decision can never be recorded from the UI).
**Found:** 2026-07-24, reviewing run `gpu-deploy-view` through `drovr serve` (state `ready`, spec written, open questions present).

### Symptom

Clicking **Submit** in the review UI does nothing: no decision is recorded, no error
message appears, and the button silently greys out (stays `disabled`). Reloading does
not help. `GET /state` on the server stays `ready`/`idle` — the browser's `POST /submit`
**never reaches the server** (the server-side handler is fine; a `curl POST /submit`
works and flips state correctly).

Reproduced only when the run has open questions AND `questions.json` is shaped as an
**object** (`{"questions": [...]}`) rather than the **bare array** the UI expects. A run
with no `questions.json` (server serves `[]`) submits fine.

### Root cause (proven)

The UI's question contract is a **bare JSON array** of
`{id, prompt, options:[{value, label, recommended}]}` — see the server's own test at
`cli/src/review.rs:842` and `renderQuestions` / `collectAnswers` in `cli/web/index.html`.

The live `questions.json` for this run is instead an **object**:
`{"questions": [{"id": "...", "question": "...", "options": ["str", ...]}]}` — wrong at
three levels (object vs array, `question` vs `prompt`, string options vs objects).

The failure chain (`cli/web/index.html`):

1. `refresh()` fetches `/questions` and calls `renderQuestions(questionsData)` (line 1104).
2. `renderQuestions` assigns `currentQuestions = questions || []` (line 1006) — so
   `currentQuestions` becomes the **object**. It then hits
   `if (!currentQuestions.length) { ...; return; }` (line 1008): an object has no
   `.length` (`undefined`), so it **returns early without throwing**, leaving
   `currentQuestions` set to the object. (This is why the form still renders and the
   button looks normal — the throw is deferred to submit time.)
3. On Submit, `submitDecision()` disables the button (line 1158), then builds the
   payload. `answers: collectAnswers()` (line 1163) runs **before** the `try` block
   (line 1167). `collectAnswers()` calls `currentQuestions.forEach(...)` (line 1032),
   which throws `TypeError: currentQuestions.forEach is not a function`.
4. Because that throw is **outside** the `try/catch`, it is uncaught: `fetch('/submit')`
   never fires, and the `catch` that would call `showError(...)` and re-enable the
   button never runs. The button is left disabled with no message → "Submit doesn't
   work."

Verified live: `curl -X POST /submit` with a well-formed body **does** flip state
(the server side is correct), and replaying the exact live `questions.json` payload
through `collectAnswers()` reproduces the uncaught `TypeError` before any fetch.

### Reproduction

1. Start `drovr serve` for a run whose `questions.json` is an object
   (`{"questions":[...]}`) instead of a bare array.
2. Open the review page, provide feedback, click **Submit**.
3. Observe: button greys out, no decision recorded, `GET /state` unchanged, and a
   `TypeError: currentQuestions.forEach is not a function` in the browser console.

### Workaround

- Unblock a stuck reviewer by submitting via `curl` directly (server side works):
  ```
  # request changes (safe, reversible; increments turn, flips state -> waiting)
  curl -s -X POST http://<addr>/submit -H 'Content-Type: application/json' \
    -d '{"decision":"request-changes","feedback":"<msg>","answers":{},"annotations":[]}'
  # approve
  curl -s -X POST http://<addr>/submit -H 'Content-Type: application/json' \
    -d '{"decision":"approve","feedback":"","answers":{},"annotations":[]}'
  ```
- Or rewrite `questions.json` in the run dir into the UI's bare-array shape.

### Fix ideas (for a future drovr change)

1. **Harden the UI against the wrong shape** (defense-in-depth): in `renderQuestions`,
   normalize the payload — accept both a bare array and `{questions:[...]}`, and coerce
   to an array (`Array.isArray(x) ? x : (x && x.questions) || []`) before assigning
   `currentQuestions`. This alone prevents the `collectAnswers` throw.
2. **Move the payload build inside the `try`** in `submitDecision()` (or guard
   `collectAnswers`/`collectAnnotations`) so any exception surfaces via `showError(...)`
   and re-enables the button instead of silently killing Submit.
3. **Fix the producer contract**: make whatever writes `questions.json` emit the exact
   schema the UI/tests expect (bare array of `{id, prompt, options:[{value,label,
   recommended}]}`), or normalize where `/questions` is served in `cli/src/review.rs`
   so the wire format is authoritative regardless of the writer.
4. Add a UI/integration test that feeds a malformed `questions.json` and asserts Submit
   still posts (or shows an error), locking in the fault tolerance.

## `approve` discards the reviewer's question answers

**Severity:** medium (multiple-choice answers on the spec gate are silently lost on approval, so the downstream plan phase never sees the reviewer's picks).
**Found:** 2026-07-24, run `gpu-deploy-view` — reviewer answered 4 open questions and approved; no answers were persisted anywhere.

### Symptom

When the reviewer **approves**, `questions.json` answers (and annotations) chosen in the
UI are not written to disk. The run dir gets only a 9-byte `approved` marker; `feedback.json`
is left at whatever the previous turn wrote (often empty). Callers driving the pipeline can
recover the *decision* (approved) but not *which options the reviewer selected* — they have
to re-ask the human out-of-band.

### Root cause

In `POST /submit` (`cli/src/review.rs:~372`) the `decision == "approve"` branch writes only
the `approved` marker and returns. The branch that persists `feedback.json` — including
`answers` and `annotations` — runs only for the **request-changes** path (`~line 400`). So
answers survive a "request changes" but are dropped on "approve".

### Fix idea

On approve, also write `feedback.json` (or a dedicated `approved.json`) carrying
`{decision:"approve", answers, annotations, turn}`, so `review wait` / the driver can read
the reviewer's selections. Mirror the request-changes persistence.

## Serving a spec doesn't start a watcher — the reviewer's decision gets missed

**Severity:** medium (a driver that serves a spec for review but never runs `drovr review wait` is not notified when the human acts; it only learns the outcome if it happens to poll `/state`).
**Found:** 2026-07-24, standalone spec review on run `compress-spec` — the spec was served via `drovr serve`, the human approved in the UI, and the driver kept manually curling `GET /state` instead of being woken. "Why didn't your watch fire" — because no watch was ever started.

### Symptom

The human approves (or requests changes) in the review UI, but the driver does not react.
Nothing surfaces the decision until the driver next polls `/state` by hand. The approval is
recorded correctly on disk (`approved` marker, state `approved`) — the gap is purely that
**no process is watching the gate**, so there is no signal to act on.

### Root cause

`drovr serve` and `drovr review wait` are separate commands, and serving does not imply
watching. The **`drovr:pipeline`** skill documents backgrounding `review wait` and explicitly
warns against busy-polling `/state` (SKILL.md lines ~85, ~214) — but that guidance only fires
inside a full pipeline's brainstorm gate. A **standalone** spec/design review (serving a
`spec.md` for approval outside `drovr:pipeline`) has no skill routing the driver to the gate
discipline at all, so it is easy to `serve` and then never `review wait`. Manual `/state`
polling is the anti-pattern the skill already names, reached here by the routing gap.

### Fix ideas

1. **Route standalone reviews:** `drovr:using-drovr` should point *any* human-approval-on-a-spec
   (not only a full pipeline) at the gate — `drovr serve` **plus** a backgrounded
   `drovr review wait <run>` — cross-referencing `drovr:pipeline`'s "The spec gate" mechanics.
   (Done in this change.)
2. **Couple serve + watch in the CLI:** either have `drovr serve` print the exact
   `drovr review wait <run>` invocation on startup, or add a combined `drovr review gate <run>`
   that serves, blocks until the reviewer acts, and returns the decision (exit 0 approved / 2
   request-changes) — so the watch cannot be forgotten.
3. `drovr serve` is a foreground process; if it is backgrounded in a slot tied to the session
   shell it dies (SIGTERM 143) when that shell is torn down, taking the gate down mid-review.
   Launch it detached (`setsid`/`nohup`) when it must outlive the turn.

## `drovr phase compress` regurgitates the seed instead of the phase's artifact

**Severity:** medium (the next phase is seeded from a handoff that describes the *previous*
phase's state, not the work just done — so it must re-read the source artifact anyway, and a
driver that trusts the handoff would seed the next phase with stale/wrong context).
**Found:** 2026-07-24, run `gpu-deploy-view`. Hit on BOTH the brainstorm and the plan phase.

### Symptom

- **Plan phase:** the plan agent wrote a complete 538-line `plan.md` (8 tasks, verified
  signatures). `drovr phase compress ... plan` produced a ~35–43-line handoff whose State
  section says *"No implementation done… the source has NOT been read… all signatures
  UNKNOWN"* — i.e. it summarized the **brainstorm-level seed**, not the plan. Re-running
  compress produced the same wrong content.
- **Brainstorm phase (first attempt):** compress emitted 2 lines of meta-garbage
  ("Backend still down… write the plan to …splendid-nova.md… ExitPlanMode"). A retry produced
  a correct 7-section handoff.

### Root cause (suspected)

`phase compress` runs a fresh `claude -p` that reads the pane transcript via `herdr agent
read`. When the phase's real output lives in a **file** the agent wrote (`plan.md` via the
Write tool), the pane transcript shows tool *calls*, not the file's content — while the
injected briefing (which contained the prior phase's handoff) is fully present in the
transcript. The compressor over-weights the visible briefing and summarizes *that*. The
2-line garbage case looks like a transient API/tool error (the `502 classifier unreachable`
blips seen this session) that the compressor surfaced instead of a handoff.

### Workaround

Don't trust the handoff as the sole seed. Seed the next phase from the **artifact file**
(`spec.md` / `plan.md`) directly; use the handoff only as a supplement. For per-task interface
fold-forward, read the task's own `task<N>-report.md` (written by the implement agent), not the
compressed handoff.

### Fix ideas

1. Have `phase compress` also read the phase's declared output artifact(s) (`spec.md`,
   `plan.md`, `task<N>-report.md`) from the run dir, not only the pane transcript.
2. Detect and reject a degenerate handoff (e.g. < N lines, or missing the fixed sections, or
   an obvious API-error body) and auto-retry before writing it.

## `drovr code-review run` panel never completes (reviewer panes don't attach)

**Severity:** medium (the automated review-until-clean panel is unusable; the driver must fall
back to spawning its own read-only reviewer).
**Found:** 2026-07-24, run `gpu-deploy-view`, tasks 1–2. Only `claude` has a herdr integration
here (cursor not integrated).

### Symptom

- On the pre-update binary: `drovr code-review run <run> task-N` → `code-review run failed:
  agent target w61:pX not found` (the reviewer pane is created but the agent isn't attached
  when the panel tries to drive it — the same startup race as `phase send`).
- On the updated binary (past the phase-send readiness fix): the panel writes its per-angle
  seed files (`task-N-review-<angle>-seed.md` for correctness/error-handling/security/
  type-design) but the reviewer panes never reach `done`; `code-review run` times out with no
  `task-N-review.json` produced.

### Workaround

Drive the between-task review with a self-spawned read-only reviewer (Claude Code Agent tool,
`general-purpose`, read-only) over `git diff <base>..HEAD` **plus** the working tree, and feed
Critical/Important findings back to the implement agent. Same find-then-fix discipline, no
herdr panel.

### Fix idea

Apply the `phase send` agent-readiness fix (poll `agent_status` until attached/at-composer) to
the reviewer-spawn path in `code_review.rs`, and bound each reviewer with a liveness check so a
never-attached pane fails fast instead of hanging the whole panel.

## `drovr phase send` still lands a large briefing unsubmitted (post-readiness-fix)

**Severity:** low (recoverable, but every phase injection needs a manual nudge, so an
unattended pipeline stalls silently at each phase start).
**Found:** 2026-07-24, run `gpu-deploy-view`, every phase injection — including on the updated
binary that carries the phase-send agent-readiness fix.

### Symptom

`drovr phase send <run> <phase> "<large briefing>"` returns success, and (post-fix) no longer
errors with "agent target not found" — but the briefing sits in the agent's composer as a
collapsed bracketed paste (`❯ [Pasted text #1 +NN lines]`, cost `$0.00`) and is **not
submitted**. The agent never starts; `phase wait` would time out.

### Root cause (suspected)

The readiness fix (await attach/composer before sending) resolved the *race* that caused
"target not found", but the submit itself — a large **bracketed paste** followed by a single
CR — still leaves the paste uncommitted in the composer for big payloads; the trailing CR does
not submit it.

### Workaround

After `phase send`, submit with `herdr agent send-keys <pane> Enter` (verify first with
`herdr agent read <pane>`).

### Fix idea

For large payloads, either send the submit key(s) separately after a short settle, or detect a
still-populated composer post-send and re-issue the submit until the input clears.

## Spawned agents park on the "New MCP server" approval prompt, undetected

**Severity:** medium (every fresh agent in a project with an MCP server stalls at spawn until
someone answers `1/2/3`; unattended pipelines wedge silently).
**Found:** 2026-07-25, every `drovr phase start` / browser-launched session in this repo (has a
`datadog` MCP server).

### Symptom

A freshly spawned `claude` sits on `New MCP server found: datadog … 1. Use  2. Use all  3.
Continue without` — a numbered menu, cost `$0.00`, never starting. `agent_status` reports
`idle`/none (not `blocked`), so `phase send` readiness and blocked-triage don't catch it.

### Root cause (proven)

herdr's prompt-detection manifest (`~/.local/state/herdr/agent-detection/remote/claude.toml`)
has no rule matching this prompt's wording ("Use this MCP server" / "Enter to confirm"), so it
resolves to not-blocked. herdr can *read* the text (`agent read --source detection`) but does
not classify it or parse the options.

### Workaround

Clear it manually: `herdr agent send-keys <pane> 3` then `… enter`. (Blind — options aren't
structured.)

### Fix ideas

Add a manifest rule so the prompt reports `blocked`; and give the browser mirror a "send keys"
control (arrows/enter/number) so menus are answerable from the UI — today `/send` types text
only.

### Status: half fixed (2026-07-25, `drovr/send-keys-mirror`)

The **answering** half is done: `POST /api/runs/<run>/keys` (`{"keys":["3","enter"]}`) →
`Herdr::agent_send_keys` → `herdr agent send-keys`, wired to an Enter/Esc/↑/↓/1–5 key row in the
Live-session panel, so a parked agent can be cleared from the browser without attaching.

The **detection** half is still open: herdr's manifest has no rule for this prompt, so
`agent_status` still reports `idle` and an unattended pipeline still wedges silently — a human
has to notice the mirror and press `3`. Fixing that needs the herdr-side manifest rule (or a
drovr-side `agent explain --json` / `visible_blocker` poll to surface it in the UI).

## `drovr review wait` fails (not "approved") if the server restarts mid-wait

**Severity:** medium (a failed wait can be *misread* as approval and advance the pipeline past
an unapproved gate).
**Found:** 2026-07-24, gate wait for run `clean-content`.

### Symptom

A backgrounded `drovr review wait <run>` prints `could not connect to review server …
Connection refused` and exits **1** while the reviewer has NOT acted. If the exit code is read
loosely (e.g. a harness reporting the wrapper's 0), it looks like approval and the driver
compresses/advances past a gate that is still `ready`.

### Root cause

`review wait` resolves the server addr once, then polls it; restarting the always-on server
(e.g. to load new code) drops the socket, and the next poll's connect fails → `Err` → exit 1.

### Workaround

Never restart the always-on server while a `review wait` is in flight. Verify the *inner* exit
code and the authoritative `GET /state` (`approved` vs `ready`) before advancing — do not trust
a wrapper's exit alone. `phase wait` (filesystem markers) is unaffected by server restarts.

### Fix idea

Make `review wait` treat a transient connect failure as retryable (re-run `ensure_server` and
resume) rather than a hard error, so a server restart doesn't surface as a spurious terminal
exit.

## Test suite flakes under parallel `cargo test`; needs `--test-threads=1`

**Severity:** low (green when run serially; false failures otherwise).
**Found:** 2026-07-24.

### Symptom

`cargo test` intermittently fails ~50+ tests across `config`, `herdr`, `run`, `phase` with
unrelated assertion errors; the same tests pass in isolation and under `--test-threads=1`.

### Root cause

Those tests mutate **process-global** state (`XDG_DATA_HOME`, auth env vars) guarded by an
`ENV_LOCK`, but the lock only serializes the tests that take it — other parallel tests read the
polluted env between a mutation and its restore.

### Workaround

Run `cargo test -- --test-threads=1` (CI should pin this).

### Fix idea

Have env-mutating tests set state via a scoped guard that restores on drop and is held across
every read, or move them behind a single serial test harness.

## Session mirror shows raw terminal chrome, not clean conversation content

**Severity:** low (cosmetic; the mirror is readable but noisy).
**Found:** 2026-07-24.

### Symptom

`GET /api/runs/<run>/pane` (the Live-session mirror) returns herdr's raw terminal snapshot —
status bar (`ctx:… | $… | …`), the `❯` input box, separators, box-drawing — not just the
agent↔user conversation.

### Root cause

herdr's `agent read` mirrors the rendered TUI; there is no structured "just the conversation"
source. (Claude's own session JSONL has clean turns, but reading it is claude-specific.)

### Fix idea

Add an agent-agnostic "clean" mode that strips the known chrome (status line, `❯` composer,
separator rules) from the snapshot; keep raw as a toggle. Avoid a claude-only JSONL parser as
the primary path.

### Status: still open, but less costly (2026-07-25, `drovr/send-keys-mirror`)

The rendering is unchanged — the mirror is still raw chrome. What changed is that the chrome is
no longer *inert*: the menus it renders (numbered prompts, pickers) are now answerable from the
panel's key row via `POST /keys`, so noisy output no longer means an unactionable panel.
