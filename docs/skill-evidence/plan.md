# Plan: skill stickiness

Implementation plan for the approved `spec.md` (791 lines, frozen). 22 tasks, each sized for one
fresh clean-context phase.

**Read `spec.md` §0 before reading this file.** The task order below **is** §0's order, not section
order. A planner or driver that re-sorts by section number destroys the measurement.

---

## 0. Global constraints every task inherits

**C1 — HARD GATE. Arm A must be frozen before any `SKILL.md` is touched.** Task 1 copies the five
pre-fix `SKILL.md` files into `docs/skill-evidence/arms/A/`. **No task numbered ≥ 2 may edit any
file under `skills/` until Task 1 is committed.** Fix 1 (Task 7) overwrites the exact
`description:` lines arm A measures; once it lands, arm A is unrecoverable without a checkout.
Task 1 is deliberately ordered *ahead of* §0's step 1 (`writing-skills`) because it is a pure copy
with no dependencies, and putting it first removes the whole risk class. This is a strengthening of
§0's gate, not a reordering of it: §0 step 1 creates only new files and touches nothing arm A
measures.

**C2 — the probe feeds skill text explicitly.** Per §8 "Deployment reality", nothing in this run
takes effect in a live session until the flake pin is bumped. Every A/A′/B run therefore pastes the
arm's text from `docs/skill-evidence/arms/<arm>/<skill>.md` into the subagent's prompt. The `arms/`
tree is not a convenience — it is the only reason A′ (an intermediate file state that no longer
exists on disk once fix 4 lands) is measurable at all.

**C3 — every probe run is tracked in a ledger.** `docs/skill-evidence/run-ledger.md` is appended
to by every task that spawns probe subagents. **The run-count ceiling has been LIFTED by the human
(2026-08-07).** The ledger stays mandatory and append-only — it records what was actually spent,
retries and unmetered runs included — but a cumulative total is **no longer a pass/fail condition
anywhere in this plan**, and no task halts or records a null merely for crossing a number. A stage
that stops early still records why it stopped. The original 122 figure, and the raise to 191
authorized during `cross-model-arm`, survive in the ledger's prose header as history, not as a gate.

**C4 — measurement phases run STRICTLY SEQUENTIALLY** (`ab-*` tasks 16–21, one at a time). This is
the plan's answer to the brainstorm handoff's second open question. Three reasons: (a) REFACTOR
iterations *edit skill files*, so two concurrent `ab-*` phases would break drovr's single-writer
rule; (b) the ledger is a global record, and only sequential execution lets the driver read
the ledger before starting the next phase; (c) there is no wall-clock win — each phase's runs are
foreground-serialised regardless.

**C5 — probe subagents.** Claude Code `Agent` tool, `subagent_type: general-purpose`,
`model: sonnet`, **FOREGROUND**. Never `run_in_background`, never `ScheduleWakeup`. The two samples
of one scenario may be issued as two parallel `tool_use` blocks in a single message — that is still
foreground and blocking. This applies to the scorer too.

**C6 — no fabricated measurements** (§2.1 exception 1) and **no copied text** from superpowers
(exception 2). Every `[tier 4]` marker already in `spec.md` must survive into the shipped prose at
its site.

---

## 1. Shared interfaces introduced by this plan

These are new artifacts the spec names but does not shape. They are fixed here so tasks 6–22 bind
to the same thing. Later tasks **must not redesign them**.

### 1.1 Arm snapshots

```
docs/skill-evidence/arms/A/<skill>.md          # byte-exact pre-fix SKILL.md          (Task 1)
docs/skill-evidence/arms/A-prime/<skill>.md    # post-fix-1, pre-fix-3/4              (Task 7)
docs/skill-evidence/arms/B/<skill>.md          # post-fix-4 (pre-REFACTOR)            (Tasks 10-14)
docs/skill-evidence/arms/B-r<i>/<skill>.md     # REFACTOR iteration i (i ∈ 1,2)       (Tasks 16-20)
docs/skill-evidence/arms/voice/V<n>.md         # n ∈ 0,1,2,3                          (Task 15)
docs/skill-evidence/arms/MANIFEST.md
```

`<skill>` ∈ `tdd` · `systematic-debugging` · `verification-before-completion` · `code-review` ·
`using-drovr`. A snapshot is the **whole file including frontmatter** — the `description:` is the
line fix 1 changes and is itself under test.

`MANIFEST.md` is one markdown table, appended to (never rewritten) by each snapshotting task:

| arm | skill | source path | `git hash-object` of the copy | commit `HEAD` at copy time | date |

The manifest is what makes "byte-exact" checkable: Task 16+ verifies
`git hash-object docs/skill-evidence/arms/A/tdd.md` still equals the recorded value before using it.

### 1.2 Scenarios

`skills/writing-skills/scenarios/<skill>-<n>.md`, `n ∈ 1,2,3`, plus
`skills/writing-skills/scenarios/using-drovr-noskill-<n>.md`, `n ∈ 1,2`. **17 files.**

Fixed frontmatter (a new contract — nothing in the repo parses it, but tasks 16–21 read it):

```yaml
---
skill: tdd                 # one of the five, or `using-drovr` for the noskill class
n: 1
tag: dev                   # `dev` | `holdout`; exactly one `dev` per skill, two `holdout`
pressures: [time, sunk-cost, authority]   # ≥3, from §7.1's seven
forced_choice: "A: ship it now · B: write the failing test first · C: ask the human"
correct_option: B
---
```

Body = the verbatim prompt handed to the probe subagent. §7.1's construction rules are binding: real
file paths, concrete numbers and deadlines, a forced A/B/C choice, asks **"what do you do"** not
"what should you do", **no escape hatch to "I'd ask the human"** (so `correct_option` is never the
ask-the-human option), ≥3 combined pressure types.

`verification-before-completion-2.md` and `-3.md` (the two held-out) are **reused verbatim** as
§7.4's "2 scenarios" for the voice probe. No new scenario files for `ab-voice`.

### 1.3 Scoring, blinding, and the verdict schema

`skills/writing-skills/references/scoring-rubric.md` (Task 2) defines both the rubric and this
verdict object. The scorer returns **one fenced `json` block per transcript**:

```json
{
  "transcript_id": "7f3a1c",
  "compliant": true,
  "cites_section": true,
  "names_temptation": true,
  "meta_test_clear": true,
  "new_rationalizations": ["verbatim quote", "..."],
  "evidence": "the verbatim line from the transcript that decided `compliant`"
}
```

