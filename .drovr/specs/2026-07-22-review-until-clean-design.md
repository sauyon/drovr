# drovr:code-review — automatic review-until-clean pass

**Status:** Draft (rev 4) — awaiting review · **Branch:** review-until-clean

## Problem

"Self-review before done" is only prose in the pipeline phase-prompts: no enforced loop, no
machine-checkable "clean" signal, and it fires only in a full pipeline. `agy-review` proved
independent read-only review over a change, but it's external, Gemini-only, one-shot. Make it
a first-class drovr primitive: an independent, read-only, backend-agnostic review **panel**
that the pipeline runs automatically after each implement task and reruns until clean.

## Who drives the loop

A skill is prompt-context loaded into an agent, **not** an orchestrator. The **pipeline
driver** (the agent running `drovr:pipeline` — the single writer of orchestration) drives the
loop: after an implement task drops its done marker, the driver calls the **blocking**
`drovr code-review run <run> <task>`, reads its exit code, and either re-enters the implement
phase (sending it the findings to fix) or proceeds. `drovr code-review` itself runs **one**
review panel and returns. The `drovr:code-review` skill supplies discipline/prompting for the
driver, implementer, and reviewers — never control flow.

## The review panel

Per pass, `drovr code-review run <run> <task>` spawns a **panel of read-only reviewer phases**
— one per configured angle (correctness, security, error-handling, type-design; extensible) —
via the hardened spawn path. Each reviewer:

- is spawned **read-only** (per the per-agent flag map, e.g. claude `--permission-mode plan`);
- is seeded with `<run_dir>/<task>-review-<angle>-seed.md` (angle brief + base/head SHAs + task
  description + the findings JSON schema);
- reviews `git diff <base>..<head>` **plus** the working tree, may read any file and **run
  tests**, and writes `<run_dir>/<task>-review-<angle>.json`;
- drops its `drovr phase done` marker and **exits**.

drovr waits for every angle's marker, then **merges** the per-angle files into
`<run_dir>/<task>-review.json` — a **union**, each finding tagged with its `angle` (no semantic
LLM de-dup in v1; overlaps simply both appear). Exit: `0` clean (no ≥ Important) · `3` findings
· `2` timeout · `1` error.

- **Read-only trust boundary:** "read-only" means a reviewer **never edits project source or
  `state.json`** — that is the single-writer invariant. `--permission-mode plan` is how we stop
  edits, but it is not a kernel sandbox; running tests may still have benign side effects
  (build artifacts, caches), which is accepted. The invariant that matters is "not a writer,"
  not filesystem-level read-only.
- **Phase namespace:** reviewer phases are named `review:<task>:<iter>:<angle>` and tracked in a
  **separate `review_phases` list** on the run, so they never pollute the pipeline's
  `brainstorm/plan/implement/review` phases or `first_incomplete`/progress.
- **Sequencing:** the panel runs → **all reviewers drop done and exit** → only then does the
  implementer apply fixes (Important **and** nits). No reviewer is alive while the implementer
  writes, so single-writer holds.

## Diff scope: `base..head`

drovr has no "task commits" concept, so the implement phase establishes it: when an implement
task **starts**, it writes `<run_dir>/<task>-base.sha` = current `HEAD`. `drovr code-review`
reads `base` from that file and uses current `HEAD` as `head`; reviewers diff `base..head` +
the working tree. This is a **required** interface, not deferred.

## Termination — impact-scaled judgement, no cap

The driver keeps looping while blocking findings (≥ Important) remain and it judges more rounds
help; it stops when the change is clean-and-converged **for its impact** (low-impact + clean
stops early; critical earns repeated passes), or when it judges iteration isn't converging
(e.g. a round yields no net progress, or the same finding recurs). **No hardcoded floor or
ceiling** — a deliberate decision; the `2` timeout (wall-clock, resumable) is the only
mechanical bound. Nits never block but are fixed each round.

## Backend-agnostic agents

The spawned agent is configurable (generalizes the hardcoded `"claude"` in `phase_start`). A
per-agent flag map declares each backend's command + read-only flag:

```toml
default_agent = "claude"
angles = ["correctness", "security", "error-handling", "type-design"]

[agents.claude]
command = "claude"
readonly_flag = "--permission-mode plan"

[agents.codex]
command = "codex"
readonly_flag = "--sandbox read-only"
```

A backend with no read-only flag can't serve as a reviewer.

## Human gate / server integration (in scope)

The automated review **surfaces through the existing review server**, not a separate headless
path: it renders the reviewed `base..head` diff + the merged findings as inline annotations in
the browser (`cli/src/review.rs` + `cli/web/`), so a human can **watch the review loop and
optionally intervene** — one review surface, not two. (This also embraces the server UI/flow
fixes we've been making rather than excluding them.)

## Interfaces

- **`drovr code-review run <run> <task> [--timeout-ms N]`** — blocking; spawns the panel, waits
  for all angles, merges, exits `0`/`3`/`2`/`1`. Named `code-review` to avoid clashing with the
  existing `review summary` / `review wait`.
- **`<run_dir>/<task>-base.sha`** — `HEAD` at task start (written by the implement phase).
- **`<run_dir>/<task>-review-<angle>-seed.md`** — reviewer seed (angle + base/head + task +
  findings schema).
- **`<run_dir>/<task>-review-<angle>.json`** → merged **`<run_dir>/<task>-review.json`**:
  `{"verdict":"clean|changes","findings":[{"file","line?","severity":"critical|important|nit","angle","summary","rationale"}],"impact?":"..."}`.
  Clean = no `critical`/`important`.
- **Config** — `${XDG_CONFIG_HOME:-~/.config}/drovr/config.toml` (agent map + angles).
- **`<task>`** — a caller-provided **label** (the plan's task name/number), not a new data-model
  entity; a repeated label just overwrites that task's artifacts (latest review wins).
- **`drovr:code-review` skill** — discipline/prompting for driver + implementer + reviewers.

## Scope

**In:** the `drovr code-review` CLI + panel, per-angle seed/findings/merge, `<task>-base.sha`,
per-agent flag map, XDG config, the review-server rendering of findings, the loop wired into
pipeline implement (driver-run), a `review_phases` list on the run, the skill, tests.

**Out:** no change to the handoff/compress contracts; not auto-wired into `drovr:handoff`
(pipeline implement only).

## Testing

Rust unit-tests: findings parse + merge (union + angle tag) + clean gate + exit codes +
reviewer spawned read-only with the configured agent + `base.sha` read + `review_phases`
isolation from pipeline progress. The loop + impact-scaled stop are skill discipline
(agent-driven), validated per `writing-skills`.

## Open questions

- Server rendering: reuse the existing diff view + a findings-annotation layer, or a dedicated
  review view? (decide during implementation)
