# Known issues

## The agent's change summary is hidden on the first review — FIXED 2026-07-25

**Status:** fixed on `feat/questions-ui`. The banner is gated on `summaryText` alone;
the `turn > 0` clause is gone. Regression check: "the agent change summary shows on the
first review (turn 0)" in `cli/tests/web/nav.mjs`, whose fixture seeds a `ready` run at
turn 0 with a posted summary.

**Severity:** medium (on turn 0 the reviewer sees a spec with no statement of what the agent did or is asking — exactly the turn where that context is most needed).
**Found:** 2026-07-25, QA'ing a full gate cycle on a throwaway run (`qa-cache`): `drovr review summary` succeeded, `GET /api/runs/<run>/summary` returned the text, and the banner still did not render.

### Symptom

The brainstorm agent posts a summary, the run flips to `ready`, and the reviewer opens the
page — but the "Agent change summary" banner is not shown. It appears only from the second
review turn onward (after one request-changes round trip).

### Root cause

`cli/web/index.html:1643` gates the banner on the turn counter:

```js
if (summaryText && turn > 0) { ... showEl('summary-banner'); }
else { hideEl('summary-banner'); }
```

`turn` only increments when the reviewer submits, so it is `0` for the whole first review.
The `turn > 0` test appears to be aimed at "only show a summary once there is a previous
version to describe", but the summary is not a diff — `review summary` is the agent's own
statement of what it wants reviewed, and it is equally meaningful on turn 0. Introduced in
`8f98013` (interactive review server), unrelated to the questions/navigation work.

### Fix

Dropped the `turn > 0` clause and gated on `summaryText` alone. The empty-summary case is
already covered by the same condition, so a run that never posted a summary still shows no
banner.

## Editing `cli/web/index.html` can silently test the OLD page

**Severity:** low (no runtime bug — but it wastes debugging cycles and can make a real fix look broken).
**Found:** 2026-07-25, while adding the review UI's keyboard navigation.

### Symptom

You edit `cli/web/index.html`, run `cargo build` (which reports `Compiling drovr`),
restart `drovr serve` — and the browser still shows the previous markup. Checking the
served HTML for a string you just added returns nothing.

### Root cause

`cli/src/review.rs` embeds the page with `include_str!("../web/index.html")`, so the HTML
lives in the **binary**, not on disk at request time. `serve` never re-reads the file. Cargo
does track `include_str!` inputs, but a rebuild triggered by an unrelated source change can
finish without re-embedding the newer HTML, so the build "succeeds" while the binary keeps
the old page.

### Working around it

`touch cli/web/index.html` before `cargo build` whenever the page changed, then confirm the
binary actually carries the change before you debug anything:

```
grep -ac '<a-string-you-just-added>' cli/target/debug/drovr    # -a: it's a binary
```

`grep` without `-a` prints nothing useful here and reads as "not present" either way.
`cli/tests/web_nav.rs` has the same exposure — it drives whatever HTML was compiled in.

## Review-server Submit button does nothing when `questions.json` is not a bare array

