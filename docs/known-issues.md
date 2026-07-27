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
bug — it is the unsubmitted-prompt failure documented in the next section. All four cursor
reviewer panes launched, attached, and received their seed, but the brief sits in the composer as
`→ [Pasted text #1 +46 lines]`, never submitted. The agents therefore never start, never reach
`done`, and `code-review run` times out with no `<task>-review.json` — exactly as reported.

Reading a reviewer pane (`herdr agent read <pane>`) shows the full seed rendered in the composer
with the correct `base..head` scope, so seeding and scope selection are fine; only the submit
keystroke is missing. Fixing "`drovr phase send` returns success with the prompt left
unsubmitted" (below) fixes the panel too — they are one bug, and the panel is simply its most
visible victim. Keep the self-spawned-reviewer workaround above until that lands.

## `drovr phase send` returns success with the prompt left unsubmitted

**Severity:** high — an unattended pipeline stalls silently at every phase injection. (Filed as
`low` originally on the grounds that it is recoverable; that undersold it. Recovery requires a
human noticing that nothing is happening, and the failure is indistinguishable from an agent
that is simply working.)
**Found:** 2026-07-24, run `gpu-deploy-view`, every phase injection — including on the updated
binary carrying the phase-send agent-readiness fix.
**Reproduced:** 2026-07-25 (`mcp-endpoint`), 2026-07-26 (`skill-stickiness`, three times). See
"Occurrences".

### Symptom

`drovr phase send <run> <phase> "<text>"` exits `0` with no stderr. The text reaches the agent's
composer but is **never submitted** — it sits at the `❯` prompt, cost `$0.00`. The agent is idle
and unaware. `phase wait` runs to its full timeout, and any watch keyed on the work the message
asked for stays correctly silent, because nothing happened.

Two distinct renderings, depending on payload:

- large payloads appear as a collapsed bracketed paste — `❯ [Pasted text #1 +NN lines]`;
- small payloads appear as ordinary inline wrapped text.

Both fail the same way. There is also a rarer third mode where the send lands **nothing at all**
and the composer stays empty (see Occurrences, `mcp-endpoint` case 1) — the payload is dropped
outright while the command still reports success.

### Root cause — not established

Unknown. Two plausible-sounding explanations have been **ruled out** by evidence; do not fix
against either.

- **Not payload size, and not a bracketed-paste commit failure.** Three sends of a few hundred
  bytes each failed on 2026-07-26, none rendering as a paste. Whatever fails, fails for inline
  text too.
- **Not a stale herdr-version assumption.** `cli/src/herdr.rs:265-271` issues the socket call
  `agent.prompt`, documented to type *and* submit natively, which is why the 0.7.3 flush-CR
  handshake was removed. herdr was 0.7.5 during the 2026-07-26 failures, so the version premise
  held and the submit still did not happen.

One unconfirmed contributor: in the first 2026-07-26 case the target had been failing tool calls
against a degraded classifier and had parked itself, with the TUI showing a `new task? /clear to
save …` hint. A readiness probe reporting "ready" for an agent parked mid-error would explain
both the exit `0` and the swallowed submit. The other two cases had no such state, so it is at
most partial.

### Workaround

Treat exit `0` as "text reached the composer", never as "the agent received it". Follow **every**
send — large or small, paste or inline — with an explicit submit, then verify:

```sh
drovr phase send "$RUN" "$PHASE" "$TEXT"
sleep 2
herdr pane send-keys "$PANE" Enter                    # pane_id is in the run's state.json
herdr pane read "$PANE" --source recent --lines 12    # confirm the composer cleared
```

A redundant `Enter` on an already-submitted message is harmless — it lands on an empty prompt.

**A follow-up empty `phase send` does not work.** `drovr phase send <run> <phase> ""` is rejected
with `drovr: phase send failed: agent prompt must not be empty`. If you are carrying that as a
remembered workaround, drop it; `herdr pane send-keys` is the only thing that submits.

**Sending a short pointer instead of a large briefing does not avoid this bug** — it was tried
and failed (Occurrences, 2026-07-26 case 3). The write-to-a-file-and-send-a-pointer pattern is
still worth using, but for an unrelated reason: the agent can re-read the file if its context
compacts mid-task. It is not a mitigation for this issue and must still be followed by an
explicit submit.

**Never read a quiet watch as progress.** Silence is equally consistent with "working", "never
started", and "dead". When a watch has been quiet longer than the work plausibly takes, read the
pane — that is the only thing that distinguishes them. This bug is invisible from the outside;
it was caught both times only by reading the pane directly.

