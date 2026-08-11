# Spec: skill stickiness

Make drovr's skill docs *bind* under context pressure instead of merely describing correct
practice. Five fixes, one new meta-skill, and an empirical acceptance test.

---

## 0. Order of work

Section numbers are not the build order. Fix 5 produces the wording fixes 2 and 4 depend on, and
arm A must be captured before fix 1 overwrites it.

1. **`drovr:writing-skills` + the scenario corpus** (§7.1, §7.2). Nothing else can be tested until
   the harness exists.
2. **Capture arm A / RED per skill** (§7.3) — at the pre-fix `HEAD`, before any skill file is
   touched. This is a hard gate: once fix 1 lands, arm A is unrecoverable without a checkout.
3. **Fixes 1 + 3** (§3, §5) — descriptions, phase-framing demotions, task-binding directives.
4. **Fix 4** (§6), using the RED wording from step 2 for the rationalization tables.
5. **Held-out arm B + the voice probe** (§7.3, §7.4).
6. **Fix 2's hook layer** (§4.2) — the only Rust work; independently orderable, no dependency on
   steps 1–5. Fix 2's doc layer (§4.1) belongs with step 4.

---

## 1. Problem

drovr's skills read well and do not hold.

**All four discipline skills are scoped to drovr phases in their `description:`** —
`skills/tdd/SKILL.md:3` ("in a drovr phase"), `skills/systematic-debugging/SKILL.md:3`
("in a drovr phase"), `skills/verification-before-completion/SKILL.md:3` ("a drovr task …
signalling phase done"), `skills/code-review/SKILL.md:3` ("when a drovr phase has produced an
artifact"). The bodies repeat it (`tdd/SKILL.md:17-19`, `systematic-debugging/SKILL.md:14-17`,
`verification-before-completion/SKILL.md:15-17,40-42`, `code-review/SKILL.md:13-14,41`).
Meanwhile `using-drovr/SKILL.md:54` makes **inline work the default mode**. The trigger text
therefore tells the agent the skill does not apply to what it is actually doing.

**The compliance surface is partial.** Genuinely absent everywhere in `skills/`: Iron Laws,
rationalization tables, loophole closures, announcement strings, checklist→task binding. Already
present and being *expanded* rather than introduced: `## Red flags — STOP` sections in three of
the four discipline skills (`tdd:30`, `systematic-debugging:33`,
`verification-before-completion:29`).

**Nothing binds a checklist to durable state.** A repo-wide search for `TodoWrite` / `TaskCreate`
/ `todo` returns two incidental hits in `HERDR-0.7.5-PORT.md`.

**The enforcement budget is aimed at mechanics, not discipline.** Counting `NEVER|MUST|STOP`:
mechanics = 6 (`pipeline` 3, `handoff` 3); the four discipline skills = 3 combined (`tdd` 1,
`systematic-debugging` 1, `verification-before-completion` 1, `code-review` **0**) — a 2:1 tilt
toward mechanics. The same count over superpowers gives 13 for its discipline skills against 12
for mechanics/meta, a 1.08:1 tilt carried mostly by one skill; so superpowers is roughly balanced
rather than inverted, and the claim here is only that drovr's own budget is pointed the wrong way.

**Length gap**: drovr discipline skills run 39–51 lines; superpowers' equivalents run 139–371.

**The reflex is a one-shot.** drovr's entire hook surface is a single blocking `SessionStart`
entry, matcher `startup|clear|compact` (`hooks/hooks.json:3-5`), which additionally no-ops
inside phases (`hooks/session-start:22-24`, on non-empty `DROVR_PHASE`). Nothing re-asserts the
discipline after turn one. superpowers is also `SessionStart`-only — its "before ANY response"
rule is doc text in a session-resident router, not a mechanism — but Claude Code does expose
`UserPromptSubmit` and `PreToolUse`, so a real per-turn re-assertion is available and unused.

### Root cause

superpowers has `writing-skills`, a meta-skill that applies TDD to the skill documents
themselves: RED = run a pressure scenario against a subagent *without* the skill and transcribe
its rationalizations verbatim; GREEN = write the minimal text countering exactly those; REFACTOR
= close each newly-observed loophole and re-run. Every compliance device in that corpus is
recorded output of that loop. drovr has no such loop, so its skills were written by describing
correct practice — which is why they read well and do not bind.

**Fixes 1–4 without fix 5 are assertions about what an agent rationalizes. Fix 5 makes them
evidence.**

---

## 2. Design principles

### 2.1 The authoring rule — four-tier precedence

1. **Published evidence exists** → the evidence decides, with the citation kept in the text.
2. **No published evidence, but the question is reachable by this run's loop** → **measure it**
   (§7.4). "Reachable" means it can be expressed as a text variant of an existing probe skill and
   scored by the existing rubric, within the §7.3 budget table's stated ceiling. Cost may only
   discharge a tier-2 obligation when a measurement would exceed that ceiling — never because it
   seems not worth it.
3. **Not reachable, or the measurement is inconclusive, and superpowers has a convention** →
   **follow superpowers**, and say plainly it is a convention-follow, not an evidence-backed
   choice.
4. **No superpowers analogue at all** → **engineering judgement, marked as such.** Tier 3 cannot
   apply to decisions superpowers never faced: the gate's byte budget, the card length, the
   `per_turn` default, the reflex-marker boundary, the probe's model choice, the §2.4 caps. These
   are judgement calls, they are labelled **[tier 4]** at each site, and none of them may be
   defended as though a convention or a study backed it.

superpowers' corpus is the output of an adversarial testing loop drovr has not yet run, so it is
the best available **prior and tie-break** — not a substitute for measurement, and never a reason
to skip one that is reachable. Deviation from it requires a citation, a measurement, or an
explicit tier-4 marking.

