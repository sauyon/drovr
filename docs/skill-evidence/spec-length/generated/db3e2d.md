# Spec: skill stickiness

Make drovr's skill docs bind under context pressure instead of merely describing correct
practice. Five fixes, one new meta-skill, and an empirical acceptance test.

---

## 0. Build order and dependencies

Section numbers are not the build order.

1. **`drovr:writing-skills` + the scenario corpus** (§7.1, §7.2) — must exist first; nothing
   else is testable without the harness.
2. **Capture arm A / RED per skill** (§7.3) at the pre-fix `HEAD`, before any skill file is
   touched. Hard gate: once fix 1 lands, arm A is unrecoverable without a checkout.
3. **Fixes 1 + 3** (§3, §5).
4. **Fix 4** (§6), using the RED wording from step 2 for the rationalization tables.
5. **Held-out arm B + the voice probe** (§7.3, §7.4).
6. **Fix 2's hook layer** (§4.2) — the only Rust work; independently orderable, no dependency
   on steps 1–5. Fix 2's doc layer (§4.1) belongs with step 4.

---

## 1. Problem

drovr's skills read well and do not hold.

- All four discipline skills scope their `description:` to drovr phases —
  `skills/tdd/SKILL.md:3`, `skills/systematic-debugging/SKILL.md:3`,
  `skills/verification-before-completion/SKILL.md:3`, `skills/code-review/SKILL.md:3` — and the
  bodies repeat the scoping (`tdd:17-19`, `systematic-debugging:14-17`,
  `verification-before-completion:15-17,40-42`, `code-review:13-14,41`). Meanwhile
  `using-drovr/SKILL.md:54` makes inline work the default mode. The trigger text tells the agent
  the skill does not apply to what it is actually doing.
- The compliance surface is partial: Iron Laws, rationalization tables, loophole closures,
  announcement strings, and checklist→task binding are absent everywhere in `skills/`.
  `## Red flags — STOP` sections already exist in three of the four discipline skills
  (`tdd:30`, `systematic-debugging:33`, `verification-before-completion:29`) and are being
  expanded, not introduced.
- Nothing binds a checklist to durable state: a repo-wide search for `TodoWrite`/`TaskCreate`/
  `todo` returns two incidental hits in `HERDR-0.7.5-PORT.md`.
- Enforcement budget is aimed at mechanics over discipline: `NEVER|MUST|STOP` counts 6 for
  mechanics (`pipeline` 3, `handoff` 3) vs. 3 for the four discipline skills combined
  (`code-review` has 0) — a 2:1 tilt. superpowers' equivalent ratio is 13:12 (~1.08:1,
  roughly balanced); the claim here is only that drovr's own budget is inverted, not that
  superpowers is a counterexample of imbalance.
- Length gap: drovr discipline skills run 39–51 lines; superpowers' equivalents run 139–371.
- The reflex is a one-shot: drovr's entire hook surface is a single blocking `SessionStart`
  entry (`hooks/hooks.json:3-5`, matcher `startup|clear|compact`), which additionally no-ops
  inside phases (`hooks/session-start:22-24`, on non-empty `DROVR_PHASE`). Nothing re-asserts
  the discipline after turn one. Claude Code exposes `UserPromptSubmit` and `PreToolUse` for a
  real per-turn re-assertion; drovr uses neither today.

**Root cause.** superpowers has `writing-skills`, a meta-skill applying TDD to the skill
documents: RED = run a pressure scenario against a subagent without the skill and transcribe
its rationalizations verbatim; GREEN = write the minimal text countering exactly those;
REFACTOR = close each newly-observed loophole and re-run. Every compliance device in that
corpus is recorded output of that loop. drovr has no such loop, so its skills were written by
describing correct practice — which is why they read well and do not bind. Fixes 1–4 without
fix 5 would be assertions about what an agent rationalizes; fix 5 makes them evidence, which is
why fix 5 (§7) is in scope alongside the textual fixes.

---

## 2. Design principles

### 2.1 Authoring precedence (four tiers)

Every decision below is tagged with the tier that produced it:

1. **Tier 1 — published evidence exists.** The evidence decides; citation kept in the text.
2. **Tier 2 — no published evidence, but reachable by this run's loop.** Measure it (§7.4).
   "Reachable" = expressible as a text variant of an existing probe skill, scorable by the
   existing rubric, within the §7.3 budget ceiling. Cost may discharge a tier-2 obligation only
   when measurement would exceed the ceiling — never because it seems not worth it.
3. **Tier 3 — not reachable, or measurement inconclusive, and superpowers has a convention.**
   Follow superpowers; recorded plainly as a convention-follow, not an evidence-backed choice.
4. **Tier 4 — no superpowers analogue at all.** Engineering judgement, marked `[tier 4]`.
   Applies to: the gate's byte budget, the card length, the `per_turn` default, the
   reflex-marker boundary, the probe's model choice, §2.4's caps. None of these may be defended
   as convention- or evidence-backed.

Deviation from superpowers requires a citation, a measurement, or an explicit tier-4 marking.

**Two standing constraints, decided:**

1. **No fabricated measurements.** drovr states no number, duration, frequency, or comparative
   as measured unless drovr measured it or a citation supports it. Rhetorical emphasis (e.g.
   "every time") is allowed; a false measurement claim is not.
2. **No copied text.** Both projects are MIT-licensed, so copying with attribution would be
   legal, but drovr writes its own sentences — mechanisms are ported, expression is drovr's.
   Any verbatim line found in review gets the MIT notice and credit added retroactively.