**Severity:** high (the human spec gate is unusable — the reviewer's decision can never be recorded from the UI).
**Found:** 2026-07-24, reviewing run `gpu-deploy-view` through `drovr serve` (state `ready`, spec written, open questions present).
**Re-verified against source 2026-07-25:** still live; line refs and endpoint paths below updated
(the API is now run-scoped under `/api/runs/<run>/…`).

### Symptom

Clicking **Submit** in the review UI does nothing: no decision is recorded, no error
message appears, and the button silently greys out (stays `disabled`). Reloading does
not help. `GET /api/runs/<run>/state` on the server stays `ready`/`idle` — the browser's
`POST /api/runs/<run>/submit` **never reaches the server** (the server-side handler is
fine; a `curl POST …/submit` works and flips state correctly).

Reproduced only when the run has open questions AND `questions.json` is shaped as an
**object** (`{"questions": [...]}`) rather than the **bare array** the UI expects. A run
with no `questions.json` (server serves `[]`) submits fine.

### Root cause (proven)

The UI's question contract is a **bare JSON array** of
`{id, prompt, options:[{value, label, recommended}]}` — see the server's own test
`questions_served_when_file_present` at `cli/src/review.rs:1819` and `renderQuestions` /
`collectAnswers` in `cli/web/index.html`.

The live `questions.json` for this run is instead an **object**:
`{"questions": [{"id": "...", "question": "...", "options": ["str", ...]}]}` — wrong at
three levels (object vs array, `question` vs `prompt`, string options vs objects).

The failure chain (`cli/web/index.html`):

1. `refresh()` fetches `questions` (line 1402) and calls `renderQuestions(questionsData)`
   (line 1454).
2. `renderQuestions` (line 1350) assigns `currentQuestions = questions || []` (line 1351)
   — so `currentQuestions` becomes the **object**. It then hits
   `if (!currentQuestions.length) { ...; return; }` (line 1353): an object has no
   `.length` (`undefined`), so it **returns early without throwing**, leaving
   `currentQuestions` set to the object. (This is why the form still renders and the
   button looks normal — the throw is deferred to submit time.)
3. On Submit, `submitDecision()` (line 1514) disables the button (line 1533), then builds
   the payload (line 1536). `answers: collectAnswers()` (line 1539) runs **before** the
   `try` block (line 1543). `collectAnswers()` (line 1375) calls
   `currentQuestions.forEach(...)` (line 1377), which throws
   `TypeError: currentQuestions.forEach is not a function`.
4. Because that throw is **outside** the `try/catch`, it is uncaught: the
   `fetch(api('submit'))` never fires, and the `catch` that would call `showError(...)`
   and re-enable the button never runs. The button is left disabled with no message →
   "Submit doesn't work."

Verified live: `curl -X POST …/submit` with a well-formed body **does** flip state
(the server side is correct — `handle_post_submit`, `cli/src/review.rs:808`), and
replaying the exact live `questions.json` payload through `collectAnswers()` reproduces
the uncaught `TypeError` before any fetch.

### Reproduction

1. Start `drovr serve` for a run whose `questions.json` is an object
   (`{"questions":[...]}`) instead of a bare array.
2. Open the review page, provide feedback, click **Submit**.
3. Observe: button greys out, no decision recorded, `GET /api/runs/<run>/state`
   unchanged, and a `TypeError: currentQuestions.forEach is not a function` in the
   browser console.

### Workaround

- Unblock a stuck reviewer by submitting via `curl` directly (server side works):
  ```
  # request changes (safe, reversible; increments turn, flips state -> waiting)
  curl -s -X POST http://<addr>/api/runs/<run>/submit -H 'Content-Type: application/json' \
    -d '{"decision":"request-changes","feedback":"<msg>","answers":{},"annotations":[]}'
  # approve
  curl -s -X POST http://<addr>/api/runs/<run>/submit -H 'Content-Type: application/json' \
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
   recommended}]}`), or normalize where `questions` is served
   (`cli/src/review.rs:487-490` — today it streams the file through verbatim) so the wire
   format is authoritative regardless of the writer.
4. Add a UI/integration test that feeds a malformed `questions.json` and asserts Submit
   still posts (or shows an error), locking in the fault tolerance.

## `approve` discards the reviewer's question answers — FIXED 2026-07-25

**Status:** fixed on `feat/questions-ui`. `handle_post_submit`'s approve branch now
writes the same `feedback.json` the request-changes branch does
(`{turn, decision:"approve", feedback, answers, annotations}`), with the turn advanced so a
driver can tell this turn's answers from a stale previous turn's. Regression test:
`review::tests::submit_approve_persists_question_answers`.

