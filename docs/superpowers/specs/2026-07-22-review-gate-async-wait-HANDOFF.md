<!--
  Hand-authored drovr HANDOFF doc (7-section shape per skills/handoff/HANDOFF-template.md)
  for a FOLLOW-UP task surfaced during the split-pane fix. No run/transcript exists yet —
  this seeds a fresh agent (or a `drovr new` run) to design + build it.
-->

# HANDOFF: make the review-gate flow survive long human reviews

## Objective
drovr's spec review gate forces the **driver** to babysit a blocking `drovr serve` and
**busy-poll** for the reviewer's decision. For a review that takes hours, that means a live
session holding a poll loop (or repeated 30–60 min timeouts). Design and build a review-gate
wait that is **efficient and resumable** — the driver blocks cheaply, or exits and resumes
later, without a hot poll loop.

## State
- Surfaced 2026-07-22 while fixing the phase-pane split bug (that work is a separate branch).
- Nothing implemented for this task yet — design stage.
- Observed live this session: after `drovr serve <run>` + `drovr review summary`, the only
  way to learn the reviewer acted was to `curl GET /state` on an interval. Two ~1h polls both
  timed out before the human finished; the approval eventually landed but the driver had no
  cheap way to wait for it.
- The durable signal **already exists on disk**: the server writes `feedback.json` (on
  "request changes") and an `approved` marker (on approve) into the run dir. There is just no
  blocking primitive that waits on them.

## Decisions + rationale
- **Mirror `phase_wait`, don't invent a new mechanism.** `cli/src/phase.rs::phase_wait`
  already blocks by polling a filesystem marker (`<phase>.done`) at `POLL_INTERVAL` until a
  deadline — deliberately NOT watching herdr status. The review gate should get the same
  shape: a `drovr review wait <run> [--timeout-ms N]` that blocks on the `approved` marker
  and/or a `feedback.json` turn bump, returning a distinct exit code per outcome
  (approved / changes-requested / timeout). WHY: consistency with the proven wait pattern,
  and the markers are the single source of truth the server already writes.
- **Resumable over long-lived.** A driver must be able to exit and later re-run
  `drovr review wait` (or `drovr status`) and still learn the outcome from the on-disk
  markers — state must not live only in the driver's memory. WHY: hours-long reviews should
  not pin a session.
- **Keep the human-side flow unchanged.** The browser gate, `drovr review summary`, and the
  `LoopState` machine (`Idle → Ready → Waiting → Approved`) stay as-is; this adds a *driver
  wait*, not a protocol change.
- **Open: push vs poll.** A filesystem poll (like phase_wait) is the low-risk default. A push
  path (herdr `notification`, or a server callback) is a possible enhancement, deferred until
  the poll version proves insufficient.

## Interfaces / contracts
- Review server (`cli/src/review.rs`): `GET /state` → `{state, turn}`; `POST /submit`
  (approve → writes `approved` marker + state `Approved`; request-changes → writes
  `feedback.json` + state `Waiting`); `POST /summary` → state `Ready`. `serve()` writes
  `review.addr`.
- Run-dir files (`~/.local/share/drovr/runs/<run>/`): `review.addr`, `spec.md`, `prior.md`,
  `feedback.json` (`{turn, decision, feedback, answers, annotations}`), `summary.txt`,
  `questions.json`, `approved`.
- Pattern to reuse verbatim: `cli/src/phase.rs::phase_wait` (marker poll + deadline + exit
  codes) and its `done_marker` path helper.
- Proposed new surface (decide in design): `drovr review wait <run> [--timeout-ms N]` under
  the existing `Review` subcommand group in `cli/src/main.rs`.

## Open questions
- Exact exit-code contract for `drovr review wait` (0=approved, 2=timeout, 3=changes-
  requested?, 1=io-error) — align with `phase wait`'s existing codes.
- Should `wait` return on **each** `Waiting` turn (so the driver can forward feedback and
  loop), or only on terminal `Approved`? Likely: return on any state change, caller loops.
- Push notification (herdr `notification` / harness `PushNotification`) — in scope or defer?
- Interaction with `drovr:pipeline`'s spec-gate step (skills/pipeline/SKILL.md) — update it to
  call `drovr review wait` instead of prescribing a manual poll.

## Next step
Brainstorm the design for `drovr review wait` (blocking, marker-based, resumable), starting
from `phase_wait`'s structure; then TDD it in `cli/src/review.rs` + wire the subcommand in
`main.rs`, and update `skills/pipeline/SKILL.md` / README's review-loop section to use it.

## Artifact pointers
- `cli/src/review.rs` — serve, `review_summary`, `LoopState`, `/state`/`/submit`/`/summary`.
- `cli/src/phase.rs` — `phase_wait` / `phase_done` / `done_marker` (the pattern to mirror).
- `cli/src/main.rs` — `cmd_serve`, `cmd_review`, the `Review` subcommand group.
- `README.md` — "Review loop flow"; `skills/pipeline/SKILL.md` — "The spec gate".
- Git: this session's split-pane fix lives on branch `fix-split-pane`; this task is
  independent of it. Read `git log`/`git diff` on whatever branch carries it once started.