### 2.2 Evidence-backed decisions

- **Persuasion levers used: authority, unity, commitment** (of Cialdini's seven, tested against
  LLMs by Meincke et al.). Authority = rank/non-negotiability (`MUST`, `NEVER`, imperative
  second person). Unity = shared identity, not rank and not liking (liking is banned in both
  superpowers and drovr as sycophancy-prone); drovr's unity framing is literally true rather
  than rhetorical: **the agent that inherits this work is the same agent, minus its context.**
  Commitment = consistency with a prior public declaration or tracked item.
- **Commitment devices are adopted as primary mechanisms** (fixes 3, 4: announcement strings,
  checklist→tracked-task binding) — commitment ranks top-two in both Meincke studies and is the
  cheapest, best-supported lever available.
- **Unity is applied to discipline skills**, a deviation from superpowers (whose own matrix
  withholds unity from discipline skills per the 2025 ranking) — the 2026 replication ranks
  unity top-two, above authority. This is a tier-1 decision but not a stronger one than
  authority: the same paper gives authority no procedural-adherence isolation either. Both are
  additionally measured in §7.4.
- **Binding content top-loads the document, and teaching content does not repeat itself**
  (§2.4) — based on Liu et al.'s U-shaped retrieval-by-position finding. Two caveats carried
  forward honestly: that study measures document retrieval, not obedience of an instruction
  placed mid-document, and its models are three generations old — so this is the basis for
  top-loading binding content, not proof mid-document rules are ignored.
- **Load-degradation figures for concurrent-task formatting compliance (−2–21%, recovered to
  90–100% by salience-enhanced prompting) are cited nowhere as a drovr-relevant measured
  number** — they come from secondary summaries of unread 2026 preprints. They inform (but do
  not establish) the redundancy rule (§2.4) and the per-turn gate (§4.2).
- **Every prohibition pairs with the required action in the same breath, and every rule states
  its reason** — per Anthropic's prompting guidance (tell Claude what to do, not just what not
  to do; give the motivation). Rationalization-table rebuttals are written as instructions
  ("Confidence is not evidence; run the command"), not negations ("Confidence ≠ evidence").
- **Social proof is used only in its true form** — naming a failure mode actually observed in
  `docs/skill-evidence/` — never in the invented-frequency form superpowers uses ("Every
  time."), per exception 1 above. The 2026 ranking does not place social proof top-two, so it is
  not promoted to primary.
- **Moral vocabulary is tier 2, not tier 1 and not rejected** — an earlier reading incorrectly
  treated an EmotionPrompt null result as evidence against moral framing; EmotionPrompt measures
  benchmark accuracy from emotional stimuli, not procedural adherence to a rule, so it does not
  settle the construct. Moral framing is therefore measured in §7.4 (variant V3) on the same
  footing as authority and unity, with superpowers' moral-framing-throughout as the tier-3
  fallback if the measurement is inconclusive.
- **The generic "helpful assistant" rule-wrapper ablation (arXiv:2601.22025) is read as
  non-monotonic, not as "generic rules hurt.**" It reports extraction 100%→90% and RAG
  compliance 93.3%→80% degrading, but instruction-following improving 13% — the closest analogue
  to §4.2's card. Decision: keep the card short and drovr-specific; do not claim to know its net
  effect.
- **Absolute compliance rates from the Meincke studies (33%→72%, 35%→51%) are never quoted as
  drovr numbers.** Transfer from a chat model being talked into a safety violation, to a coding
  agent adhering to a procedure it already agrees with over many turns, is not established by
  that literature — which is why fix 5 (§7) exists to measure drovr's own case directly.

### 2.3 Voice — decided devices and their tier

The full authority register (`MUST`/`NEVER`/`No exceptions`, imperative second person) has no
published isolation study for procedural adherence. Decision: **tier 2 — measured as arm V2 in
§7.4**, with superpowers' full register as the tier-3 fallback if the measurement is
inconclusive.

**Devices adopted** (superpowers convention unless a tier is named):

| Device | Countering |
|---|---|
| Iron Law — one fenced, all-caps line | Case-by-case renegotiation; a short string to cite back |
| Spirit-vs-letter line, before the rules | "I'm honoring the intent differently" |
| Unity line — the next phase agent is you, with your context gone | Detachment from downstream cost. Tier-1 prior; measured in §7.4 |
| Announcement string; checklist→task binding | Silent skipping. Evidence-backed — commitment |
| Rationalization table (thought → reality, reality as instruction) | The specific excuses observed in RED |
| Red flags — the agent's own inner-monologue fragments | Mid-drift self-detection |
| "No exceptions" bullets, each pairing prohibition with required action | Partial compliance, verb reinterpretation |
| Claim → required evidence → not sufficient table | Evidence substitution |
| Numeric escalation trigger (after 3 failed fixes, stop) | Thrash loops |
| ✅/❌ paired utterances | Ambiguity about what compliance sounds like |
| Top-loading binding content; restating near point of use | Decay under load |

**Moral vocabulary figures, stated precisely** (for the record, since moral framing is a tier-2
measured decision above): EmotionPrompt's headline +115% was BIG-Bench-only, from selecting the
best-performing stimulus per task; the replication authors' re-analysis gives **+4.42% on
BIG-Bench and +2.58% across all benchmarks**, and their own measurement found no significant
effect (χ²=0.11, p=.74).

**Flowcharts — adopted, tier 3.** No published comparison of flowchart vs. numbered list for
agent compliance exists, and the variant is not probe-reachable (it changes document structure,
not a text register). superpowers' convention stands. Placement rule (also adopted from
superpowers): flowcharts for genuine branching and for loops with a stop-too-early risk — never
for linear instructions or reference material. Applied to: the router's decision flow (§4.1),
the RED/GREEN/REFACTOR loop (§7.1), `drovr:tdd`'s red-green-refactor cycle (superpowers marks
this exact cycle with a fenced `dot` block at `test-driven-development/SKILL.md:49`, so
withholding one here would be an unmarked deviation), and `systematic-debugging`'s loop on the
same grounds. All other discipline procedures stay numbered lists (linear). Format: fenced `dot`
blocks, so the source stays legible to an agent reading raw text whether or not anything renders
it.