**Severity:** medium (multiple-choice answers on the spec gate are silently lost on approval, so the downstream plan phase never sees the reviewer's picks).
**Found:** 2026-07-24, run `gpu-deploy-view` — reviewer answered 4 open questions and approved; no answers were persisted anywhere.
**Re-verified against source 2026-07-25:** still live at that point; fixed later the same day (see Status above).

The Symptom and Root cause below describe the behaviour **before** the fix, kept for history.

### Symptom

When the reviewer **approves**, `questions.json` answers (and annotations) chosen in the
UI are not written to disk. The run dir gets only a 9-byte `approved` marker; `feedback.json`
is left at whatever the previous turn wrote (often empty). Callers driving the pipeline can
recover the *decision* (approved) but not *which options the reviewer selected* — they have
to re-ask the human out-of-band.

### Root cause

In `handle_post_submit` (`POST /api/runs/<run>/submit`, `cli/src/review.rs:808`) the
`decision == "approve"` branch (`cli/src/review.rs:837`) writes only the `approved` marker
and returns — even though `answers`/`annotations` were parsed off the request body
(`cli/src/review.rs:813-821`). The branch that persists `feedback.json` — including
`answers` and `annotations` — ran only for the **request-changes** path (now
`cli/src/review.rs:895-902`; the fix inserted the approve-side write above it). So answers
survived a "request changes" but were dropped on "approve".

### Fix

Mirrored the request-changes persistence: on approve, write `feedback.json` carrying
`{turn, decision:"approve", feedback, answers, annotations}`. The consuming half was wired
too — `drovr review wait` names `feedback.json` in its approval message, and the brainstorm
phase prompt tells the agent to fold the answers into `spec.md` rather than re-ask.

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
2. **Couple serve + watch in the CLI.** *(Done — `drovr review summary` now prints the hint.)*
   `drovr serve` turned out to be the wrong hook: it is global and takes no run argument, so it
   cannot name the run to watch. The run-scoped moment the gate actually opens is
   `drovr review summary <run>`, which previously printed nothing on success. It now prints the
   reviewer's page URL and the exact `drovr review wait <run>` invocation, flagged to run
   backgrounded. Still open if the reminder proves too weak: a combined `drovr review gate <run>`
   (**sketch — no such subcommand exists**) that serves, blocks, and returns the decision.
3. `drovr serve` is a foreground process; if it is backgrounded in a slot tied to the session
   shell it dies (SIGTERM 143) when that shell is torn down, taking the gate down mid-review.
   Launch it detached (`setsid`/`nohup`) when it must outlive the turn.

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

~~Apply the `phase send` agent-readiness fix (poll `agent_status` until attached/at-composer)
to the reviewer-spawn path in `code_review.rs`~~ — **done**, see below.

~~Still open: bound each reviewer with a liveness check so a never-attached (or
attached-but-wedged) pane fails fast instead of hanging the whole panel. Today the only bound is
the single panel-wide `timeout_ms` deadline in the marker poll loop; an individual reviewer is
never probed for liveness, and a timed-out pass just returns `ReviewOutcome::Timeout` with no
`<task>-review.json`.~~ — **addressed** by the resume path (2026-07-25): a timeout is now a
pause rather than a dead end. Each reviewer is harvested to `<task>-review-<angle>.json` the
moment it finishes, and a plain re-run of `drovr code-review run` resumes the same iteration —
waiting only on the stragglers and respawning any whose pane no longer exists (`Herdr::pane_exists`,
which unlike `agent_status` distinguishes "pane gone" from "status unparseable"). A wedged
reviewer still needs the human's `--fresh`; what is fixed is that it no longer costs the whole
panel's work.

### Also seen (2026-07-25, run `harden-review`, `harden/supply-chain`) — root cause since fixed

Reproduced dogfooding the panel on the supply-chain-hardening change, on a host where
**`cursor` IS integrated** (so the review agent resolves to cursor, not claude). Here the
panel failed at the *first* step — `code-review run failed: agent target <ws>:p2 not found` —
on **both** the merged binary (`main`) **and** a fresh build of `fix/phase-send-await-agent-ready`
(`a71d1a8`), because at that time the readiness wait lived only on the **`drovr phase send` CLI
path** while `code-review run` used a bare `agent_send`.

**That gap is now closed** (commit `c12adb0`, on `main`): the readiness gate lives inside
`phase::phase_send` itself (`cli/src/phase.rs:339-375`, via `wait_agent_ready` at
`cli/src/phase.rs:309-326`), which is the *same* function `code_review.rs:318` calls after
`spawn_reviewer`. Both paths now poll `agent_status` before sending, and a never-attached
reviewer raises a `TimedOut` error that aborts the pass instead of erroring with "target not
found". So the "agent target not found" symptom above should no longer occur.

**CONFIRMED 2026-07-25** (run `review-resume`, branch `drovr/review-resume`, dogfooding the panel
on the code-review resume change): the *second* symptom reproduces, and it is **not** a distinct
bug — it is the unsubmitted-paste failure documented in the next section. All four cursor
reviewer panes launched, attached, and received their seed, but the brief sits in the composer as
`→ [Pasted text #1 +46 lines]`, never submitted. The agents therefore never start, never reach
`done`, and `code-review run` times out with no `<task>-review.json` — exactly as reported.

Reading a reviewer pane (`herdr agent read <pane>`) shows the full seed rendered in the composer
with the correct `base..head` scope, so seeding and scope selection are fine; only the submit
keystroke is missing. Fixing "`phase send` lands a large briefing unsubmitted" (below) fixes the
panel too — they are one bug, and the panel is simply its most visible victim. Keep the
self-spawned-reviewer workaround above until that lands.

## `drovr phase send` still lands a large briefing unsubmitted (post-readiness-fix)

**Severity:** low (recoverable, but every phase injection needs a manual nudge, so an
unattended pipeline stalls silently at each phase start).
**Found:** 2026-07-24, run `gpu-deploy-view`, every phase injection — including on the updated
binary that carries the phase-send agent-readiness fix.
**Reproduced 2026-07-25** (run `mcp-endpoint`) — see "Still reproducing" below.

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

### Still reproducing (2026-07-25, run `mcp-endpoint`, pane `wAC:p1`)

Confirmed live on the installed nix-profile binary with a 6586-byte / 124-line briefing:

1. The **first** `drovr phase send` landed **nothing at all** — the composer stayed empty at
   `$0.00`. That is the readiness race described in the entry above's addendum (
   "`code-review run` panel never completes") reaching the `phase send` CLI path too, not just
   the reviewer-spawn path: the command reports success while the payload is dropped.
2. A **second, identical** send landed as the documented collapsed paste:
   `❯ [Pasted text #1 +124 lines]`, `$0.00`, **unsubmitted**.
