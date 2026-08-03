# The per-turn gate — an explicit unmeasured bet

**Status: `[tier 4]` throughout.** superpowers is `SessionStart`-only, so there is no convention to
follow, and no published study covers per-turn re-injection of an instruction card. This is drovr's
most novel mechanism and it ships as **engineering judgement, not evidence** — every number below is
either something drovr measured on this machine (and says so, with the method) or a budget drovr
chose. Nothing here is a finding about whether the gate makes agents behave better. **That question
is not measured and is not claimed.**

Implementation: `cli/src/reflex.rs` (`GATE_CARD`, `gate_json`, `skill_invoked_last_turn`),
`cli/src/config.rs` (`ReflexConfig::per_turn`), `hooks/user-prompt`, `hooks/hooks.json`.

---

## What it is

A `UserPromptSubmit` hook injects a small card before every user turn:

```
<SUBAGENT-STOP>Dispatched as a subagent for one task? Ignore this card — do your task.</SUBAGENT-STOP>
DROVR GATE — before every response, including clarifying questions and read-only exploration:
1% rule: even a 1% chance a drovr:* skill applies → invoke it. Wrong fit? Drop it; invoking costs almost nothing.
Announce it: "Using drovr:<skill> — <purpose>."
Checklist in the skill → one tracked task per step, followed before you respond.
Single writer: one agent edits; reviews go to drovr:code-review. Unsure? Skill drovr:using-drovr.
```

The bet: a `SessionStart` injection is one prompt at the top of a session and scrolls out from under
the agent as the context fills, so the discipline has to be re-reachable at turn 200. The cost of
being wrong is a few hundred bytes per turn of noise the agent learns to skim past.

## Cost, stated both ways

**Per injection: 547 bytes** of rendered `additionalContext`, against a chosen budget of **≤600**.
(656 bytes for the whole pretty-printed JSON envelope, 657 with the trailing newline; the budget and
the test are on `additionalContext`, which is what enters the context window.) Measured by running
the built binary and counting the bytes it actually wrote.

**What is pinned is the ≤600 budget, not the 547.** `cli/src/reflex.rs::gate_card_within_600_bytes`
and `cli/tests/reflex_hook.rs::user_prompt_hook_emits_gate_json` both assert `<= 600` and nothing
tighter, so a card edit landing anywhere between 548 and 600 bytes leaves the suite green and
silently invalidates the 547, the 656, and the ~55 KB below. Re-measure them when the card changes;
do not trust them because the tests are green.

**Cumulative, not a rate.** This is the figure that matters and the one easy to state wrongly.
`additionalContext` is appended to the conversation and *stays* there. A 100-turn session in which
every turn emitted would carry **~55 KB** of card by the end (spec §4.2 budgets the ceiling at
~60 KB). That is real weight in exactly the window drovr exists to keep tight, which is why the
suppression rule below is part of the mechanism rather than an optimisation.

**The byte budget is bytes, not tokens.** The CLI has no tokenizer and length ≠ tokens. Every figure
here is bytes; no token count is claimed.

## The suppression rule, and its direction

The card is emitted **only when the previous turn did not already invoke a `drovr:*` skill**. A
session demonstrably running the discipline does not need re-telling; a drifted one does. This is
what bounds the cumulative cost in the common case.

**The rule fails OPEN — toward emitting.** The gate's only inputs are evidence that the card is
*unnecessary*, and absent evidence is not evidence of absence. Everything below emits:

- no stdin payload, unparseable stdin, or no `transcript_path` in it;
- an absent, unreadable, or non-regular transcript file;
- a transcript line that will not parse (it might have been this turn's user message);
- a `config.toml` that will not load at all (warn on stderr, run on reflex defaults) — note this is
  the *opposite* of the `SessionStart` reflex, which exits 1, because that one injects a whole skill
  body and a half-read config would frame it wrongly;
- any transcript shape the scan does not recognise.

**Two things silence it BY DESIGN:** an explicit `enabled = false` or `per_turn = false` in a config
that loaded successfully, and a previous turn that demonstrably invoked a `drovr:*` skill *and* whose
`tool_result` says the call succeeded. Those are the only two `None` conditions in `gate_json`.

**A third way produces no card without being a decision:** the hook cannot run at all — `drovr` not
on `PATH`, a bad `$DROVR_BIN`, the script not executable. `exec` makes that exit non-zero, so it is
loud rather than silent, but the turn is still cardless. "Fails loudly" is a claim about the exit
code, not about the card.

A wrong emit costs 547 bytes; a wrong suppression is silent drift, which is the failure drovr exists
to prevent. That asymmetry is the whole justification, and it is the property to preserve in any
future edit. **Stated precisely, because the absolute version is false:** the emit/suppress *logic*
resolves every ambiguity toward emitting, so no change to that logic can make the gate quieter by
accident. A change to the transcript *schema* can. If Claude Code ever tagged real user prompts
`isMeta` with a `sourceToolUseID`, or recorded a prompt as a content array of only `tool_result`
blocks, the backward walk would step past this turn's prompt, reach an earlier turn's successful
skill call, and suppress. That is exactly why "What is NOT known" item 4 exists.

**The transcript read is bounded to a 1 MiB tail** (`TRANSCRIPT_TAIL_BYTES`), because live
transcripts reach 29 MB and this runs before every prompt. Consequence — **inherited from task 4's
measurement, recorded in `~/.local/share/drovr/runs/skill-stickiness/implement-task-4-HANDOFF.md`,
and not re-measured here:** of 4,470 real turns on that machine, **27 (0.6%) have a turn longer than
the window**, largest 5.1 MiB. Those emit a redundant card rather than suppressing wrongly — the
fail-open direction again. Widening the window would cost I/O on every prompt to remove 0.6% of
redundant cards.

## Asymmetric suppression: the gate does NOT no-op inside a drovr phase

`hooks/session-start` exits early when `DROVR_PHASE` is set, because a phase agent runs on its
injected briefing. `hooks/user-prompt` deliberately does not: **a phase is exactly where the
discipline has to hold**, and a phase agent's briefing scrolls out of the window like anything else.

The CLI does not consult `DROVR_PHASE` at all, so `hooks/user-prompt` is the only place this can be
got wrong. It is pinned by
`cli/tests/reflex_hook.rs::user_prompt_hook_not_suppressed_in_phase`, whose sibling
`suppressed_when_drovr_phase_set` pins the opposite behaviour for the other hook.

## Measured: the hook actually injects, and injects every turn

The integration tests drive `hooks/user-prompt` under `bash` directly. That proves the script and the
CLI agree; it does not prove Claude Code delivers the result. So the wired hook was run by the real
harness: `hooks/user-prompt` registered as the only `UserPromptSubmit` entry via `claude -p
--settings`, `DROVR_BIN` pointed at the freshly built binary (hash checked — `cargo test` does not
rebuild it, and validating against a stale binary has already cost this run a false result once).

- **Turn 1** → the session transcript gained exactly one record containing the card, as an
  `attachment` of type `hook_additional_context`. The card reaches the model.
- **Turn 2** (`--resume` on the same session, no `drovr:*` skill invoked in between) → **two**
  records. The gate is per-turn in the real harness, not once-per-session, and the no-skill-last-turn
  path emits as designed.

## Measured: does `UserPromptSubmit` fire for Agent-tool subagents?

**No.** Claude Code 2.1.220 on this machine fires `UserPromptSubmit` once per *session* user prompt
and not at all for subagents dispatched via the `Agent` tool.

**Method.** A probe hook script that appends one line per firing (recording the full stdin payload)
and emits nothing was registered as the only `UserPromptSubmit` entry via `claude -p --settings`, in
a scratch directory outside this repo. Two headless sessions were run, each with a single user
prompt instructing the agent to dispatch foreground `general-purpose` subagents and report back:

| run | subagents requested | subagents actually dispatched | hook firings |
|---|---|---|---|
| 1 | 1 | 1 | **1** |
| 2 | 2 (one message, concurrent) | 2 | **1** |

The dispatch count is not taken from the agent's word for it: each run's transcript was checked for
`tool_use` blocks named `Agent`, and Claude Code wrote one
`<session>/subagents/agent-<id>.jsonl` per subagent — **3 subagent transcripts across the two runs**,
against **2 total hook firings**, both carrying the *main* session's `transcript_path`. The single
firing per run is also the probe's positive control: the hook was correctly registered and did fire,
so "0 subagent firings" is a measurement and not a silent misconfiguration.

**Scope of the claim.** Claude Code 2.1.220, Linux, `claude -p` headless, `Agent` tool with
`subagent_type: general-purpose`. It does **not** cover other harnesses, other subagent mechanisms,
or future versions. Re-run the probe before relying on it elsewhere.

**This does not make the card's `<SUBAGENT-STOP>` line conditional.** It ships unconditionally, for
two reasons that outlive the measurement: the answer is a harness behaviour drovr does not own and
cannot pin, and §7.3/§7.4's probe subagents plus drovr's own read-only reviewers all launch from a
gate-on session — a card leaking into one of them would contaminate the very measurements this run
depends on. The line costs 105 of the 547 bytes (104 plus its newline) and buys insurance against a
harness change that would otherwise be invisible.

## What is NOT known

1. **Whether the gate works.** No measurement here says the card changes agent behaviour. Its
   *presence* is machine-checked (all six §4.2 content items, the ≤600 budget, every fail-open and
   both by-design silencing paths); its *effect* is not, and §4.2 says so outright.
2. **Whether the wording is the right wording.** The card is a fixed `const` in `reflex.rs` rather
   than an extract of `using-drovr/SKILL.md`, so card and skill can drift. The mitigation is a
   two-sided phrase test (`gate_card_phrases_present_in_router_skill`) and it is **thin in two
   ways**: `GATE_CARD_PHRASES` is three strings (`<SUBAGENT-STOP>`, `Single writer`,
   `drovr:code-review`), so none of the card's *novel* content — the 1% rule, the announcement
   string, the checklist-binding line — is guarded at all until the task that writes those phrases
   into the router adds them; and the test cannot defend its own assertions, since deleting one side
   of the two-sided check leaves the suite green.
3. **Whether 547 bytes × N turns is the right trade.** The suppression rule bounds it in the common
   case, but no one has measured how often a real session actually drifts.
4. **The transcript JSONL schema is not drovr's to own.** The suppression scan is written against a
   shape verified on live transcripts *today*; no contract covers it. The mitigation is direction,
   not detection (see above).

## How to withdraw the bet

`per_turn = false` under `[reflex]` in `config.toml` disables the gate per-user and leaves the
`SessionStart` reflex untouched. `enabled = false` disables both. If the bet is judged lost, deleting
the `UserPromptSubmit` entry from `hooks/hooks.json` and `hooks/user-prompt` reverts the mechanism
entirely; nothing else depends on it.