**Review-UI flowchart rendering — decided out of scope, tier 4 product judgement.** Authoring
the fenced `dot` blocks inside the skill docs is in scope (§8); making the review UI render them
and pairing a graph with the plan as a progress display is not (§8). Recorded as a follow-up in
`docs/known-issues.md` under a `## Resolved`-adjacent entry: *render fenced `dot` blocks in the
review UI and show the phase/gate graph alongside the plan.*

### 2.4 Length and the shipped budget test

**Decision: the byte budget applies to teaching content only. Binding content is deliberately
redundant** — stated 3–4 times per skill (Iron Law, red flag, rationalization row, checklist
item), because redundancy is the mechanism. Teaching content stays terse and is
cross-referenced, never restated.

- `cli/tests/skills_valid.rs:17` is the **single authoritative size check**. `wc -l`/`wc -w`
  are author guidance only, never asserted as a verification criterion (§9).
- **`BODY_BUDGET` for the four methodology skills (`skills_valid.rs:20-25`) rises from 2200 to
  12000 bytes** — `[tier 4]`, no superpowers analogue and no evidence sets a correct length.
- **`using-drovr` gets its own cap: 9000 bytes, and joins the checked list** — it is currently
  exempt by accident of history (5087 B / 775 words / 93 lines today; §4.1 adds six blocks) and
  is the single most expensive document in the repo, injected in full on every `SessionStart`.
- **Above cap, split into `references/`.**

The 12000/9000 caps deviate from superpowers in both directions (its TDD skill is 371 lines /
1496 words; its own authoring guidance recommends <200 words for frequently-loaded skills). This
is a tier-4 judgement call, not cited or measured.

### 2.5 Skill docs are prompts, not replies

Decision: the user's global terseness preference for assistant replies does not apply to skill
document length. A skill document's job is to survive a filling context window, which is a
different optimization target than a terse reply.

---

## 3. Fix 1 — un-scope the four methodology skills

**Decision.** All five `description:` fields below are changed to the literal strings in the
table (RED, per §7.1's four-part closure, may tighten wording later; these are the shipped
defaults). Reason: an agent working inline reads "in a drovr phase" and correctly concludes the
skill does not apply — this is a defect, not a style issue. A `description:` must be a trigger,
not a summary: a summary lets the agent take a shortcut instead of reading the skill body, and
`using-drovr:3` was doing this. Anthropic's skill-authoring guidance independently names the
trigger description as the highest-leverage line in a skill.

| File | New `description:` |
|---|---|
| `skills/tdd/SKILL.md:3` | `Use when implementing any feature or bugfix, before writing implementation code — requires a test you have watched fail before any implementation exists; no production code without a red test first` |
| `skills/systematic-debugging/SKILL.md:3` | `Use when encountering any bug, test failure, or unexpected behavior, before proposing or writing a fix — requires a reproduction and a mechanistic root cause before any code change` |
| `skills/verification-before-completion/SKILL.md:3` | `Use when about to claim any work is done, fixed, or passing, before reporting, committing, or handing off — requires running the verification command in this message and reading its output; evidence before assertion, always` |
| `skills/code-review/SKILL.md:3` | `Use when you have written any change, before calling it done or handing it forward — requires read-only reviewer subagents run in the foreground, with every Critical and Important finding resolved or explicitly recorded as deferred` |
| `skills/using-drovr/SKILL.md:3` | `Use at the start of every session and before every response, including before clarifying questions and before reading any file — routes to the right drovr skill and requires invoking it whenever there is even a 1% chance one applies` |

**Body demotions (decided):**

| File | Change |
|---|---|
| `skills/tdd/SKILL.md:17-19` | Phase framing demotes to one subordinate line: "Inside a drovr phase this also binds the next phase's contract." |
| `skills/systematic-debugging/SKILL.md:14-17` | Same demotion. The read-only-explorer rule is unconditional and stays. |
| `skills/verification-before-completion/SKILL.md:15-17,40-42` | `drovr phase done` demotes to conditional: "If you are in a phase, this is also what gates `drovr phase done`." |
| `skills/code-review/SKILL.md:13-14,41` | Same demotion for the pipeline-review-phase and report-done references. |

**Rule for all five:** the skill reads as unconditional discipline; every drovr-phase reference
becomes a clearly marked *additional* consequence, never a precondition.

**`docs/known-issues.md`:** one entry, in the file's existing convention (`## <symptom title> —
FIXED <date>`, `**Status:**`, `**Severity:** medium`, `**Found:**`, `### Symptom` / `### Root
cause` / `### Fix`), recording that four descriptions scoped unconditional discipline to phases
while `using-drovr` makes inline the default. One entry only — the remaining fixes are design
work, not defect repair.

---

## 4. Fix 2 — make `using-drovr` a per-turn gate

