# Known issues

**A fixed defect is DELETED from this file, not annotated as fixed.** What made an entry worth
recording was the live bug; once the fix is in the code and pinned by a test, the entry is a
record of the past that every reader has to work out no longer applies. Two exceptions, and only
these two: an entry that is still **partly open** stays — retitled so the heading says which half
is live, or corrected in place if the heading was already honest; and a retiree whose value was
never the defect — an expensive root cause guarded by nothing, or a rule that stops a settled bug
being re-filed — moves to **`## Lessons kept from retired issues`**. Before adding a `— FIXED`
marker, delete instead. If the fix is worth recording, it is worth recording in the code.

## With `review_agent = "opencode"` the seed reaches no composer at all — OPEN

**Severity:** high — `drovr code-review run` cannot complete, so the gate every task is supposed
to pass through is unreachable. Two consecutive iterations failed identically.
**Found:** 2026-08-07, `drovr code-review run skill-stickiness task-22`.

### Symptom

```
drovr: code-review run failed: phase 'review:task-22:1:correctness' … the seed was NOT
delivered — herdr saw no state change after the prompt, and the payload is nowhere in the
agent's composer, so it was swallowed rather than left unsubmitted.
```

Re-running without `--fresh` advances the iteration (`:1:` → `:2:`) and fails the same way.

### This is NOT the `[Pasted text #1]` case, and the standard recovery does not apply

The entry below on the panel's stalls documents **cursor** parking with the brief visible in the
composer as `→ [Pasted text #1 +46 lines]`, unsubmitted — where drovr's detector cannot see the
paste, the diagnosis is inverted, and `herdr pane send-keys <pane> Enter` recovers it.

**Check before applying that recovery.** Here the composer is genuinely empty:

```
$ herdr pane read <pane> --source visible --lines 45
   ┃  Ask anything... "Fix a TODO in the codebase"
   ┃  Plan · Qwen3.6 35B A3B (abliterated) ko.ag
```

Nothing pasted, nothing pending. Pressing Enter into that would send an empty prompt at best, and
drovr's error text says why it refuses to press a key on your behalf. **A `send-keys Enter` habit
formed on the cursor bug will silently do the wrong thing here** — read the pane first, every
time. The pane is also titled `OpenCode` rather than `Correctness Reviewer`, unlike the cursor
reviewer panes, which is a second cheap way to tell the two situations apart.

### Suspected cause — not confirmed

`review_agent = "opencode"` is set in `~/.config/drovr/config.toml`, which routes the panel to the
opencode backend. On this host that resolves to a local `Qwen3.6 35B A3B (abliterated)` model.
Whether the seed is lost in the launch, in the paste, or in opencode's composer handling was not
established — the failure was hit inside a task whose deliverable was something else, and
diagnosis stopped at two attempts rather than becoming a third.

### What to try next

1. Confirm the backend is the variable: temporarily point `review_agent` at `cursor` or `claude`
   and re-run the same task. If the seed lands, this is opencode-specific.
2. If it is, check whether it is the paste path or the launch: `herdr pane send-text <pane>
   'hello'` into a live opencode reviewer pane and see whether the text appears.
3. **Do not silently rewrite `~/.config/drovr/config.toml`** to work around it. It is the user's
   global config, the setting is plausibly deliberate (the `cross-model-arm` stage ran qwen
   through opencode), and swapping the reviewer changes what reviewed the work.

## `claude --plugin-dir <checkout>` loads the skills but NOT the hooks — OPEN

**Severity:** medium, and silent in the same way as the entry below it: the plugin's skills
appear and work, so the session looks correctly wired while neither reflex ever fires.
**Found:** 2026-08-07, running `spec.md` §9.4's integration check against a worktree.

### Why this one matters more than it looks

`plan.md` Task 22 §9.4 *prescribes* this method — *"run against this worktree with
`CLAUDE_PLUGIN_ROOT=<worktree>` so it does not depend on the flake pin being bumped"*. So the
check written to prove fix 2 works was, as specified, running with fix 2's entire hook layer
absent. Anyone re-running a §9.4-style check inherits that.

### Symptom

```
claude -p --plugin-dir <worktree> --output-format stream-json --verbose "<prompt>"
```

`Skill{"skill":"drovr:using-drovr"}` resolves and runs, so the plugin is clearly loaded. But no
`UserPromptSubmit` hook fires, no `hook_response` for it appears in the stream, and the gate
card's text (`DROVR GATE`) is nowhere in the session. `hooks/hooks.json` is present and correct
in the checkout.

### Distinguish it from the entry below

That one is a **packaging** gap — the Nix store path has no `hooks/` directory at all. This one
happens with a full repo checkout that *does* have `hooks/hooks.json`; `--plugin-dir` simply does
not register it. Same silent symptom, different cause, and fixing the flake will not fix this.

### Workaround

Wire the two hooks explicitly and pass them with `--settings`, setting `CLAUDE_PLUGIN_ROOT`
yourself since nothing else will:

```json
{ "hooks": {
    "SessionStart":     [ { "matcher": "startup|clear|compact",
      "hooks": [ { "type": "command", "command": "CLAUDE_PLUGIN_ROOT=<W> <W>/hooks/session-start" } ] } ],
    "UserPromptSubmit": [ { "hooks": [ { "type": "command",
      "command": "CLAUDE_PLUGIN_ROOT=<W> <W>/hooks/user-prompt", "timeout": 10 } ] } ] } }
```

**Verify it rather than assuming it.** Neither hook's output is echoed into `stream-json`, so a
silent no-op is indistinguishable from a working hook. Wrap each script in a two-line shell
wrapper that tees its stdout and byte count to a log; correct output is ~9500 bytes for the
SessionStart reflex and ~646 bytes for the gate envelope.

**And unset `DROVR_PHASE` first.** If you are running the check from inside a drovr phase it is
set in your shell, the session inherits it, and `hooks/session-start` no-ops by design — the
suppression contract working correctly, and easy to misread as this bug.

## Nested `claude -p` sessions can lose every tool but `Read` — `classifier unreachable: 404`

**Severity:** medium — blocks any probe that needs a real agent to *act*, while leaving one that
only needs to observe which skill it reaches for intact.
**Found:** 2026-08-07, mid-way through `spec.md` §9.4's integration runs. Earlier runs the same
hour were unaffected, so it appears and disappears on its own.

### Symptom

Every `Bash`, `Write`, `Edit`, `Skill` and `ToolSearch` call in a `claude -p` subprocess returns

```
classifier unreachable: HTTP Error 404: Not Found
```

`Read` keeps working. The agent retries each call once, then reports itself blocked. Nothing is
written to disk, and the parent session is unaffected.

### It is environmental — check before blaming your harness

Two hours were nearly spent bisecting `--plugin-dir`, `--settings` and `DROVR_PHASE` against it.
None of them matter. The one-line check:

```
claude -p --permission-mode acceptEdits "run: echo hello" 2>&1 | grep -c 'classifier unreachable'
```

No plugin, no settings, no drovr. Non-zero means the service is down and no flag will help.
Related in kind to the `submit_findings`/`ToolSearch` 502 recorded further down this file: the
same class of classifier outage, a different endpoint and status.

### What a probe can still conclude while it lasts

**Which tools an agent reaches for, and in what order** — the calls are still emitted, and only
their results fail. §9.4's first-two-tool-calls verdict was recorded through the outage for
exactly that reason. **What it cannot conclude** is anything about the *effect* of those calls:
an attempted `Write` before an attempted `Edit` is an ordering, not an applied change. Say which
of the two you measured.

## The Nix-installed plugin ships no hooks, so neither reflex ever runs — OPEN

**Status:** open, pre-existing — the `UserPromptSubmit` per-turn gate inherits the gap
rather than causing it.

**Severity:** medium (silent — the plugin loads, its skills work, and both reflexes are
simply absent with no error anywhere).
**Found:** 2026-08-03, while wiring `hooks/user-prompt`: reviewing what a store-path
install actually contains.

### Symptom

Install the plugin from the flake's output rather than from a repo checkout and neither
`SessionStart` nor `UserPromptSubmit` fires. No error is printed; `$out/share/drovr/`
simply has no `hooks/` directory, so Claude Code finds no `hooks.json` to read.

### Root cause

`flake.nix`'s `postInstall` copies exactly two trees:

```
cp -r ${./skills} $out/share/drovr/skills
cp -r ${./.claude-plugin} $out/share/drovr/.claude-plugin
```

`hooks/` was never added. Claude Code discovers hooks by reading `hooks/hooks.json`
under the plugin root, so an absent directory is indistinguishable from a plugin that
declares no hooks. Nothing fails loudly because nothing is expected to.

### Fix

Add `cp -r ${./hooks} $out/share/drovr/hooks` to `postInstall`, and check the exec bits
survive the store copy (both scripts are mode 755 in the index, pinned for the gate by
`cli/tests/reflex_hook.rs::user_prompt_hook_is_executable_in_the_index`). Deliberately
NOT done as part of the gate task: it is a packaging change affecting both hooks equally
and deserves review as one.

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
touch ~/.local/share/drovr/runs/<run>/<phase>.done
```

An empty marker is accepted for a phase with no recorded `pass`, which is exactly the pre-token
case.

## Cross-run state leaks in the review UI — a bug CLASS, with follow-ups still open

**Kept, though its original defect is fixed.** Fixed entries are deleted from this file; this one
stays because the defect was never the point — the same mistake was found **five** times in the
one page (the spec panel, annotations, three more sites the review panel caught on the fix, and a
sixth the fix itself introduced), and `### Still open (follow-ups, not blocking)` below is live.
Read it as "here is a mistake this page keeps making", not as a bug to reproduce.

**Status:** the original defect is fixed on `drovr/review-ui-stale-doc`. `refresh()` now clears and hides the doc
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
turn badge, summary banner and (the then-current) questions panel all correctly updated to the
new run, so the
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

## An interrupted `cargo test` can wedge the whole bin suite permanently — OPEN

**Severity:** high (225 of 803 tests fail, on every subsequent run, in every worktree on the
machine, and none of the 225 failure messages names the cause. It stays wedged until someone
chmods a directory back).
**Found:** 2026-08-07, run `ask-channel`, task 8 — after a `drovr code-review run` was killed
by a 10-minute shell timeout mid-suite.

### Symptom

`cargo test --bin drovr` reports a few hundred failures across `phase::`, `run::`, `review::`
and the root `tests::` module. Almost all of them are:

```
called `Result::unwrap()` on an `Err` value: PoisonError { .. }
```

Re-running does not help. Running any one of the 225 alone passes. The suite was green minutes
earlier and nothing in the working tree changed.

### Root cause

Two ordinary things that are fine apart.

1. **`phase.rs`'s `capture_run` isolates onto a FIXED path**, not a fresh tempdir:
   `set_var("XDG_DATA_HOME", format!("/tmp/drovr-capture-test-{name}"))`. Every run of the
   suite — and every worktree on the machine — shares it.
2. **`a_capture_whose_save_failed_is_also_retried` chmods that run dir read-only** (`0o555`) on
   purpose, to force a save failure, and restores it at the end of the test.

Kill the process between those two points and `/tmp/drovr-capture-test-save-fails/drovr/runs/save-fails`
is left at `dr-xr-xr-x` **forever**. The next run's `capture_run` does
`remove_dir_all(run_dir(name))` into it, `phase_start` then fails with `PermissionDenied`, and
the test panics **while holding `ENV_LOCK`** — poisoning the process-global mutex. Every other
test that takes `ENV_LOCK` (which is nearly all of them, since it guards the `XDG_DATA_HOME`
mutation) then dies on `.lock().unwrap()`.

So one leftover directory bit produces hundreds of failures whose messages point at the mutex,
not at the directory. The three earlier "flakes" this task recorded — ten `code_review::tests`
in one run, one `herdr::tests` in another — were the same cascade landing on a different
thread schedule.

### Workaround

```
chmod u+w /tmp/drovr-capture-test-save-fails/drovr/runs/save-fails
```

Or delete `/tmp/drovr-capture-test-*` entirely; the tests recreate them. **Check first that no
other agent's suite is mid-run** — the path is shared across worktrees, which is half the bug.

### Diagnosis, when it happens again

The poison messages are noise. Find the one panic that is **not** a `PoisonError`:

```
cargo test --manifest-path cli/Cargo.toml --bin drovr > /tmp/bin.log 2>&1
grep -A2 'panicked at' /tmp/bin.log | grep -v PoisonError | grep -A2 'panicked at'
```

### Fix directions (none applied)

1. **Isolate onto a tempdir**, as the other helpers do, so nothing is shared or persistent.
   `capture_run`'s fixed path is the whole reason a leftover can outlive the process.
2. **Restore the mode from a guard**, not from a straight-line statement, so a panic or a
   kill still puts it back.
3. **Recover from a poisoned `ENV_LOCK`** (`unwrap_or_else(|e| e.into_inner())`) so one
   panicking test fails one test. This alone would have turned 225 failures into 1 — and the
   1 would have named the directory.

## `cli/tests/web_nav.rs` can fail with a CDP timeout even though nothing changed

**Severity:** low (a flaky test, not a product bug — but it makes `cargo test` red and can be
mistaken for a regression the current task introduced).
**Found:** 2026-08-01, run `skill-stickiness`, task 1.

### Symptom

`cargo test` reports `web_keyboard_navigation ... FAILED`, with the driver's stderr showing:

```
Error: Page.navigate: no CDP response within 20s
    at .../cli/tests/web/nav.mjs:52
```

The same test passed earlier in the same worktree with no intervening change to
`cli/web/index.html`, `cli/src/review.rs`, or the test itself.

### Root cause