3. `herdr agent send-keys wAC:p1 Enter` submitted it — the documented workaround still works.

So there are two failure modes on this path, not one: a silent *drop* and a silent
*non-submit*. Any fix must cover both — verifying the composer is non-empty after the send is
what distinguishes them.

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
(Route: `cli/src/review.rs:524` → `handle_post_keys`.)

The **detection** half is still open, re-confirmed 2026-07-25: no file under
`~/.local/state/herdr/agent-detection/remote/` — `claude.toml` included — matches "mcp", so
`agent_status` still reports `idle` and an unattended pipeline still wedges silently. A human
has to notice the mirror and press `3`. Fixing that needs the herdr-side manifest rule (or a
drovr-side `agent explain --json` / `visible_blocker` poll to surface it in the UI). Note this
half lives **outside** this repo, so it cannot be closed by a drovr change alone.

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

Still true in source as of 2026-07-25: `review_wait` calls `ensure_server()` exactly once
(`cli/src/review.rs:1283`) and then the poll loop propagates any connect error with `?`
(`cli/src/review.rs:1287`, calling `fetch_state` at `cli/src/review.rs:1251` whose
`TcpStream::connect` failure becomes the `could not connect to review server …` error). There
is no retry and no re-`ensure_server` on the polling path.

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
`ENV_LOCK` (`cli/src/main.rs:951`, taken in `run.rs`, `code_review.rs`, `herdr.rs`, `phase.rs`,
`config.rs`), but the lock only serializes the tests that take it — other parallel tests read
the polluted env between a mutation and its restore. Unchanged as of 2026-07-25: the lock is
still a plain `Mutex<()>` with no restore-on-drop guard.

### Workaround

Run `cargo test -- --test-threads=1`. (CI should pin this — note the repo currently has **no**
CI workflow at all, so nothing enforces it today.)

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
Confirmed unchanged 2026-07-25: `handle_get_pane` (`cli/src/review.rs:607`) returns
`SystemHerdr::agent_read(pane)` verbatim as `text/plain` — no filtering, and no `clean`/`raw`
query parameter exists.

### Fix idea

Add an agent-agnostic "clean" mode that strips the known chrome (status line, `❯` composer,
separator rules) from the snapshot; keep raw as a toggle. Avoid a claude-only JSONL parser as
the primary path.

### Status: still open, but less costly (2026-07-25, `drovr/send-keys-mirror`)

The rendering is unchanged — the mirror is still raw chrome. What changed is that the chrome is
no longer *inert*: the menus it renders (numbered prompts, pickers) are now answerable from the
panel's key row via `POST /keys`, so noisy output no longer means an unactionable panel.

## Review UI shows a Changes view when the spec has not changed

**Severity:** low at turn 0 (cosmetic/confusing — it wastes reviewer attention and makes a
no-op revision look like a real one), but **medium from turn 1 onward**, where it really is
data-losing — see "Severity escalates after the first review turn" below.
**Found:** 2026-07-25, run `mcp-endpoint` (observed at `turn: 0`, i.e. the cosmetic case).

### Symptom

The reviewer opens the spec at the gate and sees a Changes/diff panel, but the diff is empty —
nothing actually changed between the baseline and the current spec. Verified on the live run:

- `~/.local/share/drovr/runs/mcp-endpoint/prior.md` and `spec.md` are **byte-identical**
  (`cmp -s` equal, both 40284 bytes). `last_summarized.md` is identical to both.
- `GET /api/runs/mcp-endpoint/prior` returns `200` with the full 40284-byte body rather than
  the `204` the handler emits for "no prior" (`cli/src/review.rs:473-478`).
- Gate state at the time: `{"state":"ready","turn":0}` — no reviewer action had occurred, so
  the reviewer-submit snapshot path (`cli/src/review.rs:868-874`) had NOT run.

### Root cause (verified against source + the run dir)

`handle_post_summary` (`cli/src/review.rs:900`) re-baselines the diff on **every** call, with no
check that `spec.md` actually changed: it promotes `last_summarized.md` → `prior.md`
(`cli/src/review.rs:926-933`), then re-snapshots the current spec into `last_summarized.md`
(`cli/src/review.rs:935-940`).

**The trigger is a redundant `review summary` call, not a bad first-summary seed.** Evidence:

- The **first** summary call cannot produce this. With `last_summarized.md` absent or empty the
  `match` at `cli/src/review.rs:926-933` falls through its `_ => {}` arm, so `prior.md` is never
  written and `/prior` correctly 204s. The first-summary path is fine.
- On the run dir, `prior.md` and `last_summarized.md` share an mtime of `01:58:53.523`, with
  `summary.txt` and `review.state.json` at `01:58:53.524` — exactly the write order of
  `handle_post_summary` (prior → last_summarized → summary.txt → state). The submit path is
  ruled out both by `turn: 0` and because it never writes `summary.txt`.
