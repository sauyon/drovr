# Known issues

## Reviewers judge an intermediate task against the WHOLE run's goal

**Severity:** medium (every intermediate task of every multi-task run draws a spurious CRITICAL, and
each one costs a review round to adjudicate).
**Found:** 2026-07-25, run `phase-reap` task-2 iteration 5.

### Symptom

On a multi-task run, `drovr code-review run <run> <task>` returns a CRITICAL finding of the form
"task behavior is not implemented" for work the plan deliberately schedules in a *later* task.

Observed on run `phase-reap`, `task-2`, iteration 5: task 2 adds herdr capability only — the plan
says "Nothing is ever closed until task 6" — and the correctness reviewer reported the absence of
reaping and of `--resume` rehydration as a critical defect of that diff.

### Root cause

`build_seed` (`cli/src/code_review.rs`) seeds every reviewer with `run.task`, the run's overall goal
("Reap finished phase panes, with rehydrate-in-the-UI"). The reviewer never sees the per-task brief
that bounds the diff, so it measures an intermediate diff against the finished feature and correctly
observes that most of the feature is missing.

### Impact

It fires on every intermediate task of every multi-task run, and it is expensive: a spurious CRITICAL
costs a review round to adjudicate, and the driver must recognise it as a scope artifact rather than
route it as a fix. It also crowds out real findings in the same angle.

### Fix idea

Seed the reviewer with the per-task brief instead of `run.task` — or pass both, and state explicitly
which one bounds the diff's scope.

## `phase wait` times out on a phase completed by a PRE-pass-token drovr build

Introduced by the pass-token change (task 1 of the phase-reap work).

### Symptom

`drovr phase wait <run> <phase>` exits 2 (timeout) for a phase whose `state.json` already says
`"status": "Done"`, and the run dir has no `<phase>.done` marker.

### Root cause

The build before pass tokens CONSUMED the completion marker when a wait accepted it, and relied on
a `status == Done` short-circuit to make a re-wait idempotent. That short-circuit is gone: a stale
`Done` on disk is no longer accepted as evidence, because every "marker destroyed but state write
did not land" failure produced exactly that state and would have been reported as a false
completion. The verdict now derives solely from the marker plus its pass token.

So a phase completed by the older binary has `Done` recorded but no marker left to prove it, and
the new binary honestly reports that it has no evidence.

### Working around it

Only affects a phase that was already `Done` before the upgrade AND is waited on again. The normal
flow self-heals: `drovr phase send <run> <phase> "<instructions>"` re-opens the phase (clearing the
stale status), and the live agent's next `drovr phase done` completes it normally.

To accept the old completion as-is, re-signal it deliberately from the run dir:

```
: > "$(drovr path <run>)/<phase>.done"      # or: touch <run_dir>/<phase>.done
```

An empty marker is accepted for a phase with no recorded `pass`, which is exactly the pre-token
case.

## Review UI presents one run's state as another's (spec, feedback, annotations, findings) — FIXED 2026-07-25

**Status:** fixed on `drovr/review-ui-stale-doc`. `refresh()` now clears and hides the doc
panel in the empty case, and `route()` no longer shows it. Regression checks: "a run with no
spec shows no doc at all" / "...and does not claim to be showing a spec" in
`cli/tests/web/nav.mjs`, against a new `epsilon-nospec` fixture run seeded with `state.json`
but no `spec.md`.