### Occurrences

**2026-07-25, run `mcp-endpoint`, pane `wAC:p1`** — installed nix-profile binary, 6586-byte /
124-line briefing:

1. The first `drovr phase send` landed **nothing at all** — composer empty at `$0.00`. That is
   the readiness race described under "`drovr code-review run` panel never completes" reaching
   the `phase send` CLI path, not just the reviewer-spawn path: success reported, payload
   dropped.
2. A second, identical send landed as a collapsed paste: `❯ [Pasted text #1 +124 lines]`,
   `$0.00`, unsubmitted.
3. `herdr agent send-keys wAC:p1 Enter` submitted it.

**2026-07-26, run `skill-stickiness`, panes `wAG:p1` / `wAG:p2`** — herdr 0.7.5. Three sends,
all small, none rendering as a paste, all unsubmitted until an explicit `Enter`:

1. `wAG:p1`, ~300 bytes — "GATE APPROVED … Read `<path>` … then run `drovr phase done`".
2. `wAG:p1`, ~430 bytes — a one-paragraph correction.
3. `wAG:p2`, ~400 bytes — the plan phase's pointer injection, i.e. already using the
   short-pointer pattern.

So the failure spans at least two orders of magnitude of payload size and both composer
renderings.

### Fix ideas

1. **Verify submission rather than assuming it.** After `agent.prompt`, poll for a bounded
   interval and confirm the composer cleared / the agent moved to `working`. Exit non-zero with a
   distinct code if the text is still sitting there. This covers the drop mode too — checking
   that the composer is non-empty *before* submitting is what distinguishes a drop from a
   non-submit.
2. **Re-issue the submit as a fallback** — not the unconditional 0.7.3 handshake, but a single
   `Enter` sent only when step 1 detects the prompt was not consumed, retried until the input
   clears.
3. **Harden `wait_agent_ready`.** If an agent parked after an error reports ready, readiness is
   measuring the wrong thing; it should distinguish "idle and accepting input" from "idle because
   it gave up".

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

`cmd_cleanup` (`cli/src/main.rs`) now writes `state.json` to set `archived: true`. `RunState::save`
(`cli/src/run.rs`) is a whole-file `fs::write` with no locking, no read-modify-write and no
version check, so a `drovr phase ...` running in a still-live pane can have its status write
silently reverted.

Before the archived flag existed the non-purge cleanup path never wrote `state.json` at all, so
this window is new — it is a real (if small) regression introduced alongside the fix.

### Why it is small

The write was deliberately placed immediately after the pane teardown (`close_run_panes`, which
closes every pane the run recorded), and it re-reads `state.json` from disk rather than saving the
copy loaded at the top of the function. The race therefore needs a phase agent to write during
that teardown itself, after which it no longer exists.

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

## Piping a `wait` command destroys its exit-code contract — a timeout reads as approval

**Severity:** high (the failure is silent and points the wrong way: a *timeout* is
indistinguishable from an *approval*, so an unapproved spec can walk straight into the implement
phase — the exact outcome the gate exists to prevent).
**Found:** 2026-07-25, run `skill-stickiness`, brainstorm spec gate.

### Symptom

The driver backgrounded the gate watch as:

```
drovr review wait skill-stickiness 2>&1 | tail -5
```

The harness reported **exit code 0** — which `drovr:pipeline` defines as *approved*. The command's
actual output was `review: no reviewer action for run 'skill-stickiness' within timeout (re-run to
resume)`, i.e. a **timeout (exit 2)**. On-disk state confirmed no decision: `review.state.json`
still `{"state":"ready"}`, no `approved` marker, no `feedback.json`.

### Root cause