Not fully diagnosed. The test's own prerequisite checks (`node` and a chromium binary on PATH)
both pass, so it does not take its skip path — it boots headless chromium, connects to the debug
port, and then `Page.navigate` never answers. The 20s budget in `nav.mjs` is the only timeout,
so a chromium that starts slowly or contends with an already-running browser instance fails the
whole test rather than retrying.

### Telling it apart from your own regression

Do not assume it is yours, and do not assume it is not. A/B it against `HEAD` — copy your
uncommitted files aside, `git checkout --` them, re-run `cargo test --test web_nav`, then copy
them back. (Do **not** `git stash`: the stash stack is repo-global and will pick up other
worktrees' parked work.) If it fails identically with your changes reverted, it is this issue.

`web_nav` is the only browser-dependent test in the suite; the other four binaries
(`e2e`, `reflex_hook`, `skills_valid`, and the unit tests) are hermetic.

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

## The panel can review less than you think — the PARTIAL commit is still open

**Kept, though the empty-range half is fixed.** Fixed entries are deleted from this file; this
one stays because only half of it closed. An **empty** `base..HEAD` is now refused outright
(see the fix section below), but the **partial** commit — a real, non-empty range that is
missing the work you did not commit — is open and deliberately unguarded. That is the live
hazard, and it is why "commit everything first" is a rule and not advice.

**Severity:** high — it manufactured the exact signal the pipeline uses to advance a task, and a
vacuous clean is indistinguishable from a real one.
**Found:** run `skill-stickiness`, task 3, panel 1, 2026-08-02. **Fixed** the same day, in run
`panel-roles` — see the fix section at the end of this entry.

### Reproduction

```
~/.local/share/drovr/runs/skill-stickiness/task-3-base.sha      5c8a7da
~/.local/share/drovr/runs/skill-stickiness/task-3-review-1.head 5c8a7da   <- identical
```

Base equals head. The reviewed range `5c8a7da..5c8a7da` is empty. All four angles
(`task-3-review-1-{correctness,error-handling,security,type-design}.json`) returned
`"verdict": "clean"`, `"findings": []` — and nothing anywhere said the range was empty.

The agent had authored 17 scenario files and committed none of them, so nothing had moved HEAD
since the base was recorded.

**When `base == HEAD`, the panel returns a clean verdict having read no committed change.**

### State the condition exactly

It is tempting to write this as "uncommitted work reviews as clean." That is wrong, and the wrong
version is more dangerous than no note at all, because it tells an agent that one commit makes it
safe. The precise statement:

- The committed scope is `git diff <base>..<head>`. It is empty when that range **contains no
  change** — usually because nothing was committed since `drovr code-review base` ran, so
  base == head. **But equal SHAs are not the property.** `git commit --allow-empty` advances HEAD
  without touching the tree, so base != head while the diff is still empty. The first version of
  the guard below compared the two SHAs and this case walked straight past it.
- Commit some of the work and HEAD moves past base, so the range is real and the panel can return
  real findings. Whatever is *still* uncommitted is simply outside that range.
- So the hazard has two shapes: the empty range (this entry), and the quieter one where a partial
  commit yields a real-looking review whose scope silently excludes the rest of the work.

The seed does tell reviewers the scope is `git diff <base>..<head>` "**plus** the current working
tree" (`cli/src/code_review.rs:326`), so uncommitted work is nominally in scope. That clause is
not a substitute for a real range. Untracked files never appear in a `git diff`, and in the
observed case there were 17 of them and all four angles still concluded clean. A prose
instruction to also look around is not a scope, and it should not be relied on as one.

### Why it is dangerous, not merely useless

A clean verdict is not decoration — it is the signal `drovr:pipeline` branches on to advance a
task (exit 0 → proceed to task N+1). This one is byte-identical to a real pass: same schema, same
four angles, same exit code. Nothing downstream can tell the two apart, so the vacuous pass
inherits the authority of a genuine one.

That makes it the **fifth appearance of the vacuous-pass class in a single run** — a check that
reports success by not checking anything. Each was found and fixed in the run's own reports:

1. **task 1** — a git-missing skip that printed ok.
2. **task 1** — assertion 4 compared nothing and passed.
3. **task 2** — `discover_corpus_roots` dropped `read_dir` entry errors and unreadable version
   dirs, so the overlap test could pass having indexed only part of the corpus
   (`task2-report.md`, panel round 2, finding 3).
4. **task 2** — the overlap check guarded *our* side against producing no shingles but not the
   *corpus* side; the report calls the asymmetry "worse than neither being guarded, because it
   reads as deliberate" (`task2-report.md:17`).
5. **this** — the panel's own empty range.

Four of the five were defects the panel *caught*. The fifth is the panel committing it. As
`task2-report.md:116` puts it: it recurs "because a vacuous pass and a real one are
indistinguishable from outside."

### FIXED 2026-08-02 — an empty range is refused, and cannot be spelled `Clean`

**Refuse to run when `base..head` contains no change, and say why.** An empty range is always a
mistake, and there are only two ways to reach it:

- the base was recorded *after* the work was committed (`drovr code-review base` run too late), or
- nothing has been committed yet — the usual case, since the panel is reached at the end of a task
  when uncommitted work is exactly what is on hand.

Neither is a state in which a clean verdict means anything, so neither produces one. What shipped:

- **A new `ReviewOutcome::EmptyRange` variant**, not a special case at the call site. The fix was
  type-level: vacuous and real clean shared one outcome, and one `if` at one call site would have
  left the illegal state representable for the next caller.
- **The check asks what the range CONTAINS** (`range_is_empty`, via `git diff --quiet
  base..head`: exit 0 = no differences, 1 = differences). **It deliberately does not compare the
  two SHAs.** The first version did, and `git commit --allow-empty` defeats it — HEAD advances,
  the tree does not, so `base != head` with an empty diff and the vacuous `Clean` returns
  untouched. That near-miss is the entry's own lesson recurring inside its fix: *equal names* is
  not *equal content*, and only one of them is the property worth checking.
- **"Could not tell" is not "not empty".** If git cannot answer (an unresolvable base, say), the
  guard prints a loud warning saying it did not run, and proceeds — it does not silently treat
  the range as non-empty, and it does not refuse a pass that worked before the guard existed.
- **Refused before any reviewer is spawned**, so a vacuous panel costs nothing rather than four
  reviewer panes.
- **Exit 1**, the setup-error channel the pipeline's failure model already routes to
  STOP-and-diagnose. It does **not** warn and proceed: a warning on stderr next to a `clean`
  verdict and a 0 exit is read as the verdict.
- **Three tests.** `an_empty_review_range_is_refused_before_any_reviewer_is_spawned`,
  `a_non_empty_range_still_reaches_the_reviewers`, and — the one that carries the property —
  `an_empty_commit_is_refused_even_though_the_shas_differ`. Both guards were mutation-tested:
  reverted to the SHA comparison, the empty-commit test failed with `left: Timeout, right:
  EmptyRange` (the panel had spawned reviewers over nothing) while the other two still passed,
  which is exactly why that third test earns its place.

**Still open, deliberately:** the *partial* commit. It produces a real, non-empty range whose
verdict is honest but silently narrower than the task's work, and nothing detects that — the
panel cannot know what you meant to commit. `implement-task.md` tells the task agent to keep
committing, and `ReviewOutcome::Clean`'s doc states in terms that a clean verdict covers the
committed range and not the task. Both are documentation, not enforcement. Fixing it would mean
redesigning what the panel's scope IS (e.g. refusing on a dirty tree), which is a larger question
with real false-positive cost.

## An author-run panel is not a gate: five author-run panels, then the driver's caught what they missed (2026-08-02)

**Severity:** medium — the author's own verdict silently becomes the release decision, so the
defect ships and the review cost is paid twice.
**Found:** run `skill-stickiness`, task 3 (authoring the 17-scenario pressure corpus), 2026-08-02.
**Not a bug in `code_review.rs`** — the panel worked exactly as built. The defect was in the docs:
nothing distinguished *running* the panel from *being gated by* it.

### What happened

The task agent ran `drovr code-review run` **five times** on its own work (four angles each:
correctness, error-handling, security, type-design), fixing what it found — 7 Important and 1 nit
across the five — and committed `b16dea0 fix(writing-skills): remove a self-contradiction and a
collapsed pressure set`. Its fifth panel returned clean on all four angles. It declared the task
done.

The **driver** then ran a sixth panel. The `correctness` angle returned an Important:

> `skills/writing-skills/scenarios/verification-before-completion-1.md`: "Pressure list still
> collapses to two effective levers: time, social, and exhaustion are all hurry-up variants;
> pragmatic is the only distinct one."

That is the same defect class `b16dea0` had just claimed to fix. The agent had correctly
identified it, fixed it incompletely, and then adjudicated its own fix as sufficient.

### The sharp version — a controlled comparison, not a count

The count of five is the weaker fact, and one of the five was vacuous (see the entry above), so
only four were real passes. The evidence that carries this entry is the pair at the end, which is
controlled: panels 5 and 6 reviewed the **identical tree**.

```
task-3-review-5.head: b16dea0de3ee89ad14b2f2ba22fcea9548846756   author-run  → 4/4 angles clean
task-3-review-6.head: b16dea0de3ee89ad14b2f2ba22fcea9548846756   driver-run  → correctness: 1 Important
```

Same base, same HEAD, same four angles, same reviewer configuration. The only variable was who
invoked it. (Artifacts: `~/.local/share/drovr/runs/skill-stickiness/task-3-review-{5,6}-*.json`.)

### Root cause — and what it is *not*

Running the panel on your own work is good practice, and the five self-runs found real defects:
7 of their 8 findings were Important, and every Important was fixed. That is the panel working
as a test suite, which is what it should be. The defect is that **nothing named the difference** between
that use and the acceptance gate, so a clean author-run verdict was read as permission to report
done — and `drovr code-review run` offers no signal to tell the two uses apart.

`skills/pipeline/SKILL.md` said the driver runs the panel; it never said an author-run panel is
*not* the gate. `phase-prompts/implement-task.md`'s *"Self-review before reporting done"* step told
the task agent to self-review with `drovr:code-review`, which reads as licence to substitute the
panel for the decision. (Named by title, not by number: that file's steps renumber whenever one is
inserted, and a stale number sends a reader to the wrong step with no sign anything is wrong.)

Two nearby effects worth knowing:

- **The first of the five was vacuous, so only four were real passes.** Panel 1's head equalled
  the recorded base (`5c8a7da`), so it reviewed an empty diff and reported clean from all four
  angles. The agent caught this itself. It is a *different* defect class, recorded above — see
  "The panel can review less than you think — the PARTIAL commit is still open". It does not
  weaken this entry, because this entry's evidence
  is the controlled 5-vs-6 comparison, not the count.
- **Cost.** Task 3 was reviewed 6×. Of the five author-run rounds, three (panels 2–4) returned
  findings; panel 1 was the vacuous one and panel 5 was clean. The sixth is the one that decided
  anything. An author-run panel is not wasted — three of them earned their keep — but it is
  never the one that counts.

### Why this is expected, not a fluke

The panel is sampled and non-deterministic, so run 6 is not magic — it is one more independent
draw. That is precisely the point: a single clean sample is evidence, not proof, and the party
who wants to stop is the worst party to decide the sampling is finished.

This repo already encodes the argument one level down. `spec.md` §7.3 of the `skill-stickiness`
run mandates that arm B be scored by a read-only reviewer that is **not arm B's author**, with
arm labels stripped, because "unblinded self-scoring by arm B's author is exactly what the
replication literature this spec cites warns about." The panel gate is the same argument one
level up. Consistent with, though not proof of, the published finding that intrinsic
self-correction without external feedback can degrade output (Huang et al., *Large Language
Models Cannot Self-Correct Reasoning Yet*, ICLR 2024, arXiv:2310.01798) and that self-correction
depends on reliable external feedback (Kamoi et al., TACL 12 (2024) 1417–1440,
arXiv:2406.01297). Panickssery et al. (*LLM Evaluators Recognize and Favor Their Own
Generations*, NeurIPS 2024) is about an evaluator scoring its own generations, which is not quite
this case — the reviewers here were independent agents — but it bears on the adjudication step,
which is the one that failed.

### Fix in place — documentation, deliberately

The same paragraph now appears verbatim in `skills/code-review/SKILL.md`,
`skills/pipeline/SKILL.md` and `skills/pipeline/phase-prompts/implement-task.md`: anyone may run
the panel, only the driver's run is the gate, a clean author-run verdict is evidence and never
permission. `implement-task.md` additionally requires the task report to list every panel the
agent invoked itself, labelled author-run, so the distinction is visible downstream.

**No mechanism was added, and this is the known gap.** `drovr code-review run` has no caller
identity and cannot acquire one honestly: both roles run the same command on the same machine, so
any "who ran it" field would be self-declared — a permission system agents route around, and
worse, one that launders a self-declaration into the appearance of authority. The task report's
author-run labels are self-reported *facts about the past*, which is a different thing from a
self-declared *right*. What enforces the gate is the driver running its own panel after every
task, unconditionally, regardless of what the report claims.

## `drovr cleanup` auto-commits whatever the worktree is holding (2026-08-02)

**Severity:** low, but it puts junk — including large binaries — into your branch's history under
a message that describes the RUN, not the content.

`drovr cleanup <run>` commits the worktree before pruning it, which is the right default: it is
how work in progress survives a teardown. What it does not do is distinguish work from litter. On
run `fix-lock-flake` it produced:

```
2e23622 drovr(fix-lock-flake): the serve-lock test shares the developer's lock path, …
 cli/rust_out | Bin 0 -> 4517008 bytes
```

A 4.5 MB binary — `rust_out` is what a bare `rustc foo.rs` writes when nobody passes `-o`, left
behind by something probing in that checkout — committed under the run's task description, which
made it read like a real change.

**Consequences to expect:**

- `git branch -d <run branch>` will REFUSE after cleanup, because that auto-commit is not in
  `main`. If you force it without looking, you are discarding a commit you never read. Check what
  it contains first (`git show --stat <branch>`); it may be litter, or it may be work.
- The commit message names the run's task, so the diff is the only thing that tells you which it
  is.

**Mitigation in place:** `rust_out` is now in `.gitignore`, alongside `target/` and `.drovr/`.
That closes the specific case; the general one — cleanup committing anything untracked — stands
by design.

**Before running cleanup on a worktree you have been probing in:** `git status --short` it, and
remove or ignore what you do not want in the branch.

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

## One failing test cascades: a panic while holding `ENV_LOCK` poisons it (2026-07-27)

**Severity:** low, but it wastes debugging time.

The env-dependent tests serialize on `test_util::ENV_LOCK` and take it with
`ENV_LOCK.lock().unwrap()`. A test that panics *while holding* it poisons the mutex, so every
later test that locks also panics — one real failure reports as several. Seen while fixing the
round-1 review findings: one genuine assertion failure in `brief::tests` surfaced as three
failures, two of which passed in isolation.

If a run reports N failures, re-run the first one alone before believing the other N-1.

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
drop).

