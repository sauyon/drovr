<!--
  Contract for `<phase>-HANDOFF.md` — the doc the finishing phase agent authors (as its
  final action, before `drovr phase done`) and `drovr collect` reads. These seven sections
  and headings are fixed. This file IS the authoring contract: read it when authoring your
  own handoff, or when reviewing one. Nothing compresses the handoff for you — you, the
  finishing agent, hold the whole session and write it yourself.
-->

## Objective
One or two lines: what this phase was for, and what the next phase must accomplish.

## State
What is done now — files created/changed, what works, what is verified. Bullet points.
If the phase failed or is incomplete, say so plainly here.

## Decisions + rationale
Every decision that constrains the next phase, each with its WHY and any rejected
alternative. **This is the load-bearing section** — a fresh agent that lacks a rationale
here will re-derive or silently contradict it. Preserve exact names, values, flags, paths.

## Interfaces / contracts
The concrete signatures, schemas, file paths, commands, endpoints, or data shapes the next
phase must bind to — verbatim. No prose where a signature will do.

## Open questions
Anything unresolved that the next phase must decide or ask about. If none, write "None."

## Next step
The single instruction to the next agent: what to do first.

## Artifact pointers
Paths to the real files (specs, code, logs) — **pointers, NOT pasted content**. The next
agent re-reads source on demand. This is what keeps each phase's context small. This section
MUST include git references — the branch and the commit range/SHAs that carry this phase's
work — so the next agent reads `git log`/`git diff` to reconstruct state from history, not
just trust this summary. Git is the durable cross-check against lossy compression.

## Authoring rules

- **Bind these seven sections to tracked task state before you write the first one.**

  > When a skill or briefing gives you a numbered checklist, create **one tracked item per step**
  > using whatever task tool this harness exposes — `TodoWrite`, or `TaskCreate`/`TaskUpdate` —
  > before you start step 1. Mark each in-progress when you start it and complete when its
  > evidence is in hand. If the harness exposes no task tool, write the checklist to
  > `~/.local/share/drovr/runs/<run>/checklist.md` when inside a run, or `CHECKLIST.md` at the
  > repo root otherwise, and tick items there. An untracked checklist decays with the context
  > window; that decay is the exact failure drovr exists to fight.

- **Compress hard.** Drop process narration, tool logs, retries, restated instructions, and
  anything the next agent can re-read from an artifact pointer.
- **Never drop a decision or an interface** to save space — drop narration instead.
- **Preserve exact identifiers** (function names, flags, file paths, config keys, versions).
- **Report your own dead-ends honestly.** "Tried X, it failed because Y — don't retry" is
  load-bearing for the next phase. You are summarizing your OWN work, so resist the pull to
  launder your mistakes out; the git pointers make omissions catchable anyway.
- **If the phase failed or is incomplete, say so plainly** in State and Next step.
- **Do not invent facts** not present in your context. If something is unknown, say so.
- **Pointers, not pasted content** — the successor re-reads source on demand.