### 4.1 Doc layer (`skills/using-drovr/SKILL.md`)

**Decision: new content, in this order** (placement follows §2.2's position-effects
finding). Items 2 and 4 are **tier-3 convention-follows**, structurally close to
`using-superpowers/SKILL.md:11-29` — wording is drovr's own, devices are ported wholesale.

1. `<SUBAGENT-STOP>` — kept as-is (`:8-11`).
2. **The 1% rule** `[tier 3]`, above the H1, inside the existing `<EXTREMELY_IMPORTANT>` framing:
   even a 1% chance a drovr skill applies means invoke it. Paired with a cost-lowering clause: if
   it turns out not to fit, drop it — invoking costs almost nothing.
3. **The per-turn rule**: check before any response, including before a clarifying question and
   before any read-only exploration.
4. **Instruction-priority ladder** `[tier 3]`: human's explicit instructions > drovr skills >
   default behaviour. Safety counterweight to the MUST language; not optional.
5. **Gate function** — a fenced `dot` flowchart (branches, per §2.3's placement rule): message
   received → does any skill apply (≥1%) → invoke → announce → does it have a checklist →
   create one tracked item per step → follow it → only then respond.
6. **Red-flag table for the router's own failure mode** (invoking nothing at all, currently
   zero coverage). Candidate rows: "this is a one-line change", "I'll just look at the file
   first", "they asked a question, not for work", "I'm already mid-task, the router was for turn
   one", "escalating to a phase would be overkill so no skill applies". Final wording is decided
   by RED (§7.1), not fixed here.
7. Existing sections (single-writer, always-review, methodology, escalation) retained unchanged.

**Decision: items 2–5 sit outside every `<!-- reflex:section:NAME -->` marker** `[tier 4 —
no superpowers sectioned-reflex analogue]`. `[reflex.sections]` may subtract advisory sections
but cannot silently delete the routing core; only `[reflex] enabled = false` removes it. Reason:
a half-disabled router produces an agent that believes it is running drovr and is not — an
explicit master-switch-off is an informed choice, a section-subtraction accident is not.

### 4.2 Hook layer (new `UserPromptSubmit` hook)

Every decision in this subsection is `[tier 4]` — superpowers is `SessionStart`-only, so there
is no convention, and no study covers per-turn re-injection. It ships as an explicit,
unmeasured bet, recorded as such in `docs/skill-evidence/per-turn-gate.md`.

- `hooks/hooks.json` gains a `UserPromptSubmit` entry running a thin `hooks/user-prompt` script
  that `exec`s `drovr reflex --gate`. `UserPromptSubmit` takes **no `matcher`** — the existing
  `SessionStart` entry's `startup|clear|compact` matcher must not be copied across.
- **Cost is cumulative, not a rate**: `additionalContext` is appended every turn and stays in
  the window. Budget stated both ways: **≤600 bytes per injection**, and a **cumulative ceiling
  of ~60 KB over a 100-turn session**.
- **Suppression rule** (bounds the cumulative cost): the card is emitted only when no `drovr:*`
  skill was invoked in the previous turn. A session already running the discipline is not
  re-told; a drifted session is. This bounds the common case to a handful of injections.
- **Byte budget, not tokens** — the CLI has no tokenizer. Assertion: rendered
  `additionalContext` ≤ 600 bytes, checked in `cli/src/reflex.rs`'s test module.
- **Card content**: the 1% rule, the per-turn check, the announcement string, the
  checklist-binding line, a `<SUBAGENT-STOP>` line, and a pointer to `Skill drovr:using-drovr`.
- **Three API facts resolved, all verified against source:**
  1. `reflex.rs:143-152`'s `envelope()` hardcodes `"hookEventName": "SessionStart"` — it is
     parameterized to take the event name.
  2. `main.rs:114-118` declares `Reflex { skill: PathBuf }` as required, and `main.rs:1276-1278`
     asserts bare `drovr reflex` errors. `--gate` forces `Option<PathBuf>`, a clap arg-group
     rule (`--gate` xor `--skill`), and an update to that test.
  3. **Card text source is a `const` in `reflex.rs`, not extraction from `SKILL.md`** —
     extraction would need markers inside the region §4.1 deliberately places outside all
     `reflex:section` markers. Accepted cost: drift between card and skill, mitigated by a test
     asserting the card's key phrases appear in `using-drovr/SKILL.md`.
- **Subagents.** `using-drovr:8-11` already carries `<SUBAGENT-STOP>`. §7.3/§7.4's foreground
  probe subagents and drovr's own read-only reviewers all launch from a gate-on session; if
  `UserPromptSubmit` fires for Agent-tool subagents in this harness, the card would inject into
  every one and contaminate the probes. **Decision: the card carries its own subagent-stop line
  unconditionally.** Step 1 of §0 verifies empirically whether the hook fires for subagents,
  recording the answer in `docs/skill-evidence/per-turn-gate.md`.
- **Config**: new `[reflex] per_turn` bool in `ReflexConfig` (`cli/src/config.rs:39-64`),
  **default true** `[tier 4]`, suppressible per-user. Uses a **named default fn**, not bare
  `#[serde(default)]` — the trap already documented at `config.rs:108-110`.
- **Asymmetric suppression, decided deliberately**: the full `SessionStart` reflex stays
  suppressed inside phases (`DROVR_PHASE` set), because a phase agent runs on its injected
  briefing. The per-turn gate does **not** suppress in phases — a phase is exactly where the
  discipline must hold.