Two standing exceptions, both about honesty rather than style:

1. **No fabricated measurements.** drovr does not state a number, duration, frequency, or
   comparative ("faster than", "better than") as measured unless drovr measured it, or a citation
   supports it. superpowers cites "15–30 minutes vs 2–3 hours" and "from 24 failure memories";
   drovr cites only what `docs/skill-evidence/` contains. Rhetorical emphasis ("every time") is
   fine; a false measurement claim is not.
2. **No copied text.** Both projects are MIT (superpowers © 2025 Jesse Vincent; drovr © 2026
   Sauyon Lee), so copying with attribution would be legally fine. drovr still writes its own
   sentences: it is a self-contained replacement, which is why the `drovr:` namespace exists.
   Mechanisms are ported; expression is drovr's. If any verbatim line survives review, the MIT
   notice and credit get added.

### 2.2 What the evidence says

The persuasion findings below come from Cialdini's seven principles — authority, commitment,
scarcity, social proof, **unity**, reciprocity, liking — as tested against LLMs. Three names do
work in this spec:

- **Authority** — *"I outrank you."* Compliance from rank and non-negotiability: `MUST`, `NEVER`,
  "no exceptions", imperative second person.
- **Unity** — *"I am one of you."* Compliance from shared identity: the instruction reads as coming
  from inside your own group with a common stake, rather than from above. Distinct from authority
  (rank) and from liking (*"I am pleasant to you"*, which superpowers bans and drovr bans too, as
  it breeds sycophancy). In superpowers this is the "your human partner" register. drovr's version
  is literally true rather than rhetorical: **the agent that inherits this work is the same agent,
  minus its context.**
- **Commitment** — consistency with a prior public declaration or action. Get the agent to *state*
  it is doing X, or to record X as a tracked item, and it is likelier to then do X.

The authority/unity contrast is the one to hold onto: §7.4 measures them against each other, and
they are opposite theories of why an instruction lands — *because you must* versus *because it is
your own work on the line*.

**Position effects — retrieval accuracy is U-shaped.** Liu et al., *Lost in the Middle* (read in
full), measures **retrieval accuracy by gold-document position**: highest at the start and end of
the context, lowest in the middle, on 2023-vintage models. Two transfer caveats, held to the same
standard §2.2 applies to Meincke: it measures retrieval of a *document*, not obedience of an
*instruction* placed mid-document, and the models are three generations old. It is the basis for
top-loading binding content (§6), not proof that mid-document rules are ignored.

**Load degradation — directional only, source not read.** Figures circulating for
formatting-compliance loss under concurrent task load (−2–21%, recovered to 90–100% by
salience-enhanced prompting) come from **secondary summaries of 2026 arXiv preprints that were not
read in full**. They are consistent with the redundancy rule (§2.4) and the per-turn gate (§4.2)
but they do not establish it, they are not "the highest-confidence finding", and under exception 1
they may not be quoted as measured drovr-relevant numbers anywhere in the shipped skills.

**Commitment devices are the best-evidenced persuasion lever.** Commitment ranks top-two in both
Meincke studies. The announcement string and the checklist→tracked-task binding *are* commitment
devices — the cheapest and best-supported things in this spec. Promoted to primary (fixes 3, 4).

**Unity ranks top-two as well** in the 2026 replication, above authority. superpowers' own matrix
(`persuasion-principles.md:128-133`) withholds unity from discipline skills, following the 2025
ranking it cites (`:176`). drovr applies unity to discipline skills — a deviation backed by the
newer data — with the literal-truth phrasing above. Note the honest limit: this is the *same
paper* that gives authority no procedural-adherence isolation, so unity gets no stronger standing
than authority does. Both are measured in §7.4; unity's published rank is a prior, not a licence.

**Persuasion effects shrink on newer, smaller models — and the comparison is not clean.** Meincke
et al. measured a ~40-point lift on GPT-4o-mini (N=28,000; 33.3% → 72.0%). The 2026 replication
across **Claude Haiku 4.5, GPT-5 mini and Gemini 3 Flash** (N=126,000) found a **16-point** lift
(35.3% → 51.3%); reasoning models are "more resistant, but not truly or consistently resistant."
Two limits on reading that gap as a model-generation effect: the request sets differ (2025 used
two request types, 2026 the regulated-substance request only, and dropping the easier request
lowers the aggregate regardless of model), and **none of the tested models are the ones drovr
runs** — drovr's probes use Sonnet (§7.3) and its sessions run Opus-class. Extrapolating from
Haiku/mini/Flash to Sonnet and Opus is **a stated assumption, not a finding** [tier 4]; Anthropic's
own guidance asks for testing across Haiku, Sonnet and Opus, which this run does not do. Register
is treated as a real but secondary lever on that assumption.

**Give the reason; say what to do.** Anthropic's prompting guidance for these models: *tell
Claude what to do instead of what not to do*, and providing "context or motivation behind your
instructions" improves targeting. So every prohibition pairs with the required action in the same
breath, and every rule carries its why. The rationalization table's second column reads as an
instruction, not a rebuttal — "Confidence is not evidence; run the command" rather than
"Confidence ≠ evidence".

**Generic rule wrappers are non-monotonic — including one result that cuts toward the gate.** The
ablation (arXiv:2601.22025, n=50, Llama 3 8B / Qwen 2.5 7B) reports a generic "helpful assistant"
rule wrapper degrading extraction 100%→90% and RAG compliance 93.3%→80%, **and improving
instruction-following by 13%**. Instruction-following is the closest analogue to what §4.2's card
is for, so the honest reading is: **non-monotonic, dependent on the task contract, and untested at
drovr's model scale** — not "generic rules hurt". It argues for keeping the card short and
drovr-specific, and against claiming to know its net effect.

