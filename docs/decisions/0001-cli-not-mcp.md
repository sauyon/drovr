# 0001 — drovr's agent surface stays a CLI, not an MCP server

**Status:** decided · **Date:** 2026-07-25

## The question

drovr exposes nine agent-facing verbs: `phase start/send/wait/done`, `collect`,
`review summary/wait`, `code-review base/run`. Agents drive them by shelling out through the
Bash tool. (`reflex` is excluded — the SessionStart hook invokes it, no agent ever calls it.
The human-facing `new`/`list`/`status`/`attach`/`cleanup`/`resurrect`/`serve` are excluded for
the same reason.) Should those nine become MCP tools instead?

The concrete plan considered was: add an MCP endpoint to the always-on review server
(`cli/src/review.rs`) over streamable HTTP, sharing the existing `Ctx`, and hand every spawned
`claude` a `--mcp-config` pointing at it.

## Decision

**Keep the CLI.** Do not port the verbs to MCP.

## Why

### 1. The headline benefit was wrong

The port was pitched mainly on breaking the Bash timeout ceiling so `phase wait` could block for
the real duration of a phase. That benefit does not exist:

- **A backgrounded Bash call already has no such ceiling.** The 600 000 ms cap applies to
  *foreground* calls. Background the wait, end the turn, and the harness wakes you on process
  exit. This is now the documented pattern in `drovr:handoff` step 3. Cost: zero new surface.
- **MCP would have been *worse* here, not better.** MCP transports carry their own idle
  timeouts, and streamable HTTP is the strictest: a ~5 minute idle timeout (stdio gets ~30)
  plus a 60 s first-byte timer that stdio does not have. The design picked streamable HTTP
  because the review server is *already an HTTP server* and reusing it looked free — but that
  is a false economy twice over. `cli/src/review.rs` is plain HTTP/1.x on `tiny_http`; MCP's
  streamable HTTP is a distinct transport with its own framing, so this was new transport code,
  not a new route. And the transport it would have bought us is the worst available option for
  exactly the long-blocking verb the port was meant to fix.
- **Client-side auto-backgrounding does not cover us.** Claude Code backgrounds slow MCP calls
  past ~2 minutes, but excludes subagent calls (no escape hatch) and non-interactive mode. Every
  drovr phase agent is one or both.

### 2. Nine verbs is below the threshold where MCP's advantages start

Anthropic's own guidance puts standard tool calling ahead of tool search below ~10 tools, and
notes tool search is "less beneficial" in that range. The widely-cited context-bloat numbers
that argue *against* MCP (tens of thousands of tokens for 50+ tools) are one to two orders of
magnitude away from nine verbs. Context cost is simply not the deciding axis at this size — in
either direction.

### 3. Handing every spawned agent an `--mcp-config` is an active liability

See `docs/known-issues.md`, *"Spawned agents park on the 'New MCP server' approval prompt,
undetected"*. A freshly spawned `claude` can sit on that prompt while herdr reports it `idle`
rather than `blocked`, so `phase send` readiness checks and blocked-triage both miss it and the
run wedges silently. The port would have put that failure mode on the critical path of every
agent drovr spawns.

### 4. What we'd have gained is small and partly already free

MCP genuinely wins on schema-validated arguments, discoverability, and permission granularity.
But the "code execution with MCP" pattern that Anthropic introduced to fix MCP's upfront-loading
problem is structurally the same progressive disclosure a subcommand CLI with `--help` gives for
nothing. And a CLI keeps properties drovr actively depends on: one interface shared by the human
and the agent, zero context cost until invoked, composability with shell plumbing, and no client
configuration to distribute.

## What would reopen this

- The verb count grows well past ~10 **and** they need schema-validated structured input.
- A verb genuinely must block longer than a backgrounded Bash call can survive.
- The "New MCP server" parking prompt becomes detectable (herdr prompt-detection rule) *and*
  `--mcp-config`-supplied servers are confirmed not to trigger it.

Note that the first two are independent of transport choice; if this is reopened, benchmark
stdio against streamable HTTP rather than assuming the existing HTTP server is the right host.

## Consequences

- `cli/src/review.rs` keeps a single HTTP surface: the human review UI and its JSON API.
- Long waits are backgrounded, not held open. `drovr:handoff` step 3 and `drovr:pipeline`'s
  implement loop document this.
- Structured output, if wanted, is a `--json` flag on the existing commands — not a protocol.