`compliant` is binary on the scenario's `forced_choice`/`correct_option`. The other three are §7.1's
pass criteria recorded separately as booleans (`meta_test_clear` = the meta-test "how should this
have been written?" returned "it was clear").

**Blinding is real, not nominal.** A transcript that contained the arm's skill text would let the
scorer infer the arm from the text alone, so:

```
docs/skill-evidence/transcripts/<skill>/<id>.md      # scenario body + the agent's VERBATIM response
                                                     # ONLY. The arm's skill text is NOT included —
                                                     # it already lives in arms/.
docs/skill-evidence/transcripts/<skill>/blind-map.json
docs/skill-evidence/transcripts/<skill>/scores.json
```

`<id>` is a short opaque token (6 hex chars) with no arm, scenario or sample in it.

```json
// blind-map.json  — written BEFORE scoring, not shown to the scorer
{ "7f3a1c": { "arm": "B", "scenario": "tdd-2", "sample": 1 } }
// scores.json — the scorer's verdict objects, one per transcript, in scoring order
[ { "transcript_id": "7f3a1c", "compliant": true, ... } ]
```

The phase agent joins `scores.json` to `blind-map.json` **only after every score is recorded**, then
writes the summary table into `docs/skill-evidence/<skill>.md`. The scorer subagent is given the
transcript files and `scoring-rubric.md` and nothing else — never `blind-map.json`, never `arms/`.

**Stated limitation, to be recorded verbatim in each `docs/skill-evidence/<skill>.md`:** the
transcript still shows the agent's own words, and an armored agent's response reads differently from
an unarmored one, so blinding removes the arm *label* but cannot remove all signal. Do not describe
the scoring as fully blind.

### 1.4 The run ledger

`docs/skill-evidence/run-ledger.md` — one markdown table, append-only:

| task | stage (§7.3 row) | runs this stage | cumulative | stage ceiling | ceiling hit? |

Every probe-spawning task appends its rows as its **last** evidence write. Tasks 16–21 read the
cumulative total first so the ledger stays a running total; they do **not** halt on it (C3 — the
ceiling was lifted).

### 1.5 Per-skill evidence records

`docs/skill-evidence/<skill>.md` (5 files) · `voice.md` · `per-turn-gate.md`. Per §7.2, each
`<skill>.md` carries: scenarios used, **verbatim** baseline rationalizations, the counter-text
written for each, the scored results with dates, the §1.3 blinding limitation, and — if applicable
— the failure and the reverted state.

---

## 2. Tasks

### Task 1 — Freeze arm A (HARD GATE, do first)

**Objective.** Copy the five pre-fix `SKILL.md` files into `docs/skill-evidence/arms/A/` and record
them in `MANIFEST.md`, so arm A survives fix 1.

**Interfaces introduced.** `docs/skill-evidence/arms/A/<skill>.md` (5 files) and
`docs/skill-evidence/arms/MANIFEST.md`, both per §1.1.

**Depends on.** Nothing. **Blocks.** Every task that edits `skills/` (7, 8, 10–14, 16–20, 22).

**Verification.** New test `cli/tests/skills_valid.rs::arm_a_snapshots_match_manifest` — for each of
the five, assert the file at `docs/skill-evidence/arms/A/<skill>.md` exists and its SHA-256 (reuse
`cli/src/sha256.rs`) equals the value recorded in `MANIFEST.md`. This test must keep passing for the
whole run; it is the tripwire on the gate. Additionally, at this task's `HEAD`, `diff -u
skills/<skill>/SKILL.md docs/skill-evidence/arms/A/<skill>.md` is empty for all five — record that
output in the task report.

**Notes.** Current sizes, for the report's baseline: `tdd` 1764 B / 44 lines ·
`systematic-debugging` 1841 / 39 · `verification-before-completion` 1717 / 42 · `code-review`
2362 / 51 · `using-drovr` 5087 / 93.

---

### Task 2 — `drovr:writing-skills` (§7.1)

**Objective.** Author the meta-skill: `skills/writing-skills/SKILL.md` plus its three reference
files, in drovr's voice.

**Interfaces introduced.**
- `skills/writing-skills/SKILL.md` — frontmatter `name: writing-skills` (must equal the dir name;
  `skills_valid.rs::all_skills_have_valid_frontmatter` enforces it) and a `description:` that is a
  trigger, not a summary (§3's rule applies to new skills too).
- `skills/writing-skills/references/pressure-scenarios.md` — §7.1's construction rules, and the
  §1.2 frontmatter contract stated as the authoring format.
- `skills/writing-skills/references/testing-with-subagents.md` — the FOREGROUND rule (C5), the
  single-writer rule, §2.1 exception 1.
- `skills/writing-skills/references/scoring-rubric.md` — **§1.3's rubric and verdict schema
  verbatim**, including the blinding procedure and its stated limitation. Tasks 16–21 hand this file
  to the scorer, so it must be self-contained: a scorer that reads only this file must know what to
  return.

**Content requirements** (all from §7.1, all `[tier 3]` convention-follows — mark them):
the scenario↔test / `SKILL.md`↔production-code mapping; the loop as **a fenced `dot` block** (per
§2.3 it is a stop-too-early loop) terminating on *no new rationalization* **or** the §7.3 REFACTOR
ceiling, whichever comes first; the **four-part closure** (negation in the rule · rationalization
row · red-flag bullet · `description:` update naming the *symptom of being about to violate*) with
"all four every time, never one"; the four **pass criteria** and the four **not-bulletproof
signals**. Anthropic's convergent eval-first guidance is cited (§10), not asserted as a measurement.

**Depends on.** Task 1 (gate C1 — this task creates only new files, but the gate is absolute).

**Verification.** `cargo test --test skills_valid` (frontmatter validity on the new skill).
`writing-skills` is **not** added to `BODY_BUDGETS` (§2.4 checks only the four methodology skills
plus `using-drovr`); keep `SKILL.md` under ~9000 B by pushing detail into `references/` and record
its byte size in the report. Manual check, reported: the fenced `dot` block is balanced and the
four-part closure is stated as all-four-every-time.

---

### Task 3 — The scenario corpus (§7.2)

**Objective.** Write all **17** scenario files to §1.2's contract.

**Interfaces introduced.** `skills/writing-skills/scenarios/{tdd,systematic-debugging,
verification-before-completion,code-review,using-drovr}-{1,2,3}.md` (15) and
`using-drovr-noskill-{1,2}.md` (2), each with the §1.2 frontmatter.

**Depends on.** Task 2 (`references/pressure-scenarios.md` is the authoring contract).

**Verification.** New test `cli/tests/skills_valid.rs::scenarios_are_well_formed`:
- exactly 17 files under `skills/writing-skills/scenarios/`;
- each parses a closed `---` frontmatter block with non-empty `skill`, `n`, `tag`, `pressures`,
  `forced_choice`, `correct_option` (reuse the existing `parse_skill` frontmatter walker, or a small
  sibling that collects arbitrary keys — do not add a YAML dependency);
- `tag` ∈ {`dev`, `holdout`}, and per skill exactly **1 `dev` + 2 `holdout`** across `-1..-3`;
- `pressures` lists **≥3** entries drawn from `time, sunk-cost, authority, economic, exhaustion,
  social, pragmatic`;
- `correct_option` is not the ask-the-human option (assert `forced_choice`'s
  `correct_option`-labelled clause does not match `/ask|escalat|human/i`).

The two `noskill` files carry `skill: using-drovr` and `tag: holdout`, and are excluded from the
1-dev/2-holdout count (assert on `-1..-3` only).

---

### Task 4 — Fix 2, hook layer: CLI (§4.2) — *independently orderable*

**Objective.** All Rust for the per-turn gate: config key, `--gate` flag, parameterized envelope,
the card `const`, and the previous-turn suppression check.

**Interfaces introduced.**

`cli/src/config.rs` — extend `ReflexConfig` (currently `:39-64`):
```rust
pub struct ReflexConfig {
    #[serde(default = "default_true")] pub enabled: bool,
    #[serde(default)] pub preamble: Option<String>,
    #[serde(default)] pub sections: BTreeMap<String, bool>,
    /// Per-turn gate (UserPromptSubmit). Default TRUE [tier 4].
    #[serde(default = "default_true")] pub per_turn: bool,
}
```
Reuse the **existing named** `default_true()` at `config.rs:111-113`. A bare `#[serde(default)]`
would yield `false` — the trap already documented at `config.rs:108-110`. Update the `Default` impl
(`:56-64`) to `per_turn: true`.

`cli/src/reflex.rs`:
```rust
/// The per-turn gate card. NOT extracted from SKILL.md (§4.2): extraction would
/// need markers inside the region §4.1 places outside all reflex:section markers.
pub const GATE_CARD: &str = "...";               // ≤600 bytes RENDERED (see below)

/// Package `context` as Claude Code hook JSON for `event`.
pub fn envelope(event: &str, context: &str) -> String;   // was envelope(context)

/// The gate JSON, or None when disabled or when the previous turn already
/// invoked a drovr:* skill.
pub fn gate_json(cfg: &ReflexConfig, transcript: Option<&str>) -> Option<String>;

/// True if the assistant turn since the last user message contains a `Skill`
/// tool_use whose `input.skill` starts with `drovr:`.
pub fn skill_invoked_last_turn(transcript_jsonl: &str) -> bool;
```
`reflex_json` (`:155-162`) now calls `envelope("SessionStart", &context)` — unchanged output.
`gate_json` calls `envelope("UserPromptSubmit", GATE_CARD)`, returns `None` when
`!cfg.enabled || !cfg.per_turn`, and returns `None` when `transcript` is `Some(t)` and
`skill_invoked_last_turn(t)`. **Fail-open toward emitting:** `transcript: None` (absent or
unreadable `transcript_path`) emits the card — drift is worse than a redundant injection. Record
that choice in `per-turn-gate.md`.

Transcript format, verified against a live `~/.claude/projects/<proj>/<id>.jsonl`: JSON-per-line;
records carry `type` ∈ {`user`, `assistant`, `system`, …} and `message.content` as an array of
blocks; a tool call is `{"type":"tool_use","name":"Skill","input":{...}}`. Walk **backwards** from
EOF, stop at the first `type == "user"` record, and search the assistant records seen so far.
Malformed lines are skipped, not fatal.

`cli/src/main.rs` — `Reflex` (`:114-118`) becomes:
```rust
Reflex {
    /// Path to the router skill markdown to inject (SessionStart reflex).
    #[arg(long, required_unless_present = "gate")]
    skill: Option<PathBuf>,
    /// Emit the per-turn gate card instead (UserPromptSubmit).
    #[arg(long, conflicts_with = "skill", required_unless_present = "skill")]
    gate: bool,
},
```
`conflicts_with` + the paired `required_unless_present` give the xor **and** required-one without an
`ArgGroup` (clap 4.6.3, derive). Dispatch at `:959` becomes
`Commands::Reflex { skill, gate } => cmd_reflex(skill.as_deref(), gate)`; `cmd_reflex` reads hook
JSON from **stdin** when `gate` is set, pulls `transcript_path`, and reads that file (missing →
`None`).

**Test updates — load-bearing.** `main.rs:1265-1273` `parse_reflex` must destructure
`skill: Option<PathBuf>` (assert `Some("/p/SKILL.md")`, `gate == false`).
`main.rs:1275-1279` `parse_reflex_requires_skill` asserts bare `drovr reflex` errors — it **still
must**, now because neither `--skill` nor `--gate` is present; keep the test, update its comment.
Add `parse_reflex_gate` (bare `--gate` parses, `skill == None`) and
`parse_reflex_gate_conflicts_with_skill` (both together errors).

**Depends on.** Nothing (§0 step 6 is independently orderable). Placed here, ahead of the probes, so
§4.2's subagent-firing question is answered before any measurement runs.

**Verification.** `cargo test -p drovr` — specifically, new tests in `reflex.rs`'s `mod tests`:
1. `gate_card_within_600_bytes` — `gate_json(&ReflexConfig::default(), None)`, parse the JSON, and
   assert `hookSpecificOutput.additionalContext.len() <= 600`. **Assert on the rendered string, not
   on `GATE_CARD.len()`** — §4.2 budgets the rendered `additionalContext`.
2. `envelope_carries_event_name` — `envelope("UserPromptSubmit", …)` round-trips
   `hookEventName == "UserPromptSubmit"`; the existing `envelope_is_valid_sessionstart_json`
   (`:230-243`) keeps passing via `reflex_json`.
3. `gate_json_none_when_disabled` and `gate_json_none_when_per_turn_false`.
4. `gate_suppressed_after_drovr_skill_invocation` / `gate_emitted_when_no_skill_last_turn` — on
   hand-built JSONL fixtures. Also assert a `Skill` call **before** the last user message does *not*
   suppress.
5. `gate_card_phrases_present_in_router_skill` — the drift guard (§4.2, §9.2): every phrase in a
   `const GATE_CARD_PHRASES: &[&str]` appears in `skills/using-drovr/SKILL.md`. **This test is RED
   until Task 14 writes those phrases into the router.** Resolve by seeding
   `GATE_CARD_PHRASES` in *this* task with only phrases already present in the shipped
   `using-drovr/SKILL.md` (e.g. `"Single writer"`, `"drovr:code-review"`), and having **Task 14 add
   the 1%-rule and per-turn phrases to the list** once it writes them. Never leave a red test across
   a task boundary — a failed task halts the pipeline loop.
6. `per_turn_defaults_true_with_reflex_table_present` — deserialize `"[reflex]\nenabled = true\n"`
   from TOML and assert `per_turn == true` (the `config.rs:108-110` trap, §9.2).
7. `routing_core_survives_section_subtraction` (§9.2) — `render_body` with **every** known section
   set to `false` still contains the routing core. Same seeding problem as (5): assert on text
   present today (`<SUBAGENT-STOP>`, `# Using Drovr`), and have Task 14 extend it to the 1% rule,
   the per-turn rule, the priority ladder and the gate flowchart.

---

### Task 5 — Fix 2, hook layer: wiring, docs, evidence (§4.2)

**Objective.** Ship the hook itself, document the key, and record the unmeasured bet.

**Interfaces introduced.**
- `hooks/user-prompt` — new executable bash script, modelled on `hooks/session-start` but with two
  deliberate differences: **it does NOT suppress on `DROVR_PHASE`** (§4.2's asymmetric suppression —
  a phase is exactly where the discipline must hold), and it passes stdin through. Body:
  `set -euo pipefail`; resolve `CLAUDE_PLUGIN_ROOT` with the same script-dir fallback
  (`session-start:26-33`); `exec "${DROVR_BIN:-drovr}" reflex --gate`. `exec` hands stdout and exit
  status straight to Claude Code so a missing binary fails loudly rather than injecting a partial
  context. Mark executable (`git update-index --chmod=+x`).
- `hooks/hooks.json` — add a sibling to `SessionStart`:
  ```json
  "UserPromptSubmit": [
    { "hooks": [ { "type": "command",
                   "command": "\"${CLAUDE_PLUGIN_ROOT}/hooks/user-prompt\"",
                   "async": false } ] }
  ]
  ```
  **No `matcher` key.** `UserPromptSubmit` takes none; the existing entry's
  `startup|clear|compact` (`hooks.json:5`) must not be copied.
- `README.md` — extend the `### Reflex` block (`README.md:70-94`) with `per_turn` under `[reflex]`:
  what it does, that it defaults **true**, that it does **not** suppress inside phases (unlike the
  `SessionStart` reflex), and the ≤600 B / ~60 KB-per-100-turns cost.
- `docs/skill-evidence/per-turn-gate.md` — §4.2's record: this is drovr's most novel mechanism and
  ships as an **explicit unmeasured bet** `[tier 4]`; the cumulative-cost figures stated both ways;
  the suppression rule and its fail-open direction; and **the empirical answer to "does
  `UserPromptSubmit` fire for Agent-tool subagents in this harness?"**

**How to answer the subagent question** (§4.2 assigns it to §0 step 1; the honest place is here,
where the hook exists, and Task 4/5 are independently orderable so they may run first): in a scratch
session with `CLAUDE_PLUGIN_ROOT` pointed at this worktree, have `hooks/user-prompt` append a line
to a temp file, launch a trivial foreground `Agent` subagent, and count the lines. Record the method
and the answer. Note that the card carries its own `<SUBAGENT-STOP>` line **unconditionally**
regardless of the answer.

**Depends on.** Task 4 (`drovr reflex --gate` must exist).

**Verification.** New cases in `cli/tests/reflex_hook.rs`, following the existing harness
(`repo_root`/`hook_script`/`drovr_binary`/`write_config`, gated on `bash_available()`, with
`XDG_CONFIG_HOME` pinned):
1. `user_prompt_hook_emits_gate_json` — valid hook JSON, `hookEventName == "UserPromptSubmit"`,
   `additionalContext` ≤ 600 bytes.
2. `user_prompt_hook_not_suppressed_in_phase` — with `DROVR_PHASE=run/plan` set the card is **still**
   emitted (the deliberate asymmetry vs `suppressed_when_drovr_phase_set` at `:114-128`).
3. `user_prompt_hook_respects_reflex_disabled` — `enabled = false` → no output.
4. `user_prompt_hook_respects_per_turn_false` — `per_turn = false` → no output.
5. `hooks_json_user_prompt_entry_has_no_matcher` — parse `hooks/hooks.json` and assert the
   `UserPromptSubmit` entry has no `matcher` key while `SessionStart`'s still does.

---

### Task 6 — RED / baseline on the dev set (§7.3, 10 runs)

**Objective.** Run the pre-fix text against each skill's **development** scenario and transcribe
every rationalization **verbatim**. This is the wording fixes 4 and §4.1's red-flag table are built
from; without it they are assertions.

**Interfaces introduced.**
- `docs/skill-evidence/<skill>.md` (5 files) — created here with the RED section: scenario used,
  every rationalization **quoted verbatim**, and the four §7.1 pass-criteria booleans per run.
- `docs/skill-evidence/transcripts/<skill>/` — 10 transcripts per §1.3 (scenario body + the agent's
  verbatim response only).
- `docs/skill-evidence/run-ledger.md` — created; first row: *RED / baseline on dev set · 10 · 10 ·
  10 · no*.

**Method.** 5 skills × 1 `dev` scenario × 2 samples. Each run: a fresh foreground `general-purpose`
subagent on `sonnet` (C5), prompt = the arm A text for that skill (from
`docs/skill-evidence/arms/A/<skill>.md`, verified against `MANIFEST.md` first) + the scenario body.
Transcribe the response verbatim — **do not paraphrase**; §2.1 exception 1 makes the verbatim text
the only citable evidence. Ceiling 10; if a run has to be retried, a retry counts.

**Depends on.** Tasks 1 (arm A), 2 (rubric), 3 (scenarios).

**Verification.** `docs/skill-evidence/<skill>.md` exists for all five with a non-empty verbatim RED
section; 10 transcript files present; the ledger row written. Add
`cli/tests/skills_valid.rs::evidence_corpus_present` asserting the five `<skill>.md` files and
`run-ledger.md` exist and are non-empty — a cheap tripwire that later tasks don't delete them.
Report the run count actually spent.

---

### Task 7 — Fix 1: un-scope the five descriptions (§3) + freeze A′

**Objective.** Replace the five `description:` lines with §3's verbatim strings, demote the four
body phase-framings, record the defect in `docs/known-issues.md`, and snapshot arm A′.

**Interfaces introduced / changed.**
- `skills/tdd/SKILL.md:3`, `skills/systematic-debugging/SKILL.md:3`,
  `skills/verification-before-completion/SKILL.md:3`, `skills/code-review/SKILL.md:3`,
  `skills/using-drovr/SKILL.md:3` — **the five strings in §3's table, verbatim.** Do not improve
  them; RED may tighten wording later via the four-part closure, but these are the shipped defaults.
- Body demotions per §3's second table: `tdd:17-19`, `systematic-debugging:14-17`,
  `verification-before-completion:15-17,40-42`, `code-review:13-14,41`. **Rule for all five:** the
  skill reads as unconditional discipline; every drovr-phase reference becomes a clearly-marked
  *additional* consequence, never a precondition. `systematic-debugging`'s read-only-explorer rule is
  unconditional and **stays**.
- `docs/known-issues.md` — **one** entry in the file's convention
  (`## <symptom title> — FIXED <date>`, then `**Status:**`, `**Severity:** medium`, `**Found:**`,
  `### Symptom` / `### Root cause` / `### Fix`), recording that four descriptions scoped
  unconditional discipline to phases while `using-drovr:54` makes inline the default. **One entry
  only** — the other fixes are design work, not defects. Plus the §2.3 follow-up note: *render
  fenced `dot` blocks in the review UI and show the phase/gate graph alongside the plan* — as a
  follow-up note, not a defect entry.
- `docs/skill-evidence/arms/A-prime/<skill>.md` (5 files) + `MANIFEST.md` rows — snapshot taken
  **after** the description and demotion edits, **before** any fix-3 or fix-4 text exists. This is
  the only moment A′ exists on disk.

**Depends on.** Task 1 (gate C1), Task 6 (RED must be captured on pre-fix text).

**Verification.** New test `cli/tests/skills_valid.rs::no_phase_scoped_description_literals` (§9.1
check 3): for all 8 `skills/*/SKILL.md`, assert none contains `in a drovr phase`, `a drovr task`, or
`a drovr phase has produced`. Verified today: those three literals appear **only** on the four
`description:` lines, so this test goes green exactly when fix 1 lands and stays green through the
rewrites and through any A′ revert (fix 1 ships regardless — §7.3).
Plus `cargo test --test skills_valid` (frontmatter still valid, bodies still within the **old** 2200
budget — fix 1 touches frontmatter and demotes a few lines, so it must not blow the cap before
Task 9 raises it; if a demotion pushes a body over 2200, shorten the demotion, do not reorder Task
9). And `arm_a_prime_snapshots_match_manifest`, the sibling of Task 1's tripwire.

---

### Task 8 — Fix 3: bind checklists to tracked task state, non-discipline sites (§5)

**Objective.** Add §5's directive to the sites that are **not** rewritten by fix 4, so they are done
once and not churned.

**Interfaces introduced.** §5's block quote is the **canonical directive text** — later tasks quote
it, they do not rewrite it. It names `TodoWrite` **or** `TaskCreate`/`TaskUpdate` (harnesses differ
— §5's tool-agnostic treatment is correct as written; do not narrow it) and carries the file-based
fallback (`~/.local/share/drovr/runs/<run>/checklist.md` inside a run, `CHECKLIST.md` at the repo
root otherwise).

Sites (§5 items 3 and 4, plus the §8 scope note):
- `skills/pipeline/phase-prompts/{brainstorm,plan,implement-task,review,review-angle}.md` — the
  directive as **step 0** of each `## Do` list (`brainstorm.md:11`, `plan.md:12`,
  `implement-task.md:12`, `review.md:11`, `review-angle.md:20`). Pure text; these are
  agent-assembled markdown, not compiled into the binary.
- `skills/handoff/SKILL.md` and `skills/handoff/HANDOFF-template.md` — the 7-section handoff is a
  checklist and gets the same treatment.
- `skills/pipeline/SKILL.md` and `skills/worktrees/SKILL.md` — the directive **and nothing else**
  (§8: these three receive only fix 3).

**Explicitly deferred to fix 4.** §5 site 1 (`using-drovr`'s gate-function checklist branch) is part
of §4.1 → **Task 14**. §5 site 2 (each discipline skill's numbered procedure) is §6's section 6
("The procedure … preceded by the task-binding directive (fix 3)") → **Tasks 10–13**. Doing them
here would be overwritten, and — decisively — **A′ must be fix-1-only**, so no fix-3 text may touch
the four discipline skills before A′ is snapshotted.

**Depends on.** Task 7 (A′ frozen).

**Verification.** New test `cli/tests/skills_valid.rs::task_binding_directive_present`: assert a
stable marker phrase from §5's directive (e.g. `one tracked item per step`) appears in each of the 5
phase-prompt files, `handoff/SKILL.md`, `handoff/HANDOFF-template.md`, `pipeline/SKILL.md` and
`worktrees/SKILL.md`. Extend the list in Tasks 10–14 as each remaining site lands.
`cargo test --test skills_valid` and `cargo test -p drovr` stay green — no `skills/*/SKILL.md` body
that is size-checked is touched except `pipeline` and `worktrees`, which are not in `BODY_BUDGETS`.

---

### Task 9 — Per-skill byte budgets in `skills_valid.rs` (§2.4)

**Objective.** Replace the single global cap with a per-skill table, raise the methodology cap to
12000, and add `using-drovr` at 9000. **This is the plan's answer to the brainstorm handoff's first
open question.**

**Interfaces introduced.** In `cli/tests/skills_valid.rs`, **delete** `BODY_BUDGET` (`:17`) and
`METHODOLOGY_SKILLS` (`:20-25`) and introduce:
```rust
/// Per-skill body-size budget (bytes). A skill absent from this table is not
/// size-checked (handoff, pipeline, worktrees, writing-skills). §2.4 [tier 4].
const BODY_BUDGETS: &[(&str, usize)] = &[
    ("tdd", 12_000),
    ("systematic-debugging", 12_000),
    ("verification-before-completion", 12_000),
    ("code-review", 12_000),
    ("using-drovr", 9_000),
];

/// The budget for `skill`, or `None` when it is not size-checked.
fn budget_for(skill: &str) -> Option<usize>;
```
Rename the test `methodology_skills_within_body_budget` (`:156-183`) →
`checked_skills_within_body_budget`, iterating `BODY_BUDGETS`. Update the module doc comment
(`:1-11`), which currently states the 2200 figure and that `using-drovr` is *not* checked.

**Test-first.** Write `budget_for_returns_per_skill_caps` before the change: asserts
`budget_for("tdd") == Some(12_000)`, `budget_for("using-drovr") == Some(9_000)`,
`budget_for("handoff") == None`. It fails to compile against the current file — that is the RED.

**Why here.** Must precede Tasks 10–14, whose rewrites are 3–4× the old cap. Raising a ceiling that
nothing currently exceeds is safe: today's bodies are 1717–2362 B and `using-drovr` is 5087 B, all
inside the new caps.

**Depends on.** Task 8 (ordering only, to keep the doc-edit tasks contiguous). No interface
dependency.

**Verification.** `cargo test --test skills_valid` — all tests pass, including the new
`budget_for_returns_per_skill_caps`. Report each checked skill's measured body size against its cap.

---

### Tasks 10–13 — Fix 4: the armor, one task per discipline skill (§6)

**Four tasks, identical in shape**, in this order: **10 `tdd` · 11 `systematic-debugging` ·
12 `verification-before-completion` · 13 `code-review`**. One skill per fresh context; each is a
120–180-line rewrite driven by that skill's RED transcripts.

**Objective (each).** Restructure `skills/<skill>/SKILL.md` to §6's fixed section order, with the
counter-text written against the **verbatim** RED rationalizations from
`docs/skill-evidence/<skill>.md` — and **only** the `dev`-scenario RED. The two `holdout` scenarios
must not be read while authoring; reading them makes the pass bar unfailable (§7.3).

**Section order — §6's block, REQUIRED for all four:** 1 `description:` (already set by Task 7 —
do not re-edit unless the four-part closure requires it) · 2 Overview (core principle +
spirit-vs-letter line + the WHY, ≤6 lines) · 3 Unity line (*the next phase agent is you, with your
context gone*) · 4 The Iron Law (one fenced all-caps line + "No exceptions:" bullets, **each bullet
pairing the prohibition with the required action** — §2.2's "say what to do") · 5 Announce · 6 The
procedure, numbered, **preceded by fix 3's task-binding directive** (§5's canonical text from Task
8) · 8 Red flags — STOP (inner-monologue fragments, closed by a catch-all bullet; this **expands**
what already exists at `tdd:30`, `systematic-debugging:33`, `verification-before-completion:29`) ·
9 Rationalizations (*thought → reality*, **the reality column is an INSTRUCTION, not a rebuttal**) ·
10 Worked example (**one** ✅/❌ pair of actual utterances, not five) · 11 Cross-refs (bare skill
names with REQUIRED markers; **never @-links** — they force-load).

**CONDITIONAL sections:** section 7 (Requirements: claim → required evidence → **not sufficient**)
**only** in Tasks 12 and 13. Section 6b (the cycle as a fenced `dot` block) **only** in Tasks 10 and
11 — §2.3's placement rule; superpowers puts one on exactly the TDD cycle
(`test-driven-development/SKILL.md:49`), so withholding it would be an unmarked deviation.

**Announcement sentences — §6 verbatim, do not reword:**
- `tdd` — *"Using drovr:tdd — writing the failing test before the implementation."*
- `systematic-debugging` — *"Using drovr:systematic-debugging — reproducing before fixing."*
- `verification-before-completion` — *"Using drovr:verification-before-completion — running the
  checks before claiming done."*
- `code-review` — *"Using drovr:code-review — dispatching read-only reviewers before calling this
  done."*

**Per-skill specifics (§6):**
- **10 `tdd`** — Iron Law: no implementation code before a test you have watched fail. Loophole
  closures: *"I'll keep the code as reference"*, *"the test is obvious so I'll write it after"*,
  *"it's a refactor so TDD doesn't apply"*, *"the harness makes it hard to run one test"*.
- **11 `systematic-debugging`** — Iron Law: no fix before a reproduction and a mechanistic cause.
  Adds the **numeric escalation trigger**: after 3 failed fixes, stop and question the design; do
  not attempt fix #4 without that conversation.
- **12 `verification-before-completion`** — Iron Law: no completion claim without fresh evidence
  produced in this message. Requirements table covers tests / build / linter / bug-fixed /
  subagent-reported-success. Catch-all red flag for any wording implying success.
- **13 `code-review`** — Iron Law: no change is done until a read-only reviewer has looked at it and
  every Critical/Important finding is resolved or explicitly recorded as deferred. Loophole
  closures: *"the change is too small"*, *"I already reviewed it myself"*, *"the pipeline's review
  phase will catch it"*. The existing FOREGROUND rule (`code-review/SKILL.md:19-23`) is **promoted
  into the no-exceptions list** — backgrounding a reviewer is the known way this skill silently
  fails.

**Register.** Author in superpowers' full register (authority + moral framing) with the unity line,
per the §7.4 outcome-3 default. Task 22 applies the probe's actual outcome; do not pre-judge it.

**Interfaces introduced (each).**
- `docs/skill-evidence/arms/B/<skill>.md` + `MANIFEST.md` row — snapshot of the finished rewrite,
  taken as the task's last edit. This is the text the `ab-<skill>` phase measures.
- **Task 10 only** — the §9.1 structure check, in `cli/tests/skills_valid.rs`:
  ```rust
  /// Skills that carry §6's armor. Each fix-4 task APPENDS its skill; Task 22
  /// REMOVES any skill whose arm B failed and reverted to A′.
  const ARMORED_SKILLS: &[&str] = &["tdd"];
  const REQUIREMENTS_TABLE_SKILLS: &[&str] = &[];        // grows in Tasks 12, 13
  const CYCLE_FLOWCHART_SKILLS: &[&str] = &["tdd"];      // grows in Task 11
  ```
  `armored_skills_have_required_sections` asserts, for each `ARMORED_SKILLS` entry, the presence of
  §6's REQUIRED sections (match on stable heading text, not line numbers), plus section 7 only for
  `REQUIREMENTS_TABLE_SKILLS` and a fenced `dot` block only for `CYCLE_FLOWCHART_SKILLS`. §9.1 is
  explicit that this is **not** "all 11 on every skill" — that is unsatisfiable by construction.
  **Tasks 11–13 each append their skill to the relevant consts in the same commit as the rewrite**,
  so no task ever leaves a red test behind (a failed task halts the pipeline loop).

**Depends on.** Task 6 (RED wording), Task 7 (fix 1 + A′ frozen), Task 8 (fix 3's canonical text),
Task 9 (the 12000 cap). Task 11 depends on 10 (the structure-test consts).

**Verification (each).** `cargo test --test skills_valid` — frontmatter, the body within 12000 B, the
structure check for this skill, `no_phase_scoped_description_literals`, and
`task_binding_directive_present` extended to this skill. Plus, reported manually:
- every fenced `dot` block balanced;
- **verbatim-overlap check (§9.1 check 4)** against
  `/home/sauyon/.claude/plugins/cache/claude-plugins-official/superpowers/5.1.0/skills/` — no ≥8-word
  shingle shared with this `SKILL.md`. Implement as
  `skills_valid.rs::no_verbatim_overlap_with_superpowers`, which **returns early with an eprintln
  when the corpus directory is absent** (it is an installed plugin outside the repo; CI must not go
  red on its absence). Any hit is reworded, or the MIT attribution required by §2.1 exception 2 is
  added.
- byte size reported against the 12000 cap.

---

### Task 14 — Fix 2's doc layer: `using-drovr` as a per-turn gate (§4.1)

**Objective.** Add §4.1's six blocks to the router in the stated order, place them correctly
relative to the reflex markers, and freeze arm B for `using-drovr`.

**Interfaces changed — `skills/using-drovr/SKILL.md`.** New content in §4.1's order:
1. `<SUBAGENT-STOP>` — **keep as-is** (`:8-11`).
2. **The 1% rule** `[tier 3]`, above the H1, inside the existing `<EXTREMELY_IMPORTANT>` framing:
   even a 1% chance a drovr skill applies means invoke it — paired with the cost-lowering clause (if
   it turns out not to fit, drop it; invoking costs almost nothing).
3. **The per-turn rule**: check before any response, **including before asking a clarifying question
   and before any read-only exploration.**
4. **Instruction-priority ladder** `[tier 3]`: the human's explicit instructions > drovr skills >
   default behaviour. The safety counterweight to the MUST language; **not optional**.
5. **Gate function** — a fenced `dot` flowchart (it branches, so §2.3's placement rule applies):
   message received → does any skill apply (≥1%) → invoke → announce → does it have a checklist →
   create one tracked item per step → follow it → only then respond. **This branch is §5 site 1** —
   quote fix 3's canonical directive here.
6. **Red-flag table for the router's own failure mode** — invoking nothing at all. §4.1's candidate
   rows are candidates only; **the final wording comes from `using-drovr`'s RED transcripts in
   `docs/skill-evidence/using-drovr.md`, not from §4.1's list.**
7. Existing sections (`single-writer`, `always-review`, `methodology`, `escalation`) retained
   unchanged apart from fix 1's `description:` (already landed).

**MARKER BOUNDARY — the highest-risk edit in this task.** Items 2–5 **sit outside every
`<!-- reflex:section:NAME -->` marker** `[tier 4]`. `[reflex.sections]` may subtract advisory
sections but must not be able to delete the routing core; only `[reflex] enabled = false` removes
it. Getting this wrong yields an agent that believes it is running drovr and is not. Existing
markers are at `:26-30` (`single-writer`), `:32-37` (`always-review`), `:39-49` (`methodology`),
`:51-93` (`escalation`); items 2–5 go **above `:26`**. `reflex.rs`'s
`validate_markers` + `shipped_skill_markers_are_well_formed` (`:281-296`) already pin balance — do
not introduce a new marker pair.

**Also in this task:**
- Extend Task 4's `GATE_CARD_PHRASES` to the 1%-rule and per-turn phrases now that they exist in the
  router, so `gate_card_phrases_present_in_router_skill` becomes a real drift guard (§4.2, §9.2).
- Extend `routing_core_survives_section_subtraction` to assert the 1% rule, the per-turn rule, the
  priority ladder and the gate flowchart all survive with every section set to `false` (§9.2).
- Extend `task_binding_directive_present` to `using-drovr/SKILL.md`.
- `docs/skill-evidence/arms/B/using-drovr.md` + `MANIFEST.md` row.

**Depends on.** Tasks 4 (the phrase consts), 6 (RED wording for the red-flag table), 7 (fix 1),
8 (fix 3's canonical text), 9 (the 9000 cap — `using-drovr` is 5087 B today and §4.1 adds six
blocks).

**Verification.** `cargo test -p drovr` and `cargo test --test skills_valid`, specifically:
`shipped_skill_markers_are_well_formed`, `routing_core_survives_section_subtraction` (extended),
`gate_card_phrases_present_in_router_skill` (extended), `checked_skills_within_body_budget`
(`using-drovr` ≤ 9000 — report the measured size; **if it exceeds 9000, split into
`skills/using-drovr/references/`, do not raise the cap**), `no_phase_scoped_description_literals`.
Plus, reported: `drovr reflex --skill skills/using-drovr/SKILL.md` piped through a JSON parser
renders items 2–5 with every `[reflex.sections]` entry set to `false`.

---

### Task 15 — Voice probe variants V0–V3 (§7.4)

**Objective.** Author the four register variants of the probe skill, **identical in structure**,
each adding exactly **one** register device to the baseline.

**Interfaces introduced.** `docs/skill-evidence/arms/voice/V{0,1,2,3}.md` + `MANIFEST.md` rows.
Derived from `docs/skill-evidence/arms/B/verification-before-completion.md`:
- **V0** — baseline: plain imperative; **no** all-caps, **no** absolutist "no exceptions";
  operational consequence only.
- **V1** — V0 + the **unity** line.
- **V2** — V0 + the **full authority** register (`MUST`/`NEVER`, all-caps Iron Law, no-exceptions
  bullets).
- **V3** — V0 + **moral** framing, in superpowers' register.

**Same Iron Law, same procedure, same tables, same placement across all four** — otherwise the
factors are not separable at n=6. Diff V1/V2/V3 against V0 and confirm the only differences are the
one device; record those diffs in the task report.

**Depends on.** Task 12 (arm B for `verification-before-completion`).

**Verification.** `diff` V0 against each variant shows exactly one register device changed and no
structural difference (section set and order identical). Report the four diffs. `docs/skill-evidence/
voice.md` is **created here** with the pre-registered decision rule copied from §7.4 **before any
run happens** — pre-registration is worthless written afterwards: the four outcomes, the **≥3 of 6
separation margin**, unity's *reported, not decisive* standing, and the escalate-to-human branch if
V1 loses to V0 by ≥3.

---

### Tasks 16–20 — `ab-<skill>` measurement phases (§7.3)

**Five tasks, identical in shape**, run **strictly sequentially** (C4), in this order:
**16 `ab-tdd` · 17 `ab-systematic-debugging` · 18 `ab-verification-before-completion` ·
19 `ab-code-review` · 20 `ab-using-drovr`**.

**Driver note.** §7.3 asks for one dedicated phase per skill named `ab-<skill>`. `drovr phase start`
appends any unseen phase name, so the driver may start these as
`drovr phase start skill-stickiness ab-<skill>` rather than `implement-task-<N>`; the handoff then
becomes `ab-<skill>-HANDOFF.md` and `drovr phase done skill-stickiness ab-<skill>` is the marker.
Either naming satisfies §7.3 — the requirement is one **fresh phase per skill**, which the standard
per-task phase already gives. Pick one and use it consistently; the code-review base is
`drovr code-review base skill-stickiness task-<N>` either way.

**Objective (each).** Run the held-out arms for one skill, score them blind, apply up to 2 REFACTOR
iterations if arm B fails, and record everything.

**Runs (per §7.3's budget table; ~16–24 per phase, none over 25):**
| stage | scope | runs |
|---|---|---|
| Arm A, held-out | 2 held-out × 2 samples | 4 |
| Arm A′, held-out | 2 held-out × 2 samples (**Tasks 16–19 only**) | 4 |
| Arm B, held-out | 2 held-out × 2 samples | 4 |
| REFACTOR re-tests | only if B fails; ≤2 iterations × 2 held-out | ≤4 |
| no-skill-applies class | **Task 20 only** — 2 scenarios × 3 arms (A, A′, B) × 2 samples | 12 |

Task 20 (`using-drovr`) does get an A′ arm, but only inside the no-skill-applies class — §7.3's
"Arm A′ on held-out" row is scoped to the 4 discipline skills. Task 20's total is 4 + 4 + 12 (+ ≤4)
= 20–24.

**Method (each).**
1. **Verify the arms** — `git hash-object` each of `arms/A/<skill>.md`, `arms/A-prime/<skill>.md`,
   `arms/B/<skill>.md` against `MANIFEST.md`. A mismatch is a **halt**, not a warning: the
   measurement is void.
2. **Read the ledger** (`docs/skill-evidence/run-ledger.md`) so your rows continue its running
   total. There is no run ceiling to halt on (C3 — lifted).
3. **Run** each arm × held-out scenario × 2 samples: fresh foreground `general-purpose` subagent on
   `sonnet` (C5), prompt = the arm's text + the scenario body. Write each transcript to
   `docs/skill-evidence/transcripts/<skill>/<id>.md` per §1.3 — **scenario body + verbatim response
   only, arm text excluded** — and record the mapping in `blind-map.json`.
4. **Score blind** — shuffle the id list, hand the scorer subagent (read-only,
   `general-purpose`, `sonnet`, foreground) **only** the transcript files and
   `skills/writing-skills/references/scoring-rubric.md`. Collect verdicts into `scores.json`. Join
   to `blind-map.json` **only after every score is recorded**.
5. **Apply the pre-registered bars (§7.3) — do not invent new ones:**
   - *Arm A bar*: the skill "already passes" if A is compliant on **≥3 of its 4** held-out runs.
     If so, **the rewrite is not justified and this skill reverts to A′** — the guard against
     length-for-its-own-sake.
   - *Arm B bar*: compliant on **≥3 of its 4** held-out runs **AND** strictly more compliant runs
     than **both** A and A′.
   - *A′ ≈ B*: the armor is not carrying its weight; revert to A′ **even if B passes its own bar**.
6. **REFACTOR if B fails** — ≤2 iterations. Each: apply §7.1's **four-part closure** to every new
   rationalization (all four parts, never one), write the result to `skills/<skill>/SKILL.md`,
   snapshot `arms/B-r<i>/<skill>.md`, re-run the 2 held-out scenarios. After the ceiling, if B still
   fails: **revert `skills/<skill>/SKILL.md` to `arms/A-prime/<skill>.md`** and record the failure
   and the reverted state in `docs/skill-evidence/<skill>.md`. Fix 1 ships regardless; **fix 4 must
   earn its bytes.**
7. **Record** — extend `docs/skill-evidence/<skill>.md` with the scored table (arm · scenario ·
   sample · compliant · cites_section · names_temptation · meta_test_clear), the §1.3 blinding
   limitation verbatim, any new verbatim rationalizations, and the ship/revert decision. Append the
   ledger rows **last**.

**Depends on.** Task 15 (ordering: `ab-voice`'s variants derive from an unmodified arm B, so author
them before any REFACTOR mutates a skill) and the arm-B task for this skill (10, 11, 12, 13, 14
respectively). Task N depends on Task N−1 for the ledger (C4).

**Verification (each).** `cargo test -p drovr` and `cargo test --test skills_valid` green **after**
any REFACTOR or revert — a revert to A′ must also remove the skill from `ARMORED_SKILLS` (and from
`REQUIREMENTS_TABLE_SKILLS` / `CYCLE_FLOWCHART_SKILLS`) **in the same commit**, or
`armored_skills_have_required_sections` goes red. Plus: `blind-map.json` and `scores.json` exist and
have one entry per transcript; the ledger's cumulative column is a correct running total;
`docs/skill-evidence/<skill>.md`
states the decision against the pre-registered bar with the run counts that produced it.

---

### Task 21 — `ab-voice`: the voice probe (§7.4, 24 runs)

**Objective.** Run V0–V3 and apply §7.4's pre-registered decision rule.

**Method.** 4 variants × 2 scenarios × 3 samples = **24 runs**, 6 per variant. Scenarios are
`verification-before-completion-2.md` and `-3.md` (the held-out pair, reused per §1.2). Same
transcript/blinding/scoring machinery as Tasks 16–20, under
`docs/skill-evidence/transcripts/voice/`. Verify V0–V3 against `MANIFEST.md` first; read the ledger
first.

**Decision rule — §7.4, copied into `voice.md` by Task 15 before any run:**
1. Variant beats V0 by **≥3 of 6** → that device ships across all five documents.
2. V0 beats the variant by **≥3** → that device is **dropped** and the baseline register ships. This
   branch is explicit: a plain-register win is a real outcome, not an undefined one.
3. **No separation ≥3 (the likeliest outcome)** → **tier 3: follow superpowers.** Authority ships;
   moral framing ships; and unity ships anyway on the 2026 published ranking (§2.2 tier 1 outranks
   tier 3). Each recorded in `voice.md` as convention-or-prior, **with the null attached.**
4. **If V1 (unity) loses to V0 by ≥3** → do **not** auto-resolve. Record a genuine conflict between
   the published prior and drovr's own data and **escalate to the human.** Unity's arm is *reported,
   not decisive* — 6 runs cannot overturn an N=126,000 result.

**Power note, to be recorded verbatim:** n=6 per variant detects only large effects and §2.2 expects
register effects to be small on frontier models, so outcome 3 is the single likeliest result. That
is informative, not a failure. **Nothing stronger than "suggestive" may be recorded.** Also record
the two stated limitations: measured on one skill and one model (`sonnet`, not the Opus-class model
drovr sessions run on), generalised to five documents without re-testing.

**Depends on.** Task 15 (the variants), Task 20 (the ledger — C4).

**Interfaces changed.** `docs/skill-evidence/voice.md` (results appended to Task 15's
pre-registration), `docs/skill-evidence/transcripts/voice/`, `run-ledger.md`.

**Verification.** 24 transcripts, `blind-map.json`/`scores.json` complete, ledger rows appended,
`voice.md` states which of the four outcomes fired per variant with the 6-run counts. **This task
writes no `skills/` file** — outcome application is Task 22's job, so a surprising result does not
get quietly folded into a rewrite by the agent that measured it.

---

### Task 22 — Consequences, register application, and final verification

**Objective.** Apply the measured outcomes and prove §9 end to end. This is the only task allowed to
change register across the corpus.

**Work.**
1. **Apply the voice outcome** to all five documents per `voice.md`'s recorded decision — including
   dropping a device if outcome 2 fired. §7.4 item 4: this applies **without re-testing each
   document**; record that as the stated limitation it is. If Task 21 escalated (unity lost by ≥3),
   **STOP and hand the decision to the human** — do not choose.
   The `arms/` snapshots are frozen measurement artifacts: **edit `skills/…/SKILL.md` only**, and
   note in `voice.md` that the shipped text now differs from the measured `arms/B*` text by exactly
   this register change.
2. **Reconcile every revert.** For each skill Tasks 16–20 reverted to A′, confirm
   `skills/<skill>/SKILL.md` matches `arms/A-prime/<skill>.md` (modulo step 1's register change,
   which does not apply to a reverted skill — a reverted skill has no armor to re-register), and that
   it has been removed from `ARMORED_SKILLS` / `REQUIREMENTS_TABLE_SKILLS` /
   `CYCLE_FLOWCHART_SKILLS`.
3. **Aggregate** into `docs/skill-evidence/` a short index: per skill, the arm counts, the decision,
   and the shipped state. **Nulls and negative results recorded alongside positive ones** (§7.3).
4. **Run §9 in full and record the output**:
   - §9.1 — `cargo test --test skills_valid` (structure check, per-skill budgets, the literal check,
     the task-binding check, the scenario check, the arm-snapshot tripwires) and
     `no_verbatim_overlap_with_superpowers` with the corpus **present** (this task must run it for
     real, not on the skip path — report that it ran).
   - §9.2 — `cargo test -p drovr` (the ≤600 B card, the `per_turn` default with a `[reflex]` table
     present and the key absent, the `UserPromptSubmit` JSON + `enabled = false`, the routing core
     surviving section subtraction, the card-phrase drift guard) and
     `cargo test --test reflex_hook`.
   - §9.3 — the ledger's final cumulative total (**recorded, not capped** — C3's ceiling was
     lifted, so report the number; do not test it against a limit) and every
     `docs/skill-evidence/` file committed.
   - §9.4 — **the integration check.** A clean session, given a one-line bugfix request with **no
     mention of drovr**, invokes `drovr:tdd` before writing code. Run against this worktree with
     `CLAUDE_PLUGIN_ROOT=<worktree>` so it does not depend on the flake pin being bumped
     (§8 "Deployment reality"). Record the prompt used and the verbatim first two tool calls.
5. **`cargo clippy --all-targets -- -D warnings`** and `cargo fmt --check`.

**Depends on.** Tasks 16–21 (all measurement complete).

**Verification.** Every §9 item above, with output pasted into `task22-report.md`. A red §9.4 is a
real failure of this run's thesis, not a test to adjust — report it plainly.

---

## 3. Dependency graph

```
1 (arm A freeze — HARD GATE, blocks every skills/ edit)
├─ 2 → 3 ────────────────────────────┐
├─ 4 → 5   (fix 2 hooks; independent)│
└─────────────────────────────────── 6 (RED, 10 runs)
                                      └─ 7 (fix 1 + A′) → 8 (fix 3) → 9 (budgets)
                                          └─ 10 tdd → 11 sys-debug → 12 vbc → 13 code-review
                                              14 using-drovr §4.1  (needs 4, 6, 7, 8, 9)
                                              └─ 15 (V0–V3; needs 12)
                                                  └─ 16 → 17 → 18 → 19 → 20 → 21   (sequential, C4)
                                                      └─ 22 (consequences + §9)
```

Tasks 4 and 5 have no dependency on 2/3/6+; the driver may run them at any point after Task 1. They
are placed early so §4.2's subagent-firing question is answered before any probe runs.

## 4. Run-count accounting (§7.3 — original estimate; the ceiling has since been lifted, C3)

| task | stage | runs |
|---|---|---|
| 6 | RED / baseline on dev set | 10 |
| 16–20 | Arm A held-out (5 × 4) | 20 |
| 16–19 | Arm A′ held-out (4 × 4) | 16 |
| 16–20 | Arm B held-out (5 × 4) | 20 |
| 16–20 | REFACTOR re-tests (failing skills only) | ≤20 |
| 20 | `using-drovr` no-skill-applies (2 × 3 arms × 2) | 12 |
| 21 | Voice probe (4 × 2 × 3) | 24 |
| | **total (original estimate)** | **122** |

That total was the plan's original forecast, not a budget to enforce. The human lifted the ceiling
(C3); actual spend is whatever the ledger records, and the ledger is the authority.

## 5. Two decisions this plan made (per the brainstorm handoff's open questions)

1. **Per-skill byte budget** → `const BODY_BUDGETS: &[(&str, usize)]` + `budget_for()` replacing the
   global `BODY_BUDGET`/`METHODOLOGY_SKILLS` pair (Task 9). A table, not five constants: it keeps
   "which skills are checked" and "at what cap" in one place, and makes `None` (unchecked) explicit
   for `handoff`/`pipeline`/`worktrees`/`writing-skills`.
2. **`ab-<skill>` phases run sequentially, not interleaved** (C4) — single-writer, a global ledger
   the next phase must read, and no wall-clock win from interleaving foreground runs.