**Social proof** — superpowers' matrix prescribes it for discipline skills; the 2026 ranking does
not put it top-two. drovr adopts it only in its true form (naming a failure mode that has actually
been observed, e.g. from `docs/skill-evidence/`), never in the invented-frequency form superpowers
uses ("steps get skipped. Every time."), which exception 1 forbids.

**Transfer is not established, and that is why fix 5 exists.** The Meincke work measures a chat
model being talked into violating its safety training; drovr needs a coding agent to adhere to a
procedure it already agrees with, over many turns, under task load. The authors offer no transfer
analysis, and the behavioural-eval literature has a documented replication problem (Vaugrante,
Niepert & Hagendorff). None of these studies measure the thing drovr cares about. They set priors;
§7.3 and §7.4 settle them. Absolute compliance rates (33%→72%, 35%→51%) transfer not at all and
are never quoted as drovr numbers.

### 2.3 Voice

The **authority register** — imperative second person, `MUST`/`NEVER`/`No exceptions` — has no
published study isolating it for procedural adherence, and it is reachable by the probe, so it is
**tier 2: a measured arm in §7.4**, with superpowers' full register as the tier-3 fallback if the
measurement is inconclusive. Not settled by this section.

*Devices adopted* (superpowers convention unless a source, a measurement, or a tier is named):

| Device | Countering |
|---|---|
| Iron Law — one fenced, all-caps line | Case-by-case renegotiation; gives a short string to cite back |
| Spirit-vs-letter line, **before** the rules | "I'm honoring the intent differently" |
| Unity line — *the next phase agent is you, with your context gone* | Detachment from downstream cost. Prior in §2.2; **measured** in §7.4 |
| Announcement string; checklist→task binding | Silent skipping. **Evidence-backed** — commitment (§2.2) |
| Rationalization table (*thought → reality*, reality as instruction) | The specific excuses observed in RED |
| Red flags — the agent's own inner-monologue fragments | Mid-drift self-detection (expanding what already exists) |
| "No exceptions" bullets, each pairing prohibition with required action | Partial compliance, verb reinterpretation |
| Claim → required evidence → **not sufficient** table | Evidence substitution |
| Numeric escalation trigger (after 3 failed fixes, stop) | Thrash loops |
| ✅/❌ paired utterances | Ambiguity about what compliance sounds like |
| Top-loading binding content; restating near the point of use | Decay under load (§2.2 position effects) |

**Moral vocabulary — measured, not rejected.** An earlier reading treated the EmotionPrompt null
as tier-1 evidence against moral framing. It is not: EmotionPrompt appends emotional *stimuli*
("this is important to my career") to raise **benchmark accuracy** on 2023–24-vintage models. It
is not moral characterisation of a rule violation and not procedural adherence, so a null on a
self-declared analogue does not settle the construct — and routing it to tier 1 while routing
authority to tier 2 applied two standards to the same quality of evidence. Moral vocabulary is
therefore **tier 2: a measured arm in §7.4**, on the same footing as authority and unity, with
superpowers (which uses moral framing throughout) as the tier-3 fallback.

For the record, the EmotionPrompt figures stated precisely: the +115% headline was BIG-Bench-only
and came from selecting the best-performing stimulus per task; the corrected averages are **+4.42%
on BIG-Bench and +2.58% across all benchmarks**, and those are the replication authors'
re-analysis of Li et al.'s reported numbers — their own measurement found **no significant
effect** (χ²=0.11, p=.74).

**Flowcharts — adopted [tier 3].** No published evidence compares a flowchart against a numbered
list for agent compliance, and the variant is not reachable by the probe (it changes document
structure rather than a text register, so it cannot share the rubric). Tier 3 applies and
superpowers' choice stands: a convention-follow, not an evidence-backed choice.

**The human-facing case — a product judgement [tier 4], not a measurement.** A flowchart rendered
next to the plan in the review UI is a different artifact for a different audience: a person
orienting in a run in progress. superpowers has no analogue (it has no review UI), so no
convention governs it. The judgement: a decision graph is a better fit than prose for *where in a
procedure the agent currently is*, because that state is a position in a branching structure and
prose has to re-linearise it. No claim is made about how much faster anyone reads it — that would
be a fabricated comparative under exception 1.

**Scope call: half in, half out.** Authoring the fenced `dot` blocks inside the skill docs is in
scope — they are part of the documents this spec rewrites, and an agent reads the source whether
or not it renders. **Making the review UI render them, and pairing a graph with the plan as a
progress display, is review-UI work and is out of scope here** (§8). Recorded as a follow-up in
`docs/known-issues.md` under `## Resolved`-adjacent follow-ups — *render fenced `dot` blocks in
the review UI and show the phase/gate graph alongside the plan* — rather than built in this run.

Placement follows superpowers' rule, which drovr adopts as well: **flowcharts for genuine
branching and for loops where you might stop too early — never for linear instructions or
reference material.** Concretely: the router's decision flow (§4.1), the RED/GREEN/REFACTOR loop
(§7.1), and **`drovr:tdd`'s red-green-refactor cycle** — superpowers puts a fenced `dot` block on
exactly that cycle (`test-driven-development/SKILL.md:49`) and it is the canonical
stop-too-early loop, so withholding one from drovr's `tdd` would be an unmarked deviation.
`systematic-debugging`'s loop qualifies on the same grounds. The remaining discipline procedures
stay numbered lists, because they are linear.

Format: fenced `dot` blocks, matching superpowers, so the source stays legible to an agent
reading raw text whether or not anything renders it.

### 2.4 Length and the shipped budget test

> **The byte budget applies to teaching content. Binding content is deliberately redundant.**

