# Key-point ledger — `skill-stickiness`

Derived from `../fixtures/skill-stickiness.spec.md` and **nothing else**. No prompt text — no arm,
no phase prompt, no plan — was read to decide a row. See `../FREEZE.md` for why that is the whole
point, and for this file's hash.

A row is **load-bearing**: an implementer holding a candidate spec that omitted it would build
something materially different, or would have to stop and ask. Rationale, motivation, history, and
examples illustrating a point already stated are not rows. `kind` is one of `decision`,
`interface`, `constraint`, `scope`. Ids are stable forever — a later task may not renumber them.

**Closed list: 91 rows.**

| id | kind | item |
|---|---|---|
| skill-stickiness-01 | constraint | Arm A (the pre-fix `SKILL.md` baseline) must be captured at the pre-fix `HEAD` before any skill file is edited, because it is unrecoverable once fix 1 lands. |
| skill-stickiness-02 | decision | Authoring decisions follow a four-tier precedence — published evidence, else measure it if reachable by the probe, else follow superpowers as a stated convention-follow, else engineering judgement labelled `[tier 4]` at each site. |
| skill-stickiness-03 | constraint | No fabricated measurements: no number, duration, frequency, or comparative may be stated as measured unless drovr measured it or a citation supports it. |
| skill-stickiness-04 | constraint | No text is copied from superpowers — mechanisms are ported but every sentence is drovr's own, and any surviving verbatim line requires the MIT notice and credit. |
| skill-stickiness-05 | decision | Discipline skills carry a unity line ("the next phase agent is you, with your context gone"), deviating from superpowers' matrix which withholds unity from discipline skills. |
| skill-stickiness-06 | constraint | Every prohibition in the shipped skills is paired with the required action in the same breath. |
| skill-stickiness-07 | constraint | The rationalization table's second column is written as an instruction ("Confidence is not evidence; run the command"), not as a rebuttal. |
| skill-stickiness-08 | constraint | Flowcharts are used only for genuine branching and for loops where the agent might stop too early — never for linear instructions or reference material. |
| skill-stickiness-09 | interface | Flowcharts are authored as fenced `dot` blocks inside the skill documents so raw source stays legible unrendered. |
| skill-stickiness-10 | scope | Making the review UI render `dot` blocks (and pairing a graph with the plan as a progress display) is out of scope and is recorded as a follow-up note in `docs/known-issues.md`. |
| skill-stickiness-11 | constraint | The byte budget applies to teaching content only; binding content is deliberately restated 3–4 times per skill (Iron Law, red flag, rationalization row, checklist item). |
| skill-stickiness-12 | interface | `BODY_BUDGET` in `cli/tests/skills_valid.rs` rises from 2200 to 12000 bytes for the four methodology skills. |
| skill-stickiness-13 | interface | `using-drovr` joins the checked skill list in `skills_valid.rs` with its own 9000-byte cap. |
| skill-stickiness-14 | constraint | The `skills_valid.rs` byte budget is the single authoritative size check; `wc -l` and `wc -w` targets are author guidance and are never asserted. |
| skill-stickiness-15 | constraint | Content that exceeds a skill's cap is split into `references/` rather than trimmed from the skill's binding content. |
| skill-stickiness-16 | decision | The four discipline skills' `description:` lines drop all drovr-phase scoping so they read as unconditional triggers. |
| skill-stickiness-17 | constraint | A skill `description:` must be a trigger, not a summary of the workflow. |
| skill-stickiness-18 | interface | `skills/using-drovr/SKILL.md:3`'s description becomes a trigger firing at the start of every session and before every response, including before clarifying questions and before reading any file. |
| skill-stickiness-19 | constraint | In the rewritten skill bodies every drovr-phase reference becomes a clearly-marked additional consequence, never a precondition. |
| skill-stickiness-20 | interface | Exactly one `docs/known-issues.md` entry is added, in the file's existing convention, recording that four descriptions scoped unconditional discipline to phases. |
| skill-stickiness-21 | decision | `using-drovr` carries the 1% rule — even a 1% chance a drovr skill applies means invoke it — paired with a cost-lowering clause that dropping it later is cheap. |
| skill-stickiness-22 | constraint | The router's per-turn rule requires a skill check before any response, including before asking a clarifying question and before any read-only exploration. |
| skill-stickiness-23 | decision | `using-drovr` states an instruction-priority ladder: the human's explicit instructions outrank drovr skills, which outrank default behaviour. |
| skill-stickiness-24 | interface | A fenced `dot` gate-function flowchart in `using-drovr` runs message received to skill applies at 1 percent to invoke to announce to checklist to one tracked item per step to follow it to respond. |
| skill-stickiness-25 | decision | `using-drovr` gains a red-flag table for the router's own failure mode of invoking nothing at all, with final wording taken from RED rather than the spec's candidate rows. |
| skill-stickiness-26 | constraint | The routing core (1% rule, per-turn rule, priority ladder, gate function) sits outside every `<!-- reflex:section:NAME -->` marker so `[reflex.sections]` cannot subtract it and only `[reflex] enabled = false` removes it. |
| skill-stickiness-27 | interface | `hooks/hooks.json` gains a `UserPromptSubmit` entry running a thin `hooks/user-prompt` script that execs `drovr reflex --gate`. |
| skill-stickiness-28 | constraint | The `UserPromptSubmit` entry takes no `matcher`, unlike the existing `SessionStart` entry's `startup\|clear\|compact`. |
| skill-stickiness-29 | constraint | Rendered `--gate` `additionalContext` must be at most 600 bytes, asserted as a byte length in `cli/src/reflex.rs`'s test module. |
| skill-stickiness-30 | constraint | The gate card is emitted only when no `drovr:*` skill was invoked in the previous turn. |
| skill-stickiness-31 | interface | The card contains the 1% rule, the per-turn check, the announcement string, the checklist-binding line, a `<SUBAGENT-STOP>` line, and a pointer to `Skill drovr:using-drovr`. |
| skill-stickiness-32 | interface | `reflex.rs`'s `envelope()` is parameterized to take the hook event name instead of hardcoding `"hookEventName": "SessionStart"`. |
| skill-stickiness-33 | interface | `Reflex { skill: PathBuf }` becomes `Option<PathBuf>` with a clap arg-group rule making `--gate` and `--skill` mutually exclusive. |
| skill-stickiness-34 | decision | The card's text lives as a `const` in `reflex.rs` rather than being extracted from `SKILL.md`. |
| skill-stickiness-35 | constraint | A drift-guard test asserts the card's key phrases still appear in `using-drovr/SKILL.md`. |
| skill-stickiness-36 | constraint | Whether `UserPromptSubmit` fires for Agent-tool subagents is verified empirically and the answer recorded in `docs/skill-evidence/per-turn-gate.md`. |
| skill-stickiness-37 | interface | A new `[reflex] per_turn` bool in `ReflexConfig` defaults to true via a named default fn rather than bare `#[serde(default)]`. |
| skill-stickiness-38 | constraint | Suppression is asymmetric: the `SessionStart` reflex stays suppressed inside phases while the per-turn gate does not suppress inside phases. |
| skill-stickiness-39 | scope | `PreToolUse` hard-deny enforcement is explicitly out of scope for this run. |
| skill-stickiness-40 | interface | The task-binding directive requires creating one tracked item per numbered checklist step, using whatever task tool the harness exposes (`TodoWrite` or `TaskCreate`/`TaskUpdate`), before starting step 1. |
| skill-stickiness-41 | interface | When no task tool exists the checklist is written to `~/.local/share/drovr/runs/<run>/checklist.md` inside a run, or `CHECKLIST.md` at the repo root otherwise. |
| skill-stickiness-42 | scope | The task-binding directive is applied at four sites: `using-drovr`'s gate branch, each discipline skill's numbered procedure, the `skills/pipeline/phase-prompts/*.md` `## Do` lists as step 0, and `handoff`'s SKILL.md plus HANDOFF-template.md. |
| skill-stickiness-43 | decision | The four discipline skills are restructured to a fixed section order: description, overview with spirit-vs-letter line, unity line, Iron Law with no-exceptions bullets, announcement sentence, numbered procedure, red flags, rationalizations, worked example, cross-refs. |
| skill-stickiness-44 | constraint | The claim to required evidence to not-sufficient Requirements table appears only in `verification-before-completion` and `code-review`. |
| skill-stickiness-45 | constraint | The cycle flowchart section appears only in `tdd` and `systematic-debugging`. |
| skill-stickiness-46 | interface | Announcement strings follow the template `Using drovr:<skill> to <purpose>.` with four shipped sentences, one per discipline skill. |
| skill-stickiness-47 | constraint | Cross-references use bare skill names with REQUIRED markers and never @-links, because @-links force-load. |
| skill-stickiness-48 | decision | `tdd`'s Iron Law: no implementation code before a test you have watched fail. |
| skill-stickiness-49 | decision | `systematic-debugging`'s Iron Law: no fix before a reproduction and a mechanistic root cause. |
| skill-stickiness-50 | constraint | `systematic-debugging` adds a numeric escalation trigger — after 3 failed fixes, stop and question the design; do not attempt fix #4 without that conversation. |
| skill-stickiness-51 | decision | `verification-before-completion`'s Iron Law: no completion claim without fresh evidence produced in this message. |
| skill-stickiness-52 | decision | `code-review`'s Iron Law: no change is done until a read-only reviewer has looked at it and every Critical and Important finding is resolved or explicitly recorded as deferred. |
| skill-stickiness-53 | constraint | `code-review`'s existing FOREGROUND reviewer rule is promoted into its no-exceptions list. |
| skill-stickiness-54 | interface | A new `skills/writing-skills/SKILL.md` applies TDD to skill documents, mapping pressure scenario to test case, `SKILL.md` to production code, RED to violation without the skill, GREEN to compliance with it, and REFACTOR to loophole closure. |
| skill-stickiness-55 | constraint | The writing-skills loop repeats until a run produces no new rationalization or the §7.3 REFACTOR ceiling is reached, whichever comes first. |
| skill-stickiness-56 | constraint | Baseline rationalizations are transcribed verbatim. |
| skill-stickiness-57 | constraint | Every newly-observed rationalization gets all four parts of the closure: explicit negation inside the rule, a rationalization-table row, a red-flags bullet, and a `description:` update naming the symptom of being about to violate. |
| skill-stickiness-58 | constraint | Scenarios must use real file paths, concrete numbers and deadlines, a forced A/B/C choice, ask "what do you do" not "what should you do", offer no escape hatch to asking the human, and combine at least 3 pressure types. |
| skill-stickiness-59 | constraint | A skill passes only if all four criteria hold: the correct option under maximum pressure, the agent cites a specific section, it names the temptation and complies anyway, and the meta-test returns "it was clear". |
| skill-stickiness-60 | constraint | Scenario subagents run in the FOREGROUND. |
| skill-stickiness-61 | interface | Scenario prompts are checked in at `skills/writing-skills/scenarios/<skill>-<n>.md`, each tagged `dev` or `holdout`. |
| skill-stickiness-62 | interface | `docs/skill-evidence/<skill>.md` records, per skill, the scenarios used, verbatim baseline rationalizations, the counter-text written for each, and the re-test result with dates. |
| skill-stickiness-63 | interface | `docs/skill-evidence/` also holds `voice.md` for the probe, `per-turn-gate.md` for the unmeasured-bet record, and raw transcripts under `transcripts/`. |
| skill-stickiness-64 | decision | Each skill gets 3 scenarios — 1 development scenario used for RED and all counter-text authoring, and 2 held-out scenarios never read while writing arm B and used only for scoring. |
| skill-stickiness-65 | decision | The acceptance test runs three arms: A (pre-fix text), A′ (fix-1-only, descriptions un-scoped with no armor), and B (full rewrite), so fix 1 and fix 4 are separable. |
| skill-stickiness-66 | interface | The run budget is a per-stage ceiling table totalling roughly 122 runs, which is a hard ceiling. |
| skill-stickiness-67 | constraint | When any stage hits its ceiling, work halts and records a null in `docs/skill-evidence/` rather than silently extending. |
| skill-stickiness-68 | interface | `using-drovr` gets an extra no-skill-applies scenario class of 2 scenarios × 3 arms × 2 samples = 12 budgeted runs. |
| skill-stickiness-69 | decision | Execution is decomposed into one dedicated phase per skill named `ab-<skill>` plus a sixth `ab-voice` phase, with no phase holding more than about 25 runs and a driver aggregating. |
| skill-stickiness-70 | interface | The rubric is binary compliant/non-compliant on the scenario's forced choice, plus the four pass criteria recorded separately as booleans. |
| skill-stickiness-71 | constraint | Scoring is done by a read-only reviewer subagent given only the transcript and the rubric, never by arm B's author. |
| skill-stickiness-72 | constraint | Arm labels are stripped and transcripts shuffled before scoring, with the mapping restored only after all scores are recorded. |
| skill-stickiness-73 | decision | `sonnet` is the model for both probe subagents and the scorer. |
| skill-stickiness-74 | constraint | Arm A's pre-registered bar: a skill "already passes" if A is compliant on at least 3 of its 4 held-out runs. |
| skill-stickiness-75 | constraint | Arm B's pre-registered bar: compliant on at least 3 of its 4 held-out runs for that skill and strictly more compliant runs than both A and A′. |
| skill-stickiness-76 | decision | A skill whose arm B fails after its REFACTOR ceiling does not ship its armor and reverts to A′, while fix 1 ships regardless. |
| skill-stickiness-77 | decision | If arm A already passes for a skill, that skill's rewrite is unjustified and reverts to A′. |
| skill-stickiness-78 | decision | If A′ is roughly equal to B for a skill, that skill reverts to A′ even if B passes its own bar. |
| skill-stickiness-79 | decision | The voice probe uses `verification-before-completion` as its probe skill. |
| skill-stickiness-80 | interface | The probe runs four structurally identical variants each adding exactly one register device: V0 plain imperative baseline, V1 plus unity, V2 plus full authority, V3 plus moral framing. |
| skill-stickiness-81 | constraint | The pre-registered separation margin is at least 3 of 6 runs against V0. |
| skill-stickiness-82 | decision | A variant beating V0 by the margin ships that device across all five documents; V0 beating a variant by the margin drops that device and ships the baseline register. |
| skill-stickiness-83 | decision | With no separation at the margin, tier 3 applies and superpowers' register ships, recorded in `docs/skill-evidence/voice.md` as convention-or-prior with the null attached. |
| skill-stickiness-84 | constraint | If V1 loses to V0 by the margin, the conflict between prior and data escalates to the human rather than being auto-resolved. |
| skill-stickiness-85 | constraint | Whichever register wins is applied to the other four documents without re-testing each. |
| skill-stickiness-86 | scope | `handoff`, `pipeline` and `worktrees` receive only the fix-3 task-binding directive and are not rewritten as documents. |
| skill-stickiness-87 | scope | No change is made to the phase state machine, the review gate, or `drovr code-review`. |
| skill-stickiness-88 | constraint | Because skills ship via the nix flake and take no effect until the pin is bumped, the probe harness feeds skill text to subagents explicitly rather than relying on ambient injection. |
| skill-stickiness-89 | constraint | `grep -L -E 'in a drovr phase\|a drovr task\|a drovr phase has produced' skills/*/SKILL.md` must return all 8 files. |
| skill-stickiness-90 | constraint | A verbatim-overlap check against the superpowers corpus must find no 8-word-or-longer shingle shared with any drovr `SKILL.md`. |
| skill-stickiness-91 | constraint | An integration check confirms a clean session given a one-line bugfix request with no mention of drovr invokes `drovr:tdd` before writing code, run with `CLAUDE_PLUGIN_ROOT=<worktree>`. |

