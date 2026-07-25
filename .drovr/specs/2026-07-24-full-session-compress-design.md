# Self-authored handoff — the finishing agent compresses in-context

**Status:** Draft (rev 3, PIVOT) — awaiting review · **Branch:** TBD

> Supersedes rev 1–2 (a herdr `agent.transcript` API). Those solved "let an external compressor
> see the whole session." This pivot removes the external compressor, so that problem — and all
> the infra it needed — disappears.

## Problem

`drovr phase compress` spawns a **fresh** one-shot `claude -p` to read the finished phase's
transcript and write `<phase>-HANDOFF.md`. But a fresh reader has no access to the session except
`herdr agent read`, which returns only a **terminal snapshot** (herdr's sources are all
pane-scoped: `visible|recent|…`). So the compressor structurally can't see the whole phase, and
the handoff silently drops whatever scrolled off. Rescuing the fresh reader means giving it the
full transcript — a new herdr API + a cross-repo deploy (rev 1–2). Heavy, and it still leaves the
fresh reader with **no salience signal** — just raw text.

## The knot

`handoff SKILL.md` justifies the fresh reader with exactly one reason — **context rot**: a
summary written from a full end-of-phase context degrades, so a fresh reader is better. But the
fresh reader is **blind** (snapshot-only) unless we build the transcript infra. The two rationales
pull opposite ways:

- context rot → *don't* let the full-context agent compress
- full-session visibility → the fresh reader *can't see* the session without heavy infra

**A neutral reader of an incomplete record is worse than a context-aware author of the complete
one.** Cut the knot: the finishing agent already holds the entire session **and** knows what's
load-bearing. Let it author the handoff.

## Decision (reverses the locked "never self-summary")

The **finishing phase agent authors `<phase>-HANDOFF.md` itself, in-context, as its final action**,
following the existing 7-section compress discipline, then drops its `drovr phase done` marker.
No external compressor, no transcript read, no herdr change.

This explicitly reverses `handoff SKILL.md`'s "the finishing agent does NOT summarize." The
recorded fear (context rot) is accepted and mitigated — see Risks.

## Why this wins

- **Full session, natively.** The agent has the whole phase in context — the original bug is gone
  for free.
- **Salience.** The agent knows which decisions/failures are load-bearing; raw text doesn't.
- **Deletes infra.** No herdr `agent.transcript`, no per-agent integration work, no cross-repo
  deploy, no `claude -p` compressor subprocess, no transcript-read-from-pane.
- **Git is the backstop** (already a design pillar): the handoff cites commits and points at
  artifacts, so the next phase **re-derives from source** rather than trusting the summary. The
  handoff is an *index into re-derivable truth, not the sole truth* — which slashes the cost of
  imperfect compression, and helps a context-aware author far more than a blind reader.

## Mechanism

**What the agent does** (its final two actions, per the completion contract):
1. Write `~/.local/share/drovr/runs/<run>/<phase>-HANDOFF.md` — the 7-section template
   (Objective · State · Decisions+rationale · Interfaces/contracts · Open questions · Next step ·
   Artifact pointers), authored from its own context, **git pointers mandatory**.
2. Run `drovr phase done <run> <phase>`.

**Where the discipline comes from.** `compress-prompt.md` stops being a `claude -p` system prompt
and becomes **agent-facing authoring instructions**, delivered via the `drovr:handoff` skill /
the phase prompt's completion contract. Same 7 sections, same "compress hard, never drop a
decision, preserve exact identifiers, require git references" rules — now addressed to the agent
itself.

**Enforcement (replaces the orchestrator compress step).** The orchestrator no longer runs
compress. Instead `drovr collect` (and/or `phase wait` on marker) **verifies the handoff exists
and is non-empty** before the phase is accepted; if missing, the driver re-prompts the agent to
write it. This keeps a machine gate without a separate reader.

## The drovr change (this repo)

- **`skills/handoff/SKILL.md`** — rewrite the "load-bearing move" paragraph (self-author +
  discipline + git backstop, not "never self-summary"); fold the old orchestrator step 4
  (`phase compress`) into step 2's completion contract (agent authors the handoff, then
  `phase done`).
- **`skills/pipeline/*`, `skills/using-drovr`** — drop references to a separate compress step;
  the phase's final action now includes authoring the handoff.
- **`cli/assets/compress-prompt.md`** — reword from second-person-to-a-subprocess into the
  agent's own completion instructions (content/sections unchanged).
- **`cli/src/compress.rs`** — remove the external-compressor machinery (`CmdRunner`,
  `compress_for`, `phase_compress`, the `claude -p` invocation, transcript-from-pane read). Keep
  only what `collect` needs.
- **`cli/src/main.rs`** — remove/deprecate the `phase compress` subcommand and its orchestration;
  make `collect`/`phase wait` enforce handoff presence. Simplify `handoff self`: it too becomes
  "the agent authors the file" (a manual trigger of the same discipline), not a `claude -p`
  compressor over a piped transcript.
- **Tests** — drop compressor/transcript-read tests; add: `collect` fails clearly when the
  handoff is absent/empty; the completion contract names the canonical handoff path.

## Risks + mitigations

- **Context rot** (the recorded fear). Weakest for *this* task: compression is recall + selection +
  restatement of in-context material, not novel synthesis, and rot hits synthesis hardest.
  Further mitigable by authoring at the **fullness threshold** (`using-drovr` already watches for
  it), not only at the max-full ceiling.
- **Self-bias / laundered failures** (the genuine loss vs a neutral reader). The compress
  discipline already mandates rejected alternatives + "say plainly if the phase failed/incomplete";
  keep that language sharp. Git pointers let the next phase catch omissions against source. (A
  fresh-reader *audit* pass was considered and **declined** for now.)

## Scope / notes

- Backend-agnostic by construction — no per-agent transcript handling anywhere.
- No `Phase`/`state.json` schema change.
- No herdr change, no cross-repo deploy. Ships entirely within this repo.
- Net deletion: this pivot removes more code than it adds.

## Open questions

1. Keep a thin `drovr phase compress` as a no-op/deprecation shim for old scripts, or remove
   outright?
2. Enforcement strength: is "collect fails if handoff missing" enough, or should `phase done`
   itself refuse to mark done until the handoff exists?