A shell pipeline's exit status is the status of its **last** command. `tail` succeeds, so the
pipeline exits 0 regardless of what `drovr review wait` returned. Both `drovr:pipeline` ("The spec
gate" → exit-code table) and `drovr:handoff` (step 3 → exit-code table) define precise exit-code
contracts for `review wait`, `phase wait`, and `code-review run`, and **neither warns that piping
the command destroys the contract**. Adding `| tail`, `| head`, `| grep`, or `| jq` to trim output
is a natural thing to do and silently voids every one of those tables.

This is the inverse of the danger the skill already names. `drovr:pipeline` warns "Only exit 0 is
approval. A non-zero exit is never an approval" — the observed failure is an **exit 0 that is not
an approval**, which no existing guidance covers.

### Workaround

Never pipe a command whose exit code you depend on. Capture it explicitly:

```
drovr review wait <run>; rc=$?; echo "EXIT=$rc"; exit $rc
```

This preserves the real status for the harness *and* records it in the output. Independently,
**verify against on-disk state before acting on an approval** — `approved`/`cancelled` markers and
`review.state.json` are the source of truth; the exit code is a convenience.

### Fix ideas

1. Add a red-flag row to `drovr:pipeline` and `drovr:handoff`: *"Piping `wait`/`code-review run`
   → the pipeline's exit status is the last command's; use `cmd; rc=$?` instead."* Cheapest fix,
   and it belongs next to the exit-code tables that create the expectation.
2. Have `review wait` / `phase wait` write their outcome to a marker file in the run dir as well as
   returning it, so a lost exit code is recoverable rather than fatal.
3. Consider making the approval path require the on-disk `approved` marker, so that no exit-code
   mishap alone can advance a gated run.

## `review.state.json` state is sticky — polling it detects a condition, not a transition

**Severity:** medium (a driver that polls for `state == "ready"` fires immediately on a
*previous* revision and reports a revision that has not happened).
**Found:** 2026-07-25, run `skill-stickiness`, while watching the gate for a post-review revision.

### Symptom

The driver armed a watch that fired when `review.state.json` reported `state: "ready"`, intending
"the agent posted a new revision". It fired at once and reported a revision that did not exist:
`spec.md`'s mtime predated the feedback file the agent was supposed to be acting on, and the
agent was still mid-work.

### Root cause

`ready` is a **resting state**, not an edge. It is set by `drovr review summary` and persists
until the reviewer acts. After any earlier revision the run sits in `ready` indefinitely, so a
predicate of the form `state == "ready"` is true continuously — it says *"a revision is available
for review"*, never *"a new revision just arrived"*.

A second bug in the same watch is worth recording because it fails silently in the dangerous
direction: the turn threshold was hardcoded (`turn > 4`) while `feedback.json` was at turn 3, so
the reviewer's *next* decision (turn 4) would never have matched and the watch would have waited
forever while the human had already acted.

### Workaround

Watch **mtimes**, not state. Capture `stat -c %Y` for `summary.txt` and `spec.md` at arm time and
fire when they increase; derive any turn threshold from `feedback.json` at arm time rather than
hardcoding it. A useful extra alarm: if `summary.txt` is re-posted while `spec.md` is unchanged,
the agent has claimed work it did not do.

### Fix ideas

1. Add a monotonically increasing `revision` counter (or a `last_summary_at` timestamp) to
   `review.state.json`, so watchers have an edge to trigger on.
2. Document in `drovr:pipeline` that `state` is a resting value and that `drovr review wait` — not
   a hand-rolled state poll — is the sanctioned way to detect a decision.

## The review server binds to the configured host, so the documented `127.0.0.1` URL can fail

**Severity:** low (cosmetic for a human who can read the bind address, but it silently breaks any
scripted `localhost` poll).
**Found:** 2026-07-25, run `skill-stickiness`.

### Symptom

`drovr:pipeline` documents the run's page as `http://127.0.0.1:8791/#/runs/<run>` and the state
endpoint as `/api/runs/<run>/state`. On this machine the server was listening on the Tailscale
address (`100.71.58.39:8791`), so:

- `curl 127.0.0.1:8791/...` returned **empty** — a scripted poll for `"ready"` never matched and
  silently ran to timeout.
- On the correct host, `/` and `/#/runs/<run>` returned **200**, but `/api/runs` and
  `/api/runs/<run>/state` returned **404**.

### Root cause

Partly configuration (the server was bound to a Tailscale host rather than loopback, which the
skill explicitly supports via `drovr serve --host <tailscale-host>`) — the skill just hardcodes
`127.0.0.1` in the URL it tells the driver to hand the human.

**The 404s are not diagnosed.** The correct API path was not determined; it may simply differ from
what the skill documents, or the endpoint may be versioned differently. Do not treat "the API path
is wrong" as established — that needs checking against `cli/src/` before anyone acts on it.

### Workaround

Read the actual bind address (`ss -ltnp | grep 8791`) rather than assuming loopback, and prefer the
on-disk markers (`review.state.json`, `approved`, `cancelled`, `feedback.json`) over HTTP for any
programmatic check. They are the source of truth and need no network.

### Fix ideas

1. Have `drovr review summary` print the URL using the address the server actually bound to (it
   already prints the reviewer URL — it should be the *real* one).
2. Confirm the correct `/api/...` path and fix either the server or the skill's documentation.

## Upstream (not a drovr bug): context-percentage readouts are computed against 200k

**Severity:** informational — recorded because it distorts drovr's primary escalation signal, not
because drovr should change.
**Found:** 2026-07-25, run `skill-stickiness`, on `claude-opus-5`.

### Symptom

The statusline reported `ctx:83%` when the session held 165,258 tokens. 165,258 / 200,000 = 82.6%
— an exact match, so the denominator is 200k. On a model with a 1M context window the true
fullness was 16.5%, i.e. readings are inflated roughly 5×.

More consequential than the display: the harness's **auto-compaction trigger uses the same
number**, so an agent is compacted at ~200k regardless of the model's real capacity. The
practical ceiling therefore *is* ~200k in behaviour even though the displayed percentage is wrong.

### Why it is recorded here

`drovr:using-drovr`'s escalation contract names **context fullness as the primary signal** for
escalating a task into its own phase. An inflated reading pushes drovr to escalate far earlier
than warranted — chopping work that would fit comfortably in one context, which inverts the
project's value. During this run it nearly triggered an unnecessary mid-flight handoff at a
displayed 63% (true fullness ~13%).

### Status — do not design around this

This is an upstream harness bug that is expected to be fixed, and the maintainer's explicit
instruction was **not to change any drovr skill because of it**. No drovr change is warranted.
Recorded only so that a reading taken before the upstream fix is not mistaken for a drovr defect,
and so the interaction with the escalation contract is on record.

Until it is fixed: when a context reading would actually change a decision, read real token counts
from the session transcript (`~/.claude/projects/<munged-cwd>/<session>.jsonl`, summing
`input_tokens + cache_read_input_tokens + cache_creation_input_tokens` on the last `usage` entry)
rather than trusting the percentage.

## A stale `server.addr` plus an occupied port deadlocks server discovery permanently

**Severity:** high — `drovr review summary` / `review wait` fail with no path to recovery, so a
run's gate cannot be opened at all.
**Found:** 2026-07-26, run `skill-stickiness`.

### Symptom

Every `drovr review summary` fails with `timed out waiting for `drovr serve` to start`, while a
perfectly healthy review server is running and reachable the whole time. Opening the URL a
previous `summary` printed gives a connection refused — the human reads this as "the server isn't
live" when in fact a server *is* live, just not the one drovr is looking for.

Observed state during the incident:

```
~/.local/share/drovr/server.addr  ->  127.0.0.1:18732   (written 2026-07-25 20:37:46)
~/.local/share/drovr/server.pid   ->  1662301           (process DEAD)
actual live server                ->  100.71.58.39:8791 (pid 1289722, serving every run fine)
```

### Root cause

Three mechanisms compose into a trap. Each is individually reasonable.

1. **`server.addr` is a single global last-writer-wins pointer.** `serve()`
   (`cli/src/review.rs:1052-1053`) writes `server.addr`/`server.pid` unconditionally right after
   binding. Every drovr binary on the machine shares one `~/.local/share/drovr/`, so **dev builds
   from other worktrees overwrite the pointer for everyone.** This repo routinely has 10+
   worktrees live, several running their own `serve` on their own port — so the pointer churns.
2. **A writer that exits leaves the pointer dangling.** Nothing clears `server.addr` on shutdown.
   The last binary to start wins the pointer, and when it dies the pointer survives it, now naming
   a dead port.
3. **The recovery path cannot recover, because it has no port fallback.** `ensure_server()`
   (`:1090-1112`) correctly detects the dead pointer — `live_server_addr()` connect-tests it and
   returns `None` — and calls `spawn_daemon()`. But `spawn_daemon()` (`:1206-1221`) shells a bare
   `drovr serve` with **no `--port`**, so the child always tries the default `8791` on the config
   `serve_host`. That address is already held by the live server. The child dies instantly,
   `server.addr` is never updated, and `ensure_server` polls a dead pointer for 5s and errors.

The deadlock is stable: it recurs on every invocation and cannot self-heal, because the very
condition that makes discovery fail (a live server on the port) is also what makes the fix
attempt fail. Note the healthy-looking failure — the server is *up*, the runs are *fine*, and the
error message points at startup, which is the one thing that is not the problem.

### Workaround

Point the file at the server that is actually running:

```sh
# find the live server and its bound address
pgrep -af 'drovr serve'
# then, with its real host:port
printf '%s' '100.71.58.39:8791' > ~/.local/share/drovr/server.addr
```

Do **not** kill the dev-build servers to "clean up" — they belong to other worktrees and other
people's sessions. Repointing the file is sufficient and non-destructive.

To confirm before and after: `curl -s -m2 http://$(cat ~/.local/share/drovr/server.addr)/api/runs`
should return a JSON array of runs. An empty `[]` means you have found a server pointed at a
*different data dir* (another worktree's dev build) — that is a different, equally misleading
failure: discovery succeeds, and the UI shows no runs.

### Fix ideas

1. **Give `spawn_daemon` a port fallback.** If the configured port is occupied, bind `:0`, let the
   OS choose, and record the real bound address. This alone breaks the deadlock.
2. **Validate before trusting, and self-heal.** `live_server_addr()` already connect-tests. Extend
   it to also confirm the responder is a drovr server *for this data dir* (a `/api/health`
   returning the runs root) — that catches the empty-`[]` cross-worktree case too.
3. **Do not let dev builds clobber the shared pointer.** Namespace the discovery files by data dir,
   or have non-default `--port`/`--host` invocations write a per-instance file instead of the
   global one. A `serve` on a non-default port is almost by definition not the one to advertise.
4. **Clear `server.pid`/`server.addr` on clean shutdown**, and treat a dead `server.pid` as
   grounds to ignore `server.addr` without waiting for the TCP timeout.

## A finished phase reports `running` forever unless the driver happens to run `phase wait`

**Severity:** high — every read-only view of the run lies about its state, and `drovr status`
actively instructs you to resume a phase that already finished.
**Found:** 2026-07-26, run `skill-stickiness`.

### Symptom

The phase agent ran `drovr phase done <run> brainstorm` successfully. The marker
`~/.local/share/drovr/runs/<run>/brainstorm.done` exists. And yet:

```
$ drovr status skill-stickiness
  [ 0] brainstorm      running <-- resume
  [ 1] plan            pending
resume at phase 0: brainstorm
```

The phase had been complete for some time. `drovr list` and the review web UI agree with
`status`, because they read the same field. There is no indication anywhere that the run is
ready to advance — and the one line that looks like guidance (`resume at phase 0`) is wrong.

### Root cause

`phase done` deliberately writes only a marker file and never mutates `state.json` — by design,
so the orchestrator stays the sole writer of run state (`cli/src/phase.rs:377-382`). The
reconciliation from marker to `PhaseStatus::Done` happens in exactly **one** place:

```rust
// cli/src/phase.rs:466-471  — inside phase_wait's poll loop
if marker.exists() {
    run.phases[idx].status = PhaseStatus::Done;
    run.save()?;
    return Ok(PhaseWaitOutcome::Done);
}
```

So `state.json` only catches up **if the driver runs `drovr phase wait` for that phase**. Any
path that skips it strands the run:

- the driver drove the phase by hand (as here — the spec gate was managed directly, and the
  brainstorm phase never got a `phase wait`);
- the driver's context was compacted or its session ended, and the resumed driver did not know
  a wait was owed;
- the wait was run, returned `Blocked` or `TimedOut`, and was never re-run.

Everything downstream reads the stale field: `cmd_status` (`cli/src/main.rs:436-454`) prints
`p.status` verbatim and derives `<-- resume` from `first_incomplete()` (`cli/src/run.rs:144`),
which is itself `status`-based. `review.rs:715`'s `status_str` feeds the web UI the same value.
None of them consult the marker that is sitting right next to `state.json` in the same
directory.

The failure is silent and stable: nothing times out, nothing errors, and the run simply never
advances.

### Consequence for orchestration

**Do not write a watch keyed on `state.json` phase status.** It is a field only the driver can
change, so a driver waiting on it is waiting on itself — the watch can never fire. This cost a
long stall in the run where it was found: a monitor polled `phases[0].status` while the phase
had already dropped its marker.

The completion signal is the marker file, and only the marker file:

```sh
ls ~/.local/share/drovr/runs/<run>/<phase>.done
```

### Workaround

Check the markers, not the status, whenever you need ground truth:

```sh
ls ~/.local/share/drovr/runs/<run>/*.done
```

To repair a stranded `state.json`, run the wait that was skipped — it reconciles immediately
and returns, because the marker is already there:

```sh
drovr phase wait <run> <phase> --timeout-ms 5000
```

### Fix ideas

1. **Make the read-only views marker-aware.** `cmd_status`, `drovr list` and `status_str`
   should treat "marker present" as done regardless of `state.json`, so a stranded run is at
   worst a cosmetic lag and never a wrong instruction. This is the cheap fix and it removes the
   misleading `<-- resume`.
2. **Reconcile on load.** Have `load_run` (or `RunState::first_incomplete`) sweep for `.done`
   markers and promote statuses, so any command touching the run heals it. Keeps the
   sole-writer intent — the reconciliation still happens in drovr, not in the agent.
3. **Surface the discrepancy loudly** if 1 and 2 are both rejected: `drovr status` should print
   something like `marker present, state not reconciled — run: drovr phase wait <run> <phase>`
   rather than silently reporting `running`.
4. **Document the invariant** in `drovr:pipeline`: every phase needs its `phase wait`, including
   ones whose completion the driver observed by other means. The skill's flow implies this but
   never says that skipping the wait corrupts run state.

## `drovr cleanup` can leave an empty workspace behind when herdr cannot list its panes

**Severity:** low (cosmetic — an empty workspace in the switcher, closable by hand).
**Found:** 2026-07-26, while making cleanup reap only drovr's own panes.

### Symptom

`close_run_panes` (`cli/src/main.rs`) decides whether it may call `workspace_close` by diffing
`pane.list` for the run's workspace against the panes the run recorded. If that listing fails —
daemon blip, changed result shape — it cannot prove the workspace holds nothing of the human's, so
it closes only the recorded panes and leaves the workspace open. The workspace may then be empty
but still listed.

### Why it is deliberate

The alternative is closing the workspace on an answer we do not have, which is exactly how the
human's own tabs used to die. An empty workspace is a cosmetic mistake; a closed pane holding
someone's unsaved work is not. Same reasoning for a pane drovr created but never recorded in
`state.json` (see `RunState::retired_panes`): unrecorded panes are treated as the human's and left
running.

### Fix ideas

1. Retry `pane.list` a couple of times before giving up — most failures here are transient.
2. Or ask herdr whether the workspace is empty after the pane closes (`workspace.get`
   `pane_count`) and close it only on a definitive zero.

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

## Two `drovr serve` daemons can still slip past the single-server guard

**Severity:** low (the ordinary duplicate — a second `drovr serve` on any port, or several
racing at once — is refused; see `cli/tests/serve_single.rs`).
**Found:** 2026-07-26, while adding the guard on `drovr/single-server`. The prompting incident:
`~/.local/share/drovr/server.pid` named a **dead** pid (1662301) while a live server was serving
on `100.71.58.39:8791` as pid 1289722 — i.e. two servers had run, and one had died, leaving
discovery pointing at neither.

### How the guard works

`drovr serve` takes an advisory exclusive lock on `server.pid` (`acquire_pid_lock` /
`try_take_lock` → `File::try_lock`, i.e. `flock`) and refuses to start if another process holds
it. The kernel holds that lock for the server's lifetime and releases it however the process
dies, so a crashed server never leaves a claim anyone has to judge stale.

That lock is the *only* check. `server.addr` is read solely to put a URL in the refusal message.

### The gaps

- **A server that holds no lock is invisible.** Two ways to get one: a `drovr serve` from a build
  older than this guard, or a current one whose `server.pid` was deleted while it ran (`flock` is
  on the inode, not the path, so a later start creates a fresh inode there and locks it happily).
  Either way the next `drovr serve` starts, and discovery moves to it. During an upgrade this is
  guaranteed, not unlucky: **restart the server after upgrading drovr**, or the first new-build
  start will duplicate the running old one.
- **A data dir on a filesystem where `flock` is not enforced** (some NFS mounts / `nolock`) has no
  protection at all. drovr assumes a local data dir.

### Fix ideas

- Re-check after taking the lock that the file we hold is still the one at the path (compare
  inode) and refuse if it is not — closes the delete-while-held case in one direction.
- Ask `server.addr` whether a drovr server answers there as a second signal. This existed and
  was removed deliberately: it made a start's outcome depend on a *stale* file plus a network
  probe, which mistook unrelated services for drovr and needed a "delete `server.addr`" escape
  hatch that could itself cause the split brain. Any second signal needs to identify *which*
  server answered (e.g. a per-server nonce in the response and in a discovery file), not just
  that something did.
