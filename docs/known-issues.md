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