- `README.md:70-94` updated with the new key.

### 4.3 Out of scope

`PreToolUse` hard-denial (e.g. blocking an `Edit` that skipped `drovr:tdd`, or a `git commit`
with no verification evidence) is enforcement rather than persuasion — a larger design with real
false-positive risk. Not built in this run.

---

## 5. Fix 3 — bind checklists to tracked task state

**Decision — directive text, stated harness-agnostically** (some sessions expose
`TaskCreate`/`TaskUpdate`/`TaskList` and no `TodoWrite`; stable Claude Code exposes `TodoWrite`):

> When a skill or briefing gives you a numbered checklist, create **one tracked item per step**
> using whatever task tool this harness exposes — `TodoWrite`, or `TaskCreate`/`TaskUpdate` —
> before you start step 1. Mark each in-progress when you start it and complete when its
> evidence is in hand. If the harness exposes no task tool, write the checklist to
> `~/.local/share/drovr/runs/<run>/checklist.md` when inside a run, or `CHECKLIST.md` at the
> repo root otherwise, and tick items there. An untracked checklist decays with the context
> window; that decay is the exact failure drovr exists to fight.

**Applied at four sites (decided):**

1. `skills/using-drovr/SKILL.md` — the gate function's checklist branch (§4.1 step 5).
2. Each discipline skill's numbered procedure gets a one-line binding directive above it.
3. `skills/pipeline/phase-prompts/*.md` — the `## Do` lists (`implement-task.md:14-64`, 7 steps;
   and the equivalents in `brainstorm.md`, `plan.md`, `review.md`, `review-angle.md`) gain the
   directive as step 0. These are agent-assembled markdown, not compiled into the binary, so
   this is a pure text change.
4. `skills/handoff/SKILL.md` and `skills/handoff/HANDOFF-template.md` — the 7-section handoff is
   a checklist and gets the same treatment.

No CLI change required for this fix.

---

## 6. Fix 4 — move the armor onto the discipline skills

**Decision: `tdd`, `systematic-debugging`, `verification-before-completion` and `code-review`
are restructured to a fixed section order** `[tier 3 — order is superpowers' convention,
adopted wholesale]`:

```
   REQUIRED — all four skills
1. description:      trigger, not summary (fix 1)
2. Overview          core principle + spirit-vs-letter line + the WHY, ≤6 lines
3. Unity line        "the next phase agent is you, with your context gone"
4. The Iron Law      one fenced all-caps line + "No exceptions:" bullets closing named loopholes,
                     each bullet pairing the prohibition with the required action
5. Announce          the exact sentence to emit when invoking the skill (commitment device)
6. The procedure     numbered, preceded by the task-binding directive (fix 3)
8. Red flags — STOP  inner-monologue fragments, closed by a catch-all bullet
9. Rationalizations  thought → reality; the reality column is an INSTRUCTION, not a rebuttal
10. Worked example   one ✅/❌ pair of actual utterances. One good example, not five
11. Cross-refs       bare skill names with REQUIRED markers; never @-links (they force-load)

   CONDITIONAL — only where named below
7. Requirements      claim → required evidence → not sufficient
                     ONLY: verification-before-completion, code-review
6b. Cycle flowchart  a fenced dot graph of the loop
                     ONLY: tdd, systematic-debugging   (§2.3 placement rule)
```

Not every skill carries all 11 sections — §9's mechanical check accounts for this by construction.

**Announcement sentences (decided, template `Using drovr:<skill> to <purpose>.`):**

- `tdd` — "Using drovr:tdd — writing the failing test before the implementation."
- `systematic-debugging` — "Using drovr:systematic-debugging — reproducing before fixing."
- `verification-before-completion` — "Using drovr:verification-before-completion — running the
  checks before claiming done."
- `code-review` — "Using drovr:code-review — dispatching read-only reviewers before calling this
  done."

**Placement rationale.** Sections 2–5 sit at the top per §2.2's position-effects finding.
Sections 8–9 sit late for proximity to the point of temptation — `[tier 4]`, not the U-shape
finding. The procedure (6) sits mid-document deliberately: it is the section the agent actively
re-reads while working, so it is least dependent on single-pass recall. The position finding is
not cited to justify sections 6–9's placement.

**Per-skill specifics (decided):**

- **`tdd`** — Iron Law: no implementation code before a test you have watched fail. Loophole
  closures: "I'll keep the code as reference", "the test is obvious so I'll write it after",
  "it's a refactor so TDD doesn't apply", "the harness makes it hard to run one test".
- **`systematic-debugging`** — Iron Law: no fix before a reproduction and a mechanistic cause.
  Adds the numeric escalation trigger: after 3 failed fixes, stop and question the design; do
  not attempt fix #4 without that conversation.
- **`verification-before-completion`** — Iron Law: no completion claim without fresh evidence
  produced in this message. Requirements table covers tests / build / linter / bug-fixed /
  subagent-reported-success. Catch-all red flag for any wording implying success.
- **`code-review`** — Iron Law: no change is done until a read-only reviewer has looked at it
  and every Critical/Important finding is resolved or explicitly recorded as deferred. Loophole
  closures: "the change is too small", "I already reviewed it myself", "the pipeline's review
  phase will catch it". The existing FOREGROUND rule (`code-review/SKILL.md:19-23`) is promoted
  into the no-exceptions list — backgrounding a reviewer is the known way this skill silently
  fails.

Expected sizes: 39–51 lines today → roughly 120–180 lines each. The binding constraint is
§2.4's 12000-byte `BODY_BUDGET`, not the line count.