- For that call to have written `prior.md` at all, `last_summarized.md` must already have been
  non-empty — i.e. an **earlier** `review summary` had run. And since the promoted `prior.md`
  equals the current `spec.md` byte-for-byte (and `spec.md`'s mtime, `01:58:06`, predates both
  calls), the spec was unchanged between the two summary calls.

`skills/pipeline/phase-prompts/brainstorm.md` instructs the agent to run `review summary` after
every edit, but nothing prevents a redundant or double call — and downstream, a redundant call
is indistinguishable from a real revision.

### Severity escalates after the first review turn

At `turn: 0` (the captured run) there is no reviewer feedback yet, so an empty Changes panel is
merely noise. From `turn: 1` onward the same re-baseline **destroys the reviewer's reference
point**:

1. Reviewer submits request-changes → the submit path (`cli/src/review.rs:868-874`) snaps both
   `prior.md` and `last_summarized.md` to the spec the reviewer acted on.
2. The agent revises `spec.md` and calls `review summary` once → `last_summarized.md` advances
   to the new spec, `prior.md` still holds what the reviewer saw. The diff is correct.
3. The agent calls `review summary` **again without editing** → `prior.md` is overwritten with
   the current spec. The reviewer now sees an empty diff, and the snapshot that showed "here is
   what I asked you to change from" is gone.

So a redundant summary call can hide whether requested changes were actually made. Fix idea (1)
below also closes this case.

### Fix ideas

1. **Guard the re-baseline:** skip the `prior.md` promotion when the current spec is
   byte-identical to `last_summarized.md`, and have `review summary` return a distinguishable
   "no change" result so the caller knows nothing was published.
2. **Or guard at render time:** have the UI hide the Changes panel when the computed diff has
   zero hunks.
3. Tradeoff: (1) prevents the bogus revision from ever existing; (2) only hides it, and the
   empty revision still occupies a turn. (1) is the stronger fix but changes the `review
   summary` contract, so a caller that treats any 200 as "published" needs updating too.

## `drovr cleanup` can clobber a concurrent `state.json` write

**Severity:** low (narrow window, and the panes it would race are already dead).
**Found:** 2026-07-25, during review of the session-completion change.

### Symptom

Two writers now do load-modify-save on `state.json` without locking. `cmd_cleanup`
(`cli/src/main.rs`) sets `archived: true`; so does `handle_archive`
(`cli/src/review.rs`), the review server's archive endpoint. The endpoint's window is the
more reachable of the two: the server is multi-threaded and the endpoint is a button a human
can press mid-phase, whereas `cleanup` is a one-shot command. Both re-read immediately before
writing to narrow the window; neither closes it.

`cmd_cleanup` (`cli/src/main.rs`) writes `state.json` to set `archived: true`. `RunState::save`
(`cli/src/run.rs`) is a whole-file `fs::write` with no locking, no read-modify-write and no
version check, so a `drovr phase ...` running in a still-live pane can have its status write
silently reverted.

Before the archived flag existed the non-purge cleanup path never wrote `state.json` at all, so
this window is new — it is a real (if small) regression introduced alongside the fix.

### Why it is small

The write was deliberately placed immediately after `herdr.workspace_close`, which kills every
pane in the run, and it re-reads `state.json` from disk rather than saving the copy loaded at the
top of the function. The race therefore needs a phase agent to write during the `workspace_close`
call itself, after which it no longer exists.

### Fix ideas

1. Give `RunState::save` a compare-and-swap: re-read, compare against the copy that was loaded,
   and refuse or retry on divergence.
2. Or take a per-run lockfile in the run dir around load-modify-save, and have `phase_*` honour it.
3. (1) is cheaper and fixes only this class of clobber; (2) is the general answer and would also
   cover the server's own writers.

### Not fixed here, on purpose

`cmd_cleanup`'s `process::exit(1)` paths (dirty worktree, failed squash-commit) cannot be driven
from a unit test, so the *ordering* guarantee — archived is written before any git work, so a
failed prune still leaves the run correctly marked — is enforced by construction and comment
rather than by a test. `cleanup_marks_the_run_archived` (`cli/src/main.rs`) covers the
run-to-completion path only.

## The session list rebuilds via `innerHTML` every 2s, which is what makes rows "vanish"

**Severity:** low as shipped (the symptoms are fixed), but it is the root of a whole bug class.
**Found:** 2026-07-25, design review after the archive button.

`renderRunList` (`cli/web/index.html`) replaces `#run-list-items` wholesale on every 2s poll.
Every row element is therefore destroyed and recreated constantly, so anything the user has
"on" a row — the keyboard cursor, DOM focus — has to be re-derived from scratch each time.