Teaching content stays terse and gets cross-referenced, never restated. Binding content — the
prohibition the agent will try to talk itself out of — is stated 3–4 times per skill (Iron Law,
red flag, rationalization row, checklist item), because redundancy is the mechanism.

**There is already a shipped test enforcing this, and this spec breaks it.**
`cli/tests/skills_valid.rs:17` sets `const BODY_BUDGET: usize = 2200` bytes over exactly the four
methodology skills (`:20-25`); `using-drovr`, `handoff` and `pipeline` are deliberately unchecked.
Current bodies pass; §6's rewrite is 3–4× over.

- **The byte budget in `skills_valid.rs` is the single authoritative check.** `wc -l` / `wc -w`
  targets are guidance for the author, never a verification criterion — §9 does not assert them.
- **New values** [tier 4 — no superpowers analogue, and no evidence sets a correct length]:
  `BODY_BUDGET` for the four methodology skills rises **2200 → 12000 bytes**.
- **`using-drovr` gets its own cap and joins the checked list at 9000 bytes.** It is the most
  expensive document in the repo — injected in full on every `SessionStart` — and it is currently
  exempt by an accident of history (5087 B / 775 words / 93 lines today; §4.1 adds six blocks).
  Leaving the highest-cost file uncapped while capping the others is indefensible.
- Above cap, split into `references/`.

The caps themselves deviate from superpowers in both directions (its TDD is 371 lines / 1496
words; its own guidance says <200 words for frequently-loaded skills). That deviation is a tier-4
judgement, not a cited or measured one, and is marked as such rather than dressed up.

### 2.5 Skill docs are prompts, not replies

The user's global terseness preference governs assistant *replies*. It has no bearing on the
length of a document whose job is to survive a filling context window.

---

## 3. Fix 1 — un-scope the four methodology skills

An agent working inline reads "in a drovr phase" and correctly concludes the skill does not
apply. This is a defect, not a stylistic weakness.

Related trap fixed at the same time: **a `description:` must be a trigger, not a summary.** A
description that summarises the workflow creates a shortcut the agent takes *instead of* reading
the skill. `using-drovr:3` is currently a summary. Anthropic's skill-authoring guidance
independently identifies the trigger description as the highest-leverage line in a skill.

**Literal target `description:` strings** (all five; RED may tighten the wording, per the
four-part closure's description clause, but these are the shipped defaults):

| File | New `description:` |
|---|---|
| `skills/tdd/SKILL.md:3` | `Use when implementing any feature or bugfix, before writing implementation code — requires a test you have watched fail before any implementation exists; no production code without a red test first` |
| `skills/systematic-debugging/SKILL.md:3` | `Use when encountering any bug, test failure, or unexpected behavior, before proposing or writing a fix — requires a reproduction and a mechanistic root cause before any code change` |
| `skills/verification-before-completion/SKILL.md:3` | `Use when about to claim any work is done, fixed, or passing, before reporting, committing, or handing off — requires running the verification command in this message and reading its output; evidence before assertion, always` |
| `skills/code-review/SKILL.md:3` | `Use when you have written any change, before calling it done or handing it forward — requires read-only reviewer subagents run in the foreground, with every Critical and Important finding resolved or explicitly recorded as deferred` |
| `skills/using-drovr/SKILL.md:3` | `Use at the start of every session and before every response, including before clarifying questions and before reading any file — routes to the right drovr skill and requires invoking it whenever there is even a 1% chance one applies` |

**Body demotions:**

| File | Change |
|---|---|
| `skills/tdd/SKILL.md:17-19` | Phase framing demotes to one subordinate line: *"Inside a drovr phase this also binds the next phase's contract."* |
| `skills/systematic-debugging/SKILL.md:14-17` | Same demotion. The read-only-explorer rule is unconditional and stays. |
| `skills/verification-before-completion/SKILL.md:15-17,40-42` | `drovr phase done` demotes to conditional: *"If you are in a phase, this is also what gates `drovr phase done`."* |
| `skills/code-review/SKILL.md:13-14,41` | Same demotion for the pipeline-review-phase and report-done references. |

**Rule for all five:** the skill reads as unconditional discipline; every drovr-phase reference
becomes a clearly-marked *additional* consequence, never a precondition.

**`docs/known-issues.md`:** one entry, in the file's convention (`## <symptom title> — FIXED
<date>`, `**Status:**`, `**Severity:** medium`, `**Found:**`, `### Symptom` / `### Root cause` /
`### Fix`), recording that four descriptions scoped unconditional discipline to phases while
`using-drovr` makes inline the default. One entry only — the other fixes are design work, not
defects.

---

## 4. Fix 2 — make `using-drovr` a per-turn gate

### 4.1 Doc layer (`skills/using-drovr/SKILL.md`)

New content in this order — placement per §2.2's position effects. **Items 2 and 4 are tier-3
convention-follows**, structurally close to `using-superpowers/SKILL.md:11-29`; drovr's wording is
its own but the devices are ported wholesale.

1. `<SUBAGENT-STOP>` (keep, `:8-11`).
2. **The 1% rule** [tier 3], above the H1, inside the existing `<EXTREMELY_IMPORTANT>` framing:
   even a 1% chance a drovr skill applies means invoke it. Paired with the cost-lowering clause —
   if it turns out not to fit, drop it; invoking costs almost nothing.
3. **The per-turn rule**: check before any response, including before asking a clarifying question
   and before any read-only exploration.
4. **Instruction-priority ladder** [tier 3]: the human's explicit instructions > drovr skills >
   default behaviour. The safety counterweight to the MUST language; not optional.
5. **Gate function** — a fenced `dot` flowchart (it branches, so §2.3's placement rule
   applies): message received → does any skill apply (≥1%) → invoke → announce → does it have a
   checklist → create one tracked item per step → follow it → only then respond.