---

## 7. Fix 5 — the meta-skill and the empirical loop

### 7.1 New skill: `drovr:writing-skills`

`skills/writing-skills/SKILL.md`, in drovr's voice. Pass criteria, the four-part closure, and
the scenario-construction rules below are `[tier 3]` convention-follows, ported from
`testing-skills-with-subagents.md:182-275` in drovr's own wording. Anthropic's own
skill-authoring guidance independently prescribes the same loop (evaluations first, baseline
without the skill, minimal instructions, iterate), so this is convergent, not merely borrowed.

- **Mapping**: pressure scenario ↔ test case; `SKILL.md` ↔ production code; RED ↔ the agent
  violates the rule without the skill; GREEN ↔ it complies with the skill; REFACTOR ↔ close each
  new loophole.
- **Loop** (fenced `dot` flowchart, a stop-too-early loop per §2.3): build the scenario set → run
  the baseline → transcribe every excuse verbatim → write the minimal counter-text → re-run on
  held-out scenarios → apply the four-part closure to each new rationalization → repeat until a
  run produces no new rationalization **or** the §7.3 REFACTOR ceiling is reached, whichever
  comes first. The ceiling is not optional — an uncapped loop is the unbounded-cost defect this
  spec exists to fix elsewhere.
- **Four-part closure** (all four every time): explicit negation inside the rule; a row in the
  rationalization table; a bullet in red flags; a `description:` update adding the symptom of
  being about to violate.
- **Scenario construction rules**: real file paths, concrete numbers and deadlines, a forced
  A/B/C choice, "what do you do" not "what should you do", no escape hatch to "I'd ask the
  human", and ≥3 combined pressure types (time, sunk cost, authority, economic, exhaustion,
  social, pragmatic).