That is why the cursor needs `navCursorKey`, `knownRunNames` and `listFetchSeq` to tell "this
row is hidden" from "this run is gone", and why five review rounds went into that one
function. The archive button did not introduce the fragility; it made it reachable, by being
the first thing that removes a row out from under the poll while the reviewer is looking at
it (archive/restore, and liveness flapping a row into and out of the collapsed group).

It is also why real Tab focus on a row control is destroyed on the next tick — pre-existing,
and now slightly worse with a second focusable control per row.

### Fix idea

Diff and patch rows instead of rebuilding: keyed by run name, update in place, add/remove only
what changed. The cursor's element then simply persists and the entire hidden-vs-gone question
disappears, along with the state that answers it. This is a rewrite of `renderRunList` — it has
to preserve `<details>` open state, filter state and the delegated button listener — so it
wants to be done deliberately, not folded into a feature branch.

## Zombie detection goes quiet while herdr is unreachable

**Severity:** low (transient and self-healing), but it is a deliberate trade rather than a fix.
**Found:** 2026-07-25, round-six review of the archive button.

An archived run whose `workspace_close` failed is a *zombie*: filed away while an agent may
still be running in panes we believe we shut. `list_runs_json` (`cli/src/review.rs`) keeps such
a row out of the collapsed "Completed" group so it stays visible.

That detection is `archived && live == Some(true)`. When `herdr workspace list` fails, `live`
is `None` for every row, no run is judged a zombie, and a genuine one collapses into the group
with no warning.

### Why it is not `live != Some(false)`

Treating unknown as live would stamp "panes still live" on **every** archived run on any herdr
blip — false alarms on a claim we cannot support, which is how a warning stops being read.
The archive *confirm* does treat unknown as live (`cli/web/index.html`), and that asymmetry is
intentional: the confirm gates a destructive act, where being wrong means killing a live agent.

The residual is bounded: the next successful poll surfaces the zombie again, and the list
header shows a "could not reach herdr — liveness unknown" banner so the grouping is not read
as verified.

### Fix ideas

1. Cache the last known-good `workspace_list` result and fall back to it, so a blip does not
   erase liveness at all — with an age limit, since stale liveness is its own lie.
2. Or have `handle_archive` record `workspace_closed: false` durably in `state.json`, making a
   zombie a fact about the run rather than something re-derived from herdr on every poll.
   (2) is the stronger fix: it survives herdr being down entirely.

## Restoring an archived run does not make it runnable again

**Severity:** low (restore is for undoing a misclick), but the naming invites the wrong
expectation.
**Found:** 2026-07-25, re-reviewing the archive button.

`POST /api/runs/<run>/archive {"archived":false}` — the UI's Restore button — clears the flag
and moves the row back to the active list. It cannot bring the agent back: archiving closed
the run's herdr workspace, and nothing recreates one. `phase_start` (`cli/src/phase.rs`) only
reuses a recorded `pane_id`, then `root_pane`, then `tab_create` against the run's existing
`workspace` id; all three are dead after a close, and the only code that creates a workspace
is `cmd_new`. So `drovr phase start` on a restored run fails.

The run's artifacts survive (spec, handoffs, branch), so the work is not lost — but continuing
it means a new run seeded from the handoff, not a restore.

### Fix ideas

1. Have `phase_start` create a fresh workspace when the recorded one is gone, and write the new
   id back to `state.json` — makes Restore mean what it looks like it means.
2. Or rename the control to something that does not imply resumability, and have it clear
   `workspace`/`root_pane`/`pane_id` so the failure is a clean "no workspace" error rather than
   a herdr rejection.

## The review server still has no authentication (cross-origin writes blocked; direct ones are not)

**Severity:** low on loopback, medium once `serve_host` leaves it.
**Found:** 2026-07-25, reviewing the archive button.

### What IS guarded

`handle` refuses any `POST` whose `Host` is not an address this server actually bound, and
then any whose `Origin` is cross-origin or opaque (`write_allowed`, `cli/src/review.rs`). The
`Host` check is the load-bearing one: comparing `Origin` to `Host` alone is defeated by DNS
rebinding, since a browser derives both from the same attacker-controlled URL. That closes the drive-by case: a page the user happens to visit can no
longer make their browser POST `/api/runs/<run>/archive` and close a live herdr workspace,
nor `/send` into a live pane, nor `/submit` a spec decision. Browsers always attach `Origin`
on a cross-origin request and script cannot suppress it; curl and drovr's own CLI send none
and are unaffected.

### What is NOT guarded

There is still no authentication of any kind. Anything that can open a TCP connection to the
port can do everything — the `Origin` check constrains *browsers*, and a non-browser client
simply omits the header. This matters because `serve_host` is documented as configurable
beyond loopback (`cli/src/config.rs`; the Tailscale/LAN case is called out in `display_addr`).
On a shared or untrusted network that is a full remote-control surface: close workspaces,
type into live agent panes, approve or cancel specs.

### Fix ideas