6. **Red-flag table for the router's own failure mode** — invoking nothing at all, which drovr has
   zero coverage of. Candidate rows: *"this is a one-line change"*, *"I'll just look at the file
   first"*, *"they asked a question, not for work"*, *"I'm already mid-task, the router was for
   turn one"*, *"escalating to a phase would be overkill so no skill applies"*. **Final wording
   comes from RED (§7), not from this list.**
7. Existing sections (single-writer, always-review, methodology, escalation) retained.

**Items 2–5 sit outside every `<!-- reflex:section:NAME -->` marker** [tier 4 — superpowers has no
sectioned-reflex analogue]. `[reflex.sections]` can subtract advisory sections but cannot silently
delete the routing core; only `[reflex] enabled = false` removes it. A half-disabled router
produces an agent that believes it is running drovr and is not; an explicit master-switch off is
an informed choice.

### 4.2 Hook layer (new `UserPromptSubmit` hook)

Every decision in this subsection is **[tier 4]**: superpowers is `SessionStart`-only, so there is
no convention to follow, and no study covers per-turn re-injection. It is drovr's most novel
mechanism and it ships as **an explicit unmeasured bet**, recorded as such in
`docs/skill-evidence/per-turn-gate.md`.

- `hooks/hooks.json` gains a `UserPromptSubmit` entry running a thin `hooks/user-prompt` script
  that `exec`s `drovr reflex --gate`. **`UserPromptSubmit` takes no `matcher`** — unlike the
  existing `SessionStart` entry, whose `startup|clear|compact` matcher must not be copied across.
- **Cost is cumulative, not a rate.** `additionalContext` is appended every turn and *stays* in
  the window. The budget is therefore stated both ways: **≤600 bytes per injection, and a
  cumulative ceiling of ~60 KB over a 100-turn session** — real weight in the window drovr exists
  to keep tight.
- **Suppression rule, to stop the cumulative cost being unbounded:** the card is emitted only when
  no `drovr:*` skill was invoked in the previous turn. A session already running the discipline
  does not need re-telling; a session that has drifted does. This bounds the common case to a
  handful of injections.
- **Byte budget, not tokens.** The CLI has no tokenizer and length ≠ tokens. The assertion is
  **rendered `additionalContext` ≤ 600 bytes**, checked in `cli/src/reflex.rs`'s test module.
- **Card content** — the 1% rule, the per-turn check, the announcement string, the
  checklist-binding line, a `<SUBAGENT-STOP>` line (see below), and a pointer to
  `Skill drovr:using-drovr`.
- **Three API facts that must be handled** (all verified against source):
  1. `reflex.rs:143-152`'s `envelope()` hardcodes `"hookEventName": "SessionStart"`. It must be
     parameterized to take the event name.
  2. `main.rs:114-118` declares `Reflex { skill: PathBuf }` as **required**, and `main.rs:1276-1278`
     asserts that bare `drovr reflex` errors. Adding `--gate` forces `Option<PathBuf>`, a clap
     arg-group rule (`--gate` xor `--skill`), and an update to that test.
  3. **Card text source:** a `const` in `reflex.rs`, *not* extraction from `SKILL.md`. Extraction
     would need markers inside the region §4.1 deliberately places outside all `reflex:section`
     markers. The cost is drift between card and skill, mitigated by a test asserting the card's
     key phrases appear in `using-drovr/SKILL.md`.
- **Subagents.** `using-drovr:8-11` already carries `<SUBAGENT-STOP>` for exactly this case, and
  §7.3/§7.4's foreground probe subagents plus drovr's own read-only reviewers all launch from a
  gate-on session. If `UserPromptSubmit` fires for Agent-tool subagents in this harness, the card
  would inject into every one of them and contaminate the probes. **The card therefore carries its
  own subagent-stop line unconditionally**, and step 1 of §0 verifies empirically whether the hook
  fires for subagents, recording the answer in `docs/skill-evidence/per-turn-gate.md`.
- Config: new `[reflex] per_turn` bool in `ReflexConfig` (`cli/src/config.rs:39-64`),
  **default true** [tier 4], suppressible per-user. Uses a **named default fn**, not bare
  `#[serde(default)]` — the trap already documented at `config.rs:108-110`.
- **Asymmetric suppression, deliberately.** The full `SessionStart` reflex stays suppressed inside
  phases (`DROVR_PHASE` set) because a phase agent runs on its injected briefing. The per-turn
  gate does **not** suppress in phases — a phase is exactly where the discipline must hold.
- `README.md:70-94` updated with the new key.

### 4.3 Out of scope

`PreToolUse` could hard-deny an `Edit` that skipped `drovr:tdd`, or a `git commit` with no
verification evidence. That is enforcement rather than persuasion, a larger design with real
false-positive risk. Not this run.

---

## 5. Fix 3 — bind checklists to tracked task state

Stated harness-agnostically, because harnesses differ: some sessions expose
`TaskCreate`/`TaskUpdate`/`TaskList` and no `TodoWrite`, while stable Claude Code exposes
`TodoWrite`.

> When a skill or briefing gives you a numbered checklist, create **one tracked item per step**
> using whatever task tool this harness exposes — `TodoWrite`, or `TaskCreate`/`TaskUpdate` —
> before you start step 1. Mark each in-progress when you start it and complete when its evidence
> is in hand. If the harness exposes no task tool, write the checklist to
> `~/.local/share/drovr/runs/<run>/checklist.md` when inside a run, or `CHECKLIST.md` at the repo
> root otherwise, and tick items there. An untracked checklist decays with the context window;
> that decay is the exact failure drovr exists to fight.

Applied at four sites:

1. `skills/using-drovr/SKILL.md` — the gate function's checklist branch (§4.1 step 5).
2. Each discipline skill's numbered procedure gets a one-line binding directive above it.
3. `skills/pipeline/phase-prompts/*.md` — the `## Do` lists (`implement-task.md:14-64`, 7 steps;
   and the equivalents in `brainstorm.md`, `plan.md`, `review.md`, `review-angle.md`) gain the
   directive as step 0. These are agent-assembled markdown, not compiled into the binary, so this
   is a pure text change.
4. `skills/handoff/SKILL.md` and `skills/handoff/HANDOFF-template.md` — the 7-section handoff is a
   checklist and gets the same treatment.

No CLI change required.

---

## 6. Fix 4 — move the armor onto the discipline skills

`tdd`, `systematic-debugging`, `verification-before-completion` and `code-review` are
restructured to a fixed section order [tier 3 — the order is superpowers' convention, adopted
wholesale].

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

**Section 5's announcement sentences** — template `Using drovr:<skill> to <purpose>.`, and the
four shipped strings:

- `tdd` — *"Using drovr:tdd — writing the failing test before the implementation."*
- `systematic-debugging` — *"Using drovr:systematic-debugging — reproducing before fixing."*
- `verification-before-completion` — *"Using drovr:verification-before-completion — running the
  checks before claiming done."*
- `code-review` — *"Using drovr:code-review — dispatching read-only reviewers before calling this
  done."*

**Placement rationale, stated honestly.** Sections 2–5 sit at the top because §2.2's position
effects favour the start of a document. Sections 8–9 sit late for a *different* reason —
**proximity to the point of temptation**, which is a tier-4 judgement, not the U-shape finding.
The procedure (6) sits mid-document, the weakest position, deliberately: it is the section the
agent is actively executing and re-reading while it works, so it is least dependent on recall from
a single pass. Do not cite the position finding for sections 6–9.

Per-skill specifics:

- **`tdd`** — Iron Law: no implementation code before a test you have watched fail. Loophole
  closures: "I'll keep the code as reference", "the test is obvious so I'll write it after",
  "it's a refactor so TDD doesn't apply", "the harness makes it hard to run one test".
- **`systematic-debugging`** — Iron Law: no fix before a reproduction and a mechanistic cause.
  Adds the **numeric escalation trigger**: after 3 failed fixes, stop and question the design; do
  not attempt fix #4 without that conversation.
- **`verification-before-completion`** — Iron Law: no completion claim without fresh evidence
  produced in this message. Requirements table covers tests / build / linter / bug-fixed /
  subagent-reported-success. Catch-all red flag for any wording implying success.
- **`code-review`** — Iron Law: no change is done until a read-only reviewer has looked at it and
  every Critical/Important finding is resolved or explicitly recorded as deferred. Loophole
  closures: "the change is too small", "I already reviewed it myself", "the pipeline's review
  phase will catch it". The existing FOREGROUND rule is promoted into the no-exceptions list —
  backgrounding a reviewer is the known way this skill silently fails
  (`code-review/SKILL.md:19-23`).

Expected sizes: 39–51 lines → roughly 120–180 lines each; the binding constraint is §2.4's
12000-byte `BODY_BUDGET`, not the line count.

---

## 7. Fix 5 — the meta-skill and the empirical loop

### 7.1 New skill: `drovr:writing-skills`

`skills/writing-skills/SKILL.md`, in drovr's voice. The pass criteria, the four-part closure and
the scenario-construction rules below are **[tier 3] convention-follows**, ported from
`testing-skills-with-subagents.md:182-275` in drovr's own wording.

- **The mapping** that gives it authority for free: pressure scenario ↔ test case; `SKILL.md` ↔
  production code; RED ↔ the agent violates the rule *without* the skill; GREEN ↔ it complies
  *with* the skill; REFACTOR ↔ close each new loophole.
- **The loop**, as a fenced `dot` flowchart (a stop-too-early loop, per §2.3): build the scenario set →
  run the baseline → transcribe every excuse **verbatim** → write the minimal counter-text →
  re-run on held-out scenarios → apply the four-part closure to each new rationalization →
  repeat **until a run produces no new rationalization or the §7.3 REFACTOR ceiling is reached,
  whichever comes first.** The ceiling is not optional: an uncapped "repeat until clean" loop is
  the same unbounded-cost defect this spec exists to fix elsewhere.
- **The four-part closure** (all four every time, never one): explicit negation inside the rule; a
  row in the rationalization table; a bullet in red flags; and a `description:` update adding the
  *symptom of being about to violate*.
- **Scenario construction rules**: real file paths, concrete numbers and deadlines, a forced
  A/B/C choice, ask "what do you do" not "what should you do", no escape hatch to "I'd ask the
  human", and ≥3 combined pressure types (time, sunk cost, authority, economic, exhaustion,
  social, pragmatic) — agents resist single pressures and break under combined ones.