## Derivation notes

Recorded so a later reader can see what was weighed, not only what survived. These are the deriving
subagent's own exclusions, kept verbatim in substance:

- Excluded as rationale/motivation: all of §1 (the diagnosis and counts), §2.2's study summaries and
  transfer caveats, §2.5, §6's placement rationale, and §10's source list.
- Excluded as illustrative: §6's candidate loophole-closure phrasings and §4.1's candidate red-flag
  rows (the spec itself defers final wording to RED), and §6's "120–180 lines" expectation (§2.4
  makes only the byte cap authoritative).
- Excluded as restatement: the device table in §2.3 (each device is rowed at its concrete site),
  §7.4's per-variant run arithmetic (covered by the budget row), and §9's unit-test list where it
  merely re-asserts §4.2 decisions.
- Collapsed deliberately: the five literal `description:` strings are covered by rows 16/18 plus each
  skill's Iron Law row rather than five separate literal-string rows; row 42 keeps fix 3's four
  application sites as one coverage decision.
- Omitted with low confidence that they are load-bearing: the `README.md` key-table update,
  `cli/tests/reflex_hook.rs` as a named file, and the two `writing-skills/references/*` filenames.

**Rows 28 and 89 escape literal `|` characters as `\|`.** An unescaped pipe in a cell is read as a
column separator, so a row carrying one silently gains columns. Both rows record a shell alternation
(`startup|clear|compact`, and a `grep -E` pattern); the fixture itself is frozen byte-for-byte and
carries the unescaped originals.