1. A bearer token in the data dir, required on every write and handed to the page at load.
   Cheap, and makes a non-loopback bind honest.
2. Refuse to bind a non-loopback host unless such a token is configured — the bind guard
   already sketched in the `mcp-endpoint` run's spec.
3. (2) is the smaller change and prevents the dangerous configuration outright; (1) is what
   would make serving across a tailnet actually usable.

## `serve --port 80` locks the reviewer out of every write button

Found 2026-07-26 during review of the archive button. Not fixed — narrow, and it fails closed.

`allowed_hosts_for` (`cli/src/review.rs`) always builds its candidates as `"{host}:{port}"`.
Browsers omit the port from the `Host` header, and from `location.origin`, when it is the
scheme's default — 80 for plain HTTP, which is all this server speaks. Bind with `--port 80`
(needs root or `setcap`, so this is unusual but not impossible for a memorable local URL) and
every browser request arrives as `Host: <host>` with no `:80`, matching no allowed host. Every
POST 403s, including from the server's own same-origin page. `wildcard_ip_host` does not
rescue it: a portless `Host` never matches there either.

Fix shape: when the bound port is 80, also accept the bare host. Left undone because the
failure is loud, immediate, and safe — nothing is exposed, the buttons simply stop working.

## `save_preserving_archived` rescues one field, and only that field

Found 2026-07-26. Working as designed; recorded so the limit is not mistaken for a guarantee.

`RunState::save_preserving_archived` (`cli/src/run.rs`) re-reads `archived` from disk and
carries it forward, so a command holding a long-stale state cannot un-archive a run. Every
*other* field is still written from the snapshot the caller loaded, which for
`code-review run` or `phase wait` can be the full timeout ago. A concurrent writer touching
`phases`, `cursor`, `workspace` or `root_pane` in that window is still silently lost. Only
`archived` is rescued because it is the one field a *different* process sets while we hold
our copy; the general fix is the compare-and-swap or lockfile already proposed above for the
`state.json` clobber window.

Two consequences worth knowing:

1. The `|=` merge (rescue false→true, never true→false) is deliberately defensive but
   currently unreachable: every caller reaching it has `archived == false` in memory, because
   `code_review_run` refuses archived runs up front. Changing it to `=` passes the whole
   suite. It is kept as `|=` because a future writer that legitimately holds `true` should
   not have it cleared, but no test defends that and none can until such a writer exists.
2. The re-read swallows a load error (`if let Ok(disk)`), then `save` does `create_dir_all`.
   If `drovr cleanup --purge` deletes the run directory while a review is blocked, the
   eventual save recreates a `state.json` for a run the human explicitly deleted. This
   predates the change — plain `save` always did this — but it is now reachable from two more
   writers.

## Archive/restore failures are reported only to the browser console

Found 2026-07-26. Not fixed.

`toggleArchive` (`cli/web/index.html`) returns silently on both failure paths — a non-OK
response and a thrown fetch — logging only via `console.error`. The button is not disabled or
spun while the request is in flight either, so "still working", "failed", and "nothing
happened" are indistinguishable to the reviewer. Reachable today: archiving a run whose
`state.json` does not parse answers 409, and a run deleted concurrently answers 404. The
`workspace_closed: false` case is the one failure that does speak up, via an alert.

Fix shape: an inline error on the row, or reuse of the alert path. Left undone because it is
UI work with no failing behaviour behind it, and this branch was already several rounds deep
in cursor correctness.

## `code-review run` only checks `archived` at entry

Found 2026-07-26. Deliberate, and narrow; recorded because it is not obvious.

`code_review_run` refuses to start against an archived run, but never re-checks. If the human
archives mid-review AND the workspace close fails (the zombie case, so the reviewer panes are
still alive), the review keeps going: it harvests findings and flips `review_phases` to Done
on a run the UI shows as filed away. Nothing is corrupted and `archived` itself survives (see
the preserving-save entry), but work continues on a run the human believes they stopped.

A mid-run re-check would need to decide what to do with reviewers already in flight, which is
a bigger question than this branch should answer.

## `cleanup --purge` can leave a run with a destroyed workspace and `archived: false`

Found 2026-07-26. Pre-existing, not introduced here.

`cmd_cleanup` sets `archived: true` only on the non-purge path. `--purge` closes the workspace
and then deletes the run directory — so if that delete fails (permissions, a busy file), the
run is left on disk with its workspace destroyed and `archived` still false. In `/api/runs`
that is indistinguishable from a normal idle run: `live: false`, `archived: false`, and not a
zombie, since zombie requires `archived == true`.

Worth noting because the liveness/zombie machinery this branch added exists to surface exactly
this class of mismatch, and this is the one shape it does not reach.

## The `afterSeq` guard on the archive hand-off is defence in depth, not load-bearing

Found 2026-07-26. Deliberate; recorded so the missing coverage is not read as an oversight.