- **Pass criteria** (all four): the correct option under maximum pressure; the agent **cites a
  specific section**; it names the temptation and complies anyway; and the meta-test ("how should
  this have been written?") returns "it was clear". Not-bulletproof signals: new rationalizations,
  the agent arguing the skill is wrong, invented hybrid approaches, or asking permission while
  arguing hard for the violation.
- **drovr-specific constraints**: scenario subagents run in the **FOREGROUND**; the author is the
  single writer; and §2.1's no-fabricated-measurements rule.
- Reference files: `references/pressure-scenarios.md` and `references/testing-with-subagents.md`,
  keeping `SKILL.md` under §2.4's cap.

Anthropic's own skill-authoring guidance prescribes the same loop — build evaluations first,
establish a baseline without the skill, write minimal instructions, iterate — so this is
convergent, not merely borrowed.

### 7.2 Evidence corpus

- `skills/writing-skills/scenarios/<skill>-<n>.md` — checked-in scenario prompts, each tagged
  `dev` or `holdout` (§7.3). They are tests; they live with the skill.
- `docs/skill-evidence/<skill>.md` — per skill: scenarios used, **verbatim** baseline
  rationalizations, the text written to counter each, and the re-test result with dates.
- `docs/skill-evidence/voice.md` — the §7.4 probe.
- `docs/skill-evidence/per-turn-gate.md` — §4.2's unmeasured-bet record and the subagent-firing
  answer.
- Raw transcripts under `docs/skill-evidence/transcripts/`. This is the corpus drovr skill text
  cites whenever it makes a numeric claim (§2.1 exception 1).

### 7.3 The acceptance test — runs in this run

This answers *"how do we know stickiness improved rather than that the docs got longer?"*

**Held-out design.** Authoring arm B from the same scenarios that grade it would fit the text to
the test and make the pass bar unfailable. So, per skill, **3 scenarios: 1 development, 2
held-out.** RED transcription and all counter-text authoring use the **development** scenario
only. The pass bar is pre-registered and scored on the **held-out** pair, which is never read
while writing arm B.

**Arms** — three, so fix 1 and fix 4 are separable:

| Arm | Text |
|---|---|
| **A** | Current `SKILL.md` at the pre-fix `HEAD` (captured in §0 step 2) |
| **A′** | Fix-1-only: descriptions un-scoped, no armor |
| **B** | Full rewrite (fixes 1 + 3 + 4) |

Arm A′ exists because arm A's descriptions are phase-scoped — the defect §3 fixes. Without A′, a
non-phase-framed scenario handicaps A for reasons unrelated to the armor and B > A proves nothing
about fix 4. **A′ isolates the armor's contribution.**

**Budget table.** Every line has a hard ceiling. When a ceiling is hit, work **halts and records a
null** in `docs/skill-evidence/` — it does not silently extend.

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

`using-drovr`'s extra scenario class checks the router's own failure mode — invoking nothing —
without inducing the opposite failure of invoking everything reflexively. It is budgeted, not
free.

**Who runs it, and where** — 122 foreground subagent runs with verbatim transcription will not fit
in one implement-task context, which is the exact failure drovr exists to prevent. Decomposition:
**one dedicated phase per skill**, `ab-<skill>` (5 phases), each running its own stages, writing
`docs/skill-evidence/<skill>.md` plus raw transcripts, and exiting. The voice probe is a sixth
phase, `ab-voice`. The driver aggregates. No phase holds more than ~25 runs.

**Scoring** — pre-registered, and not by the author of arm B:

- **Rubric**: binary compliant / non-compliant on the scenario's forced choice, plus the four §7.1
  pass criteria recorded separately as booleans.
- **Scorer**: a read-only reviewer subagent, given the transcript and the rubric only.
- **Blinding**: arm labels stripped and transcripts shuffled before scoring; the mapping is
  restored only after all scores are recorded. Unblinded self-scoring by arm B's author is exactly
  what the replication literature this spec cites warns about.
- **Model**: `sonnet` for probe subagents and for the scorer.

**Pre-registered bars**, written before arm B runs:

- **Arm A bar** (defined, not just B's): a skill "already passes" if A is compliant on **≥3 of its
  4 held-out runs** — 2 held-out scenarios × 2 samples.
- **Arm B bar**: compliant on **≥3 of its 4 held-out runs for that skill**, AND strictly more
  compliant runs than both A and A′.
- **Consequence of failure — not merely documentation.** A skill whose B fails after its REFACTOR
  ceiling **does not ship its armor**: it reverts to arm A′ (fix-1-only), and
  `docs/skill-evidence/<skill>.md` records the failure and the reverted state. Fix 1 is a defect
  repair and ships regardless; fix 4 must earn its bytes.
- **Falsifiable.** If arm A already passes for a skill, that skill's rewrite is not justified and
  reverts to A′. This is the guard against length-for-its-own-sake.
- If A′ ≈ B for a skill, the armor is not carrying its weight there and that skill reverts to A′
  even if B passes its own bar.

Null and negative results are recorded alongside positive ones.

### 7.4 Voice as a measured variable

§2.1 tier 2: three register questions have no published study isolating them for procedural
adherence, and all three are reachable as text variants of one probe skill.

**Probe skill: `verification-before-completion`.** Sharpest binary outcome (did the agent claim
done without running the command), shortest scenarios. **Model: `sonnet`**, matching §7.3 — note
this is *not* the Opus-class model drovr sessions run on, which is a stated limitation alongside
the §2.2 extrapolation.

Four variants, identical in structure — same Iron Law, same procedure, same tables, same placement
— each adding exactly **one** register device to the baseline, so each factor is separable at this
sample size:

| Variant | Register |
|---|---|
| **V0** | Baseline: plain imperative; no all-caps, no absolutist "no exceptions"; operational consequence only |
| **V1** | V0 + **unity** line |
| **V2** | V0 + **full authority** register (`MUST`/`NEVER`, all-caps Iron Law, no-exceptions bullets) |
| **V3** | V0 + **moral** framing, in superpowers' register |

4 variants × 2 scenarios × 3 samples = **24 runs**, 6 per variant.

**Pre-registered decision rule.** Each variant is compared against V0 on 6 runs per side.
**Separation margin: ≥3 of 6** — set to match the stated power rather than below it; a 2-of-12
margin on binary outcomes is roughly one standard deviation and would fire on noise.

1. **Variant beats V0 by ≥3** → that device ships across all five documents.
2. **V0 beats the variant by ≥3** → that device is **dropped**, and the baseline register ships.
   This branch is explicit: a plain-register win is a real outcome, not an undefined one.
3. **No separation ≥3 (the likeliest outcome)** → **tier 3: follow superpowers.** Authority ships
   (superpowers uses it), moral framing ships (superpowers uses it throughout), and unity — which
   superpowers *withholds* from discipline skills — ships anyway on the strength of the 2026
   published ranking (§2.2 tier 1 outranks tier 3). Each is recorded in
   `docs/skill-evidence/voice.md` as convention-or-prior, with the null attached.
4. Whatever wins applies to the other four documents **without re-testing each**. Stated
   limitation: measured on one skill, one model, generalised to five documents.

**Unity's standing, stated plainly.** Unity is adopted on a published prior and its arm here is
**reported, not decisive** — 6 runs cannot overturn an N=126,000 result, and this spec does not
pretend otherwise. What the arm can do is show a large local effect if one exists. No directional
escape hatch: if V1 loses to V0 by ≥3, that is recorded as a genuine conflict between the prior
and drovr's own data, and the decision escalates to the human rather than being auto-resolved.

**Power note.** n=6 per variant detects only large effects, and §2.2 expects register effects to be
small on frontier models — so outcome 3 is the single likeliest result. That is informative, not a
failure. Nothing stronger than "suggestive" is recorded.

---

## 8. Scope boundaries

**In scope:**

- Skill docs: all 8 `skills/*/SKILL.md`. Five get `description:` changes (§3); the four discipline
  skills get the §6 rewrite; `handoff`, `pipeline` and `worktrees` receive **only** the fix-3
  task-binding directive and nothing else.
- `skills/handoff/HANDOFF-template.md`; the 5 files under `skills/pipeline/phase-prompts/`.
- New: the `skills/writing-skills/` tree (SKILL.md, `references/`, `scenarios/`);
  `docs/skill-evidence/`.
- One `docs/known-issues.md` entry, plus the review-UI flowchart follow-up note (§2.3).
- Hooks: `hooks/hooks.json`, new `hooks/user-prompt`.
- Rust: `cli/src/reflex.rs` (`--gate`, `envelope()` parameterization), `cli/src/config.rs`
  (`per_turn`), `cli/src/main.rs` (arg-group wiring).
- **Tests** (previously unlisted and load-bearing): `cli/tests/skills_valid.rs` (`BODY_BUDGET`
  2200 → 12000, add `using-drovr` at 9000 — §2.4), `cli/tests/reflex_hook.rs` (per-turn hook
  cases), and `main.rs:1276-1278`'s bare-`drovr reflex` assertion.
- `README.md:70-94`.

**Out of scope:** rewriting `handoff` / `pipeline` / `worktrees` as documents; any `PreToolUse`
enforcement; any change to the phase state machine, the review gate, or `drovr code-review`;
rendering `dot` blocks in the review UI (§2.3 follow-up).

**Deployment reality.** These skills ship via the nix flake. Nothing here takes effect in the
running session until the flake pin is bumped; the probe harness therefore feeds skill text to
subagents explicitly rather than relying on ambient injection.

---

## 9. Verification

1. **Mechanical**, each expressed as a runnable check:
   - Every discipline `SKILL.md` contains the 10 required sections of §6, plus section 7 where §6
     names it (`verification-before-completion`, `code-review`) and 6b where §6 names it (`tdd`,
     `systematic-debugging`). Not "all 11 on every skill" — that is unsatisfiable by construction.
   - `cargo test --test skills_valid` passes at the new budgets (§2.4). **This is the authoritative
     size check**; `wc -l`/`wc -w` are not asserted.
   - `grep -L -E 'in a drovr phase|a drovr task|a drovr phase has produced' skills/*/SKILL.md`
     returns all 8 files — i.e. none of those literals survives anywhere.
   - Verbatim-overlap check against the corpus at
     `/home/sauyon/.claude/plugins/cache/claude-plugins-official/superpowers/5.1.0/skills/`:
     no ≥8-word shingle shared with any drovr `SKILL.md`. Any hit is either reworded or gets the
     MIT attribution required by §2.1 exception 2.
2. **Unit tests:** rendered `--gate` `additionalContext` ≤ 600 bytes; `per_turn` defaults to true
   with a `[reflex]` table present but the key absent (the `config.rs:108-110` trap); the
   `UserPromptSubmit` hook emits valid hook JSON with the correct `hookEventName` and respects
   `enabled = false`; the routing core survives `[reflex.sections]` subtraction; the card's key
   phrases still appear in `using-drovr/SKILL.md` (drift guard, §4.2).
3. **Empirical:** §7.3's held-out A/A′/B and §7.4's voice probe, ≤122 runs, results — including
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
  100%→90%, RAG 93.3%→80%, instruction-following +13%. —
  https://arxiv.org/html/2601.22025v2
- Anthropic. *Prompting best practices* (current models incl. Opus 5). "Tell Claude what to do
  instead of what not to do"; providing "context or motivation behind your instructions" improves
  targeting. —
  https://platform.claude.com/docs/en/build-with-claude/prompt-engineering/be-clear-and-direct
- Anthropic. *Agent Skills — skill authoring best practices.* Prescribes the eval-first loop and
  treats the trigger description as the highest-leverage line. —
  https://platform.claude.com/docs/en/agents-and-tools/agent-skills/best-practices
- Load-under-task-degradation figures (−2–21% formatting compliance, recovered to 90–100%) are
  from **secondary summaries not read in full**; labelled as such at point of use in §2.2 and not
  load-bearing for any decision.
- superpowers, read first-hand: `skills/writing-skills/persuasion-principles.md` (cites Cialdini
  2021 and the 2025 Meincke figure only), `testing-skills-with-subagents.md`,
  `anthropic-best-practices.md`, `test-driven-development/SKILL.md`. MIT, © 2025 Jesse Vincent.