**The third sighting delivered that evidence (2026-08-02, run `phase-reap`, task 5's fixes).**
The message was:

```
after drop: …/server.pid contains "1123224"; this pid is 1123224
```

Two things follow, and both narrow it:

- **The pid file held OUR OWN pid.** So the step that failed is the re-claim after `drop(held)` —
  `try_take_lock` returned `WouldBlock` on a path nothing but this test has ever named, against a
  lock this very process had just released. It is not another writer and not a stale file from an
  earlier run; there is no second claimant to find.
- **The rate was measured at this branch's test count: 1 failure in 8 full `--bin drovr` runs.**
  It fired twice in three consecutive runs at one point, which read as a regression and was not —
  8 runs put it back on the documented 5–8%. Before calling a red suite a regression, measure;
  three runs is not a sample.

**A fourth sighting, 2026-08-04, reproduced the signature exactly** (run `phase-reap`, task 7's
review round, on a docs-only tree):

```
a lock released by dropping its File was still held.
after drop: /tmp/drovr-review-test-lock-claimbpQZgS/server.pid contains "2430561";
this pid is 2430561
```

Same step (re-claim after `drop`), same own-pid evidence, on a tree whose only changes were
markdown. That rules out this branch's code as a contributor and makes the signature stable
rather than a one-off reading. It passed on the immediate re-run and on a second full suite.

**The rate may be machine- or load-dependent, and the documented 5–8% may be low.** Across task
7's review rounds it fired **3 times in ~8 full-suite runs** on one machine — the same binary,
the same test count, only markdown changing between runs. That is too small a sample to restate
the rate from, and it does not contradict the earlier 1-in-8 and 2-in-30 measurements so much as
suggest they are not a single number. If you are measuring, record the machine and whether
anything else was loading it.

This is consistent with the `O_CLOEXEC` / fork-window hypothesis in the entry below (a concurrent
`Command::spawn` elsewhere in the suite briefly duplicates the fd between fork and exec, and an
inherited open file description holds the flock). It does not confirm it — confirming means
correlating red runs with a spawn, which still has not been done.

**If you hit it:** paste that message here rather than re-investigating from scratch.

## The code-review panel's stalls — what is fixed, and the unsubmitted seed that is not

**Kept, partly fixed.** Fixed entries are deleted from this file; this one stays because two of
its three halves closed and one did not. **Fixed:** the pane-attach race (commit `c12adb0`, on
`main`) and the findings channel, which is now the file `submit_findings` writes rather than a
scraped transcript. **Still live:** the seed that lands in the composer unsubmitted, so the
reviewer never starts. Its misdiagnosis notes are kept deliberately — the obvious checks lie here.

**Severity:** medium (when it stalls, the automated review-until-clean panel is unusable; the
driver must fall back to spawning its own read-only reviewer).
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
panel reads the pane the emitted JSON has left the recent window.

**That half is now fixed and the description above is history.** At the time, the reviewer seed
said *"Emit the fenced JSON, then exit"*, so reviewers wrote **no** `<task>-review-<angle>.json`
and the lossy transcript was the only channel. Fix idea 1 below shipped: reviewers now submit
through the MCP `submit_findings` tool, which performs the write for them, `obtain_findings_json`
reads **only** that file — there is no transcript path left in it at all — and
`code_review::delivered_review` treats it as the completion signal. The seed now asserts the
opposite of what it used to: reviewers are told *not* to print JSON.

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

1. **Make the findings channel durable, not a viewport.** — **SHIPPED**, though not as written
   here: reviewers run read-only and so cannot write the file themselves, so `drovr mcp-findings`
   exposes a single `submit_findings` tool and drovr performs the write for them.
   `obtain_findings_json` reads **only** that file — it is the sole channel findings enter drovr
   through, and it is the completion signal (`code_review::delivered_review`).
2. ~~If the transcript must stay the channel, read full scrollback rather than
   `source:"recent"`.~~ **Dead advice.** The transcript is not the channel and cannot become one:
   `herdr agent read` truncates long lines mid-word (see "Lessons kept from retired issues"), so
   scraping cannot be made correct and drovr does not attempt it.
3. Submit the seed reliably (see the entry below) — one fix removes failure mode 1 for the panel,
   `phase send`, and the review gate at once.

**Workaround used:** send `enter` to each reviewer pane after the panel spawns them, then read the
four panes directly and hand-merge into `<run_dir>/<task>-review.json`. Both passes of task-1's
review were merged this way, and both produced real, actionable findings — the reviewers work; only
the plumbing around them fails.

Reading a reviewer pane (`herdr agent read <pane>`) shows the full seed rendered in the composer
with the correct `base..head` scope, so seeding and scope selection are fine; only the submit
keystroke is missing. This is the same bug as *"`drovr phase send`: the false success is fixed;
`until` is still a LEVEL, not an edge"* (below) — the panel is simply its most visible victim.
The false-success half landed 2026-07-30; the underlying race did not, so keep the
self-spawned-reviewer workaround above.

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

## `drovr phase send`: the false success is fixed; `until` is still a LEVEL, not an edge

**Kept, half-fixed.** Fixed entries are deleted from this file; this one stays because only the
first half closed — a send that does not take now exits 2 naming which failure it was, instead of
exiting 0 (see `### Status` below). What is live is the `until` semantics in `### Still open`.

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
   the readiness race described under "The code-review panel's stalls" reaching
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
- pane already `done` — very narrow, because `done` is momentary (see *"herdr's `agent_status:
  "done"` is an EDGE, not a level"* under "Lessons kept from retired issues"): an
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

## `lock_records_our_pid_and_releases_on_drop` flakes at ~5–8%, independent of test threads

**Severity:** low as a bug, higher as a process hazard — it fails the nix build, so it can
break `home-manager switch` for an unrelated change, and a *green* nix build is therefore not
evidence the test is sound.
**Found:** 2026-07-26, twice and independently: during task 3 of run `phase-reap`, and on
`52db1cd`.

### Symptom

```
---- review::tests::lock_records_our_pid_and_releases_on_drop stdout ----
panicked at src/review.rs:2326:14: released lock must be free
```

The test (`cli/src/review.rs:2311`) claims the server lock on `tmp.path().join("server.pid")`
in its own fresh `tempfile::tempdir()`, drops the `File`, and re-claims it. Intermittently the
second `try_take_lock` comes back `Ok(None)` (`TryLockError::WouldBlock`) for a lock it just
released.

### Measured

| how it was run | result |
|---|---|
| `cargo test lock_records_our_pid` alone | 100% green |
| the `lock_*` tests alone, 25 consecutive runs | 25/25 green |
| the whole `--bin drovr` suite, 12 consecutive runs (`52db1cd`) | **1/12 red** |
| `cargo test --bin drovr`, 30 consecutive runs (`8173f03` / `310fa7f`) | **2/30 red** |
| the same tree with task 3's changes applied, 45 runs | ~3/45 red |
| nix sandbox build of `52db1cd` (`home-manager switch`) | red once, green on immediate retry |

The rate is the same with and without task 3's changes, so it is **not** caused by anything
task 3 did. It passes 100% when run alone, so it is a whole-suite interaction, not a bug in
`try_take_lock`'s logic.

### It is NOT the env-pollution flake above

That one is `XDG_DATA_HOME` pollution across parallel tests. This test touches no
process-global env and locks a path under a `tempfile` root that no other test can name — its
own doc comment (`cli/src/review.rs:2306-2309`) already claims immunity to the env flake, and
that claim holds. Both are parallelism-only; they are different causes. Something else
releases late.

### Hypothesis, not yet confirmed

An fd inheritance window. The lock is `std::fs::File::try_lock`, whose Unix release is per
open-file-description, and an open file description survives `fork` — so a concurrently
spawning test (several here start real servers) transiently holds an inherited copy of this fd
between its `fork` and its `exec`. Rust sets `O_CLOEXEC` on files it opens, so the child drops
it at `exec`, which is exactly why the window is narrow and the failure rare. Confirming it
means tracing whether the red runs coincide with a process spawn; that has not been done.

### Workaround

Re-run — the failure does not reproduce twice in a row. Before believing a red `cargo test`,
check whether the ONLY failure is this test. That habit is also how real regressions hide,
which is why this is written down rather than tolerated.

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

## `code-review brief` tells a hand-spawned reviewer to call a tool it does not have

**Severity:** medium (every hand-spawned reviewer burns turns hunting a missing tool, and one
may silently drop its findings instead of reporting them in prose).
**Found:** 2026-08-02, run `skill-stickiness` task 4.

### Symptom

All four reviewers spawned with `drovr code-review brief … --angle <angle>` output — pasted
verbatim into an Agent-tool subagent, exactly as `drovr:code-review` instructs — reported the
same thing: the brief directs them to deliver findings via `submit_findings`, and no such tool
exists in their toolset. Each spent several turns searching (`ToolSearch` for `submit_findings`,
`mcp__drovr-findings__submit_findings`, `findings`, `drovr`, `submit`) before giving up and
reporting in prose. One opened its final message by saying it could not deliver the review
through the sanctioned channel.

### Root cause

`submit_findings` is provided by the `drovr-findings` MCP server, which `drovr code-review run`
wires into the reviewer panes it spawns. `code-review brief` emits the *same* brief text for
both paths, so a reviewer spawned by hand — the path `drovr:code-review` documents as the
fallback "when the panel is not available or is wedged" — is told to call a tool only the panel
provides.

### Workaround

Tell the subagent in its spawn prompt that `submit_findings` is unavailable and to return the
findings JSON as its final message. The reviewers still do the work correctly; only the delivery
channel is missing.

### Fix idea

Have `code-review brief` take a flag (or detect the absence of the MCP server) and emit a
prose-output instruction instead of the `submit_findings` one — the brief already owns the
findings schema, so only the delivery sentence differs.

## Concurrent writers lose whole phases from `state.json` — OPEN

**Status:** open. Reported by the `skill-stickiness` driver on 2026-08-06 and **verified from
this worktree**: `cross-model-arm` is absent from the run's `phases` array while its agent is
mid-measurement, and `run.lock` exists beside it.

**Severity:** high (silent, and it desynchronises drovr's record from reality in the direction
that loses work — the run believes a phase does not exist while that phase is writing to the
repository).
**Found:** 2026-08-06, run `skill-stickiness`, phase `cross-model-arm`.

### Symptom

A phase added by `drovr phase start` disappears from
`~/.local/share/drovr/runs/<run>/state.json` while other phases in the same run are running.
Downstream, every command that resolves the phase by name fails: `drovr phase wait` reports
`phase not found` and exits 1, `drovr phase done` refuses, and the completion marker is never
written. The phase's agent is unaffected and keeps working, which is what makes this worse
than a crash — **work proceeds while drovr believes it does not exist**, and the driver has to
poll the pane instead.

### Root cause

A lost-update race between concurrent state writers. `run.lock` exists but did not serialise
the update: the writer appears to read-modify-write from an in-memory copy of `state.json`
taken *before* the new phase was appended, so its write reinstates the older phases array over
the newer one. Nothing detects it because both writes succeed and the resulting file is
well-formed — the phase is simply not in it.

### Fix direction

Hold `run.lock` across the **whole** read-modify-write rather than around the write alone, or
re-read and merge immediately before writing. The second is the safer of the two if any writer
is outside the lock's reach.

## Externally-closed panes leave stale refs that degrade polling — OPEN

**Status:** open. Reported by the `skill-stickiness` driver on 2026-08-06.

**Severity:** low (noisy rather than wrong, but the warning misdirects triage).
**Found:** 2026-08-06, run `skill-stickiness`, after finished review panes were reaped.

### Symptom

Closing a phase pane outside drovr leaves its id in `state.json`. A later `drovr phase start`
on the same run then emits, once per stale pane:

> `herdr's pane.get failed for pane <id>: pane not found ... Agent status polling is degraded —
> phase sends and waits will run to their timeouts`

Observed for `wCG:pB8`, `wCG:pB9` and `wCG:pBA`.

### Root cause

Pane refs are never dropped for phases in a terminal state, so a `pane_not_found` is treated as
a live-pane failure. The warning then generalises from one dead pane to the whole run: it
claims polling is degraded for *"phase sends and waits"* in general, when only the phases whose
panes are gone are affected. A reader triaging a slow phase is pointed at a healthy mechanism.

### Fix direction

Drop the pane ref when a phase reaches a terminal state, or prune on `pane_not_found` rather
than warning on every subsequent invocation. Either way, scope the warning's wording to the
panes that are actually missing.

## `drovr attach` panics under a non-tty — OPEN

**Status:** open. Reported by the `skill-stickiness` driver on 2026-08-06 and **reproduced from
this worktree** with `drovr attach skill-stickiness < /dev/null`.

**Severity:** low (loud, and the workaround is to read `state.json`) — but the failure mode is
a panic where a message belongs.
**Found:** 2026-08-06, run `skill-stickiness`.

### Symptom

```
drovr: attaching to phase 'implement-task-8' of run 'skill-stickiness'
thread 'main' panicked at ratatui-0.30.0/src/init.rs:299:16:
failed to initialize terminal: Os { code: 6, kind: Uncategorized, message: "No such device or address" }
```

`drovr attach <run>` initialises a ratatui terminal unconditionally and panics when stdout is
not a tty. It matters because `attach` is the natural command an *agent* reaches for to find a
run's pane, and an agent never has a tty — so the command most likely to be run headless is the
one that cannot be.

### Root cause

No tty check before `ratatui::init`. The panic is inside the vendored ratatui rather than in
drovr's own code, so the message names a dependency's line number and says nothing about what
the caller should do instead.

### Fix direction

Detect a non-tty on stdout and print the resolved pane id plus a hint (how to attach from a
terminal, or where the run's state lives) instead of initialising the TUI. Note the command
also picks a phase to attach to before failing — it announced `implement-task-8` on a run whose
latest phases are far later — so whatever replaces the TUI path should make the phase-selection
rule visible rather than inheriting it silently.

## `cargo test` deleted the real `~/.local/share/drovr`, twice (2026-08-06)

**Severity:** critical — it destroyed the run directories of **every** run on the machine
(~65), including four other agents' in-flight work. No code was lost; git was untouched.
**Found:** 2026-08-06, run `brainstorm-rework`, during task 3.

### Symptom

`~/.local/share/drovr/` vanished entirely — every run's `spec.md`, `plan.md`, `*-HANDOFF.md`,
`feedback.json` and `state.json`. drovr then silently recreated an empty data root and carried
on, so the first visible sign was `drovr phase wait` exiting 1 with
`failed to load run '<run>': No such file or directory`.

### What is established

It happened **twice**, and both times correlate exactly with a `cargo test` run from the
`brainstorm-rework` worktree (session timestamps UTC, machine local PDT):

| `~/.local/share` mtime | command |
| --- | --- |
| 16:15 | `cargo test --bin drovr ask_cannot` at 16:15:14 |
| 16:27:10 | `cargo test` (full suite) at 16:27:03 |

No other command in any of the 40 Claude sessions active that day touches the path.

**Ruled out, each with evidence.** `home-manager switch` — it ran three times at 17:21–17:23,
modified `~/.local/share`, and a canary planted inside `drovr/` survived all three. Mount
shadowing (`/home` ext4 under the `/home/sauyon` btrfs mount) — a `mount --bind /home` showed
the underlying directory empty. `drovr cleanup --purge` traversal — `validate_run_name` rejects
it. The `ask` e2e fixtures — properly `TempDir`-isolated. The archive endpoint — flips a flag,
does not delete.

### Not reproducible on demand

Each of the seven test binaries was run separately against a planted canary, then the full
`cargo test` was run: **the canary survived all eight.** So this is an interleaving, not a
straight-line bug. Two documented hazards in this same file are the likely substrate — "Test
suite flakes under parallel `cargo test`; needs `--test-threads=1`" and "A panicking test can
poison `ENV_LOCK` for the whole suite" — and the task was in a **mutation-testing window** at
the time, deliberately running with guards disabled (`if false && !dir.is_dir()`) to watch them
fail.

### Root cause that does not depend on finding the test

`data_dir()` (`cli/src/run.rs`) resolves `XDG_DATA_HOME`, **falling back to
`$HOME/.local/share`**, and `cmd_list` repeats the same fallback inline. Tests redirect it by
mutating that variable *process-globally* under `ENV_LOCK` — a convention carried in a doc
comment (`cleanup_scratch`: "Callers must hold `ENV_LOCK`"), enforced by nothing. Any test that
loses that race resolves the **live** data root instead of a scratch one, and a fallback that
silently succeeds is what turns a lost race into deletion rather than a failure.

### Fix shape

Make the test path fail closed rather than fall back. A test-only guard — e.g. `data_dir()`
panicking when a `DROVR_TEST_DATA_ROOT` (or equivalent) is unset under `cfg(test)` — converts
"quietly operate on the human's data" into "the test fails immediately". That is one
authoritative mechanism, in place of a convention plus a comment.

Until that lands: **do not run `cargo test` in a drovr worktree while runs you care about
exist**, and treat run directories as expendable — see the follow-up below.

### The lesson that outlives the bug

**Run directories had no backing store.** `spec.md`, `plan.md` and every handoff lived only
under `~/.local/share/drovr/`, never in git, so a single bad interleaving erased design work
that no amount of `git fsck` could bring back. Artifacts worth keeping belong in the repo.

## Follow-ups

Wanted work that is not a defect — nothing here is broken today.

- **Render fenced `dot` blocks in the review UI, and show the phase/gate graph alongside the
  plan** (raised 2026-08-04, run `skill-stickiness`, spec §2.3). The skill docs author their
  decision graphs as fenced `dot` blocks, and an agent reads the source whether or not anything
  renders it — so the docs work as they are. The gap is human-facing: a reviewer orienting in a
  run in progress is asking *where in the procedure is this*, which is a position in a branching
  structure that prose has to re-linearise. spec §2.3 puts the doc side in scope and the
  rendering side out of it (§8), so this is recorded rather than built. No claim is made about
  how much faster anyone reads a graph; that would be a comparative with no measurement behind
  it.

- **Make the orchestrator itself a drovr agent, one per task** (raised 2026-08-06, run
  `brainstorm-rework`). Every phase runs as a drovr agent in its own pane; the *driver* that
  starts them does not. It is whatever session the human happened to type in, so it is the one
  role in the discipline with no pane, no `state.json` entry, no handoff, and no clean-context
  boundary — while being the role that accumulates the most context, since it outlives every
  phase it drives. `drovr never moves the driver out of the invoking checkout` (below) is one
  symptom of the same root: the driver is not a managed thing.

  Wanted shape: `drovr new` provisions an orchestrator agent for the run alongside the
  workspace, so one task gets one orchestrator, addressable like any other agent — visible in
  the agent tree, attachable, reapable, and resumable. A human then talks *to* a run rather
  than *being* its driver.

  Recorded rather than built because it changes what a "run" is at the top. The stated blocker
  is now **cleared**: the interactive-brainstorm work (`drovr ask` / `ask wait`, the interview
  panel) landed on `drovr/ask-channel` 2026-08-07 — not yet on `main` — so the orchestrator would
  have a channel to the human. An orchestrator without one is strictly worse than a human driving
  by hand, which is the exact trap that produced this entry.

  The two consequences recorded here are consequences of two *different* setups, and only one is
  now addressed. **Still true, for a brainstorm conducted in the human's chat session:** the run
  has **zero agent panes**, so `/api/runs/<run>/agents` answers `nodes: []` and the review UI
  reports "no live pane" on a run that is actively at its gate — and with no phase agent, there
  is nobody but the driver to hold the Q&A, so the interview does still land in its context.
  Those are the same root, not two findings. **Addressed, once brainstorm runs as a phase
  agent:** the interview is answered in the web UI and read back by the agent that posted it, and
  `skills/pipeline/SKILL.md` tells the driver to stay out of it.

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
   **The demand for this was already on the page, and so was the bug:** the pre-token `phase wait`
   entry above offered `: > "$(drovr path <run>)/<phase>.done"` as a workaround (added in
   `5beb62f`), but there is no `path` subcommand — `drovr path` exits with "unrecognized
   subcommand", and `cli/src/main.rs`'s `Commands` enum has no `Path` variant. **That line is now
   written against the run dir directly**, so nothing on this page prescribes a command that does
   not exist. The helper itself is still unbuilt, and still worth building.

Neither removes the underlying limit — a CLI still cannot move its parent — so both are ways of
making the documented step harder to miss, not a substitute for it.

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

## The session list rebuilds via `innerHTML` every 2s — the "vanishing rows" are fixed, the rebuild is not

**Kept, partly fixed.** Fixed entries are deleted from this file; this one stays because only the
symptom closed. The wholesale rebuild is still what `renderRunList` does, it is still the root of
a bug class, and one consequence below is live and unfixed: **real Tab focus on a row control is
destroyed on the next tick.** The `### Fix idea` (diff-and-patch keyed rows) is unimplemented.

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

1. **The `|=` merge could undo a Restore; it is now `=`, and `state.json` is the authority.**
   The rule is recorded where it binds — `cli/src/run.rs`'s `archived` field doc — and pinned by
   `archiving_mid_run_survives_every_save_the_review_makes`. The narrative below is kept only
   because it explains *why* the rule is a rule; the defect itself is gone. This paragraph
   used to say the merge was unreachable "because `code_review_run` refuses archived runs up
   front"; that is only true of runs archived *before* the panel starts.
   `archiving_mid_run_survives_every_save_the_review_makes` (`cli/src/code_review.rs`) pins the
   case that gets past it: the human archives while the panel is in flight,
   `spawn_reviewer`'s trailing `save_preserving_archived` reads `true` off disk, and the
   caller's copy is latched `true` for the rest of its life. If the human then hit Restore, the
   panel's next save ORed the live `false` with that stale `true` and wrote `archived: true`
   back — silently re-archiving a run they had just restored.

   Fixed by making the rule one rule: **`state.json` is the authority for `archived`**, stated
   on the field in `cli/src/run.rs`. `save_preserving_archived` now ADOPTS disk's value rather
   than merging it, so it is wrong in neither direction, and every site that *consults* the
   flag calls `RunState::refresh_archived`, which re-reads, adopts, and returns it — so no
   caller can act on one answer and then write a different one. Nothing is lost by the change:
   a caller that has *decided* to archive or restore owns the field and uses plain
   `save`/`save_in`, which write it verbatim.
2. The re-read swallows a load error (`if let Ok(disk)`), then `save` does `create_dir_all`.
   If `drovr cleanup --purge` deletes the run directory while a review is blocked, the
   eventual save recreates a `state.json` for a run the human explicitly deleted. This
   predates the change — plain `save` always did this — but it is now reachable from two more
   writers.

## The review gate writes nothing when the run directory is gone — and still reports approved

**Severity:** high. A driver trusting `review wait`'s `Approved` advances past a gate that
produced no artifacts, which is the failure the "piping a `wait`" entry describes, reached by a
different route.
**Found:** 2026-07-27, reviewing the archive-button merge. **Pre-existing** — not from that
branch, which never touched these handlers. Not fixed.

`handle_post_summary` and `handle_post_submit` (`cli/src/review.rs`) build a `RunPaths` from the
run name and go straight to writing. Neither checks the run directory exists, unlike
`handle_archive`, which does. Every write is `let _ = fs::write(...)`, so failures are silent,
and the `ReviewState` lives in `Ctx::cells` — an in-memory map populated independently of the
filesystem.

Reproduced live: create run `foo`, `POST /summary` (state `ready`), delete the run directory —
a concurrent `cleanup --purge`, or a name that was never `drovr new`'d. The reviewer's open page
and any `drovr review wait foo` still see a coherent run: `GET /doc` 200s, `GET /state` says
`ready`. Approving returns `200 {"ok":true,"state":"approved"}` and `/state` then says
`approved` — while `feedback.json`, the approved marker and `review.state.json` were never
written. `review_summary()` does not check either, so a mistyped run name reaches this from
ordinary CLI use.

Fix shape: both handlers should refuse a run whose directory does not exist, the way
`handle_archive` does; and the writes should report their errors rather than discarding them.

## The review-state cache is never evicted, so a reused run name inherits the old verdict

**Severity:** high — it can permanently wedge a new run's gate, with no fix but restarting the
global server, which drops every other run's cache too.
**Found:** 2026-07-27, same review. **Pre-existing.** Not fixed.

`Ctx::cell` (`cli/src/review.rs`) is `map.entry(name).or_insert_with(|| ReviewState::load(...))`
— keyed on the run's NAME, loaded from disk once, never refreshed or evicted for the life of the
always-on server.

Reproduced live: drive run `bar` to `approved`; `cleanup --purge` it; create a brand-new,
unrelated run also called `bar`. Its `drovr review summary bar "..."` answers
`409 {"ok":false,"state":"approved"}` — "the review gate is closed" — for a run that has never
been reviewed. Short, memorable, reused run names are exactly this repo's habit, and the server
is designed to stay up for days.

Worse in combination with the entry above: because the endpoints require no run to exist,
anything that can reach the port can pre-poison the cache for a run name not yet created
(`task-1`, `fix-login`) with a `cancel` submit, closing that gate before it opens.

Fix shape: key the cache on something that changes when a run is recreated (the run dir's inode
or creation time), or evict on `cleanup`.

## `drovr new` on an existing run name orphans the old workspace

**Severity:** medium — a herdr workspace no drovr command can ever close.
**Found:** 2026-07-27, same review, by inspection (not executed, to avoid stray workspaces).
**Pre-existing.** Not fixed.

`cmd_new` (`cli/src/main.rs`) never checks whether the run dir or its `state.json` already
exists. It creates a second herdr workspace — `workspace_create` has no uniqueness constraint on
the label — and overwrites `state.json`, replacing `workspace`/`root_pane`/`phases`. The first
workspace's id survives nowhere: not in the new state, not in `retired_panes`. `drovr_pane_ids`
cannot see it, so `cleanup`, `close_run_panes` and the Archive button can never close it. It is
discoverable only through raw `herdr workspace list`.

Reachable by operator error, or a driver retrying `new` after believing the first attempt
failed — plausible given how many drivers run concurrently here.

Fix shape: refuse when the run already exists, with a flag to adopt or replace deliberately.

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

## `save_preserving_archived`: where it is used, and why

Found 2026-07-26; **corrected twice**. The first version claimed three sites were redundant AND
untestable and told the reader not to add coverage — two of the three were both reachable and
testable. The second version was made stale by the merge with main, which deleted the one site
it called genuinely redundant.

The false claim in version one was that `code_review_run`'s poll loop makes no herdr calls. It
does: `agent_status` (now `pane_info`) is the fallback the loop consults whenever a reviewer's
done-marker is absent. On a RESUMED pass where every angle is still alive, `spawn_reviewer` is
skipped entirely, so the poll loop's own calls are the only ones and nothing rescues the flag
first. A human archiving a run while a resumed review polls is ordinary use.

Current state — six call sites across five tests (`phase_start` has two: the pass-persist write
and the post-launch one), each failing the suite if reverted to a plain `save`:

| site | test |
| --- | --- |
| `phase_start`'s pass-persist and post-launch saves | `phase_start_does_not_un_archive_a_run_archived_while_it_worked` |
| `spawn_reviewer` | `archiving_mid_run_survives_every_save_the_review_makes` |
| `reopen_for_re_entry` (via `phase_send`) | `phase_send_does_not_un_archive_a_run_archived_while_it_reopened` |
| `code_review_run`'s deadline save | `archiving_during_a_resumed_poll_survives_the_deadline_save` |
| `code_review_run`'s final save | `archiving_during_a_resumed_pass_survives_the_final_save_too` |

`cmd_code_review` no longer has such a site: the merge replaced its whole-state write with a
fresh load plus `transplant_review_progress`, which is strictly better — it cannot clobber
`archived` OR any other field.

Archiving pays a second herdr RPC for this. `close_for_archive` now asks `workspace_panes`
before it will close anything, so if herdr is flaky on the LIST call specifically — while the
close itself would have worked — the close is never attempted and the page warns that the
workspace is still open. That is deliberate: closing on an answer we do not have is how the
human's own tabs used to die. `cleanup` already carried the same trade-off; the Archive button
is simply clicked more often. Worst case is a spurious warning and a manual `drovr cleanup`.

`phase_wait` likewise no longer needs one: it re-reads fresh state and commits onto that.

The lesson, now demonstrated twice on this very entry: "this path cannot be reached" is a claim
about code and needs checking against the code. And an entry that names call sites goes stale
when someone else moves them — check it against the tree before trusting it.

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

## Lessons kept from retired issues

The second exception named in the policy at the top of this file: retired issues whose value was
never the defect. Carry **the rule and the evidence that confirms it** — including what was ruled
out, which is often the expensive part — and nothing else. The test is structural, not length: if
a lesson grows a reproduction and a fix list, it has become an issue again and belongs above.

- **Headless Chromium on Linux: always pass `--password-store=basic`** (retired 2026-08-01;
  the flag is in `cli/tests/web_nav.rs`). Without it, Chromium's cookie store loads its
  encryption key through OSCrypt, which asks the Secret Service over D-Bus; on a machine with
  no unlocked keyring and no prompter that call **has no timeout**, so every cookie-bearing
  request stalls forever at `COMPUTED_PRIVACY_MODE` and never reaches
  `HTTP_TRANSACTION_SEND_REQUEST`. It presents as a network or browser fault, never as a
  credential-store one: `file://` navigates instantly, the target server logs zero requests
  even though `TCP_CONNECT` completed, `curl` to the same port works, and the DevTools session
  looks dead while `/json/list` still answers. **Three wrong diagnoses were recorded before
  this one** ("environmental", "load sensitivity in the CDP deadline", "a Chromium 150
  regression"), none with measurements behind it. Ruled out and not worth re-testing: the drovr
  server, CDP socket topology, `--headless` vs `=old`/`=new`, `--no-sandbox`,
  `--disable-dev-shm-usage`, `NetworkServiceInProcess`, `--no-proxy-server`, enterprise policy,
  machine load, the agent tool sandbox, and any chromium wrapper or flag file. Diagnose with
  `--log-net-log --net-log-capture-mode=Everything`; the check that **confirms** it rather than
  suggesting it is `ReadAlias("default")` on `org.freedesktop.secrets` returning `/`, i.e. there
  is no unlocked default collection.
- **herdr's `agent_status: "done"` is an EDGE, not a level** (retired 2026-07-27). It is
  reported only for the moment a turn ends; an agent parked at its prompt reports `"idle"`.
  Anything that polls for it will miss it and can never recover — which is what hung the review
  panel forever on reviewers, who are forbidden from running `drovr phase done` and so have no
  marker either. The rule that came out of it and still binds: **completion is the artifact.**
  A parseable findings file finishes an angle whatever the pane says
  (`code_review::delivered_review`); herdr is consulted for exactly one question the artifact
  cannot answer — has a reviewer finished *without* delivering? Do not reintroduce a
  liveness-based **completion** test. `agent_status` for readiness and blocked-detection is fine
  and still in use (`cli/src/phase.rs`); it is completion, and only completion, that must come
  from the artifact.
- **`herdr agent read` is a LOSSY viewport — it truncates long lines mid-word** (retired
  2026-07-27, when the findings channel stopped being a pane scrape). Every `--source`, including
  `recent-unwrapped` with `--lines 800`. Text simply goes missing mid-token, so anything
  structured read back from a pane — JSON above all — can be unparseable no matter how it is
  extracted. Two consequences that still bind: **never make a pane transcript the durable channel
  for anything** (this is why reviewers submit through the `submit_findings` MCP tool and drovr
  does the write), and any check that looks for a payload in a pane must compare a **capped
  prefix**, never the whole string — which is exactly what `pane_shows_payload`
  (`cli/src/phase.rs`) does and why. If you must get text out of a pane, keep lines under ~100
  chars: short lines wrap instead of truncating.
- **A cursor reviewer in `--mode plan` writes its full output to a file** (recorded 2026-07-27;
  never harvested). It saves to `~/.cursor/plans/<title>-<id>.plan.md` and prints the path. That
  file holds the untruncated review — full rationale, the "no finding" verifications, findings
  separated from nits — and is far better than anything scraped from a pane. drovr does not read
  it: `submit_findings` solved the problem a different way. Recorded because it is the obvious
  channel to reach for if a read-only agent ever needs to return more than a tool call can carry,
  and because it is not discoverable without knowing it exists.
- **Two teardown paths for one resource drift apart** (retired 2026-07-27). `drovr cleanup` was
  hardened on main to refuse `workspace_close` unless every pane in the workspace is one drovr
  created — sparing the shell or editor a human keeps there. The Archive button's
  `close_for_archive` still closed outright, so one click destroyed exactly what cleanup had been
  taught to protect. **Neither side was wrong alone**, `review.rs` did not conflict during the
  merge, and ten rounds of reviewing the branch never saw it: it took reviewing the **merge** —
  asking what two features do to each other — to surface it. Both now share `close_run_panes`.
  A consequence that is still live: `workspace_closed: false` has two meanings, and the page says
  both — the close failed, or it was withheld because the human's panes are in there.
- **A skill's `description:` is a trigger, not a summary** (retired 2026-08-04; pinned by
  `cli/tests/skills_valid.rs::no_phase_scoped_description_literals`). It is the line that
  decides whether the skill is read at all, so a precondition written into it is a precondition
  on the whole discipline — four skills scoped theirs to "in a drovr phase" and silently
  disabled themselves for inline work. Phase-specific consequences belong in the body, worded
  as *additional* ("Inside a drovr phase this also binds…"), never as a precondition.
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

## Losing a run's herdr workspace — what the repair does, and what it still does not

**Kept, though the original defect is fixed.** Fixed entries are deleted from this file; this
one stays for `### What is still true` below, which is the contract every caller of
`phase::ensure_workspace` depends on and is documented nowhere else. It carries a live,
deliberately unguarded race (two drovr processes repairing one run both create a workspace, and
the loser is never recorded, so nothing reaps it). It is also the durable home for two facts a
reader will look for under "archive": **Restore is required, not optional** — `ensure_workspace`
refuses to re-provision a run that is still `archived` — and a phase that was `Running` when its
workspace went comes back **`Failed`**, not respawned.

**Severity:** was high — it made a live 23-task run at task 3 (approved spec, plan, two tasks of
committed work) reachable only by hand-editing `state.json`, and the command built for
recovery reported a resume it had not restored.
**Found:** 2026-08-02, driving `skill-stickiness`.

### What happened

1. The driver closed the last remaining pane in workspace `wAG` while reaping finished
   reviewer panes. herdr destroys a workspace when its final pane closes — reasonable of
   herdr, and `drovr cleanup` does the same thing on purpose.
2. `state.json` still read `"workspace": "wAG"`. Nothing validated it.
3. `drovr phase start` failed with the raw daemon error:
   `phase start failed: workspace wAG not found (herdr error code: workspace_not_found)`.
4. `drovr resurrect skill-stickiness` printed the full phase list and
   `To resume: drovr phase start 'skill-stickiness' 'implement'` — **an instruction that
   could not work**, because `resurrect` never touched the workspace. This is the part worth
   dwelling on: a recovery command that reports success it did not achieve is worse than one
   that errors, because the next failure lands somewhere unrelated and you go looking there.
5. Clearing `workspace` to `null` by hand produced a different refusal:
   `run '…' has no herdr workspace (creation failed at drovr new); please recreate the run
   with drovr new` — advice that would have discarded everything the run had produced.

### The near-miss, if you are recovering by hand

Recovery required calling `herdr workspace create` directly. **The first attempt omitted
`--cwd`, so the new workspace's pane opened in an unrelated repository**
(`~/devel/modular`, on someone else's branch). Nothing about the pane says so; it is a shell
prompt like any other. It was caught only because the driver checked the pane's cwd before
briefing an agent into it. One step later a phase agent would have been reading, editing and
committing in the wrong repo, believing it was in the run's worktree.

So: **always pass `--cwd <the run's project_dir>`**, and check `pwd` in the pane before you
brief anything into it.

### Digging out by hand (older binaries)

```
herdr workspace create --label 'drovr:<run>' --cwd "$(jq -r .project_dir ~/.local/share/drovr/runs/<run>/state.json)"
```

Then edit `~/.local/share/drovr/runs/<run>/state.json`: set `workspace` to the new id,
`root_pane` to the new workspace's root pane id, drop every `pane_id` in `phases` and
`review_phases` (they name panes that no longer exist), empty `retired_panes`, and change any
`"status": "Running"` to `"Failed"` — those agents are gone with their context.

### What changed

`phase::ensure_workspace` (`cli/src/phase.rs`) now runs at the three points that actually
need a workspace — `phase_start`, `spawn_reviewer` and `resurrect`. If `Herdr::workspace_exists`
reports the recorded workspace gone (or none was ever recorded), it creates a replacement,
**always with the run's `project_dir` as cwd**, and writes the new ids back. `resurrect` either
restores the workspace or exits non-zero; it no longer prints a resume it cannot deliver.
"Recreate the run" is no longer the advice for a lost workspace anywhere.

### What is still true

- **drovr does not stop a workspace from being emptied.** Holding the root pane open would
  make this rarer, but it is a second, weaker mechanism beside the authoritative one and it
  leaves an idle pane behind after `cleanup`. Re-provisioning is the guarantee; there is
  deliberately no backstop.
- **A phase that was `Running` becomes `Failed`, not respawned.** Its agent died with the
  workspace, taking its context. Restart the one you want — silently respawning would present
  work nobody is doing as still in flight, which is the same lie `resurrect` used to tell.
- **`workspace_exists` is biased toward alive**, like `pane_exists`: only a workspace listing
  herdr actually answered proves death. An unreachable daemon reads as "still there", because
  re-provisioning over a live workspace would orphan the run's own agents.
- **A repair must not decide what the human decided.** `ensure_workspace` refuses to
  re-provision a run that is `archived` — the destroyed workspace was, before this change, the
  only thing enforcing that decision, and repairing it silently would start a live agent on a
  run the UI shows as filed away. Restore first. It consults the flag through
  `RunState::refresh_archived`, which re-reads `state.json` (**the authority — see the field's
  doc in `cli/src/run.rs`**) and adopts what it finds, so the save at the end of the repair
  cannot write a stale value back over the human's decision. An unreadable `state.json` refuses
  the repair rather than falling back to the copy in hand: a read error is not evidence that a
  run is un-archived.
- **A replacement workspace that cannot be recorded is closed again.** The id only exists once
  herdr has created it, so it cannot be persisted first. If the save then fails,
  `ensure_workspace` closes the workspace it just made and returns an error saying the repair
  was rolled back — otherwise `state.json` would still name the dead workspace (so the next
  attempt makes a *second* replacement) while the first stood in the switcher with nothing
  pointing at it. If the close also fails, the message names the id and the label to close by
  hand.
- **A ZOMBIE needs no repair, and gets none.** A run archived while `workspace_close` failed
  still has a live workspace and live panes recorded — the state the UI flags as anomalous.
  `ensure_workspace` no-ops when `workspace_exists` says alive, so restoring one and running
  `drovr phase start` reuses what is there and works, as it always did.
- **A run with no `project_dir`** (created before that field existed) still cannot be repaired
  — there is no cwd to open a workspace in. The error names that field and its path rather
  than telling you to start over. Every site that refuses for this reason now raises the same
  sentence (`phase::missing_project_dir_error`), pinned by a test, because the earlier
  wording — "please recreate the run with `drovr new`" — was the same bad advice in a
  different place.
- **The repair is a check-then-act with no lock**, and `state.json` has no locking or
  compare-and-swap anywhere (see "`drovr cleanup` can clobber a concurrent `state.json`
  write"). Two drovr processes acting on one run at the same instant can both find the
  workspace gone and both create a replacement; the loser is never recorded in `state.json`,
  so nothing reaps it and it keeps a live agent process. It needs two concurrent writers on
  one run, which the single-writer discipline forbids, and locking only this one site would
  imply a guarantee the rest of the file does not make. If you suspect it: the orphan is
  labelled `drovr:<run>` like its twin, so look for two workspaces with the same label in
  herdr's switcher and close the one whose id is not in `state.json`.

## opencode's project config is a poor place for drovr's MCP server, and it is the only place (2026-08-03)

**Severity:** medium — three sharp edges, all inherent to the backend rather than bugs in
drovr. They are recorded here because each one looks like a drovr fault from the outside.
**Found:** 2026-08-03, adding the opencode backend. Probed against opencode 1.18.3.

### Why the server goes in `opencode.json` at all

opencode has no per-launch MCP flag, so a reviewer's findings server has to arrive through a
config file. There are two candidate files and the tidier one is wrong:

- **`OPENCODE_CONFIG=<path>` works** — it names an external file and *merges* it over the
  global config, leaving providers and credentials intact (probed: `opencode debug config`
  with the var set still resolves `provider`). It would keep drovr's plumbing entirely out of
  the checkout.
- **But it merges with the repository's own `opencode.json` rather than displacing it.**
  Probed directly: a project `opencode.json` declaring `repo-injected` plus an external file
  declaring `drovr-env-probe` resolves to *both* servers. A repository under review could
  therefore hand extra tools to a read-only reviewer, which is the exact thing
  `write_mcp_config` exists to prevent (see its "Why it replaces rather than merges").

So drovr writes the project's `opencode.json`, replacing it, which is what strips
repo-injected servers — the same mechanism and the same backup/exclude/symlink guards as
cursor's `.cursor/mcp.json`.

**And the same merge makes the config environment a hole in that replacement, not
just a road not taken.** An inherited value puts the servers straight back, whatever
drovr wrote to the project file. Replacing `opencode.json` is only the last word on
the subject if nothing else is speaking — and **four** things can speak. All probed
against 1.18.3 with `opencode debug config`, against a project file already holding
drovr's server:

| variable | what it does | probed result |
| --- | --- | --- |
| `OPENCODE_CONFIG` | names another config file | its server resolves *alongside* `drovr-findings` |
| `OPENCODE_CONFIG_CONTENT` | inline JSON | same — `['drovr-findings', 'injected']` |
| `OPENCODE_CONFIG_DIR` | another directory to read one from | same — `['dirinjected', 'drovr-findings']` |
| `OPENCODE_PERMISSION` | sets the permission block wholesale | overrides the other half of what drovr writes |

So a read-only opencode launch is composed as
`env -u 'OPENCODE_CONFIG' -u 'OPENCODE_CONFIG_CONTENT' -u 'OPENCODE_CONFIG_DIR' -u
'OPENCODE_PERMISSION' opencode --agent plan …` (`AgentSpec::readonly_env_unset`).
Shutting one door is not a guard; if opencode grows a fifth, this list grows with it.
Verified: with `OPENCODE_CONFIG` pointing at a file declaring its own server, the
unset launch resolves to drovr's server alone.

Two things follow, both pinned by tests. A user's own `readonly_env_unset` **unions**
with the built-in list rather than replacing it — the built-in vars are an invariant
of the backend, so `readonly_env_unset = ["HTTP_PROXY"]` is not consent to stop
clearing `OPENCODE_CONFIG`. And writer phases keep every one of them: drovr does not
replace `opencode.json` for them, so there is no guarantee to protect and clearing
them would break a user who points one at their real provider config.

### `--agent plan` is a definition the repository can overwrite — so `.opencode/` moves too

This is the one structural difference between opencode and the other two backends, and
it is worth stating plainly because it is easy to assume away. claude's
`--permission-mode plan` and cursor's `--mode plan` are **CLI flags**: the code under
review cannot reach them. opencode's `--agent plan` names an **agent definition**, and
a repository can commit its own.

Probed against 1.18.3, with drovr's `opencode.json` (`agent.plan.permission.edit =
"deny"`) already in place, plus a repo-committed `.opencode/agent/plan.md` declaring
`permission: edit: allow`:

```
edit rules in resolution order (last wins):
  {"permission":"edit","pattern":"*","action":"deny"}                  <- opencode's stock plan agent
  {"permission":"edit","pattern":".opencode/plans/*.md","action":"allow"}
  {"permission":"edit","pattern":"…/plans/*.md","action":"allow"}
  {"permission":"edit","action":"allow","pattern":"*"}                 <- THE REPOSITORY'S
```

drovr's `deny` is **not in the list at all** — the repo's file replaced the definition
drovr was amending — and the repo's `allow` is last. Separately, `.opencode/plugin/*.js`
from the checkout loads as arbitrary JavaScript in the reviewer's own process, and
`--pure` did not drop it from the resolved plugin list.

So `code_review` moves the whole `.opencode/` aside before spawning an opencode panel.
Which paths, per backend, is `AgentSpec::readonly_displace` — beside
`readonly_env_unset`, since both answer the same question ("what does a read-only
launch have to be kept away from?") and both union rather than replace on override.
Four things about that, each deliberate:

- **The whole directory, not the two subdirectories the probes convicted.** drovr
  cannot know which parts of `.opencode/` confer capability in an opencode release it
  has never seen, and a subdirectory whitelist would be a second description of
  opencode's layout maintained inside drovr — the same drift that got the
  `holds_more_than_drovrs_server` key whitelist deleted. One rule survives the next
  version; a list of exceptions does not.
- **It never deletes, and neither does the config replacement beside it.** This is the
  sharp edge, and the first version got it wrong twice — the same wrong inference in
  two places. The first version:
  it read "the `.drovr-backup` slot is already occupied" as "drovr displaced this on an
  earlier pass" and removed the live directory on that evidence. But the repository
  chooses the contents of the checkout *including the name drovr backs up to*, so a
  committed `.opencode.drovr-backup` decoy made the very first review of a repo delete
  the user's real `.opencode/` with no backup at all. The occupant of a backup slot is
  evidence of nothing. `write_mcp_config` had the identical bug one line over: on an
  occupied slot it skipped the rename and overwrote the **live** project config, so a
  committed `opencode.json.drovr-backup` (or `.cursor/mcp.json.drovr-backup`) silently
  discarded the user's config file. Both now allocate the next free
  `<path>.drovr-backup[.N]`, and the only operation on a path drovr does not own is
  `rename`. That is also why both git-exclude entries are globs: the backup name is
  not fixed.

  A related trap in the same family: a `readonly_displace` entry is *renamed*, so
  "inside the project" is not a strong enough check on it. A lone `.` is relative and
  has no `..`, yet resolves to the checkout root — drovr would move the whole
  repository aside. `validate_project_relative` therefore requires every component to
  be an ordinary name, which rejects the absolute, the traversing and the
  root-resolving cases with one rule.
- **It does not restore.** A `Timeout` outcome leaves reviewers alive and the panel
  resumable, so putting the directory back at the end of a pass would re-arm the hole
  under a reviewer still reading.
- **It re-runs immediately before *each* reviewer spawns, not once per pass.** The
  angles launch one after another, so the first reviewer is already live in the
  checkout while the last is still being launched. Note the invariant this does and
  does not give you: "`.opencode/` does not exist afterwards" is not achievable —
  anything running in the checkout can re-create it a microsecond after any check.
  What holds is that **every reviewer is launched with the path displaced immediately
  beforehand**. A repository that keeps re-creating it just accumulates numbered backup
  slots, which is why the git-exclude entry is the glob `.opencode.drovr-backup*`.

`.opencode/` is a real directory in real repositories (commands, skills, plans), so a
review will move it and leave it moved. That is the cost of running a read-only
reviewer on a backend whose read-only mode lives inside the checkout.

### The three edges

- **`opencode.json` is far more likely to be a *tracked* file than `.cursor/mcp.json`.**
  `.git/info/exclude` does not untrack anything, so on a repo that commits its opencode
  config the replacement shows up as a *modified* file — inside the working tree the seed
  explicitly tells reviewers to read ("the change under review is `git diff base..head`
  **plus** the current working tree"). Reviewers can see, and may report on, drovr's own
  plumbing. The original is at `opencode.json.drovr-backup`.
- **`--agent plan` is `ask`, not `deny`.** opencode's stock plan agent sets edits *and* bash
  to `ask`. An "ask" in an unattended reviewer pane is a hang, not a refusal, and it presents
  as a reviewer that simply never reports. drovr writes `agent.plan.permission` alongside the
  server — `edit: deny`, `bash: allow` — to turn that into a real stance. Verified with
  `opencode debug agent plan`: drovr's two rules land last in the resolved rule list.
  `bash: allow` is deliberate, not an oversight: the seed sends reviewers to `git diff` and
  the repository's tests. It leaves opencode exactly as strong as cursor's `--mode plan` —
  writes through the editing tools are refused, writes through a shell are not.
- **Plan mode can still write plan files** (with `.opencode/` displaced, into a
  directory drovr just moved). The resolved plan agent allows `edit` on
  `.opencode/plans/*.md` and on `~/.local/share/opencode/plans/*.md`. That is the same shape
  as the cursor finding above ("A read-only cursor reviewer can park at plan mode's 'Ready to
  build?' gate"): a reviewer that decides to save a plan writes a file *into the checkout
  under review*, and drovr does not exclude that path. Nothing depends on it — the findings
  channel is the MCP tool — but do not read `.opencode/plans/` appearing mid-review as
  corruption.

### The thing that is NOT a problem

opencode's argument parser accepts unknown options silently (`opencode --bogusflag123 -v`
exits 0), so a made-up workspace flag would compose a command that *looks* pinned to the
project and runs unpinned. opencode names its project positionally, and `WorkspaceArg`
models that directly rather than inventing a flag — see the enum's doc in `cli/src/config.rs`.
## A ONE-angle panel can reuse a review iteration, and inherit the dead reviewer's verdict

**Severity:** low — unreachable with the default four angles, and it needs a failing
`clear_findings_file` on top of that. Written down because it is a *decision* (not to widen the
fix), not an oversight.
**Found:** 2026-08-03, by analysis during task 6 of run `phase-reap`, while checking a claimed
"`review_phases` is append-only" invariant. **The invariant is false**, which is the more useful
half of the finding.

### The claim that turned out to be wrong

`next_iter` (`cli/src/code_review.rs`) is `max(existing iteration)+1` over `run.review_phases`,
and it was believed safe because that list only grows. It does not: `code_review_run`'s resume
path does `run.review_phases.retain(|p| p.name != phase)` before respawning an angle in place.

What actually protects the counter is two other things.

1. **Arity.** The retain removes ONE angle's entry immediately before its respawn. With ≥2
   configured angles (the default is 4), the other entries of that iteration keep `max` where it
   is even if the spawn then fails and `?` propagates.
2. **Ordering.** `clear_findings_file` runs *before* `spawn_reviewer`, so on the one path that
   prunes, that angle's findings file is already gone. Even where an iteration number were
   reused, there would be no stale verdict left to harvest.

### The residual hole

With **exactly one** configured angle, a resume whose `clear_findings_file` **fails**:

- `?` propagates with the entry already removed from `review_phases`;
- the removal is persisted anyway — `main.rs`'s `merge_panel_progress` runs on every path
  including the `Err` early-exit, and it *assigns* `review_phases` rather than merging it;
- the old `<task>-review-<iter>-<angle>.json` is still on disk, because clearing it is what
  failed.

`next_iter` then returns that same iteration, and the replacement panel can be credited with the
dead reviewer's verdict.

### Working around it

Configure more than one angle. `angles` defaults to four; a single-angle config is the only way
in, and there is no reason to run a one-angle panel except to save tokens on a trivial change.

### Why it was not fixed

Both candidate fixes make the code worse than the hole. Retaining the entry until the respawn
succeeds means a live pane no phase records if the respawn fails — the immortal-pane bug this
branch spent three tasks closing. Deriving the iteration from something other than
`review_phases` means a second source of truth for the counter. A test would either re-test
`next_iter` (the multi-angle case) or enshrine the subtlety (the single-angle one). If it is ever
worth closing, the shape to look at is making the counter monotonic on disk rather than derived.

## herdr's "polling is degraded" diagnostic fires on a reap's EXPECTED path

**Severity:** low (cosmetic, but it reads as a fault during a supported repair).
**Found:** 2026-08-03, task 6 of run `phase-reap`.

### Symptom

Reaping a phase whose pane herdr has already lost — `drovr phase reap <run> <phase>`, or the
retired-pane sweep re-probing a pane a previous sweep closed — prints:

```
drovr: herdr's pane.get failed for pane <id>: <err>. Agent status polling is degraded —
phase sends and waits will run to their timeouts with no other explanation. (A pane that has
been closed reports this too.)
```

Nothing is wrong. That is the `Gone` path, and `Gone` is exactly what authorises the reap to
clear the registration. The command then succeeds.

### Why

`Herdr::pane_info` (`cli/src/herdr.rs`) reports a failed `pane.get` once per pane per process,
because for its original caller — `phase_send`'s readiness gate and `phase_wait` — a silent
`None` means an unexplained timeout on a healthy agent. Reaping asks the same question for the
opposite reason: it *wants* to hear that the pane is gone. The diagnostic cannot tell the two
callers apart, so it warns for both. The parenthetical was added for exactly this, and is not
enough — the sentence before it still says "degraded".

### What NOT to do

Do not loosen the gate. It exists for a real failure (herdr unreachable, or a response shape
drovr cannot read) that is otherwise invisible, and the reap path is the one place where its
false positive is harmless. If it is worth fixing, the fix is a caller-supplied expectation —
`pane_info` told that "gone" is an acceptable answer here — not a quieter diagnostic.

## `cargo clippy -D warnings` is not a gate, and counting its findings is not straightforward

**Severity:** medium as a process hazard — the task briefs on run `phase-reap` named
`cargo clippy --all-targets -- -D warnings` as a per-task gate, and it has never passed on
`main`, so every task either ignored the gate or would have had to fix unrelated code to meet it.
**Found:** 2026-07-26 (task 1 of run `phase-reap`); decided 2026-08-04 (task 7).

### The decision

**The gate is PARITY, not zero: a branch must introduce no new finding.** Fixing the baseline is
a separate, deliberate change and is not any task's to absorb — the same reasoning as the
`cargo fmt` entry above, whose formatting-only commit conflicts with every branch in flight.
`cargo fmt` is not run at all on this repo; clippy is run, and compared.

Compare the **sets**, not the counts. Line numbers move as code does, so a finding is
pre-existing if the same lint already fired in the same file at base. Measured for run
`phase-reap`: base (`377dff0`) has **10**, the branch head has **8** — two fewer, because code
carrying a `collapsible_if` and a dead-code warning was rewritten. Nothing new was added.

### Measuring is subtler than it looks

Two traps, and the naive count hits both.

1. **Cargo does not re-emit warnings for targets it did not recompile.** A warm run silently
   under-reports: on this branch, touching only `src/main.rs` reported **4** findings where a
   cold run reports **8**. A number that changes with the cache is not a number.
2. **One source-level warning is reported once per compilation target.** `cli/src/*.rs` compiles
   as both `bin "drovr"` and `bin "drovr" test`, which is what the
   `warning: drovr (bin "drovr") generated 1 warning (1 duplicate)` summary lines mean.

Force a full re-check and dedupe by source location:

```sh
cd cli
touch src/*.rs tests/*.rs        # or: CARGO_TARGET_DIR=$(mktemp -d)
cargo clippy --all-targets --message-format=short 2>&1 |
  grep ': warning: ' | sort -u
```

`--message-format=short` emits one `file:line:col: warning: …` line per finding, so `sort -u`
collapses the per-target duplicates. Verified stable across a warm run and a cold one (fresh
`CARGO_TARGET_DIR`), and against a scratch export of `main`.

### Fix

Land one clippy-only commit on `main`. Ten findings — four `manual_split_once`, two
`chunks_exact`, two `collapsible_if`, one dead method, one `&PathBuf` — all mechanical, all with
a suggested rewrite. Like the `cargo fmt` cleanup it wants a quiet moment rather than a branch
that happens to notice, and until then the parity rule above is the gate.

## A phase whose agent never took a turn records a session id that cannot be resumed (2026-08-04)

**Severity:** low — hard to reach, nothing is lost, and the pane is kept. It costs the operator
one confusing exit 2 during a repair.
**Found:** 2026-08-04, the final live verification of run `phase-reap`, on a throwaway run
`reapcheck` against the release build of that branch.

### Symptom

A reaped phase advertises `rehydratable: true, resumable: true` — the UI shows ⟳ — and
`drovr phase rehydrate <run> <phase>` comes back `Incomplete(ResumeUnobserved)`, exit 2. Reading
the restored pane shows claude answering the resume with:

```
No conversation found with session ID: a143f094-0973-48ae-98ff-46ceea6ecc69
```

Observed end to end: `drovr phase start reapcheck brainstorm --no-brief` spawned the pane, herdr
reported `agent_session.value = a143f094-…` almost immediately, and `phase wait` captured it into
`state.json` as `pane_agent.session` — correctly, by design. The agent was then **never
prompted**. The phase was completed and superseded, the reap closed its pane (`reaped: true`), and
the rehydrate relaunched with `claude … --resume 'a143f094-…'` against a session claude had no
record of.

The exit 2 and the kept pane are the *correct* behaviour — `ResumeUnobserved` surrenders nothing,
precisely because a slow session id is indistinguishable from a failed resume. The gap is that
here the resume really had failed, for a knowable reason, and the operator is told "that is not
proof the resume failed".

### Root cause

herdr surfaces a session id as soon as the agent process is up; claude writes the conversation
transcript (`<session-id>.jsonl`) only once the session has content. Confirmed both ways on the
same run: no transcript existed for the never-prompted agent, and one did exist for a phase that
had taken a single turn. So there is a window in which drovr records — truthfully — a session id
that `--resume` cannot resolve.

### How narrow it is

Reaping only ever targets a phase that reached `Done`, and a phase does not normally reach `Done`
without its agent doing work. The routes in are the unusual ones: a phase completed manually
through the `DROVR_PASS` escape hatch, or an explicit `drovr phase reap` on a phase whose seed was
never delivered — and `drovr phase send` now detects a swallowed seed, so that second case is
visible rather than silent.

**Rehydrate itself works.** The contrasting case was verified on the same run: a phase that had
taken exactly one turn (session `04fa2210-0cfe-4b11-825e-299b3aa14bcd`) was reaped and rehydrated
to exit 0, *"resumed with its recorded session"*, with the marker string from the original
conversation still in the restored pane and herdr reporting the same session id afterwards. This
entry is only about the never-had-a-conversation window.

### What to do

The pane is kept, so read it — `herdr pane read <pane>` shows the "No conversation found" line,
which is what distinguishes this from a session id that is merely slow to surface. Then
`drovr phase reap <run> <phase>` clears the registration, and the phase can be re-seeded from its
handoff.

### Why it is not fixed

The honest fix would be for drovr to distinguish "session id known" from "session resumable", and
it cannot check the second without reaching into claude's storage layout. Guessing instead — a
heuristic under the authoritative flag — would make the ⟳ look trustworthy when it is not. The
current message is truthful about what drovr observed; this entry supplies what it cannot.

## The spec gate tells the driver to poll for `ready`, which loses a fast approval

**Severity:** high (the driver hangs indefinitely after a *successful* gate; the run stalls with
every artifact already on disk, and only a human noticing "why hasn't it come back" recovers it).
**Found:** 2026-08-06, run `tui-dc-picker`, brainstorm phase.

### Symptom

The reviewer approved `spec.md`. The brainstorm agent then finished normally, authored its handoff
and dropped `brainstorm.done`. The driver never woke and never started the plan phase — it sat idle
for as long as the human left it, with `approved`, `feedback.json` and `brainstorm-HANDOFF.md` all
present in the run dir the whole time.

### Root cause

`skills/pipeline/SKILL.md:61` forbids starting `drovr review wait` before the first summary and
instructs the driver to hand-roll a poll instead:

> Background a poll on the run's state for the `ready` transition; don't busy-wait inline.

`ready` is an *intermediate* state. The run passes through it on the way to the terminal `approved`,
and nothing holds it there — a reviewer who approves between two ticks of the driver's poll moves
`idle → ready → approved`, and a poll written to match `ready` matches nothing, ever again. It then
spins to its own timeout while the run is complete. The faster the human reviews, the more likely
the failure: an attentive reviewer is the worst case.

The advice is also unnecessary. `drovr review wait` **blocks** on a specless run rather than
erroring — `wait_times_out_while_idle` (`cli/src/review.rs`) asserts exactly that, and a timeout is
documented as exit 2, "re-run to resume". So the wait started immediately after `phase start` would
have blocked through `idle`, through `ready`, and exited 0 on the approval. The "churns" warning at
`SKILL.md:61` and `:311` is describing a cost that a blocking wait does not have.

### Impact

Fires on any run where the reviewer acts faster than the driver's poll interval, which is the normal
case for a reviewer already watching the page. The failure is silent and indistinguishable from a
slow phase, so the driver reports "still running" in good faith. Nothing on disk is lost — recovery
is just noticing — but the run makes no progress until a human intervenes, which defeats the point
of everything after the gate running unattended.

### Fix idea

Delete the prohibition and make the blocking primitive the only documented path: background
`drovr review wait <run>` as soon as the brainstorm phase starts, re-arm on exit 2, and branch on
0/3/5 exactly as the table below it already says. The URL-announcement rule is a separate concern
and should be decoupled — "wait for `spec.md` before showing the human the page" is good advice and
does not require the driver to hand-roll a state poll to honour it.

If a state poll is ever genuinely needed, the doc must say to match the terminal states
(`approved`/`cancelled`/`waiting`), never `ready` alone. But the better fix is that a driver should
not be hand-rolling a poll for a state machine the CLI already exposes a blocking call for — that is
the class of mistake this entry is about, and it should not be reachable from following the skill.

## Every cold `opencode` reviewer pane swallows its seed, so the panel cannot converge (2026-08-06)

**Severity:** high (blocks `code-review run` entirely under `review_agent = "opencode"`; retries
cannot help, because each one mints another cold pane that fails identically).
**Found:** 2026-08-06, run `hakobiya-accounts` task-2, against opencode 1.18.3.
**Caveat — verify before acting on this:** observed on a **nix build of 0.2.0** that predates
`da99190` and the `drovr/opencode-agent` merges. The newer opencode work may already address it.

This is the sequel to *"The code-review panel's stalls — what is fixed, and the unsubmitted seed
that is not"* above. That entry's readiness gate fix (`c12adb0`, `wait_agent_ready` inside
`phase::phase_send`, which `code_review.rs` calls after `spawn_reviewer`) is what made the
"agent target not found" symptom go away. **For opencode the gate passes and the seed still
does not land** — so `agent_status` is not sufficient evidence that this TUI will accept input.

### Symptom

`drovr code-review run <run> <task>` exits 1 with the standard undelivered-seed diagnostic
("herdr saw no state change after the prompt, and the payload is nowhere in the agent's
composer"). Observed on iterations 2 and 3 back-to-back; each attempt left a **new** pane
behind (`wJ:p2`, `wJ:p3`) and each new pane failed the same way. No `task-2-review-*-*.json`
was ever written.

### Not the cursor trust-dialog case, and not the paste-not-submitted case

The error text is shared with both, but the pane state distinguishes them:

- **cursor:** a `Do you trust the contents of this directory?` dialog sits on the pane.
  Answering once persists, so the *next* spawn succeeds — retrying converges.
- **paste-not-submitted:** the seed is visible in the composer as `→ [Pasted text #1 +N lines]`.
- **opencode (this entry):** the composer is **clean** — no dialog, no pasted payload. The pane
  is otherwise correctly wired: `Plan` agent selected (read-only wiring good) and `⊙ 1 MCP`
  connected (findings channel good). Nothing persists between spawns, so retrying never
  converges.

Purely a delivery-timing failure; the rest of the opencode integration works.

### Working around it

The failure leaves a live, MCP-connected pane, and the per-angle seed files are already on
disk. Deliver one by hand:

```
herdr agent prompt <pane> "$(cat "$(drovr path <run>)/task-<N>-review-<angle>-seed.md)"
```

The reviewer then runs and calls `submit_findings` through its own MCP connection exactly as
designed. `code-review run` is no longer supervising, so the driver must poll for
`task-<N>-review-<iter>-<angle>.json` and merge angles by hand. One warm pane serves one angle;
minting more means letting `code-review run` fail again for each.

### Fix ideas

1. Gate on something opencode-specific that proves the composer is live rather than on herdr's
   generic `agent_status` — the TUI prints its footer (`~/path:branch  ⊙ N MCP`) once
   interactive.
2. Re-send once after a short delay when the payload is absent from the composer *and* no
   dialog is detected. drovr deliberately refuses to key into that state, but
   absent-payload-plus-no-dialog is the signature of a cold TUI, not of a prompt awaiting an
   answer.

## `/send` and `/keys` are the only write buttons that ignore the response — a 409 eats the text (2026-08-06)

**Severity:** medium (the human types to the agent from the browser, the pane is gone, and the
message vanishes with no error and no way to recover the text they typed).
**Found:** 2026-08-06, run `land-interactive`, while auditing main's human↔agent surfaces against
the orphaned `feat/interactive-session` branch. Found by reading, not by hitting it.

### Symptom

In the **Live session** panel, the human types into the "Type to the agent…" box and clicks Send
(or presses one of the Enter/Esc/↑/↓/1–5 key buttons). If the run has no live writable pane the
server answers **409** and nothing is delivered — but the page shows no error. The textarea has
already been cleared, so the text is gone from the browser too: there is nothing to re-send and
nothing to copy. The only hint is the session view's "No live pane (this agent may not be running)"
placeholder, which was already there before the click and does not change in response to it.

### Root cause

`sendPane()` (`cli/web/index.html:2873`) clears the textarea (`el.value = ''`) *before* the fetch,
then does:

```js
try {
  await fetch(api('send') + paneQuery(), { method: 'POST', ... , body: text });
} catch (e) { /* ignore; next poll reflects state */ }
```

`fetch` rejects only on a network-level failure; a 409 (or 403, or 500) resolves normally, so the
`catch` never runs and the status is never read. `sendKeys()` (`cli/web/index.html:2892`) has the
identical shape at `:2902`. The comment — "next poll reflects state" — is the bug's premise: for
`/keys` a successful keypress does change the pane, but a *rejected* one changes nothing, so
"reflects state" is indistinguishable from "nothing happened".

Server side is behaving correctly and specifically: `handle_post_send`
(`cli/src/review.rs:934`) answers `409 "no live pane for this run"` when
`resolve_pane(&run, url, Access::Write)` finds no writable target — which includes the deliberate
exclusion of the workspace root `sh` pane from the writable set (`run_writable_panes`,
`cli/src/review.rs:751`). So the most likely 409 in practice is the *safety* case, and that is
exactly the one the human is told nothing about.

These two are the outliers. Every other write in the page reads the response and surfaces the
failure: submit (`index.html:2261`), create session (`:2326`), archive (`:2442`, logs
`r.status`), rehydrate (`:2817`, renders `Rehydrate failed (<status>)` into the agents note).

### Impact

Bounded but genuinely annoying: the human's typed instruction is destroyed, silently, at the moment
they are trying to unblock an agent — which is when they are least likely to be watching for a
missing acknowledgement. It also makes the pane-writability rule undiscoverable: a human who sends
to a run whose only pane is the root shell will conclude the feature is broken rather than that it
is refusing on purpose.

### Fix idea

Read the status in both handlers and surface it, matching what archive/rehydrate already do:

- On a non-`ok` response, **put the text back** in the textarea before showing the error — the
  clear-before-send is what makes this unrecoverable rather than merely confusing.
- Render the failure where the human is looking (an inline note under the session panel), with the
  409 body's own wording, and distinguish "no live pane" from a 403 untrusted-write refusal.
- For `sendKeys`, the row is already disabled during the await; re-enable and flag the row rather
  than silently returning it to normal.

## `feedback.json` is overwritten every turn, so earlier turns' annotations are unrecoverable (2026-08-06)

**Severity:** low-medium (no live failure; a reviewer's earlier-round comments cannot be re-read
once they submit a later round, and `turn` is the only evidence they ever existed).
**Found:** 2026-08-06, run `land-interactive`, same audit as the entry above.

### Symptom

The reviewer requests changes on turn 1 with a set of annotations, the agent revises, and the
reviewer submits again on turn 2. Turn 1's `feedback` and `annotations` are gone from
disk. An agent that wants to check whether it actually addressed every turn-1 annotation has no
source for them; a human who wants to see what they asked for two rounds ago has none either.
(`answers` is overwritten too, but nothing is lost there any more: the page always posts `{}`
since questions moved to `drovr ask` and the append-only `interview.jsonl`.)

### Root cause

Both submit branches do a whole-file `fs::write` to the same path:

- approve: `cli/src/review.rs:1438`
- request-changes: `cli/src/review.rs:1487`

each writing `{turn, decision, feedback, answers, annotations}` over whatever was there.
`RunPaths::feedback()` is a single fixed path per run. `rs.turn` increments on every submit
(`:1478`) and is embedded in the payload — so a reader can always tell *which* turn the surviving
file describes, which is what the `approve`-discards-answers fix (see that entry) was careful to
preserve — but no per-turn copy is kept anywhere. The browser's own annotation drafts are held in
`localStorage` keyed by turn (`cli/web/index.html:1530-1556`) and are cleared on submit, so there
is no client-side archive either.

Nothing reads history today, which is why this has never bitten: `drovr review wait` branches on
`/state` alone, and the phase prompts tell the agent to read the *current* `feedback.json`. This is
a latent gap, not a live defect.

### Impact

Real for multi-round spec reviews, which are the normal case for a contested design. The reviewer's
own record of what they asked for is destroyed by the act of asking for the next thing. It also
means a "did you address every annotation" check — the obvious thing to want from a review loop —
cannot be written against the run dir as it stands.

### Fix idea

Write `feedback-<turn>.json` alongside `feedback.json` (keep the stable path as the "current turn"
pointer so every existing reader and the documented run-dir contract in `README.md` are unaffected).
The run dir already carries per-task artifacts with this shape (`<task>-review-<n>.head`), so it
matches the existing convention and costs one extra `fs::write` per submit.

## A routine permission prompt notifies nobody when no `phase wait` is running (2026-08-06)

**Severity:** medium — the run stalls indefinitely and every alarm surface stays deliberately quiet.
**Found:** 2026-08-06, designing the blocked-agent watchers.

### Symptom

A phase agent hits an ordinary tool-permission prompt ("Do you want to make this edit to `x.rs`?").
Nothing raises an alarm: `drovr watch` keeps watching, the session-list badge renders in the quiet
weight, no notification fires, and `drovr list` shows a lowercase `blocked`. The run makes no
progress until someone happens to look.

### Root cause

Deliberate, and only half-true in the state that produces this. Alarms are gated on
`BlockedAgent::needs_human`, which is false for `BlockedClass::Routine` because
`phase::triage_blocked_phase` AUTO-ANSWERS routine prompts — a badge firing on every file-edit
dialog would train people to dismiss the one that matters.

But that triage only runs from inside `drovr phase wait`. A phase nobody is waiting on — the driver
was compacted, the wait timed out and was never re-armed, the phase was driven by hand — has no
auto-answerer, so the prompt drovr classified as "someone else will handle this" is handled by no
one. Same shape as "A finished phase reports `running` forever unless the driver happens to run
`phase wait`": a fact that is true only on the path where a wait exists.

### Impact

Bounded but real. The run is visibly stalled to anyone reading `drovr list` or the session list (the
quiet badge names the phase), so it is a *notification* gap rather than an invisible one. The
severity is that the quiet weight is the honest rendering when a wait IS running, and the two states
are indistinguishable from the scan alone.

### Fix ideas

Make the distinction observable rather than assumed. `drovr phase wait` could stamp the run dir with
a liveness marker (a `<phase>.waiting` file it removes on exit), letting the scan ask "is anyone
actually going to answer this?" instead of assuming someone is. A cheaper version: escalate a
routine prompt that has been sitting for more than N sweeps, which needs the scan to remember when
it first saw a block. Do not simply alarm on every routine prompt — the noise is what the split
exists to avoid.

## Viewing a finished run's page logs a herdr diagnostic every blocked sweep (2026-08-06)

**Severity:** low — noise in `drovr serve`'s own log, bounded and self-limiting.
**Found:** 2026-08-06, live-checking the blocked scan against a run whose workspace was gone.

### Symptom

With a run's page open in the review UI, the server's stderr grows a line per dead pane per sweep:

```
drovr: herdr's pane.get failed for pane wZZ:p9: pane wZZ:p9 not found …
Agent status polling is degraded — phase sends and waits will run to their timeouts …
```

### Root cause

A run keeps its recorded `pane_id`s forever — nothing clears them when a session ends — so
`blocked::scan_run` asks herdr about panes that no longer exist, and `SystemHerdr::pane_info`
reports every failed `pane.get` on stderr. `GET /api/runs/<run>/agents` scans unconditionally,
unlike `/api/runs` and `drovr list`, which skip runs whose workspace herdr no longer lists.

That asymmetry is deliberate: the session list sweeps EVERY run (so the noise is multiplied by the
whole data dir), while the agent tree sweeps the one run a human deliberately opened and should say
what it can about it, even if the workspace id reads oddly.

### Impact

Bounded by the 5s scan cache — at most one burst per run per 5s however many tabs are open — and it
lands in the daemon's log rather than in front of anyone. The diagnostic's own wording is also
misleading here: nothing is waiting on those panes, so nothing will "run to its timeout".

### Fix idea

Herdr's diagnostic is the thing that is wrong for this caller, not the scan: a poll whose whole
purpose is to find out whether a pane is still there does not need to be told, loudly, that it is
not. See also "herdr's 'polling is degraded' diagnostic fires on a reap's EXPECTED path" — the same
diagnostic, the same mismatch, a different caller. One fix (a quiet form of the poll for callers
that treat a missing pane as an answer) covers both.

## A partially unreadable sweep is cached as a clean answer (2026-08-06)

**Severity:** low — a five-second window, and only for a run where some panes answer and others do not.
**Found:** 2026-08-06, review round 2 of the blocked-agent watchers.

### Symptom

A run has three panes. herdr answers for two and not for the third. If the unanswered one is the
pane that is blocked, `/api/runs` reports `blocked: null` — a clean row — and the answer is cached
for the full `BLOCKED_TTL`.

### Root cause

`RunScan::inconclusive()` is `attached == 0 && unreadable > 0`: a sweep counts as having learned
something the moment ANY pane answers. A partial failure is therefore treated as conclusive.

The reason it is drawn there is `pane_info`'s contract. It returns the same `None` for "herdr is
unreachable" and for "that pane id does not name anything any more", and the second is *permanent* —
a run that keeps a dangling pane id (a pane the human closed by hand, say) would be flagged
uncertain on every poll forever. Since an inconclusive sweep is deliberately never cached, that run
would also re-sweep herdr every 2s for the rest of the server's life, which is the cost the cache
exists to avoid.

### Impact

Bounded: it takes a run where herdr answers for some panes but not the one that is blocked, and it
self-heals on the next sweep that reaches the pane. The alarm-holding rule on the browser side only
fires on `unknown`, so this window is also a window where a *cleared* alarm could be dropped.

### Fix idea

The fix is at the herdr boundary, not here: `pane_info` collapsing "socket down" and "pane gone"
into one `None` is what forces the heuristic. herdr distinguishes them — the failure carries
`pane_not_found` — so a poll result that says which would let `inconclusive()` be exact ("any pane
we could not REACH", ignoring panes herdr positively reported as gone) with no permanent-uncertainty
problem. That is a change to drovr's core poll primitive and its documented contract, so it wants
its own change rather than riding along with a feature.

## The seed-delivery detector reports the OPPOSITE of what happened on a cursor pane, and it kills the whole panel (2026-08-06)

**Severity:** high (exit 1 on a review panel that is one keypress from working, and unlike a
phase there is no hand-recovery path — the panel dies).
**Found:** 2026-08-06, run `tiered-review` task-1, reviewer pane `wD4:p6`, a **cursor** agent.

This is the `paste-not-submitted` case the opencode entry above names in its disambiguation
list. What is new here is not the delivery failure — it is that **drovr's diagnostic asserts the
opposite of the pane's actual state**, and then declines to act on the strength of that wrong
conclusion.

### Symptom

`drovr code-review run tiered-review task-1` exits **1**, twice, with:

> the seed was NOT delivered — herdr saw no state change after the prompt, and the payload is
> nowhere in the agent's composer, so it was swallowed rather than left unsubmitted.

Reading the pane showed the composer holding exactly the seed:

```
  → [Pasted text #1 +72 lines]
  Plan (shift+tab to cycle)
  Composer 2.5
```

A single `herdr pane send-keys wD4:p6 Enter` submitted it and the reviewer ran normally. The
payload was never swallowed; it was left unsubmitted, which is the branch the message explicitly
rules out.

### Root cause

cursor renders a multi-line paste as a **reference** — `[Pasted text #N +M lines]` — not as the
text. drovr's detector looks for the payload's own characters, does not find them, and concludes
it vanished. The inversion then propagates into the decision: drovr's stated reason for not
pressing Enter is *"with nothing visibly in the composer we cannot tell a cleared input from a
dialog"* — but something **is** visibly in the composer. The refusal is correct policy applied to
a state that is not the one it is guarding against.

### Impact

Worse than the phase-level variant (*"`drovr phase send`: the false success is fixed; `until` is
still a LEVEL, not an edge"* above) on three counts:

- A phase whose seed is left unsubmitted is recoverable by hand
  (`drovr phase brief … | drovr phase send … -`). **A reviewer pane is not** — drovr says so
  itself: *"this phase is not one `phase send` re-opens."* The whole panel dies with it.
- It returns exit **1**, which must never be read as approval.
- It is reproducible, not flaky: two `--fresh` retries failed identically. Seed-not-delivered hit
  **four times** this session — `tiered-review` brainstorm, `tiered-review` plan,
  `implement-task-1`, and this review pane.

### Fix direction

1. Treat a paste reference as composer content: match `[Pasted text #N +M lines]` for cursor, and
   the equivalent rendering for each other backend.
2. When composer content is detected, **submit** rather than bail — the guard exists for an empty
   composer, which this is not.
3. When the detector is genuinely unsure, report *unsure* rather than asserting *swallowed*. The
   message above states a root cause it has not established, and that is what sent two retries at
   the wrong problem.

Note the tension with the auto-suggested-prompt entry below: a detector that simply asks *"is the
composer non-empty?"* fixes this one and breaks that one.

## An agent's auto-suggested prompt is indistinguishable from composer content to anything reading a pane (2026-08-06)

**Severity:** medium (nothing breaks, but it makes pane reads unreliable for drovr and for
humans alike, and it cost this session two wrong diagnoses before the real cause was found).
**Found:** 2026-08-06, run `skill-stickiness`, across five panes.
**Not a live blocker:** the human is turning the suggestion feature off locally. Filed for the
general case — any backend that renders a suggestion into the composer region has this shape.

### Symptom

Claude Code renders an **auto-suggested next prompt** into the composer region of an idle pane.
`herdr pane read` shows it exactly like typed text, so drovr — and any agent or operator reading
panes — cannot tell a suggestion from a real undelivered payload.

Observed five times this session, each a plausible next instruction:
`gates on conditional recall, add neovim, keep the strict bar` ·
`finish the results doc and run code-review` · `who made those two commits?` ·
`continue with task 2` · `merge origin/main too`.

### Root cause

The composer region is a rendering surface, not a buffer with a type. A suggestion and a pasted
payload occupy the same rows and read identically; nothing in the pane text marks which is which.

### Impact

Diagnostic, and it compounds. It was misread first as a monitor writing nudges into composers,
then as the human typing instructions that were not being submitted — both wrong, both acted on
before being disproved. The driver then avoided sending to affected panes at all, for fear of
concatenating onto real input, and routed work to fresh phases instead. The cost is the wrong
diagnosis and the work rerouted around it, not a broken command.

### Tells that distinguish it

Worth encoding wherever drovr inspects a composer:

- It appears only on **idle/finished** panes, never mid-turn.
- It is always a plausible *next* instruction rather than a brief or a payload.
- **Enter does not submit it.**
- `ctrl+u` / BackSpace appear to "fail to clear" it — there is nothing to clear.

### Fix direction, and the constraint on it

This and the cursor paste-reference entry above **pull in opposite directions, and a fix for one
must not break the other**: that one is a real payload drovr wrongly treats as absent, this one is
a non-payload drovr could wrongly treat as present. A detector that asks *"is the composer
non-empty?"* gets this entry wrong; one that matches only literal payload text gets that one
wrong. The signals that separate them are the paste-reference pattern and whether the pane is
mid-turn — not the presence of characters.