- **Pass criteria** (all four): correct option under maximum pressure; the agent cites a
  specific section; it names the temptation and complies anyway; the meta-test ("how should this
  have been written?") returns "it was clear". Not-bulletproof signals: new rationalizations,
  the agent arguing the skill is wrong, invented hybrid approaches, asking permission while
  arguing hard for the violation.
- **drovr-specific constraints**: scenario subagents run in the FOREGROUND; the author is the
  single writer; §2.1's no-fabricated-measurements rule applies.
- Reference files: `references/pressure-scenarios.md` and `references/testing-with-subagents.md`
  — keeps `SKILL.md` under §2.4's cap.

### 7.2 Evidence corpus (decided layout)

- `skills/writing-skills/scenarios/<skill>-<n>.md` — checked-in scenario prompts, each tagged
  `dev` or `holdout` (§7.3). They are tests; they live with the skill.
- `docs/skill-evidence/<skill>.md` — per skill: scenarios used, verbatim baseline
  rationalizations, the counter-text, and re-test results with dates.
- `docs/skill-evidence/voice.md` — the §7.4 probe record.
- `docs/skill-evidence/per-turn-gate.md` — §4.2's unmeasured-bet record and the subagent-firing
  answer.
- Raw transcripts under `docs/skill-evidence/transcripts/` — the corpus any numeric claim in
  drovr skill text cites (§2.1 exception 1).

### 7.3 The acceptance test — runs in this run

Answers: how do we know stickiness improved rather than that the docs got longer.

**Held-out design (decided).** Authoring arm B from the same scenarios that grade it would fit
the text to the test. Per skill: **3 scenarios — 1 development, 2 held-out.** RED transcription
and counter-text authoring use the development scenario only. The pass bar is pre-registered and
scored on the held-out pair, never read while writing arm B.

**Arms (decided — three, so fix 1 and fix 4 are separable):**

| Arm | Text |
|---|---|
| **A** | Current `SKILL.md` at the pre-fix `HEAD` (captured in §0 step 2) |
| **A′** | Fix-1-only: descriptions un-scoped, no armor |
| **B** | Full rewrite (fixes 1 + 3 + 4) |

Arm A′ exists because arm A's descriptions are phase-scoped (the defect §3 fixes); without A′, a
non-phase-framed scenario would handicap A for reasons unrelated to the armor, and B > A would
prove nothing about fix 4 specifically. A′ isolates the armor's contribution.

**Budget table (decided, hard ceilings).** When a ceiling is hit, work halts and records a null
in `docs/skill-evidence/` — it does not silently extend.

| Stage | Scope | Runs | Ceiling |
|---|---|---|---|
| RED / baseline on dev set | 5 skills × 1 dev scenario × 2 samples | 10 | 10 |
| Arm A on held-out | 5 skills × 2 held-out × 2 samples | 20 | 20 |
| Arm A′ on held-out | 4 discipline skills × 2 × 2 | 16 | 16 |
| Arm B on held-out | 5 skills × 2 held-out × 2 samples | 20 | 20 |
| REFACTOR re-tests | failing skills only, ≤2 iterations each | ≤20 | 20 |
| Voice probe (§7.4) | 4 variants × 2 scenarios × 3 samples | 24 | 24 |
| `using-drovr` no-skill-applies class | 2 scenarios × 3 arms × 2 samples | 12 | 12 |
| **Total** | | **≈122** | **122 hard ceiling** |

`using-drovr`'s extra scenario class checks the router's own failure mode (invoking nothing)
without inducing the opposite failure (invoking everything reflexively). It is budgeted, not
free.

**Execution decomposition (decided).** 122 foreground subagent runs with verbatim transcription
will not fit in one implement-task context. **One dedicated phase per skill**, `ab-<skill>` (5
phases), each running its own stages, writing `docs/skill-evidence/<skill>.md` plus raw
transcripts, and exiting. The voice probe is a sixth phase, `ab-voice`. The driver aggregates.
No phase holds more than ~25 runs.

**Scoring (decided, pre-registered, not by arm B's author):**

- **Rubric**: binary compliant/non-compliant on the scenario's forced choice, plus the four
  §7.1 pass criteria recorded separately as booleans.
- **Scorer**: a read-only reviewer subagent, given the transcript and rubric only.
- **Blinding**: arm labels stripped and transcripts shuffled before scoring; mapping restored
  only after all scores are recorded.
- **Model**: `sonnet` for probe subagents and for the scorer.

**Pre-registered bars (decided before arm B runs):**

- **Arm A bar**: a skill "already passes" if A is compliant on ≥3 of its 4 held-out runs (2
  held-out scenarios × 2 samples).
- **Arm B bar**: compliant on ≥3 of its 4 held-out runs for that skill, AND strictly more
  compliant runs than both A and A′.
- **Consequence of failure — not merely documentation.** A skill whose B fails after its
  REFACTOR ceiling does not ship its armor: it reverts to arm A′ (fix-1-only), and
  `docs/skill-evidence/<skill>.md` records the failure and the reverted state. Fix 1 ships
  regardless (defect repair); fix 4 must earn its bytes.
- **Falsifiable**: if arm A already passes for a skill, that skill's rewrite is not justified
  and reverts to A′.
- If A′ ≈ B for a skill, the armor is not carrying its weight there and that skill reverts to
  A′ even if B passes its own bar.

Null and negative results are recorded alongside positive ones.

### 7.4 Voice as a measured variable

Three register questions have no published isolation study for procedural adherence and are all
reachable as text variants of one probe skill — tier 2, measured here.

**Decision: probe skill is `verification-before-completion`** — sharpest binary outcome (did the
agent claim done without running the command), shortest scenarios. **Model: `sonnet`**, matching
§7.3 — noted limitation: this is not the Opus-class model drovr sessions run on, alongside the
§2.2 extrapolation limitation.

**Four variants (decided), identical in structure — same Iron Law, procedure, tables,
placement — each adding exactly one register device to the baseline:**

| Variant | Register |
|---|---|
| **V0** | Baseline: plain imperative; no all-caps, no absolutist "no exceptions"; operational consequence only |
| **V1** | V0 + unity line |
| **V2** | V0 + full authority register (`MUST`/`NEVER`, all-caps Iron Law, no-exceptions bullets) |
| **V3** | V0 + moral framing, in superpowers' register |

4 variants × 2 scenarios × 3 samples = 24 runs, 6 per variant.

**Pre-registered decision rule (decided). Separation margin: ≥3 of 6** per side (matches stated
power; a 2-of-12 margin on binary outcomes is roughly one standard deviation and would fire on
noise).

1. Variant beats V0 by ≥3 → that device ships across all five documents.
2. V0 beats the variant by ≥3 → that device is dropped; the baseline register ships for that
   dimension. This is a real, defined outcome, not an undefined one.
3. No separation ≥3 (the likeliest outcome) → tier 3: follow superpowers. Authority ships,
   moral framing ships, and **unity ships anyway** on the strength of the 2026 published ranking
   (tier 1 outranks tier 3) even though superpowers itself withholds unity from discipline
   skills. Each is recorded in `docs/skill-evidence/voice.md` as convention-or-prior, with the
   null attached.
4. Whatever wins applies to the other four documents without re-testing each. Stated
   limitation: measured on one skill, one model, generalised to five documents.

**Unity's standing.** Unity is adopted on a published prior (§2.2) and its arm here is reported,
not decisive — 6 runs cannot overturn an N=126,000 result. **No directional escape hatch**: if
V1 loses to V0 by ≥3, that is recorded as a genuine conflict between the prior and drovr's own
data, and the decision escalates to the human rather than being auto-resolved.

**Power note.** n=6 per variant detects only large effects, and register effects are expected to
be small on frontier models, so outcome 3 is the single likeliest result — informative, not a
failure. Nothing stronger than "suggestive" is recorded for any voice-probe outcome.

---

## 8. Scope boundaries

**In scope:**

- Skill docs: all 8 `skills/*/SKILL.md`. Five get `description:` changes (§3); the four
  discipline skills get the §6 rewrite; `handoff`, `pipeline` and `worktrees` receive **only**
  the fix-3 task-binding directive and nothing else.
- `skills/handoff/HANDOFF-template.md`; the 5 files under `skills/pipeline/phase-prompts/`.
- New: the `skills/writing-skills/` tree (`SKILL.md`, `references/`, `scenarios/`);
  `docs/skill-evidence/`.
- One `docs/known-issues.md` entry, plus the review-UI flowchart follow-up note (§2.3).
- Hooks: `hooks/hooks.json`, new `hooks/user-prompt`.
- Rust: `cli/src/reflex.rs` (`--gate`, `envelope()` parameterization), `cli/src/config.rs`
  (`per_turn`), `cli/src/main.rs` (arg-group wiring).
- Tests: `cli/tests/skills_valid.rs` (`BODY_BUDGET` 2200 → 12000, add `using-drovr` at 9000 —
  §2.4), `cli/tests/reflex_hook.rs` (per-turn hook cases), and `main.rs:1276-1278`'s
  bare-`drovr reflex` assertion.
- `README.md:70-94`.

**Out of scope:** rewriting `handoff` / `pipeline` / `worktrees` as documents beyond the fix-3
directive; any `PreToolUse` enforcement (§4.3); any change to the phase state machine, the
review gate, or `drovr code-review`; rendering `dot` blocks in the review UI (§2.3 follow-up,
tracked in `docs/known-issues.md`).

**Deployment reality.** These skills ship via the nix flake. Nothing here takes effect in the
running session until the flake pin is bumped; the probe harness therefore feeds skill text to
subagents explicitly rather than relying on ambient injection.

---

## 9. Verification

1. **Mechanical**, each a runnable check:
   - Every discipline `SKILL.md` contains the 10 required sections of §6, plus section 7 where
     §6 names it (`verification-before-completion`, `code-review`) and 6b where §6 names it
     (`tdd`, `systematic-debugging`) — not "all 11 on every skill", which is unsatisfiable by
     construction.
   - `cargo test --test skills_valid` passes at the new budgets (§2.4); this is the authoritative
     size check, `wc -l`/`wc -w` are not asserted.
   - `grep -L -E 'in a drovr phase|a drovr task|a drovr phase has produced' skills/*/SKILL.md`
     returns all 8 files — none of those literals survives anywhere.
   - Verbatim-overlap check against
     `/home/sauyon/.claude/plugins/cache/claude-plugins-official/superpowers/5.1.0/skills/`: no
     ≥8-word shingle shared with any drovr `SKILL.md`. Any hit is reworded or gets the MIT
     attribution required by §2.1 exception 2.
2. **Unit tests:** rendered `--gate` `additionalContext` ≤ 600 bytes; `per_turn` defaults to true
   with a `[reflex]` table present but the key absent (the `config.rs:108-110` trap); the
   `UserPromptSubmit` hook emits valid hook JSON with the correct `hookEventName` and respects
   `enabled = false`; the routing core survives `[reflex.sections]` subtraction; the card's key
   phrases still appear in `using-drovr/SKILL.md` (drift guard, §4.2).
3. **Empirical:** §7.3's held-out A/A′/B and §7.4's voice probe, ≤122 runs; results — including
   nulls and reverted skills — committed to `docs/skill-evidence/`.
4. **Integration:** a clean session, given a one-line bugfix request with no mention of drovr,
   invokes `drovr:tdd` before writing code. Run against this worktree with
   `CLAUDE_PLUGIN_ROOT=<worktree>` so it does not depend on the flake pin being bumped first.

---

## 10. Sources

- Meincke, L., Shapiro, D., Duckworth, A. L., Mollick, E., Mollick, L., & Cialdini, R. (2025).
  *Call Me A Jerk: Persuading AI to Comply with Objectionable Requests.* N=28,000, GPT-4o-mini,
  33.3% → 72.0%; two request types. — https://papers.ssrn.com/sol3/papers.cfm?abstract_id=5357179
- Meincke et al., expanded replication (published 2026-05-19). N=126,000 across Claude Haiku 4.5,
  GPT-5 mini, Gemini 3 Flash; 35.3% → 51.3%; regulated-substance request only; commitment and
  unity strongest, liking and reciprocity weakest. —
  https://gail.wharton.upenn.edu/research-and-insights/persuading-llms-objectionable-requests/ ·
  https://www.pnas.org/doi/10.1073/pnas.2535868123
- Liu, N. F. et al. *Lost in the Middle: How Language Models Use Long Contexts.* arXiv 2023, TACL
  2024. Retrieval accuracy by gold-document position; 2023-vintage models. —
  https://cs.stanford.edu/~nfliu/papers/lost-in-the-middle.arxiv2023.pdf
- Vaugrante, L., Niepert, M., & Hagendorff, T. (2024). *A Looming Replication Crisis in Evaluating
  Behavior in Language Models?* Source of the EmotionPrompt correction (+4.42% BIG-Bench / +2.58%
  all-benchmark re-analysis of Li et al.; the replication's own measurement found no significant
  effect, χ²=0.11, p=.74). — https://arxiv.org/pdf/2409.20303
- Li, C. et al. (2023). *Large Language Models Understand and Can be Enhanced by Emotional
  Stimuli* (EmotionPrompt). — https://arxiv.org/abs/2307.11760
- arXiv:2601.22025 — generic rule-wrapper ablation; n=50, Llama 3 8B / Qwen 2.5 7B; extraction
  100%→90%, RAG 93.3%→80%, instruction-following +13%. — https://arxiv.org/html/2601.22025v2
- Anthropic. *Prompting best practices* (current models incl. Opus 5). "Tell Claude what to do
  instead of what not to do"; providing "context or motivation behind your instructions" improves
  targeting. —
  https://platform.claude.com/docs/en/build-with-claude/prompt-engineering/be-clear-and-direct
- Anthropic. *Agent Skills — skill authoring best practices.* Prescribes the eval-first loop and
  treats the trigger description as the highest-leverage line. —
  https://platform.claude.com/docs/en/agents-and-tools/agent-skills/best-practices
- Load-under-task-degradation figures (−2–21% formatting compliance, recovered to 90–100%) are
  from secondary summaries not read in full; labelled as such at point of use (§2.2) and not
  load-bearing for any decision.
- superpowers, read first-hand: `skills/writing-skills/persuasion-principles.md` (cites Cialdini
  2021 and the 2025 Meincke figure only), `testing-skills-with-subagents.md`,
  `anthropic-best-practices.md`, `test-driven-development/SKILL.md`. MIT, © 2025 Jesse Vincent.
