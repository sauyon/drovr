<!--
  Contract for `<phase>-HANDOFF.md` — the doc `drovr phase compress` writes and
  `drovr collect` reads. These seven sections and headings are fixed; they mirror the
  compressor's own instructions (cli/assets/compress-prompt.md), so the compressed output
  will already look like this. Use this file to understand what each section is FOR — when
  reviewing a handoff, or when hand-writing one for a phase you ran without the compressor.

  Rule of thumb: never drop a decision or an interface to save space — drop narration
  instead. Pointers, not pasted content.
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