`renderRunList` resolves a pending archive hand-off only when `seq > pendingAdvance.afterSeq`,
so that a render whose list was fetched *before* the archive committed cannot answer "did the
row leave". Removing that condition does not fail any test, and cannot be made to: `seq` is
bumped at render dispatch, and `toggleArchive` dispatches a render immediately after setting
`pendingAdvance`. Every older in-flight render is therefore already stale and bails at the
staleness guard without reaching the resolution point.

It is kept because the redundancy depends on `toggleArchive` continuing to render right after
setting the flag. If that call is ever moved or removed, the guard is the only thing stopping a
pre-archive render from answering with stale rows — which strands the cursor exactly as the
one-shot version did. Do not "simplify" it away, and do not add a test that pretends to cover
it without first building a seam that can dispatch a paint from an older render.

## `web_nav` has shown a rare, uncharacterised flake

Observed 2026-07-26. NOT fixed, and not fully diagnosed — recorded so the next person to see
it does not assume it is new.

`cli/tests/web/nav.mjs` shares one page across sections, and several sections trigger actions
(`press('a')`, clicking Archive) without awaiting the internal promise chain those actions
start. A later section can therefore be measuring the cursor while an earlier action's render
is still landing. One instance of this was found and fixed — a check asserting a row had left
immediately after dispatching a render that can lose the staleness race to `toggleArchive`'s
own; it now waits for the state it asserts.

After that fix the suite ran 54 consecutive times green. But two failures were seen in the
first 12-run batch after it, and their output was not captured, so they remain unexplained.
Do not read "54 green" as proof the class is gone.

If it recurs: run with `-- --nocapture` to get the failing check name, and suspect a section
asserting immediately after `evaluate('renderRunList(...)')` rather than waiting for the
condition. The durable fix is to make every such section either `reload()` first or wait on
the state it is about to assert, rather than trusting a render to have painted.

## Three of the six `save_preserving_archived` sites are redundant, and untestable

Found 2026-07-26. Working as intended; recorded so nobody "fixes" the missing coverage.

Six writers now call `save_preserving_archived`: `phase_start`, `spawn_reviewer`, `phase_wait`,
`code_review_run`'s deadline and final saves, and `cmd_code_review`'s. Mutating any of the
first three back to a plain `save` fails the suite. The last three cannot be caught, and the
reason is structural rather than a coverage gap:

`code_review_run`'s poll loop makes NO herdr calls — it polls marker files on disk — and every
`agent_status` call happens inside `spawn_reviewer`'s readiness wait, i.e. before that
function's own save. So there is no point in the run where an archive can land *after* the
last spawn save but *before* the deadline save. Any archive that reaches those later writers
was already rescued into memory by `spawn_reviewer`, which means a plain `save` there would
write the correct value anyway.

They are kept preserving for consistency — a future writer that saves without spawning first
would need it, and the asymmetry would be a trap. But do not add a test claiming to cover
them without first building a seam that can actually trigger it; a test named for a path it
does not exercise is worse than no test. One was written during this review and deleted for
exactly that reason.

## A panicking test can poison `ENV_LOCK` for the whole suite

Found 2026-07-26. Pre-existing, not fixed.

`test_util::ENV_LOCK` serialises tests that mutate process-global env. Almost every consumer
takes it with `.lock().unwrap()`, so the *first* test that panics while holding it poisons the
mutex and every later consumer panics on acquisition — one real failure becomes ~90
misleading ones across unrelated modules, which makes root-causing nearly impossible.

Fix shape: `.lock().unwrap_or_else(|e| e.into_inner())` at every consumer. The `herdr.rs`
helpers already do this. Not done wholesale here because it is a sweep across five files
unrelated to this change.

Related: the `SystemHerdr::with_bin` seam exists precisely so herdr's own tests need no env
mutation at all — injecting the binary path beats locking around a global. Prefer that shape
for new tests rather than adding another `ENV_LOCK` consumer.

## Resolved

- **`drovr phase compress` regurgitates the seed instead of the phase's artifact**
  (found 2026-07-24, run `gpu-deploy-view`; resolved by 2026-07-25). Obsolete: there is no
  `drovr phase compress` command any more — `PhaseCmd` is only `start`/`send`/`wait`/`done`
  (`cli/src/main.rs:122-151`), and no `Compress` variant exists anywhere in `cli/src/`.
  Removing the separate compress step *was* the fix: the finishing agent now authors its own
  `<phase>-HANDOFF.md` from its own context, and `drovr phase done` refuses for a pipeline
  phase until that file exists and is non-empty (`cli/src/phase.rs:391-412`;
  `skills/handoff/SKILL.md:55-56, 138`). Nothing compresses a transcript, so the
  over-weight-the-visible-briefing failure mode cannot recur. Do not re-file this against the
  handoff flow — a bad *self-authored* handoff is a different bug with a different cause.