**Severity:** high (silent misattribution — the reviewer reads one run's plan believing it is
another's, and every other element on the page corroborates the wrong run).
**Found:** 2026-07-25, while reviewing run `phase-reap` and being shown run
`skill-stickiness`'s plan.

### Symptom

Navigate to a run whose gate has never been opened (no `spec.md`) after viewing a run that
has one, and the previous run's rendered spec stays on screen under the new run's name. The
turn badge, summary banner and questions panel all correctly update to the new run, so the
page reads as a coherent review of it — only the document body is wrong.

Verified on the live server (`100.71.58.39:8795`):

| run | `GET /doc` | `GET /state` | `spec.md` on disk |
|---|---|---|---|
| `phase-reap` | 200, **0 bytes** | `{"state":"idle","turn":0}` | absent (only `plan.md`) |
| `skill-stickiness` | 200, 24833 bytes | `{"state":"ready","turn":0}` | present, 24833 bytes |

### Root cause

Not server-side run leakage — the run is resolved per-request from the URL path
(`cli/src/review.rs:364-370`, `452`) and `spec.md` is read fresh every time, so there is no
shared "current run" anywhere in the stack. The fault is purely client-side.

`refresh()` in `cli/web/index.html` wrote `#doc-content` only when the fetched doc was
non-empty, with no `else`:

```js
if (docText) {
  docContentEl.innerHTML = renderMd(docText);
  wireAnnotations(docContentEl, docText.split('\n'));
}
```

Meanwhile `route()` unconditionally did `showEl('doc-panel')` on entering any run detail
view. A run has no `spec.md` until its first `drovr review summary`, at which point `/doc`
answers 200-with-an-empty-body (`cli/src/review.rs:467-470` deliberately prefers an empty
200 over a 404) — so `docText` is `''`, the write is skipped, and the panel is shown still
holding the last run's markup.

Second-order hazard: `currentDocText` **was** assigned unconditionally, so the visible text
and the annotation source line array were desynced — annotations anchored against an empty
document while the reviewer selected lines of the stale one.

### The same bug class, one layer down: annotations could submit against the wrong run

Found by the review pass on the fix, and worse than the visible symptom. `loadAnnotations()`
had two fall-through paths that left the **previous run's** annotation map in `annotations`:
a swallowed `JSON.parse` failure (`catch (e) {}`), and a `stored.turn === turn` record whose
`annotations` field was missing. Previously those stale line comments at least rendered as
chips on the (stale) doc and could be deleted; with the doc correctly cleared, nothing renders
them — but `collectAnnotations()` still ships them in the submit payload and the server writes
them verbatim into `feedback.json` (`cli/src/review.rs:817-820`, `846-853`, which gates submit
on `is_terminal()` only, not on `state == ready`). Run A's line comments would land silently in
run B's `feedback.json`, invisible to the reviewer.

Fixed by resetting `annotations = {}` at the top of `loadAnnotations()`, unconditionally,
before reading localStorage. Safe because every mutation site calls `saveAnnotations()`
immediately (`cli/web/index.html:1451`, `1475`), so localStorage is authoritative and no
in-progress comment is lost.

### Fix

1. Give the empty case an explicit branch that clears `#doc-content` and hides `doc-panel`.
2. Move panel visibility out of `route()` into `refresh()`, and have `route()` defensively
   clear + hide on navigation, so the stale doc is neither briefly visible on the way in nor
   left on screen if `refresh()` throws mid-flight.
3. `#doc-panel` now carries inline `style="display:none"` like every other refresh-owned
   panel, so it is not visible-and-empty on first paint.
4. Reset `annotations` unconditionally in `loadAnnotations()` (above).

### Three more instances of the same class, found by the review panel on the fix

All pre-existing, all "run A's state presented or submitted as run B's", all fixed on the
same branch. Listed because the pattern is the point: **any run-scoped state that is not
reset synchronously in `route()` is a cross-run leak waiting to happen.**

1. **The decision radio and feedback textarea were never reset** (`cli/web/index.html:908-915`).
   The worst of the set, and worse than the doc panel: no race needed and completely silent.
   Type feedback for run A, select a decision, navigate to run B (list, bookmark, or
   back/forward), submit — and A's prose and A's decision are written into B's
   `feedback.json`, which an autonomous agent then acts on. `submitDecision()` reads the live
   DOM (`:1773`, `:1777`); nothing in `route()` or `refresh()` ever touched those values.
   Fixed by resetting both to the markup default in `route()`'s synchronous block.

2. **`annotations` was reset too late.** `loadAnnotations()` runs *after* `refresh()`'s
   awaits, so between `route()`'s synchronous body and those fetches resolving,
   `collectAnnotations()` still returned the outgoing run's line comments while
   `api('submit')` already addressed the incoming one — and if `refresh()` rejects,
   `loadAnnotations()` never runs at all and they stay submittable indefinitely with nothing
   on screen to reveal them. Fixed by dropping them synchronously in `route()`;
   `loadAnnotations()` still restores this run's own in-progress comments from localStorage.

3. **`refreshReview()` had no `routeGen` guard** (`cli/web/index.html:1286-1317`). Every other
   async flow in the file captures `routeGen` and bails if it moved; this one did not, and it
   is called fire-and-forget from `refresh()`. Its two sequential awaits outlive a
   navigation, so a late resolution painted the previous task's findings and diff and then
   unconditionally re-showed the panel — **over the session list**, which had only hidden it
   once on the way out. Strictly worse than the original symptom. Fixed with the standard
   guard after each await.

### A regression the fix itself introduced, caught by the final review round

Worth recording because it is the natural failure mode of this kind of fix: **the cure for a
cross-run leak is a reset, and a reset in the wrong place destroys the reviewer's work.**

The resets above were added to `route()` ungated. But `route()` fires on every `hashchange`,
and `#/runs/<run>?task=<t>` is a supported URL (`reviewTask()`, `cli/web/index.html:1119`, and
the router comment at the top of the file documents it) — so browser back/forward, or opening
a task link while already on that run, re-enters `route()` with the *same* run. That silently
cleared the feedback textarea and reset the decision radio mid-edit. Feedback is persisted
nowhere, so it was simply gone, with no warning.

Note the pre-existing `if (h.run !== prevRun)` guard higher in `route()` covers only the
nav-cursor bits — it is not a general run-change gate, which is easy to misread. The resets
now sit in their own `h.run !== prevRun` block.

Two reviewers disagreed on this: one reported it Critical with a live repro, the other cleared
the same code as "reset is gated on navigation, not on poll" — true but not the point, since a
same-run navigation is still a navigation. The repro decided it.

**Two test traps hit while pinning it**, both of which made the check pass against broken code:
- Waiting on `refreshSeq` was wrong — the background `pollState` → `refresh()` loop bumps it
  for a `ready` run, so it advances before `route()` has touched the hashchange. The checks
  now wait on `routeGen`, which only `route()` increments, in the same synchronous task as the
  reset block.
- The "annotations survive" check passes with the gate removed, because `loadAnnotations()`
  restores them from localStorage. It is labelled in-place as not being what proves the gate
  works.

### Still open (follow-ups, not blocking)

- `fetchText()` (`cli/web/index.html`) collapses 204, non-OK and a genuinely empty body all to
  `''`, so a 500 on `/doc` is indistinguishable from "this run has no spec" and silently
  renders as the latter. The reviewer is told "no spec yet" when the truth is "the server
  broke".
- `/doc` is the one maybe-absent-markdown endpoint that answers **200-with-empty-body**;
  its siblings `/prior` (`cli/src/review.rs:472-478`) and `/review/diff` (`:791-797`) both
  answer `204`, the latter with an explicit comment reading *"not a misleading empty 200."*
  `/doc` should match. Given `fetchText`'s current collapsing this is a consistency fix, not
  a behavioral one — but the conflation is the root design flaw behind this whole entry.
- `agents-panel` / `session-panel` are shown by `route()` before their own poll lands. Their
  catch blocks leave prior content untouched, so a persistently-failing endpoint leaves the
  previous run's agent tree visible under the new run's header. Self-heals in ~1-2s on the
  normal path.

### Testing note

The regression checks are deliberately split. "A run with no spec shows no doc" passes on
`route()`'s defensive clear alone, so it does **not** pin the real invariant; the two
`refresh() alone ...` checks plant a stale render and call `refresh()` directly, and those are
the ones that fail if the `else` branch is removed. Both halves were verified to fail against
the unfixed page before being kept. `refreshSeq` (`cli/web/index.html`) exists purely so the
driver can tell "this run has rendered" from "nothing has rendered yet" — an empty
`currentDocText` cannot distinguish the two, and waiting on it made the check vacuous under a
real page reload.

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

## "Read-only" reviewers can still mutate repo state through drovr itself (2026-07-31)

**Severity:** low, but it leaks into the repo and into `git worktree list`.

Reviewers run under a read-only flag (`cursor --mode plan`, `claude --permission-mode plan`),
which stops them EDITING files. It does not stop them running commands, and the seed explicitly
invites them to "run the tests". A reviewer verifying `drovr phase done`'s handoff gate did the
natural thing and exercised it end to end:

```
drovr new gate-test --worktree     # …and gate-test2 … gate-test5, across two rounds
```

Each of those created a real git worktree under the *driver's* checkout (`cli/.drovr/wt/gate-testN`,
because the reviewer's cwd was `cli/`) plus a branch `drovr/gate-testN`, registered in the shared
repo. They then showed up as embedded git repositories in the driver's next `git add -A`, and were
nearly committed.

**So:** read-only bounds the editor, not the process. Anything a reviewer can invoke — drovr
included — runs with the driver's permissions.

**Mitigations in place:** `.drovr/` is now in `.gitignore`, so a leaked worktree cannot be staged
by accident. Check `git worktree list` after a review round and remove strays
(`git worktree remove <path>` + `git branch -D drovr/<name>`); they are clean by construction, so
removal is safe.

**Worth considering:** the reviewer seed could tell reviewers to exercise drovr against a scratch
directory (`--dir "$(mktemp -d)"`) rather than the checkout under review.

## A phase agent can plant its own `<phase>-context.md` (2026-07-27)

**Severity:** medium — it is a back door around "drovr composes every brief", the whole point of
the structural-briefs design.
**Found:** review round 2 of run `structural-briefs`, security angle, `cli/src/brief.rs`.

`brief::resolve_context` records driver context at `<run_dir>/<key>-context.md` and, when a later
invocation passes no `--context`, reuses it. But the run dir is **agent-writable by contract** —
every phase agent writes `<phase>-HANDOFF.md` there, and `drovr phase done` requires it. So an
agent can create `<phase>-context.md` itself, and the next `drovr phase brief` / `phase start`
without `--context` will present that text to the next agent as *driver* context.

**Not fixed, deliberately.** Any check drovr could add here is a heuristic (provenance guessing,
mtime comparison, a marker the agent could also write) layered under an authoritative mechanism,
and a heuristic backstop is worse than a documented gap: it makes the hole look closed. The real
boundary is the run dir's permissions, and drovr's model already trusts agents not to write
`state.json` with nothing enforcing it — this is the same trust, in the same directory.

**What does hold:** the reuse is announced on stderr with the path every time it happens
(`drovr: reusing the recorded context for '<key>' (<path>)`), so a driver can see what is in
effect; `--context ''` clears the record; and recording uses write-then-rename, so a symlink
planted at that path is replaced rather than followed (that one WAS fixed — `fs::write` follows
symlinks, which turned recording into a clobber of the link's target).

**If you want it closed:** the context has to live somewhere agents cannot write, which means
outside the run dir — a driver-side store keyed by run+phase. That is a design change, not a
patch.

**Hardlinks sit inside this same gap.** Reading a record refuses symlinks and non-regular files,
but a HARDLINK at the record path is indistinguishable from the record itself and reads through to
its target. Not separately patched, for the same reason: an actor who can create that link can
already write the record's content directly, so refusing `nlink > 1` would add a check whose only
effect is to imply a boundary that is not there. The narrow residue — linking a file one can link
but not read — is bounded by `fs.protected_hardlinks`, which is on by default on Linux.

## A read-only cursor reviewer can park at plan mode's "Ready to build?" gate (2026-07-27)

**Severity:** medium — the reviewer never reports, and neither `idle` nor `blocked` distinguishes
it from one that finished.
**Found:** review round 4 of run `structural-briefs`, error-handling angle (`wBM:pG`).

Reviewers run `cursor --mode plan`. Plan mode's natural terminus is a confirmation dialog:

```
Ready to build?
 → 1. Yes, build locally (b)
   2. Yes, build in cloud (c)
   3. No, propose changes (p or Esc)
```

The agent had done the review and then offered to *implement* it. It sat at that gate with
`agent_status: idle` — indistinguishable from a reviewer that finished and is waiting at its
composer, which is why the panel's completion detection cannot see it either.

**Never answer 1 or 2.** A reviewer that builds violates the read-only contract and drovr's
single-writer rule at the same time.

**Remedy:** `herdr agent send-keys <pane> escape` to decline, then send a reporting-only prompt
("you are READ-ONLY; print one line per finding as SEVERITY|file:line|summary") and submit it with
another `enter`. That recovered the angle, which then reported clean.

**Distinguishing it from the unsubmitted-paste bug:** both look `idle`. Read the pane —
`→ [Pasted text #1 +N lines]` in the composer is the paste bug; a `Ready to build?` box is this
one. Same lesson as the `pgrep` mis-triage above: check the pane, not the status.

## `rustfmt src/main.rs` (and `cargo fmt`) reformats every sibling module (2026-07-27)

**Severity:** low, but it silently produces a huge unrelated diff.

`cli/src` is not rustfmt-clean: `config.rs`, `herdr.rs`, `phase.rs`, `reflex.rs`, `review.rs` and
`run.rs` all have pending formatting differences. Because rustfmt follows `mod` declarations,
formatting `main.rs` — or running `cargo fmt` at all — rewrites all of them: ~500 lines across six
files nobody touched, which then collide with other worktrees working on those files.

**Do:** format the leaf module you edited (`rustfmt --edition 2024 cli/src/brief.rs`). Verify with
`rustfmt --check` on that file and ignore diffs it reports for siblings.
**Do not:** run `cargo fmt`, or `rustfmt` on `main.rs`, unless you intend to reformat the crate.
If you do it by accident, `git checkout --` the files you did not edit.

## The findings channel loses reviewer output two ways (2026-07-26/27)

**Severity:** high — it fails the panel on any real diff, and both modes look like "the reviewer
produced nothing" rather than "drovr could not read it".
**Found:** run `structural-briefs`, task `branch`, 4 cursor reviewers over a 1152-line diff.
Panel exit 1: `reviewer 'review:branch:1:error-handling' produced no findings JSON (no file
written and none found in its transcript)` — while that reviewer's JSON was plainly visible in
its pane.

### 1. Reviewers emit UNFENCED JSON; the extractor only reads fences

`extract_findings_json` walks ` ``` ` fences and takes the last block whose body starts with `{`.
The cursor reviewers printed `Review complete. Findings below.` followed by a bare top-level JSON
object — no fence. The only fenced block in the transcript was the *schema* echoed from the seed.
So extraction found nothing even though valid findings were on screen.

**FIXED** (`829a155`, hardened again in `1022510`): a fenced candidate must now PARSE as a
`Review` — the seed's echoed schema is not JSON, and it was shadowing real findings — and when no
fenced block yields one, `last_review_object` takes the last balanced `{...}` that parses. Its
candidate starts are braces that begin a line, tried last-first: trying every brace was quadratic
on a transcript full of code, and an unbalanced brace in prose could swallow the rest of the input
so the real object was never attempted.

### 2. The pane transcript is LOSSY, so JSON cannot be reconstructed from it at all

`herdr agent read` (any `--source`, including `recent-unwrapped`, `--lines 800`) truncates long
lines **mid-word**:

```
"rationale": "resolve_context treats trimmed-empty supplied context as None and falls th... A driver pas
code-review's explicit recording semantics and dangerous on fix-loop re-runs."
```

Text is missing between `A driver pas` and `code-review's`, so the JSON is unparseable no matter
how it is extracted. A `rationale` of any length — i.e. every useful finding — can hit this.

This is the deeper form of the already-noted "make the findings channel durable, not a viewport":
the viewport is not merely small, it is *destructive*.

**Also ruled out as an escape:** asking the reviewer to write the JSON to a file. The reviewers
run read-only (`cursor --mode plan`), which refuses the write. They fall back to printing, which
is where the truncation is.

**What actually worked** (both rounds, and it is the current workaround): tell the reviewer, via
`--context`, to print one line per finding as `SEVERITY|file:line|summary` with every line under
100 characters. Short lines wrap instead of truncating, so they survive. Rationale is lost, which
is a real cost — the summary alone is usually enough to act on, but not always.

**There IS a durable channel, found 2026-07-27:** a cursor reviewer in `--mode plan` saves its
work to `~/.cursor/plans/<title>-<id>.plan.md` and prints the path
(`Saved to home/sauyon/.cursor/plans/Security Review Findings-4ea3cb76.plan.md`). That file holds
the FULL review — untruncated rationale, the "no finding" verifications, findings separated from
nits — and is incomparably better than anything scraped from a pane. Round 5's security findings
were only fully legible there.

**Fix ideas, in order of preference:** (1) harvest the plan file: match the newest
`~/.cursor/plans/*.plan.md` for the reviewer (title + mtime) and parse findings from it, falling
back to the transcript. Cursor-specific, so it needs a per-agent hook rather than a hard-coded
path; (2) any other channel drovr controls — the reviewer runs in a pane drovr spawned, so a file
it is *permitted* to write beats scraping; (3) instruct short lines in the seed rather than in
per-invocation context; (4) accept unfenced JSON — done, necessary but not sufficient.

## One failing test cascades: a panic while holding `ENV_LOCK` poisons it (2026-07-27)

**Severity:** low, but it wastes debugging time.

The env-dependent tests serialize on `test_util::ENV_LOCK` and take it with
`ENV_LOCK.lock().unwrap()`. A test that panics *while holding* it poisons the mutex, so every
later test that locks also panics — one real failure reports as several. Seen while fixing the
round-1 review findings: one genuine assertion failure in `brief::tests` surfaced as three
failures, two of which passed in isolation.

If a run reports N failures, re-run the first one alone before believing the other N-1.

## `web_keyboard_navigation` hung on every run: Chromium's cookie store waits on the keyring (2026-08-01) — FIXED

**Cause: `--password-store=basic` was missing from the test's chromium launch.** Fixed in
`cli/tests/web_nav.rs`; the suite went from failing every run to passing three for three in ~13s.

**The mechanism**, from a full NetLog (`--log-net-log --net-log-capture-mode=Everything`):

1. `COOKIE_PERSISTENT_STORE_LOAD` begins at startup and never ends.
2. Every `URL_REQUEST` that needs cookies (`privacy_mode: disabled`) stalls at
   `COMPUTED_PRIVACY_MODE` — where `URLRequestHttpJob` calls into the cookie store — and never
   reaches `HTTP_TRANSACTION_SEND_REQUEST`.
3. The cookie store is loading its encryption key through OSCrypt, which on Linux asks the Secret
   Service over D-Bus. `ReadAlias("default")` on `org.freedesktop.secrets` returns `/`: there is
   no unlocked default collection. `gnome-keyring-daemon` runs with `--components=secrets` but the
   keyring was never unlocked or aliased, and with `XDG_CURRENT_DESKTOP`/`DESKTOP_SESSION` empty
   there is no prompter to answer. **That call has no timeout.**

Every symptom follows, and each one is what made this look like something else:

| Symptom | Why |
|---|---|
| `file://` and `about:blank#x` navigate instantly | they never touch the cookie store |
| the target server logs **zero** requests | the TCP connection IS established — NetLog shows `TCP_CONNECT` completing — but no HTTP request is ever written to the socket |
| `curl` to the same port works | different process, no keyring involved |
| the DevTools session looks dead | it is not: `Page.navigate` never resolves and later commands queue behind it. `/json/list` still answers |
| chromium's own background traffic succeeds | that traffic is `privacy_mode: enabled`, i.e. cookieless |

**Three wrong diagnoses were recorded here before this one** — "environmental", then "load
sensitivity in a fixed 20s CDP deadline", then "a Chromium 150 regression". None had measurements
behind them. Ruled out along the way and worth not re-testing: the drovr server, CDP socket
topology (flattened browser-endpoint sessions behave identically), `--headless` vs `=old`/`=new`,
`--no-sandbox`, `--disable-dev-shm-usage`, `NetworkServiceInProcess`, `--no-proxy-server`,
enterprise policy, machine load, the agent tool sandbox, and any chromium wrapper or flag file
(`/usr/bin/chromium` is Arch's launcher and injects nothing; no `chromium-flags.conf` exists).

**The general lesson for headless Chromium on Linux:** always pass `--password-store=basic`. A
machine with no unlocked keyring turns every cookie-bearing request into an untimed wait, and the
failure presents as a network or browser fault rather than a credential-store one.


## `review::tests::lock_records_our_pid_and_releases_on_drop` is flaky, cause UNKNOWN (2026-07-26, 2026-08-01)

**Severity:** low — it passes on re-run. Recorded so nobody re-derives what has already been
ruled out.

Two sightings, five days apart, both on a full `cargo test`, both passing alone and on the
immediate re-run of the whole suite. **Never reproduced on demand** — 14 full-suite runs and 8
`review::tests`-only runs since.

**A previous version of this entry blamed cross-process contention on a shared lock path, and
that is wrong.** Ruled out since:

- **Not a shared path.** `make_root` is `tempfile::Builder::…tempdir()`, so each test gets a
  unique directory; no other process or worktree can be touching that `server.pid`.
- **Not cross-file lock aliasing.** `try_take_lock` is `File::try_lock` (flock), which is scoped
  to the open file description; distinct files cannot contend.
- **Not the `ENV_LOCK` poison cascade** (see the entry below). This test never takes `ENV_LOCK` —
  it passes an explicit path precisely so it does not depend on `XDG_DATA_HOME`.

So the cause is genuinely unknown, and guessing again would only produce another wrong entry.

**What was done instead:** the test now reports, on every failure path, the lock path, the file's
contents, and this process's pid — and which step failed (claim / pid record / re-claim after
drop). Neither sighting left enough evidence to diagnose; the third will.

**If you hit it:** paste that message here rather than re-investigating from scratch.

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

~~**Unverified as of 2026-07-25:** the *second* symptom — reviewer panes attach and get seeded but
never reach `done`, so `code-review run` times out with no `<task>-review.json` — has not been
re-run against current `main`.~~ **CONFIRMED 2026-07-25, independently by two runs**, and it is
**not** a distinct bug — it is the unsubmitted-prompt failure documented in the next section:

- run `review-resume`, branch `drovr/review-resume`, dogfooding the code-review resume change. All
  four cursor reviewer panes launched, attached, and received their seed, but the brief sits in the
  composer as `→ [Pasted text #1 +46 lines]`, never submitted. The agents therefore never start,
  never reach `done`, and `code-review run` times out with no `<task>-review.json` — exactly as
  reported.
- run `phase-reap`, branch `drovr/phase-reap` — the same symptom, same cause, plus a *second*
  failure mode behind it. Detailed below.
- run `m3-schema-dos` (2026-07-26) — same cause a third time, initially mis-filed as "no reviewer
  ever spawned" because the triage used `pgrep` instead of reading the pane. See below.

### Dogfooded end-to-end 2026-07-25 (run `phase-reap`, task-1, branch `drovr/phase-reap`)

Two full panel passes on a host where **cursor IS integrated** (so `review_agent_for` resolves to
cursor). The panel failed **three distinct ways**, none of which is the spawn race:

**Pass 1 — exit 2 (timeout), root cause is the unsubmitted paste.** All four reviewer panes
(`wAF:p2`–`p5`) attached fine and sat at `agent_status: idle` with the seed visible in the composer
as `→ [Pasted text #1 +46 lines]`. This is the *"`drovr phase send` still lands a large briefing
unsubmitted"* bug below, reaching the **reviewer-spawn** path via `code_review.rs`'s `phase_send`
call — the seed is never submitted, so the reviewer never starts and the panel burns its whole
`timeout_ms`. `herdr agent send-keys <pane> enter` on each pane started all four immediately, and
all four then produced valid verdicts.

**Pass 2 — exit 1, findings emitted but not extractable.** With `enter` sent proactively, all four
panes (`wAF:p7`–`pA`) ran to completion and reached `agent_status: done`. The panel still failed:

```
drovr: code-review run failed: reviewer 'review:task-1:2:correctness' produced no findings
JSON (no file written and none found in its transcript)
```

`wAF:p7`'s transcript **did** contain a well-formed `{"verdict":"changes","findings":[…]}` block.
`obtain_findings_json` could not see it because `agent_read` reads
`source:"recent"` — a *viewport* snapshot, not the full scrollback. Cursor renders long tool output
collapsed (`… NN output lines hidden · ctrl+o to expand`) and keeps scrolling, so by the time the
panel reads the pane the emitted JSON has left the recent window. The file fallback never helps
either: the reviewer seed says *"Emit the fenced JSON, then exit"*, so reviewers deliberately write
**no** `<task>-review-<angle>.json` file — the transcript is the only channel, and it is lossy.

**Correction to an easy misdiagnosis:** cursor reviewers *do* reach `done`. In pass 1 they merely
appeared `idle` because they were still parked at the composer with the seed unsubmitted. Do not
file "cursor reviewers never reach `done`" — that was an artifact of failure mode 1.

### Also seen (2026-07-26, run `m3-schema-dos`, task `schema-dos-fix`) — same unsubmitted seed

Two more passes of the unsubmitted-paste mode, from a driver that was a plain `claude` session drovr
had not started. `0 of 4 angles finished` on both (`--timeout-ms 540000`, then 1500000), all four
`schema-dos-fix-review-<angle>-seed.md` written, zero findings files.

**A misdiagnosis worth recording, because the obvious check lies.** `pgrep -af 'drovr|code-review'`
returned nothing for this panel, which reads as "no reviewer was ever spawned" — and it is wrong.
The reviewers are `cursor-agent` processes inside herdr panes, so neither pattern matches them; the
panes were registered and `Running` in `state.json` the whole time (`wB8:p2`–`p9`, both iterations).
Reading one settled it in a single command:

```
herdr pane read wB8:p6 --source recent --lines 14   # → [Pasted text #1 +46 lines]
```

So: diagnose a stalled panel by reading a pane, never by grepping the process table. Check
`state.json` for the reviewer pane ids first — they are recorded per angle per iteration.

The plain-`claude` driver is circumstance, not cause: `spawn_reviewer` needs `run.workspace` from
that run's `state.json` and nothing else, `drovr new` records it whoever invokes it, and no code
path consults the calling session. A run with no workspace fails loudly instead.

Credit where due: drovr handled the moving target correctly. HEAD changed between passes and pass
2 reported `HEAD moved since review iteration 1 was seeded — starting a fresh panel instead of
resuming it`, which is the right call — a resumed panel would have reviewed an abandoned design.

**Workarounds, both used here:** send `Enter` to each reviewer pane after the panel spawns them (the
documented fix below), or drive the review with a self-spawned read-only reviewer (Claude Code Agent
tool, `general-purpose`, blocking). The latter found two Critical defects the author's own tests had
missed, including a test that passed while allocating ~300 MB of grammar.

### Fix ideas (from the 2026-07-25 dogfood)

1. **Make the findings channel durable, not a viewport.** Have the reviewer seed instruct writing
   `<run_dir>/<task>-review-<angle>.json` *and* echoing it, then prefer the file. The file fallback
   in `obtain_findings_json` already exists but is dead code today because nothing writes the file.
2. If the transcript must stay the channel, read full scrollback rather than
   `source:"recent"`, and fail with the captured transcript attached so the driver can hand-merge.
3. Submit the seed reliably (see the entry below) — one fix removes failure mode 1 for the panel,
   `phase send`, and the review gate at once.

**Workaround used:** send `enter` to each reviewer pane after the panel spawns them, then read the
four panes directly and hand-merge into `<run_dir>/<task>-review.json`. Both passes of task-1's
review were merged this way, and both produced real, actionable findings — the reviewers work; only
the plumbing around them fails.

Reading a reviewer pane (`herdr agent read <pane>`) shows the full seed rendered in the composer
with the correct `base..head` scope, so seeding and scope selection are fine; only the submit
keystroke is missing. Fixing "`drovr phase send` returns success with the prompt left
unsubmitted" (below) fixes the panel too — they are one bug, and the panel is simply its most
visible victim. Keep the self-spawned-reviewer workaround above until that lands.

## A reviewer's `submit_findings` tool can be DEFERRED, so a tool-search outage loses the review

**Severity:** medium (the panel's only findings channel becomes uncallable; every angle
finishes having delivered nothing, and the pass fails).
**Found:** 2026-07-26, probing the claude findings channel directly (`fix-review-json`):
`claude -p --permission-mode plan --mcp-config <f> --strict-mcp-config`, asked to call the
tool.

### Symptom

The agent reports it cannot call the tool: `mcp__drovr-findings__submit_findings` is a
*deferred* tool, its schema must be loaded through `ToolSearch` first, and `ToolSearch`
answers `HTTP Error 502: Bad Gateway` (classifier unreachable). Nothing is written; the
angle looks like a reviewer that simply produced nothing.

### What the probe DID establish (both good)

- claude accepts the server from `--mcp-config` **without** the "New MCP server found"
  approval prompt (see that issue below — it applies to project-scoped `.mcp.json`, not to
  a config passed on the command line), and registers it under `--permission-mode plan`.
- The server itself is correct end-to-end: a hand-driven stdio JSON-RPC session
  (`initialize` → `tools/list` → `tools/call`) returns the tool and writes
  `<task>-review-<iter>-<angle>.json` exactly where the panel reads it.

### Mitigations

- **Seed** (`code_review::build_seed`): names the fully qualified id, says the tool may be
  deferred and must have its schema loaded first, and states that calling it is the
  *sanctioned* way to deliver from read-only mode — the same probe showed a cautious agent
  stopping to ask permission for a tool it read as "writing". Asserted by
  `code_review::tests::seed_routes_findings_through_the_submit_tool`.
- **Launch** (`config::default_agents`): claude's reviewer launch carries
  `--allowedTools=mcp__drovr-findings__submit_findings`, so plan mode's tool gate cannot
  refuse the one tool the panel depends on. Note the `=` form: `--allowedTools` is
  **variadic**, and as two argv words it swallows whatever follows it — passing it as
  `--allowedTools <tool>` before a positional prompt makes claude exit with "Input must be
  provided either through stdin or as a prompt argument". Asserted by
  `config::tests::the_claude_reviewer_launch_pre_allows_exactly_the_findings_tool`.

### Still open

If the tool-search service is down, no seed wording and no flag helps — the schema cannot be
loaded at all. The LLM leg of the probe was never completed for that reason, so "a real
**claude** reviewer calls the tool and the file lands" is verified only at the protocol level
(hand-driven stdio JSON-RPC) plus flag-parsing; the full agent-level path is verified live
for **cursor** only (during design). Re-run the probe when the service is back:

```
claude -p --permission-mode plan --mcp-config <f> --strict-mcp-config \
  --allowedTools=mcp__drovr-findings__submit_findings "call submit_findings …"
```

## One silent reviewer fails the whole `code-review run` (exit 1) instead of one angle

**Severity:** low-medium (recoverable — a plain re-run respawns the angle — but the exit code
tells the pipeline driver to STOP and diagnose, for something self-healing).
**Found:** 2026-07-26, reviewing the findings-channel wiring (`fix-review-json`). Not a
regression; the behaviour predates the MCP findings channel.

### Symptom

A reviewer that finishes without delivering anything — it never called `submit_findings`, so
`<task>-review-<iter>-<angle>.json` does not exist — makes `code_review_run` return `Err`, which the
CLI maps to **exit 1** ("setup failure: STOP and diagnose"). The other three angles' findings
are already banked on disk and no merged `<task>-review.json` is written.

### Why it is arguably wrong

The pass already knows how to recover from exactly this: the angle is marked
`PhaseStatus::Failed`, and the next `drovr code-review run` replaces that reviewer in place
(`cli/src/code_review.rs`, the respawn branch). So the state left behind is a *resumable* one,
while the exit code says *unrecoverable*. Exit 2 (timeout — "resumable, re-run me") would
describe it accurately, or the angle could simply be reported `Failed` and the pass continue.

### Status: open, deliberately not changed

Raised as an open question in the `fix-review-json` design and left alone on purpose — the exit
code is a contract the pipeline skill reads, and changing it belongs in its own task with the
driver's behaviour changed alongside. Re-running `drovr code-review run` is the workaround and
it costs one reviewer, not a panel.

## `drovr phase send` returns success with the prompt left unsubmitted

**Severity:** high — an unattended pipeline stalls silently at every phase injection. (Filed as
`low` originally on the grounds that it is recoverable; that undersold it. Recovery requires a
human noticing that nothing is happening, and the failure is indistinguishable from an agent
that is simply working.)
**Found:** 2026-07-24, run `gpu-deploy-view`, every phase injection — including on the updated
binary carrying the phase-send agent-readiness fix.
**Reproduced:** 2026-07-25 (`mcp-endpoint`), 2026-07-25 (`phase-reap`, three callers, 12 sends),
2026-07-26 (`skill-stickiness`, three times), 2026-07-30 (`land-review-json`, 3 of 4 reviewer
seeds — the measurement that settles the shape). See "Occurrences".

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

### Root cause — not established, but its SHAPE is: a race

Unknown in mechanism. What is established is that it is **non-deterministic** — the same
`phase send` code path, against four panes of the same backend, with payloads within **26 bytes**
of each other, succeeded once and failed three times (2026-07-30, `land-review-json`; see
Occurrences). Nothing about the *payload* predicts the outcome, so no fix may be predicated on
one. That rules out an entire family of explanations at once, including the two below.

Three plausible-sounding explanations have been **ruled out** by evidence; do not fix against
any of them.

- **Not payload size, and not a bracketed-paste commit failure.** Three sends of a few hundred
  bytes each failed on 2026-07-26, none rendering as a paste. Whatever fails, fails for inline
  text too. Independently confirmed from the other direction on run `phase-reap`: an **8-line**
  payload failed while rendering *as* a collapsed paste (`❯ [Pasted text #3 +8 lines]`), against
  the 6586-byte / 124-line payload previously recorded. Neither size nor rendering predicts it —
  any fix predicated on "large bracketed paste" will miss this. The 2026-07-30 four-pane
  measurement closes this off from the third direction: the pane that *worked* was neither the
  largest nor the smallest of the four.
- **Not cursor's "Workspace Trust Required" modal.** This is the most attractive wrong answer,
  because the modal is real and it does swallow prompts — but it is not what happens here, and a
  fix aimed at it makes things worse. Disproved three ways on 2026-07-30:
  1. **`--trust` does not exist on the interactive path.** `cursor-agent --mode plan --trust
     --workspace <dir>` exits immediately with `Error: --trust can only be used with
     --print/headless mode`; in the bundle the flag is read only inside a headless-only branch.
     Adding it to drovr's launch **breaks the launch outright**. Do not add `--trust` anywhere.
  2. **Inherited trust makes the modal a non-event for drovr worktrees.** A fresh directory with
     no trusted ancestor *does* show the modal, so the mechanism is genuine — but
     `~/.cursor/projects/home-sauyon-devel/.workspace-trusted` is dated **2026-04-28**, months
     before the first of these reports, and descendants inherit it. A launch into the real
     worktree `.drovr/wt/land-mcp-findings` shows **no modal** and lands straight in the composer.
  3. **No modal was present in any observed failure.** All four 2026-07-30 reviewer panes sat at
     an ordinary composer with the text visibly pasted into it.

  Related, and equally out of scope: drovr must **not** write cursor's `.workspace-trusted`
  marker itself. That means reimplementing cursor's private directory-slug algorithm in order to
  grant trust on the user's behalf — not obviously right, and not this bug.
- **Not the `drovr phase send` CLI entry point.** Run `phase-reap` reproduced it from three
  different callers, including `code_review.rs`'s reviewer spawn, so the failure is in
  `phase::phase_send` itself (and therefore `agent_send` → socket `agent.prompt`). See
  Occurrences.
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
This is reliable: `herdr agent send-keys <pane> enter` after every `phase send` worked **12/12
times** across run `phase-reap`.

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

**2026-07-30, run `land-review-json`, workspace `wC1`** — one `code-review` panel spawning four
cursor reviewers, i.e. four `phase_send` calls a few seconds apart into four freshly-spawned
panes of the same backend. This is the measurement that establishes the shape:

| Pane | Angle | Seed | Submitted itself? |
|---|---|---|---|
| `wC1:p2` | correctness | 2701 B / 61 lines | **No** — sat as `→ [Pasted text #1 +62 lines]` |
| `wC1:p3` | security | 2677 B / 61 lines | **No** |
| `wC1:p4` | error-handling | 2703 B / 61 lines | **Yes** |
| `wC1:p5` | type-design | 2680 B / 61 lines | **No** |

Same day, a **claude** pane took a ~1.2 KB `phase send` and **self-submitted** — `agent_status:
working` within 4s.

Read it carefully, because it constrains any fix:

- The four seeds span **26 bytes**. The one that submitted was neither the largest nor the
  smallest. So the failure is not a function of the payload — **it is a race**, and a single
  green run proves nothing. Any change here has to be exercised repeatedly, on cursor, until
  both branches have been seen.
- It is not backend-determined either: claude self-submits, and has been observed to *not*
  self-submit on a large paste (`phase-reap`, case 2). Cursor is merely the far worse offender.
- None of the four panes showed a trust modal or any other dialog. See "Root cause".

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


**2026-07-25, run `phase-reap`, workspace `wAF`** — installed nix-profile binary, **three
different callers**, which together show this is `phase::phase_send` itself and not the
`drovr phase send` CLI entry point:

1. **Reviewer spawn (`code_review.rs:318`) — 8 for 8.** Both panel passes, all four angles each
   (`wAF:p2`–`p5`, then `wAF:p7`–`pA`): every reviewer sat `idle` with
   `→ [Pasted text #1 +46 lines]` unsubmitted. This is what makes the review panel time out; see
   the panel entry above. Cost: the panel's entire `timeout_ms` (30 min here) per pass.
2. **Driver re-entry into a live implement phase.** `drovr phase send phase-reap
   implement-task-1 "<review findings>"` reported success; the payload landed in the claude
   pane's composer as `❯ [Pasted text #3 +8 lines]` and was never submitted, so the re-entry
   silently did nothing until nudged. This makes the implement↔review loop a silent no-op — the
   driver believes it forwarded findings and then waits on an agent that was never told anything.
3. Both **cursor** (reviewers) and **claude** (implementer) panes are affected, so it is not a
   backend-specific quirk.

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

### Status: the false SUCCESS is FIXED 2026-07-30 (branch `drovr/fix-phase-send`)

`phase_send` no longer treats `agent.prompt` returning as proof of delivery. It uses that call's
native `wait` option (`until: [working, done]`) and returns `Ok` only when herdr **observed the
agent start**; `agent_prompt_stalled` / `timeout` mean the payload did not take. On a stall it
reads the pane for positive evidence the payload is in the composer — a `[Pasted text …]`
placeholder or a verbatim prefix of its first line, in the last 8 non-empty lines, and required
to have APPEARED across the prompt. Evidence → one `enter`, then re-confirm. No evidence
(including a pane that cannot be read) → raise, never guess. See `cli/src/phase.rs`
(`phase_send`, `pane_shows_payload`) and `Herdr::agent_prompt_confirm`.

This does not stop the underlying race — it stops the **silent** failure. A send that does not
take now exits 2 with a message naming which failure it was, instead of exiting 0.

Verified live 2026-07-30 on herdr 0.7.5, all four branches:

| Branch | How it was reached | Result |
|---|---|---|
| Healthy self-submit | 8 fresh cursor panes + 7 fresh claude panes, ~4 KB seed; plus one 77 KB payload | exit 0 in 0–1s, every agent ran and answered |
| Stall → nudge → OK | claude, 77 KB, confirm deadline shortened to 250 ms to force the stall | `herdr agent send-keys <pane> enter` issued, then exit 0 |
| Swallowed → raise | a cursor pane that dropped the payload outright (composer empty) | exit 2, **no keystroke** |
| Parked on a menu → raise | claude parked on the `/model` picker, `❯ 2.` highlighted, "Enter to set as default" | exit 2, **no keystroke** |

The no-keystroke claims are not inferred from the message: `drovr` was run with a logging
`herdr` wrapper first on `PATH`, which recorded every CLI invocation. `agent_send_keys` is the
only thing that shells the CLI, so an empty log is proof. A positive control (`herdr agent
send-keys <pane> esc`) confirmed the wrapper records what it is meant to.

### Still open: `until` is a LEVEL, not an edge

Measured 2026-07-30 against herdr 0.7.5, and there is no API option for the other behaviour
(`AgentPromptWaitOptions` is only `{until, timeout_ms}`). `agent.wait` on a pane that is
*already* in one of the `until` states returns in **0.0s** with success, without observing any
transition. On a pane that is not, it blocks the full deadline and answers `timeout` — so the
wait works; it just cannot distinguish "started because of my prompt" from "was already going".

So the guarantee is exactly: **if the pane was `idle` when the prompt went out, `Ok` means herdr
saw it start.** That is the normal case — a freshly-spawned agent, or a re-entry into one parked
at its composer. Two narrow cases fall outside it:

- pane already `working` — `wait_agent_ready` admits `working`, so this is reachable when a send
  targets a busy agent. `Ok` there proves nothing about this payload.
- pane already `done` — very narrow, because `done` is momentary (see the EDGE entry below): an
  agent parked at its prompt reads `idle`, so the driver's post-`phase wait` re-entry send
  almost always lands on `idle`.

Deliberately NOT worked around. The fix would be to have the readiness gate release only on
`idle`, which changes what `phase send` does to a busy agent, and the alternative — juggling the
`until` set against the pre-prompt status — has its own failure mode (a turn short enough that
the transition is missed reads as a stall, and raises on a send that worked). Documented rather
than papered over.

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

### Worse than a stall: `agent.prompt` can ANSWER an unclassified menu

**Re-measured 2026-07-30, herdr 0.7.5**, and this is the part drovr cannot fix. A prompt
delivered into a menu herdr never classified as `blocked` does not merely get swallowed — it can
dismiss the menu and **accept the highlighted option**, inside `agent.prompt`, before drovr sees
anything.

Reproduced deterministically on claude's `/model` picker (a stand-in for the MCP approval, and
easier to arm): pane parked with `❯ 2. Opus (1M context)` highlighted and `Enter to set as
default` on screen, reporting `agent_status: idle`. One `agent.prompt` of an ordinary briefing
later, the menu was gone and the status line read the option that had been highlighted. No key
was sent by drovr — verified with a logging `herdr` wrapper on `PATH`, empty log.

`phase_send`'s refusal to nudge protects the keystroke it controls, and it correctly reports the
seed as undelivered. It cannot protect the one herdr issues. Closing this needs pre-send blocker
detection, i.e. the manifest rules below — it lives **outside** this repo.

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

## herdr's `agent_status: done` is an EDGE, not a level — FIXED 2026-07-27

**Severity:** was high — a reviewer that did everything right hung its panel forever.
**Found:** 2026-07-27, running the round-2 review panel for `land-mcp-findings`.

### Symptom

Four cursor reviewers all delivered valid findings files. The panel reported *"3 of 4 angles
finished; still waiting on error-handling"* and timed out (exit 2) on two consecutive runs,
with `branch-review-2-error-handling.json` sitting complete and parseable on disk the whole
time. `state.json` had that angle `Running`; the other three `Done`.

### Cause

herdr reports `agent_status: "done"` for only a moment as a turn ends; an agent parked at its
prompt reports `"idle"`. All four finished panes read `idle` when inspected afterwards. The
panel's completion test was `done_marker exists || agent_status == Done`, and the marker never
fires for reviewers — their seed forbids `drovr phase done` (confirmed: zero `.done` files in
the run dir). So the only signal was that momentary edge. Three angles were polled while it
was showing; the fourth was not, and no later poll could ever recover it. On resume, the
banking branch was gated on `status == Done`, so a stuck `Running` angle could not be rescued
by its own file either. Two gates, both asking the pane, both unrecoverable once missed.

### Fix

Completion is the artifact. A parseable `<task>-review-<iter>-<angle>.json` finishes the
angle at both gates, whatever the pane says (`code_review::delivered_review`). herdr is now
consulted for exactly one question the artifact cannot answer: has a reviewer finished
*without* delivering? The server's write is atomic (temp + rename) so a file in flight cannot
be read as a delivery.

### Not affected: ordinary phases

`phase.rs` uses `agent_status` for readiness and blocked-detection, never for completion —
a phase agent completes by running `drovr phase done`, which drops the marker the wait polls.
Only the review panel depended on the edge, because only reviewers are told not to run it.

## `main` is not `cargo fmt` clean, and formatting one file reformats the whole crate

**Severity:** medium — the last branch that got this wrong had to be rebuilt from scratch
(`land-mcp-findings` exists only because of it).
**Found:** 2026-07-27, formatting three files during `land-mcp-findings`.

### The trap

`cargo fmt --check` is dirty on `main` itself — currently `cli/src/herdr.rs`, `phase.rs`,
`reflex.rs`, `review.rs`, `run.rs` and `cli/tests/web_nav.rs`. So "run `cargo fmt` before
committing" sweeps ~450 lines of unrelated churn into your branch, which then collides with
whatever else lands on `main`. That is exactly what forced `drovr/fix-review-json` to be
replayed rather than merged.

**And the obvious workaround does not work.** `rustfmt cli/src/main.rs` does *not* format
one file: rustfmt follows `mod` declarations from the crate root, so naming `main.rs`
reformats every module it reaches. Naming a leaf module (`cli/src/code_review.rs`) is
likewise a crate-root entry for rustfmt's purposes if it is reachable. The sweep is silent —
the command prints nothing.

### What to do

1. Check whether the debt is yours before touching it:
   `git show main:<file> > /tmp/c.rs && rustfmt --edition 2024 --check /tmp/c.rs`.
   Clean → your edit introduced it, fix it. Dirty → it is main's; leave it.
2. After any `rustfmt`/`cargo fmt`, **`git diff --stat` before staging** and
   `git checkout --` every file you did not otherwise change.
3. Never `git add -A` a tree you have just formatted.

### Fix

Land one formatting-only commit on `main` that makes the tree clean, so `cargo fmt` becomes
a no-op for everyone and this whole hazard disappears. It has not been done because such a
commit conflicts with every branch in flight — it needs a quiet moment, not a fix.

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

### A second, distinct flake: `lock_records_our_pid_and_releases_on_drop` (2026-07-26)

`review::tests::lock_records_our_pid_and_releases_on_drop` (`cli/src/review.rs:2311`) fails
intermittently at `cli/src/review.rs:2326` with `released lock must be free` — `try_take_lock`
returns `Ok(None)` (WouldBlock) for a lock the test just dropped.

**It is NOT the env-pollution cause above**, despite also being parallelism-only. The test
locks `tmp.path().join("server.pid")` under a `tempfile` root and never reads `XDG_DATA_HOME`,
so no other test can name that path — its own doc comment (`cli/src/review.rs:2306-2309`)
already claims immunity to the env flake, and that claim holds. Something else releases late.

Measured 2026-07-26 on `52db1cd`:

| how it was run | result |
|---|---|
| the `lock_*` tests alone, 25 consecutive runs | 25/25 green |
| the whole `--bin drovr` suite, 12 consecutive runs | **1/12 red** |
| nix sandbox build of `52db1cd` (`home-manager switch`) | red once, green on immediate retry |

**Hypothesis, not yet confirmed:** an fd inheritance window. `flock(2)` locks belong to the
open file description, which survives `fork`, so a concurrently-spawning test (several here
start real servers) transiently holds an inherited copy of this fd between its `fork` and its
`exec`. Rust sets `O_CLOEXEC` on files it opens, so the child drops it at `exec` — which is
exactly why the window is narrow and the failure rare. Confirming it means tracing whether the
red runs coincide with a process spawn; that has not been done.

**Cost:** it fails the nix build, so it can break `home-manager switch` for an unrelated
change. A retry is the workaround — the failure does not reproduce twice in a row. Note this
means a *green* nix build is not evidence the test is sound.

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

## drovr never moves the driver out of the invoking checkout

**Severity:** high (the driver's every git observation is silently about the wrong tree, and on a
repo with concurrent agents it is how one clobbers another).
**Found:** 2026-07-26, run `phase-reap` — by the driver of that run, after 25 commits.

### Symptom

A driver agent runs `drovr new <run> --worktree`, is told the run lives in `.drovr/wt/<run>` on
branch `drovr/<run>` — and then keeps working **in the main checkout**. It reads main, runs
`git status` and `git log` against main, and (if careless) edits main.

Because cwd never moved, every bare git command resolves against the invoking checkout, so the
driver reports *other agents'* uncommitted files as if they were its own branch's state. During this
run the repo had **13 worktrees live at once, 7 of them `drovr/*` runs**. A driver that believes
main's dirt is its own will "clean up" or commit work belonging to someone else.

### Root cause

Two halves, and neither is sufficient on its own.

**1. There is no mechanism.** Nothing in `cli/src` ever changes the caller's directory — no
`std::env::set_current_dir`, no `chdir`, no `drovr enter`/`drovr cd` subcommand. `drovr new
--worktree` only *prints* the destination (`drovr: worktree <path> on branch <branch>`,
`cli/src/main.rs:348`). Nor can it do more: a subprocess cannot change its parent's working
directory, so a plain CLI cannot close this gap by itself. **That is precisely why the
documentation has to carry it.**

**2. The docs pointed the other way.** `skills/worktrees/SKILL.md` motivated isolation as "the
invoking checkout stays clean and usable" and described the worktree as the **run's**. Nothing told
the driver to leave the invoking checkout, so the natural reading was "the worktree is for the phase
agents; I stay put." That reading is wrong, and it is the one the text invited.

**This half is now fixed**: the skill says "clean and usable *for other work*", states that the
driver goes to the worktree too, and carries the move as an explicit step in the flow. Half 1 stands
— drovr still cannot move anyone.

### Working around it

**`cd` does not work.** In Claude Code the Bash tool's cwd resets to the session's primary working
directory after every call, so a `cd` in one command is gone by the next.

The mechanism that does work is the harness tool `EnterWorktree({path: ".drovr/wt/<run>"})`, which
switches the **session's** directory and persists across calls; `ExitWorktree({action})` leaves.
Neither drovr nor the skill mentioned it until now — `skills/worktrees/SKILL.md` now carries it as
an explicit driver step.

So: immediately after `drovr new <run> --worktree`, enter the worktree, and do not operate from the
main checkout for the rest of the run.

### Fix idea

1. Have `drovr new --worktree` print the enter-the-worktree instruction as part of its success line.
   That print is the one moment the driver is guaranteed to be paying attention, and it is where the
   path is already in hand.
2. Add a `drovr path <run>` helper that emits the worktree path alone, so the instruction is
   copy-pasteable and scriptable rather than something the driver reconstructs from a sentence.
   **The demand for this is already on the page, and so is the bug:** the pre-token `phase wait`
   entry above offers `: > "$(drovr path <run>)/<phase>.done"` as a workaround (added in `5beb62f`),
   but there is no `path` subcommand — `drovr path` exits with "unrecognized subcommand", and
   `cli/src/main.rs`'s `Commands` enum has no `Path` variant — so that command does not run today.
   Either add the helper or rewrite that line against `<run_dir>`. **Task 7's docs pass owns it**;
   left unedited here because this change is scoped to the worktree gap.

Neither removes the underlying limit — a CLI still cannot move its parent — so both are ways of
making the documented step harder to miss, not a substitute for it.

## A phase name registered in BOTH lists resolves to the wrong phase — FIXED 2026-07-26

Found by a review subagent during task 1's second fixes round of the phase-reap work; the gap itself
predates that work.

### Symptom

`drovr phase start <run> review:t:1:correctness` — pointing `phase start` at a name a REVIEWER
already holds — silently deleted that reviewer's `<phase>.done` marker, launched a second agent, and
appended a second `Phase` entry under the same name in `run.phases`.

From then on `RunState::find_phase` (which searches `phases` before `review_phases` and returns the
first match) resolved that reviewer to the impostor: `phase send` reached the wrong pane,
`phase done` from the reviewer's own pane was rejected as a token mismatch and demanded a
pipeline-only `-HANDOFF.md`, and `code-review`'s wait polled the wrong pane and could time out on a
reviewer that had actually finished.

### Root cause

`find_phase_idx` searches `run.phases` only, so a reviewer's name looked brand new to `phase_start`.
Neither creation site checked the other list.

### Fix

`require_name_unclaimed` (`cli/src/phase.rs`) refuses a name the OTHER list already holds, at both
creation sites, before any side effect. Pinned by
`a_name_a_reviewer_already_holds_cannot_become_a_pipeline_phase` and
`a_name_a_pipeline_phase_already_holds_cannot_become_a_reviewer`.

**The same list counts too.** Two entries under one name means `find_phase` resolves to whichever
was pushed first, so the second reviewer's pane is unreachable — the same corruption, within one
list. main's panel resume re-spawns under the same `review:<task>:<iter>:<angle>` name and already
drops the stale entry first (`run.review_phases.retain(|p| p.name != phase)`, "so `find_phase` cannot
resolve to the replaced pane"), so merging it is safe; the guard makes that ordering a requirement
rather than a convention. Pinned by `a_reviewer_must_be_de_registered_before_it_is_re_spawned`.

Not reachable from drovr's own naming — reviewer names carry a `review:` prefix that pipeline names
do not use — but the recovery commands drovr prints are bare `drovr phase start <run> <phase>`, so a
human or a driver pasting one against the wrong name hit it with no guard.

## A `<task>` or a review `angle` with a space or a shell metacharacter no longer produces a phase

Introduced by the phase-name hardening (task 1's second fixes round of the phase-reap work).

### Symptom

`drovr code-review run <run> "my task"` — or any run whose config sets
`angles = ["type design", "api & contracts"]` — fails with

```
invalid phase name "review:my task:1:correctness": may use only letters, digits,
'-', '_', '.' and ':' …
```

It used to work. Both halves of the name are affected: `<task>` comes from argv or from the review
server's HTTP layer, and `<angle>` comes from `${XDG_CONFIG_HOME}/drovr/config.toml`, which is
free-form and validated nowhere else.

### Root cause

`require_new_phase_name` (`cli/src/phase.rs`) is an ALLOWLIST — `[A-Za-z0-9._:-]` — applied wherever
drovr CREATES a phase (`phase_start`, `spawn_reviewer`). A reviewer phase is
`review:<task>:<iter>:<angle>`, so both interpolated parts inherit the rule.

A phase name is interpolated into file paths, into the `herdr pane run` command, and into the
remediation commands drovr PRINTS for a human to paste — three grammars, so a denylist would have to
be right in all of them forever. Rejecting at creation means no phase drovr mints from here on needs
quoting to be safe to mention. (Emission sites quote independently — `cli/src/shell.rs` — because run
and task names remain unrestricted.)

### Scope — an EXISTING phase is not affected

The strict alphabet gates creation only — a name being INTRODUCED. `require_phase_name`, used by
`phase done` / `phase wait` / `phase send` / `collect` **and by `phase start` when the phase already
exists**, keeps the older path-safety rule. So a phase an earlier drovr created under a now-illegal
name is still fully operable: its live agent can signal done, and it can still be RE-ENTERED, which
matters because `drovr phase start <run> <phase>` is the recovery drovr itself prints for a lost pass
token. Pinned by `a_phase_already_on_disk_under_an_old_name_is_still_reachable` and
`an_old_named_phase_can_still_be_re_entered`. **Do not "align" the two rules, and do not hoist the
strict check to the top of `phase_start`** — either bricks these phases with no migration path.

### Working around it

Name tasks and angles in the same alphabet drovr itself mints: `task-1`, `fix-login-bug`,
`type-design`, `api-contracts`. Hyphens instead of spaces. There is no opt-out, by design.

There is no validation at config load, so a bad `angle` is reported only when a panel is spawned —
the error names the whole phase name, which is where the offending angle is visible.

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

One exception, and it is exactly the row the UI flags as anomalous: a ZOMBIE — archived while
`workspace_close` failed — still has a live workspace and live panes recorded. Restoring one
and running `drovr phase start` should reuse them and work. The blanket "restore does not make
it runnable" is therefore wrong for the one case where the run was never really torn down.

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

## `GET /api/runs` now spawns a herdr subprocess on every poll

Found 2026-07-26. Accepted, not fixed.

The list endpoint calls `SystemHerdr::workspace_list()`, which shells `herdr workspace list`.
The page polls that endpoint every 2s while the session list is open, so each open tab spawns
a subprocess every 2 seconds for as long as it is open. Before this branch the endpoint was
pure filesystem reads.

It buys the liveness column, the zombie warning, and the archive confirm — all of which need a
fresh answer, and one call answers for every run at once (the per-run alternative is a herdr
round trip per row). Left as is because the cost is small and bounded per tab, but it is worth
knowing before leaving a review page open for hours, and it mildly amplifies the documented
"no authentication" surface: GETs are not covered by the write guard, so a page in the same
browser can drive that spawn loop.

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
Do not read a long green streak as proof the class is gone.

A later audit found the most likely cause without reproducing it: three sections asserted a
NEGATIVE ("the cursor stays on this row") immediately after a single `renderRunList` call.
Waiting for the expected value is useless there — it is already the current value — so they
implicitly depended on microtask ordering to have delivered the render, which CPU contention
can break. All three now wait for the render's observable EFFECT first (the row showing as
archived, or dropping out of the filtered list) before asserting the cursor. 125 consecutive
runs green since, including batches under deliberate load. Still not proof.

If it recurs: run with `-- --nocapture` to get the failing check name, and suspect a section
asserting immediately after `evaluate('renderRunList(...)')` rather than waiting for the
condition. The durable fix is to make every such section either `reload()` first or wait on
the state it is about to assert, rather than trusting a render to have painted.

## Only one of the six `save_preserving_archived` sites is redundant

Found 2026-07-26; **corrected 2026-07-26** after a review showed the original analysis was
wrong. The earlier version of this entry claimed three sites were redundant AND untestable,
and told the reader not to add coverage. Two of those three were both reachable and testable.

The false claim was that `code_review_run`'s poll loop makes no herdr calls. It does:
`agent_status` is the fallback the loop consults whenever a reviewer's done-marker is absent,
every iteration. On a RESUMED pass where every angle is still alive, `spawn_reviewer` is
skipped entirely — so the poll loop's own calls are the only ones in the pass, and no
spawn-time save exists to have rescued an in-flight archive first. A human archiving a run
while a resumed review polls is ordinary use, not an adversarial construction.

Both are now covered, and both fail if reverted to a plain `save`:

- the deadline save — `archiving_during_a_resumed_poll_survives_the_deadline_save`
- the final save — `archiving_during_a_resumed_pass_survives_the_final_save_too`

That leaves one genuinely redundant site: `cmd_code_review`'s save in `main.rs`. By the time it
runs, `code_review_run`'s own preserving saves have already re-read the flag into the in-memory
`RunState`, so a plain `save` there would write the correct value anyway. It stays preserving
for consistency, and because that redundancy depends on the callee's behaviour rather than on
anything local.

The lesson worth keeping: "this path cannot be reached" is a claim about code, and it needs
checking against the code rather than reasoning from the shape of a call graph. The earlier
entry deleted a real test on the strength of an unchecked one.

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

### Also hit on `code-review run` (2026-07-26, run `m3-schema-dos`)

Same trap, different command, and it produced a **false clean review**. The driver ran

```
drovr code-review run m3-schema-dos schema-dos-fix --timeout-ms 540000 2>&1 | tail -30
```

The harness reported exit 0; the driver read that as the skill's "exit 0 clean" and told the human
the panel had come back clean. The real status was **2 (timeout)** with `0 of 4 angles finished` —
no angle had reviewed anything. Re-running as `cmd > log 2>&1; echo "DROVR_EXIT=$?"` showed
`DROVR_EXIT=2` immediately.

So the hazard is not specific to `review wait`: for `code-review run` the misread is arguably worse,
because exit 0 there means "reviewed and clean" rather than merely "approved", so a piped invocation
can certify unreviewed code.

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
