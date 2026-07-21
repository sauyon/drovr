---
name: using-relay
description: Use when a task is too large or too context-heavy for one session and must run as separate agent phases that hand off to each other, before picking relay:handoff or relay:pipeline
---

# Using Relay

## Overview

Relay is a **discipline**, not an orchestration engine. It runs work as a chain of
**bounded, clean-context phases**: each phase is a fresh `claude` agent that gets one
compressed briefing (a HANDOFF doc), does one job, and is compressed into the briefing for
the next. herdr owns the panes/sessions; relay owns the handoff contract and run lifecycle.

**Core principle: single writer, read-only explorers.** One agent edits at a time. Fan-out
investigation goes to read-only explorers (e.g. `explore-mcp`), never to parallel writers.
Context rot is bounded by starting each phase fresh and re-reading source from artifact
pointers rather than carrying a bloated transcript forward.

## When to use

- A task won't fit — or will rot — in one context window (large feature, migration, audit).
- You want phase boundaries where a fresh agent re-reads a tight briefing instead of a
  200k-token transcript.
- You want a human approval gate on a spec before code gets written.

**When NOT to use:** a task that fits comfortably in one session. Relay's overhead (panes,
compression, gating) is not worth it for a quick edit — just do the work.

## Prerequisites (relay errors helpfully if absent)

- **herdr** on `PATH`, plus the claude integration: `herdr integration install claude`
  (hooks Claude Code's stop event so `relay phase wait` can detect "done"). `relay new`
  refuses to run without it.
- **claude** (Claude Code CLI) on `PATH` — phases and the compressor both shell it.
- **explore-mcp** (recommended) — read-only fan-out investigation for the phase agents.

## Choosing your skill

```
Single phase boundary (hand this finished work to one fresh agent)?  → relay:handoff
Full brainstorm → plan → implement → review run with a spec gate?     → relay:pipeline
```

- **relay:handoff** — the manual primitive: run one phase, compress it, seed the next. Use
  it directly for a one-off boundary, or read it first because **pipeline is built out of it**.
- **relay:pipeline** — the opinionated gated runner: the four-phase flow with a human
  approval gate on `spec.md` after brainstorm, everything else unattended.

**REQUIRED BACKGROUND:** both downstream skills assume the contracts in this file
(prerequisites, single-writer rule, run dir at `~/.local/share/relay/runs/<name>/`).

## The one contract that surprises everyone

`relay phase start` spawns a **plain** `claude` — it does **NOT** inject the seed. It only
records the seed path in `state.json`. **Injecting the briefing into the fresh agent is the
skill's job**, done with `relay phase send`. Both downstream skills encode this; do not
assume the CLI seeds the agent for you.
