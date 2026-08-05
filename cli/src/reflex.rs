//! Renders the context both of drovr's reflex hooks inject.
//!
//! Both bash hooks delegate here (`drovr reflex`) so their behaviour is shaped by
//! `[reflex]` config rather than baked into a shell script.
//!
//! **`SessionStart`** — the full router-skill injection, from
//! `hooks/session-start`. Three pure steps compose it:
//!   1. [`render_body`] strips `<!-- reflex:section:NAME -->` markers and omits
//!      any section disabled in config.
//!   2. [`wrap`] frames the body in the `<EXTREMELY_IMPORTANT>` envelope with the
//!      configured (or default) preamble.
//!   3. [`envelope`] packages it as Claude Code hook JSON for a [`HookEvent`].
//!
//! [`reflex_json`] threads them together and honors the master `enabled` switch.
//!
//! **`UserPromptSubmit`** — the per-turn gate card, from `hooks/user-prompt`.
//! [`GATE_CARD`] is a `const` (not extracted from the skill), and [`gate_json`]
//! emits it unless the reflex is off, `per_turn` is off, or
//! [`skill_invoked_last_turn`] shows the previous turn already ran the
//! discipline. See `docs/skill-evidence/per-turn-gate.md`.
//!
//! Each entry point takes only its own slice of `[reflex]` — [`SessionReflex`]
//! and [`GateReflex`] — so neither can read the other's keys.

use crate::config::{GateReflex, SessionReflex};
use std::collections::BTreeMap;

/// Built-in framing placed before the skill body when config sets no `preamble`.
/// An unconfigured reflex renders identically to the pre-config behavior.
const DEFAULT_PREAMBLE: &str = "You are running drovr — a single-writer, clean-context working discipline. It is your default operating mode.\n\n**Below is the full content of your 'drovr:using-drovr' skill — the router that picks the right methodology for the task in front of you and tells you when to escalate into a fresh phase. For all other skills, use the 'Skill' tool:**";

/// The opening marker prefix for a tagged section (`<!-- reflex:section:NAME -->`).
const OPEN_PREFIX: &str = "<!-- reflex:section:";
/// The closing marker prefix (`<!-- /reflex:section:NAME -->`).
const CLOSE_PREFIX: &str = "<!-- /reflex:section:";
const MARKER_SUFFIX: &str = "-->";

/// The section name in an opening marker line, or `None` if `line` isn't one.
fn parse_open_marker(line: &str) -> Option<&str> {
    line.strip_prefix(OPEN_PREFIX)?
        .strip_suffix(MARKER_SUFFIX)
        .map(str::trim)
}

/// The section name in a closing marker line, or `None` if `line` isn't one.
fn parse_close_marker(line: &str) -> Option<&str> {
    line.strip_prefix(CLOSE_PREFIX)?
        .strip_suffix(MARKER_SUFFIX)
        .map(str::trim)
}

/// Collapse any run of 2+ blank lines into a single blank line. Removing a
/// section leaves the blank lines that surrounded it back-to-back; this keeps
/// the rendered markdown tidy. A body with no dropped sections is unaffected
/// (the source markdown has no double blanks).
fn collapse_blank_runs(text: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    let mut prev_blank = false;
    for line in text.lines() {
        let blank = line.trim().is_empty();
        if blank && prev_blank {
            continue;
        }
        out.push(line);
        prev_blank = blank;
    }
    out.join("\n")
}

/// Strip all section markers and omit any section whose name maps to `false` in
/// `sections`. A section absent from the map defaults to enabled. Marker lines
/// are always removed, so an all-enabled render reproduces the source body
/// minus the markers.
pub fn render_body(skill_md: &str, sections: &BTreeMap<String, bool>) -> String {
    let mut out: Vec<&str> = Vec::new();
    // The name of the currently open section (if any) and whether we're dropping
    // its body. Tracking the name makes closing name-aware: a *mismatched* close
    // is ignored rather than prematurely ending the section, so a stray or
    // typo'd close tag can never silently reveal a disabled section's content.
    // (`validate_markers` — asserted against the shipped SKILL.md in the tests —
    // rejects such malformed markers before they can ship.)
    let mut current: Option<&str> = None;
    let mut skipping = false;
    for line in skill_md.lines() {
        let trimmed = line.trim();
        if let Some(name) = parse_open_marker(trimmed) {
            // Enter a tagged section; skip its body when disabled in config.
            current = Some(name);
            skipping = !sections.get(name).copied().unwrap_or(true);
            continue; // the marker line itself never reaches the output
        }
        if let Some(name) = parse_close_marker(trimmed) {
            if current == Some(name) {
                current = None;
                skipping = false;
            }
            continue; // drop the marker line whether or not it matched
        }
        if !skipping {
            out.push(line);
        }
    }
    collapse_blank_runs(&out.join("\n"))
}

/// Validate that section markers are well-formed: no nesting, every open has a
/// matching close of the same name, no stray close, nothing left open at EOF.
/// Returns an error describing the first fault. A test-only guard for the
/// shipped SKILL.md against typo'd or unbalanced markers; `render_body` is
/// robust to malformed markers at runtime (a mismatched close is ignored), so
/// this need not run in production.
#[cfg(test)]
fn validate_markers(skill_md: &str) -> Result<(), String> {
    let mut current: Option<&str> = None;
    for (i, line) in skill_md.lines().enumerate() {
        let trimmed = line.trim();
        if let Some(name) = parse_open_marker(trimmed) {
            if let Some(open) = current {
                return Err(format!(
                    "line {}: section '{name}' opened inside still-open '{open}'",
                    i + 1
                ));
            }
            current = Some(name);
        } else if let Some(name) = parse_close_marker(trimmed) {
            match current {
                Some(open) if open == name => current = None,
                Some(open) => {
                    return Err(format!(
                        "line {}: close '{name}' does not match open section '{open}'",
                        i + 1
                    ));
                }
                None => {
                    return Err(format!("line {}: close '{name}' with no open section", i + 1));
                }
            }
        }
    }
    if let Some(open) = current {
        return Err(format!("section '{open}' is never closed"));
    }
    Ok(())
}

/// Frame `body` in the `<EXTREMELY_IMPORTANT>` wrapper with `preamble` (or the
/// built-in [`DEFAULT_PREAMBLE`]) as the leading framing text.
pub fn wrap(body: &str, preamble: Option<&str>) -> String {
    let preamble = preamble.unwrap_or(DEFAULT_PREAMBLE);
    format!("<EXTREMELY_IMPORTANT>\n{preamble}\n\n{body}\n</EXTREMELY_IMPORTANT>")
}

/// The Claude Code hook events drovr emits for.
///
/// Claude Code ROUTES on `hookEventName`, so a misspelled literal does not fail
/// — it produces well-formed JSON that is delivered to the wrong place or not at
/// all. Two legal values, confined to the type system, so no call site can spell
/// them at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookEvent {
    /// The full router-skill injection, from `hooks/session-start`.
    SessionStart,
    /// The per-turn gate card, from `hooks/user-prompt`.
    UserPromptSubmit,
}

impl HookEvent {
    /// The wire name Claude Code matches on. These strings are the harness's,
    /// not drovr's — do not "tidy" them.
    pub const fn as_str(self) -> &'static str {
        match self {
            HookEvent::SessionStart => "SessionStart",
            HookEvent::UserPromptSubmit => "UserPromptSubmit",
        }
    }
}

/// Package `context` as the JSON a Claude Code hook consumes for `event`.
pub fn envelope(event: HookEvent, context: &str) -> String {
    let value = serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": event.as_str(),
            "additionalContext": context,
        }
    });
    // Serializing a plain object with string leaves is infallible.
    serde_json::to_string_pretty(&value).expect("serialize reflex hook JSON")
}

/// The full JSON to emit for the reflex, or `None` when the reflex is disabled.
///
/// Takes the [`SessionReflex`] view rather than the whole `[reflex]` table, so
/// this function cannot read — or be believed to honour — `per_turn`.
pub fn reflex_json(skill_md: &str, cfg: SessionReflex<'_>) -> Option<String> {
    if !cfg.enabled {
        return None;
    }
    let body = render_body(skill_md, cfg.sections);
    let context = wrap(&body, cfg.preamble);
    Some(envelope(HookEvent::SessionStart, &context))
}

// ---------------------------------------------------------------------------
// Per-turn gate (UserPromptSubmit)
// ---------------------------------------------------------------------------

/// The per-turn gate card, injected as `UserPromptSubmit` additional context.
///
/// **A `const`, not an extraction from `SKILL.md`** — extraction would need
/// `reflex:section` markers inside the region the router deliberately places
/// *outside* every marker (so `[reflex.sections]` cannot subtract the routing
/// core). The cost is drift between card and router, which
/// [`GATE_CARD_PHRASES`] and its two-sided test exist to catch.
///
/// The budget is on the **rendered** context: ≤600 bytes per injection, and the
/// cost is cumulative — it is appended to the window every turn it fires and
/// stays there. All six of the required items are therefore one terse clause
/// each, not a paragraph.
///
/// The `<SUBAGENT-STOP>` line is **unconditional**, independent of whether
/// `UserPromptSubmit` turns out to fire for Agent-tool subagents in this
/// harness: the measurement probes and drovr's own read-only reviewers all
/// launch from a gate-on session, and a card injected into them would
/// contaminate what they measure.
pub const GATE_CARD: &str = concat!(
    "<SUBAGENT-STOP>Dispatched as a subagent for one task? Ignore this card — do your task.</SUBAGENT-STOP>\n",
    "DROVR GATE — before every response, including clarifying questions and read-only exploration:\n",
    "1% rule: even a 1% chance a drovr:* skill applies → invoke it. Wrong fit? Drop it; invoking costs almost nothing.\n",
    "Announce it: \"Using drovr:<skill> — <purpose>.\"\n",
    "Checklist in the skill → one tracked task per step, followed before you respond.\n",
    "Single writer: one agent edits; reviews go to drovr:code-review. Unsure? Skill drovr:using-drovr.",
);

/// Phrases that must appear in **both** [`GATE_CARD`] and
/// `skills/using-drovr/SKILL.md` — the drift guard between the two texts.
///
/// Seeded with phrases already present in the shipped router, so the guard is
/// green the moment it lands; the task that writes the 1%-rule and per-turn
/// phrases into the router adds them here. **Task 14 did that** — the last
/// three entries are §4.1 items 2 and 3, and they are now what makes this a
/// real drift guard rather than three phrases that happened to already agree.
///
/// A phrase earns its place here only if changing it on one side would be a
/// *substantive* divergence. The card is 600 bytes of terse clauses and the
/// router is prose, so the shared text is necessarily short — but each entry
/// below is the load-bearing fragment of its rule, not an incidental word: drop
/// `even a 1% chance a drovr:* skill applies` from either text and the 1% rule
/// is gone from it, not merely reworded.
///
/// `drovr:using-drovr` is deliberately **not** in this list: the shipped router
/// does not contain that literal anywhere outside its own frontmatter `name:`,
/// so a two-sided assertion on it would either be red or be satisfied by the
/// file naming itself — a guard that cannot detect the drift it claims to.
/// The card's obligation to carry the pointer is enforced one-sided, in
/// `gate_card_carries_every_required_item`.
///
/// Test-only, like [`validate_markers`]: it is a contract between two texts,
/// checked at build time by the suite, with nothing to consume at runtime.
#[cfg(test)]
const GATE_CARD_PHRASES: &[&str] = &[
    "<SUBAGENT-STOP>",
    "Single writer",
    "drovr:code-review",
    // §4.1 item 2 — the 1% rule and its cost-lowering clause. Two entries, not
    // one: the threshold and the reason it is cheap to be wrong are separable,
    // and a router that keeps the threshold while dropping "it costs almost
    // nothing to be wrong" is the version an agent argues its way out of.
    "even a 1% chance a drovr:* skill applies",
    "invoking costs almost nothing",
    // §4.1 item 3 — the per-turn rule. The two clauses that make it per-turn
    // rather than per-session are the clarifying question and the read-only
    // look; without them the rule reads as "at the start of the session".
    "including clarifying questions and read-only exploration",
];

/// The gate JSON, or `None` when the gate is off or the previous turn already
/// ran the discipline.
///
/// `transcript` is the transcript JSONL when it could be read. **`None` fails
/// open toward emitting**: an absent or unreadable transcript path is not
/// evidence that a skill was invoked, and silent drift costs more than a
/// redundant 600-byte injection.
pub fn gate_json(cfg: GateReflex, transcript: Option<&str>) -> Option<String> {
    if !cfg.enabled || !cfg.per_turn {
        return None;
    }
    if transcript.is_some_and(skill_invoked_last_turn) {
        return None;
    }
    Some(envelope(HookEvent::UserPromptSubmit, GATE_CARD))
}

/// True if the assistant turn since the last user message invoked a `drovr:*`
/// skill.
///
/// **The governing invariant: exactly one path returns `true`** — an
/// `assistant` record carrying a `Skill` tool_use whose `input.skill` starts
/// with `drovr:`. Every other outcome, including every shape this function
/// cannot make sense of, returns `false` and emits the card. `true` means the
/// discipline demonstrably ran; `false` means anything else. Read any change to
/// this function against that sentence: a new `true` path is a new way for the
/// gate to go quiet on a turn that never ran a skill, which is silent drift.
///
/// Walks backwards from EOF and stops at the first record that is a **real user
/// message**. That qualifier is the whole subtlety: Claude Code writes tool
/// *results* as `type: "user"` records too, and every `Skill` call is
/// immediately followed by one — so stopping at the first `type == "user"` line
/// would end the scan before reaching the call it is looking for, and this
/// check would return `false` for every session that ever used a tool. It is
/// not the only synthetic `user` record either — see [`is_turn_boundary`],
/// which is the single authority on what ends a turn. Do not restate its rule
/// here; a partial copy of it is how this comment went stale before.
///
/// A line that will not parse **ends the turn** rather than being stepped over.
/// It is never fatal, but it is also not nothing: it might have been this
/// turn's user message, and walking past it would reach an earlier turn's skill
/// call and silence a turn that ran no skill at all. Treating it as a boundary
/// costs at most one redundant injection, which is the direction every
/// ambiguity in this scan resolves to. (The window boundary in
/// [`read_transcript_tail`] can cut a record in half, but that half-line is the
/// *last* one this walk reaches, where boundary and end-of-input mean the same
/// thing.)
pub fn skill_invoked_last_turn(transcript_jsonl: &str) -> bool {
    // Tool calls this turn whose result came back without `is_error`. The walk
    // runs backwards, so a call's result is always seen before the call itself
    // — which is what makes "did it actually run?" answerable in one pass.
    let mut succeeded: std::collections::HashSet<String> = std::collections::HashSet::new();
    for line in transcript_jsonl.lines().rev() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(record) = serde_json::from_str::<serde_json::Value>(line) else {
            return false; // unparseable: unknowable, so emit
        };
        match record.get("type").and_then(|t| t.as_str()) {
            Some("user") => {
                collect_successful_tool_ids(&record, &mut succeeded);
                if is_turn_boundary(&record) {
                    return false;
                }
            }
            Some("assistant") if invokes_drovr_skill(&record, &succeeded) => return true,
            // SKIPPING UNKNOWN RECORD TYPES IS LOAD-BEARING, not laziness, and
            // it is the one place this scan does NOT resolve toward emitting.
            //
            // When `UserPromptSubmit` fires, the submitting prompt is not yet a
            // `user` record: Claude Code has appended `{"type":"last-prompt",…}`
            // and `{"type":"mode",…}` after the previous turn's reply (measured
            // on a live 2-turn session). Treating either as a boundary stops the
            // walk before it can reach anything, so suppression would never fire
            // in production at all — the mechanism's whole cost bound, gone,
            // while every fixture that ends at the previous turn stayed green.
            //
            // The cost of skipping is bounded by the `user` arm above: an
            // unknown type cannot carry a skill call, and the first real prompt
            // still ends the walk. Pinned end-to-end by
            // `user_prompt_hook_suppresses_on_the_real_hook_time_tail`.
            _ => {}
        }
    }
    false
}

/// Record every `tool_use_id` in this record whose `tool_result` is not an
/// error.
///
/// Position relative to the boundary test does not matter and is not claimed
/// to: a record that ends the turn returns immediately, so nothing banked from
/// it is ever read. In practice the two are disjoint anyway — a record carrying
/// only `tool_result` blocks is by definition not a boundary.
fn collect_successful_tool_ids(
    record: &serde_json::Value,
    succeeded: &mut std::collections::HashSet<String>,
) {
    let Some(blocks) = record["message"]["content"].as_array() else {
        return;
    };
    for block in blocks {
        if block.get("type").and_then(|t| t.as_str()) != Some("tool_result") {
            continue;
        }
        // Success is `is_error` absent, or literally `false`. Anything else —
        // `true`, a string, a number — counts as failure. Asking `as_bool() ==
        // Some(true)` instead would read an unexpected type as "not an error"
        // and credit a call that failed, which is the suppressing direction.
        if block
            .get("is_error")
            .is_some_and(|e| e != &serde_json::Value::Bool(false))
        {
            continue;
        }
        if let Some(id) = block.get("tool_use_id").and_then(|i| i.as_str()) {
            succeeded.insert(id.to_string());
        }
    }
}

/// True if this `user` record is a real prompt rather than a tool result.
///
/// Only one shape is *not* a boundary: a **non-empty** content array whose
/// every block is a `tool_result`. Everything else — a bare string prompt, an
/// array carrying a `text` block, an empty array, and any shape this function
/// does not recognize — ends the turn. That default is the fail-open direction:
/// walking *past* an unrecognized record would let a skill invoked in some
/// earlier turn suppress this one, which is silent drift, the failure the gate
/// exists to catch. Ending the turn early costs at worst one redundant
/// 600-byte injection.
///
/// The emptiness test is load-bearing, not defensive noise: `[].iter().any(…)`
/// is vacuously `false`, so without it an empty array would be classified as a
/// tool-result record and walked past — the exact drift this whole function is
/// shaped to prevent, arriving as a vacuous truth rather than a wrong branch.
fn is_turn_boundary(record: &serde_json::Value) -> bool {
    // Claude Code injects a tool's follow-up content as an `isMeta` user record
    // — for a `Skill` call, the skill's whole markdown body as a `text` block.
    // It is output, not a turn, and `sourceToolUseID` is what says so. Without
    // this the very next thing after every `Skill` call reads as a fresh prompt
    // and the scan stops one record short of the call it exists to find.
    //
    // `isMeta` ALONE is deliberately not enough: a compaction's "Continue from
    // where you left off." and a slash-command caveat are also `isMeta`, carry
    // no source tool, and really do begin a turn. Walking past those would
    // reach an earlier turn — the suppressing direction.
    // `is_some()` would not do: `Value::get` returns `Some(&Value::Null)` for a
    // key that is present and null, so an explicit `"sourceToolUseID": null`
    // would read as a real source tool and this record would be walked past.
    // The exemption requires an id that could actually name a tool call.
    if record.get("isMeta").and_then(|m| m.as_bool()) == Some(true)
        && record
            .get("sourceToolUseID")
            .and_then(|s| s.as_str())
            .is_some_and(|s| !s.is_empty())
    {
        return false;
    }
    match record["message"]["content"].as_array() {
        Some(blocks) => {
            blocks.is_empty()
                || blocks
                    .iter()
                    .any(|b| b.get("type").and_then(|t| t.as_str()) != Some("tool_result"))
        }
        None => true,
    }
}

/// True if this `assistant` record carries a `Skill` tool_use for a `drovr:*`
/// skill **that actually ran**. The tool name must match exactly and the skill
/// must have `drovr:` as a **prefix** — a skill merely containing the string
/// does not count, and neither does naming one in prose.
///
/// `succeeded` holds the tool_use ids whose results came back without
/// `is_error`. Membership is **required**, not merely "not known to have
/// failed": a mistyped skill name emits a `tool_use` block indistinguishable
/// from a successful one, and a call with no recorded result cannot be shown to
/// have run at all. The invariant is that `true` means the discipline
/// demonstrably ran, so anything short of a recorded success is a `false`.
fn invokes_drovr_skill(
    record: &serde_json::Value,
    succeeded: &std::collections::HashSet<String>,
) -> bool {
    let Some(blocks) = record["message"]["content"].as_array() else {
        return false;
    };
    blocks.iter().any(|block| {
        block.get("type").and_then(|t| t.as_str()) == Some("tool_use")
            && block.get("name").and_then(|n| n.as_str()) == Some("Skill")
            && block["input"]["skill"]
                .as_str()
                .is_some_and(|s| s.starts_with("drovr:"))
            && block
                .get("id")
                .and_then(|i| i.as_str())
                .is_some_and(|id| succeeded.contains(id))
    })
}

/// How much of the transcript's end the gate will read.
///
/// This hook runs on **every** user prompt, and live transcripts reach tens of
/// megabytes; reading one whole would put that I/O in front of every prompt.
/// Only the end can matter — [`skill_invoked_last_turn`] stops at the last real
/// user message.
///
/// **1 MiB does not cover every turn, and the cost of that is measured, not
/// assumed.** Over 4,470 turns in this machine's transcripts, **27 (0.6%)**
/// exceed it; the largest single turn seen is 5.1 MiB. A turn longer than the
/// window is not seen, which emits a redundant card — the same safe direction
/// every other ambiguity resolves to. Widening the window would trade more I/O
/// on *every* prompt against 0.6% fewer redundant cards, which is the wrong way
/// round; the loss is bounded by the same fail-open rule as everything else.
const TRANSCRIPT_TAIL_BYTES: u64 = 1 << 20;

/// The last [`TRANSCRIPT_TAIL_BYTES`] of the transcript at `path`, or `None`
/// when it cannot be read at all — which fails open toward emitting the card.
///
/// The window boundary can land inside a multi-byte character or mid-record;
/// both degrade to one unparseable line at the *start* of the buffer, which the
/// backward scan reaches last — where "ends the turn" and "ran out of input"
/// mean the same thing, so both emit.
///
/// **The pre-open type check is not atomic and is not claimed to be.** It exists
/// only so the common case never *blocks*: opening a FIFO waits for a writer,
/// and a hang in front of every reply is the user's session. A path swapped
/// between the check and the open could still block. Nothing here prevents
/// that — `transcript_path` comes from the local hook harness, not from an
/// untrusted party, and closing the window properly needs a non-blocking open
/// (a `libc` dependency this crate does not have). The check on the open handle
/// below is the one that actually governs what gets read.
pub fn read_transcript_tail(path: &std::path::Path) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    if !std::fs::metadata(path).ok()?.is_file() {
        return None;
    }
    let mut file = std::fs::File::open(path).ok()?;
    let meta = file.metadata().ok()?;
    // Authoritative: this one is on the handle being read, so it cannot be
    // raced by a swap after the check above.
    if !meta.is_file() {
        return None;
    }
    let len = meta.len();
    if len > TRANSCRIPT_TAIL_BYTES {
        file.seek(SeekFrom::Start(len - TRANSCRIPT_TAIL_BYTES))
            .ok()?;
    }
    let mut bytes = Vec::new();
    // `take`, not a bare `read_to_end`: the transcript is being appended to
    // live, so the length measured a moment ago is a lower bound, not a
    // ceiling. Without this the cap would be a claim rather than a limit.
    file.take(TRANSCRIPT_TAIL_BYTES)
        .read_to_end(&mut bytes)
        .ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// How much of the hook's stdin payload is read.
///
/// The payload is a small JSON object from Claude Code's hook harness, but this
/// is a pipe read on every turn and its length is not ours to trust. Truncation
/// degrades to an unparseable payload, which is the fail-open path.
const HOOK_STDIN_MAX_BYTES: u64 = 64 * 1024;

/// Read the hook's stdin payload, capped at [`HOOK_STDIN_MAX_BYTES`]. Any read
/// error or non-UTF-8 input yields whatever was decodable — the payload is only
/// ever used to look up a transcript path, and failing to find one emits.
pub fn read_hook_input<R: std::io::Read>(reader: R) -> String {
    let mut bytes = Vec::new();
    // Errors are deliberately ignored: a partial read is handled by the parse
    // failing, and there is no better answer available at this point.
    let _ = std::io::Read::read_to_end(&mut reader.take(HOOK_STDIN_MAX_BYTES), &mut bytes);
    String::from_utf8_lossy(&bytes).into_owned()
}

/// The `transcript_path` from a hook's stdin JSON, or `None` when stdin carries
/// no usable path (absent, empty, wrong type, or not JSON at all). Every `None`
/// path fails open toward emitting the card.
pub fn transcript_path_from_hook_input(hook_json: &str) -> Option<std::path::PathBuf> {
    let value: serde_json::Value = serde_json::from_str(hook_json).ok()?;
    let path = value.get("transcript_path")?.as_str()?;
    if path.is_empty() {
        return None;
    }
    Some(std::path::PathBuf::from(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ReflexConfig;

    /// A compact synthetic skill body with two tagged sections and an untagged
    /// header, so section-toggling is exercised without depending on SKILL.md.
    const SAMPLE: &str = "# Title\n\n## Principle\n\n<!-- reflex:section:single-writer -->\nSingle writer rule.\n<!-- /reflex:section:single-writer -->\n\n<!-- reflex:section:always-review -->\nAlways review rule.\n<!-- /reflex:section:always-review -->\n\n## Tail\n";

    #[test]
    fn render_all_enabled_strips_markers_keeps_all_content() {
        let body = render_body(SAMPLE, &BTreeMap::new());
        assert!(!body.contains("reflex:section:"), "markers must be stripped");
        assert!(body.contains("Single writer rule."));
        assert!(body.contains("Always review rule."));
        assert!(body.contains("# Title"));
        assert!(body.contains("## Tail"));
    }

    #[test]
    fn render_omits_disabled_section_keeps_siblings() {
        let mut sections = BTreeMap::new();
        sections.insert("always-review".to_string(), false);
        let body = render_body(SAMPLE, &sections);
        assert!(
            !body.contains("Always review rule."),
            "disabled section body must be omitted, got:\n{body}"
        );
        assert!(
            body.contains("Single writer rule."),
            "an enabled sibling must survive, got:\n{body}"
        );
        // The untagged surrounding content stays.
        assert!(body.contains("## Principle"));
        assert!(body.contains("## Tail"));
        // No triple-newline artifact from the removed block.
        assert!(!body.contains("\n\n\n"), "blank runs must be collapsed, got:\n{body}");
    }

    #[test]
    fn render_explicit_true_keeps_section() {
        let mut sections = BTreeMap::new();
        sections.insert("single-writer".to_string(), true);
        let body = render_body(SAMPLE, &sections);
        assert!(body.contains("Single writer rule."));
    }

    #[test]
    fn wrap_default_preamble_frames_body() {
        let out = wrap("BODY", None);
        assert!(out.starts_with("<EXTREMELY_IMPORTANT>\n"));
        assert!(out.ends_with("\n</EXTREMELY_IMPORTANT>"));
        assert!(out.contains("single-writer, clean-context working discipline"));
        assert!(out.contains("BODY"));
    }

    #[test]
    fn wrap_custom_preamble_replaces_default() {
        let out = wrap("BODY", Some("MY FRAMING"));
        assert!(out.contains("MY FRAMING"));
        assert!(
            !out.contains("single-writer, clean-context working discipline"),
            "custom preamble must replace the default framing, got:\n{out}"
        );
        assert!(out.contains("BODY"));
    }

    #[test]
    fn envelope_is_valid_sessionstart_json() {
        let json = envelope(HookEvent::SessionStart, "HELLO <EXTREMELY_IMPORTANT> \"quoted\"");
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(
            parsed["hookSpecificOutput"]["hookEventName"].as_str(),
            Some("SessionStart")
        );
        // Round-trips arbitrary text (quotes/newlines) through JSON escaping.
        assert_eq!(
            parsed["hookSpecificOutput"]["additionalContext"].as_str(),
            Some("HELLO <EXTREMELY_IMPORTANT> \"quoted\"")
        );
    }

    #[test]
    fn mismatched_close_does_not_reveal_disabled_section() {
        // A disabled section whose body contains a *foreign* close tag must stay
        // fully dropped — the mismatched close must not end the section early.
        let md = "<!-- reflex:section:a -->\nSECRET A LINE\n<!-- /reflex:section:b -->\nSTILL A LINE\n<!-- /reflex:section:a -->\nAFTER";
        let mut sections = BTreeMap::new();
        sections.insert("a".to_string(), false);
        let body = render_body(md, &sections);
        assert!(!body.contains("SECRET A LINE"), "got:\n{body}");
        assert!(
            !body.contains("STILL A LINE"),
            "a foreign close tag must not reveal disabled content, got:\n{body}"
        );
        assert!(body.contains("AFTER"), "content after the real close survives");
    }

    #[test]
    fn validate_markers_accepts_balanced_and_rejects_faults() {
        assert!(validate_markers(SAMPLE).is_ok());
        // Unclosed open.
        assert!(validate_markers("<!-- reflex:section:x -->\nbody").is_err());
        // Stray close.
        assert!(validate_markers("<!-- /reflex:section:x -->").is_err());
        // Mismatched close name.
        assert!(
            validate_markers("<!-- reflex:section:x -->\n<!-- /reflex:section:y -->").is_err()
        );
        // Nested open.
        assert!(
            validate_markers(
                "<!-- reflex:section:x -->\n<!-- reflex:section:y -->\n<!-- /reflex:section:y -->\n<!-- /reflex:section:x -->"
            )
            .is_err()
        );
    }

    #[test]
    fn shipped_skill_markers_are_well_formed() {
        // Pins the real router skill: every reflex section must be balanced so a
        // section toggle can never silently truncate or leak the reflex.
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../skills/using-drovr/SKILL.md");
        let md = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("cannot read {path}: {e}"));
        validate_markers(&md).expect("SKILL.md section markers must be well-formed");
        // The four documented sections must all be present as balanced pairs.
        for name in ["single-writer", "always-review", "methodology", "escalation"] {
            assert!(
                md.contains(&format!("<!-- reflex:section:{name} -->")),
                "SKILL.md is missing the '{name}' section open marker"
            );
        }
    }

    #[test]
    fn reflex_json_none_when_disabled() {
        let cfg = ReflexConfig {
            enabled: false,
            ..ReflexConfig::default()
        };
        assert_eq!(reflex_json(SAMPLE, cfg.session()), None);
    }

    #[test]
    fn reflex_json_some_when_enabled_carries_body() {
        let cfg = ReflexConfig::default();
        let json = reflex_json(SAMPLE, cfg.session()).expect("enabled reflex must emit");
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        // The event/payload PAIRING, not just the payload. `HookEvent` stops a
        // call site misspelling a name; it cannot stop `reflex_json` handing the
        // gate's variant to the session's body. Without this, swapping
        // `HookEvent::SessionStart` for `UserPromptSubmit` here passes every
        // `--bin drovr` test — and the integration tests that would catch it
        // skip-pass wherever bash is unavailable. `gate_context` asserts the
        // mirror of this on every gate test.
        assert_eq!(
            parsed["hookSpecificOutput"]["hookEventName"].as_str(),
            Some("SessionStart"),
            "the router injection must announce itself as a SessionStart hook"
        );
        let ctx = parsed["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap();
        assert!(ctx.contains("Single writer rule."));
        assert!(ctx.contains("<EXTREMELY_IMPORTANT>"));
    }

    // -- per-turn gate ------------------------------------------------------
    //
    // Fixtures mirror the shape of a real Claude Code transcript, verified
    // against `~/.claude/projects/<proj>/<id>.jsonl`. The load-bearing fact:
    // **tool results are written as `type: "user"` records**, so "the assistant
    // turn since the last user message" cannot be found by stopping at the
    // first `type == "user"` line. Over 60 live transcripts, that naive rule
    // detected 0 of 35 real `drovr:*` skill invocations; the rule implemented
    // here detected 35 of 35.

    /// A real user message — the record that ends the previous turn.
    fn user_prompt(text: &str) -> String {
        format!(r#"{{"type":"user","message":{{"role":"user","content":"{text}"}}}}"#)
    }

    /// A tool RESULT. Claude Code writes these as `type: "user"` too; they must
    /// NOT be read as a turn boundary.
    fn tool_result(id: &str) -> String {
        format!(
            r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"{id}","content":"ok"}}]}}}}"#
        )
    }

    /// An assistant record carrying one `tool_use` block.
    fn tool_use(name: &str, input: &str) -> String {
        format!(
            r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"toolu_1","name":"{name}","input":{input}}}]}}}}"#
        )
    }

    /// An assistant record carrying one `text` block.
    fn assistant_text(text: &str) -> String {
        format!(
            r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"{text}"}}]}}}}"#
        )
    }

    /// The **real** three-record shape Claude Code writes for a `Skill` call,
    /// verified against 78 live invocations: the assistant's `tool_use`, its
    /// `tool_result`, and then an `isMeta` record carrying the skill's markdown
    /// body, tagged with `sourceToolUseID`.
    ///
    /// That third record is the one that matters. It is `type: "user"` with a
    /// `text` block, so a boundary rule that only knows about `tool_result`
    /// reads it as a fresh prompt and stops one record short of the call — and
    /// the suppression path never fires at all. A fixture that omitted it would
    /// keep every test in this module green while the feature did nothing.
    fn skill_call(skill: &str) -> String {
        format!(
            "{}\n{}\n{}",
            tool_use("Skill", &format!(r#"{{"skill":"{skill}"}}"#)),
            tool_result("toolu_1"),
            skill_body_injection("toolu_1")
        )
    }

    /// The `isMeta` record Claude Code injects after a successful `Skill` call.
    fn skill_body_injection(source_tool_use_id: &str) -> String {
        format!(
            r#"{{"type":"user","isMeta":true,"sourceToolUseID":"{source_tool_use_id}","message":{{"role":"user","content":[{{"type":"text","text":"Base directory for this skill: /nix/store/x/skills/tdd\n\n# TDD\n"}}]}}}}"#
        )
    }

    /// A tool_result that reports failure.
    fn failed_tool_result(id: &str) -> String {
        format!(
            r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"{id}","is_error":true,"content":"no such skill"}}]}}}}"#
        )
    }

    /// The rendered `additionalContext` of a gate emission, or `None`.
    fn gate_context(cfg: &ReflexConfig, transcript: Option<&str>) -> Option<String> {
        let json = gate_json(cfg.gate(), transcript)?;
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(
            parsed["hookSpecificOutput"]["hookEventName"].as_str(),
            Some("UserPromptSubmit"),
            "the gate must announce itself as a UserPromptSubmit hook"
        );
        Some(
            parsed["hookSpecificOutput"]["additionalContext"]
                .as_str()
                .expect("additionalContext must be a string")
                .to_string(),
        )
    }

    #[test]
    fn gate_card_within_600_bytes() {
        // §4.2 budgets the RENDERED additionalContext, not the source const —
        // asserting on `GATE_CARD.len()` would stop measuring the moment the
        // card is wrapped or framed on its way out.
        let ctx = gate_context(&ReflexConfig::default(), None).expect("default config must emit");
        assert!(
            ctx.len() <= 600,
            "rendered gate card is {} bytes, budget is 600:\n{ctx}",
            ctx.len()
        );
    }

    #[test]
    fn gate_card_carries_every_required_item() {
        // §4.2's card-content bullet — all six, none optional. Checked on the
        // rendered context so a future framing change cannot drop one.
        let ctx = gate_context(&ReflexConfig::default(), None).expect("default config must emit");
        for Anchor { label, needle } in [
            Anchor {
                label: "the 1% rule",
                needle: "1%",
            },
            Anchor {
                label: "the per-turn check",
                needle: "before every response",
            },
            Anchor {
                label: "the announcement string",
                needle: "Using drovr:",
            },
            Anchor {
                label: "the checklist-binding line",
                needle: "tracked task",
            },
            Anchor {
                label: "the subagent-stop line",
                needle: "<SUBAGENT-STOP>",
            },
            Anchor {
                label: "the router pointer",
                needle: "drovr:using-drovr",
            },
        ] {
            assert!(
                ctx.contains(needle),
                "gate card is missing {label} (no {needle:?}):\n{ctx}"
            );
        }
    }

    #[test]
    fn hook_event_wire_names_are_the_harness_spelling() {
        // The enum stops a call site MISSPELLING the event; it cannot stop the
        // enum itself carrying the wrong string. Claude Code routes on these
        // exact names, and a wrong one produces well-formed JSON that is
        // delivered nowhere — so the two spellings are pinned literally, here,
        // once. They are the harness's names, not drovr's.
        assert_eq!(HookEvent::SessionStart.as_str(), "SessionStart");
        assert_eq!(HookEvent::UserPromptSubmit.as_str(), "UserPromptSubmit");
        // And the two are distinct, so a copy-paste collapse of the match arms
        // cannot pass by making both variants render the same name.
        assert_ne!(
            HookEvent::SessionStart.as_str(),
            HookEvent::UserPromptSubmit.as_str()
        );
    }

    #[test]
    fn envelope_carries_event_name() {
        let json = envelope(HookEvent::UserPromptSubmit, "BODY");
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(
            parsed["hookSpecificOutput"]["hookEventName"].as_str(),
            Some("UserPromptSubmit")
        );
        assert_eq!(
            parsed["hookSpecificOutput"]["additionalContext"].as_str(),
            Some("BODY")
        );
    }

    #[test]
    fn gate_json_none_when_disabled() {
        let cfg = ReflexConfig {
            enabled: false,
            ..ReflexConfig::default()
        };
        assert_eq!(gate_json(cfg.gate(), None), None);
        // The master switch outranks the per-turn key even when it is on.
        let cfg = ReflexConfig {
            enabled: false,
            per_turn: true,
            ..ReflexConfig::default()
        };
        assert_eq!(gate_json(cfg.gate(), None), None);
    }

    #[test]
    fn gate_json_none_when_per_turn_false() {
        let cfg = ReflexConfig {
            per_turn: false,
            ..ReflexConfig::default()
        };
        assert!(cfg.enabled, "this test must isolate per_turn from enabled");
        assert_eq!(gate_json(cfg.gate(), None), None);
    }

    #[test]
    fn gate_emitted_when_transcript_absent() {
        // Fail-open toward EMITTING: an absent or unreadable transcript_path
        // yields `None`, and drift is worse than a redundant injection.
        assert!(gate_json(ReflexConfig::default().gate(), None).is_some());
    }

    #[test]
    fn gate_suppressed_after_drovr_skill_invocation() {
        let t = format!(
            "{}\n{}\n{}\n",
            user_prompt("please fix the parser"),
            skill_call("drovr:tdd"),
            assistant_text("wrote the failing test")
        );
        assert!(skill_invoked_last_turn(&t));
        assert_eq!(gate_json(ReflexConfig::default().gate(), Some(&t)), None);
    }

    #[test]
    fn gate_emitted_when_no_skill_last_turn() {
        let t = format!(
            "{}\n{}\n{}\n",
            user_prompt("please fix the parser"),
            tool_use("Read", r#"{"file_path":"/p/x.rs"}"#),
            assistant_text("here is what I found")
        );
        assert!(!skill_invoked_last_turn(&t));
        assert!(gate_json(ReflexConfig::default().gate(), Some(&t)).is_some());
    }

    #[test]
    fn tool_results_do_not_end_the_turn() {
        // THE defeat case. A `Skill` call is always followed by its
        // tool_result, which Claude Code records as `type: "user"`. A scan that
        // stops at the first `type == "user"` record therefore never sees the
        // call it exists to detect — a check that passes while checking
        // nothing, and the gate would inject on every single turn.
        let t = format!(
            "{}\n{}\n{}\n{}\n{}\n",
            user_prompt("please fix the parser"),
            skill_call("drovr:tdd"),
            tool_use("Edit", r#"{"file_path":"/p/x.rs"}"#),
            tool_result("toolu_2"),
            assistant_text("done")
        );
        assert!(
            skill_invoked_last_turn(&t),
            "tool_result records must not be read as a turn boundary"
        );
    }

    #[test]
    fn the_skill_body_injection_does_not_end_the_turn() {
        // The shape that made the whole suppression path inert. Claude Code
        // follows a `Skill` call with an `isMeta` record carrying the skill's
        // markdown — `type: "user"`, one `text` block. Read as a prompt, it
        // ends the turn one record before the call it exists to announce.
        let t = format!(
            "{}\n{}\n{}\n",
            user_prompt("fix the parser"),
            skill_call("drovr:tdd"),
            assistant_text("wrote the failing test")
        );
        assert!(
            skill_invoked_last_turn(&t),
            "the skill-body injection must not be read as a fresh prompt"
        );
        assert_eq!(gate_json(ReflexConfig::default().gate(), Some(&t)), None);
    }

    #[test]
    fn a_meta_record_that_is_not_tool_output_still_ends_the_turn() {
        // Only tool-driven injections are transparent, and `sourceToolUseID` is
        // what marks them. `isMeta` alone is not enough: a compaction's
        // "Continue from where you left off." and a slash-command caveat are
        // both `isMeta` with no source tool, and walking past those would reach
        // an earlier turn — the suppressing direction.
        for meta in [
            r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"Continue from where you left off."}}"#,
            r#"{"type":"user","isMeta":true,"message":{"role":"user","content":[{"type":"text","text":"<local-command-caveat>…</local-command-caveat>"}]}}"#,
            // The key PRESENT but carrying nothing usable. `Value::get` returns
            // `Some(&Value::Null)` here, so an `is_some()` test reads it as a
            // real source tool and walks past a record that begins a turn.
            r#"{"type":"user","isMeta":true,"sourceToolUseID":null,"message":{"role":"user","content":"Continue from where you left off."}}"#,
            r#"{"type":"user","isMeta":true,"sourceToolUseID":"","message":{"role":"user","content":"Continue from where you left off."}}"#,
            r#"{"type":"user","isMeta":true,"sourceToolUseID":0,"message":{"role":"user","content":"Continue from where you left off."}}"#,
            r#"{"type":"user","isMeta":true,"sourceToolUseID":{},"message":{"role":"user","content":"Continue from where you left off."}}"#,
        ] {
            let t = format!(
                "{}\n{}\n{}\n{}\n",
                user_prompt("older request"),
                skill_call("drovr:tdd"),
                meta,
                assistant_text("this turn invoked nothing")
            );
            assert!(
                !skill_invoked_last_turn(&t),
                "an isMeta record with no sourceToolUseID must end the turn: {meta}"
            );
        }
    }

    #[test]
    fn a_failed_skill_call_does_not_suppress() {
        // The invariant is "the discipline demonstrably ran". A `Skill` call
        // that errored — a mistyped skill name, say — emits an identical
        // tool_use block; only its tool_result says it never loaded. Crediting
        // it would silence a turn in which no skill ever ran.
        let t = format!(
            "{}\n{}\n{}\n{}\n",
            user_prompt("go"),
            tool_use("Skill", r#"{"skill":"drovr:cod-review"}"#),
            failed_tool_result("toolu_1"),
            assistant_text("that skill does not exist, continuing without it")
        );
        assert!(!skill_invoked_last_turn(&t));
        assert!(gate_json(ReflexConfig::default().gate(), Some(&t)).is_some());
    }

    #[test]
    fn only_an_explicitly_unfailed_result_counts_as_success() {
        // `is_error` is read as "success means absent or literally false".
        // A shape this code does not expect must not be read as "not an error"
        // — that would credit a call that failed, the suppressing direction.
        let with_result = |fields: &str| {
            let result = format!(
                r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"toolu_1",{fields}}}]}}}}"#
            );
            format!(
                "{}\n{}\n{}\n",
                user_prompt("go"),
                tool_use("Skill", r#"{"skill":"drovr:tdd"}"#),
                result
            )
        };
        for is_error in [r#""true""#, "1", "null", "{}", "true"] {
            let t = with_result(&format!(r#""is_error":{is_error},"content":"x""#));
            assert!(
                !skill_invoked_last_turn(&t),
                "is_error={is_error} must not count as a successful call"
            );
        }
        // Absent and `false` are the two success shapes.
        for tail in [r#""content":"ok""#, r#""is_error":false,"content":"ok""#] {
            let t = with_result(tail);
            assert!(skill_invoked_last_turn(&t), "{tail} must count as success");
        }
    }

    #[test]
    fn a_success_must_belong_to_this_call_and_follow_it() {
        // Two ways a success can be present without belonging to the call.
        // Both must emit — crediting either would silence a turn on someone
        // else's evidence.
        let other_id = format!(
            "{}\n{}\n{}\n",
            user_prompt("go"),
            tool_use("Skill", r#"{"skill":"drovr:tdd"}"#),
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"some_other_call","is_error":false}]}}"#
        );
        assert!(
            !skill_invoked_last_turn(&other_id),
            "a result for a different tool call must not count"
        );

        // Result recorded BEFORE the call in file order — so the backward walk
        // meets the call first, with nothing banked for it yet.
        let wrong_order = format!(
            "{}\n{}\n{}\n",
            user_prompt("go"),
            tool_result("toolu_1"),
            tool_use("Skill", r#"{"skill":"drovr:tdd"}"#)
        );
        assert!(
            !skill_invoked_last_turn(&wrong_order),
            "a result that precedes its call is not evidence the call ran"
        );
    }

    #[test]
    fn an_unresolved_skill_call_does_not_suppress() {
        // No tool_result at all — the call cannot be shown to have run, so it
        // does not count. Absence of evidence resolves toward emitting here as
        // everywhere else in this scan.
        let t = format!(
            "{}\n{}\n",
            user_prompt("go"),
            tool_use("Skill", r#"{"skill":"drovr:tdd"}"#)
        );
        assert!(!skill_invoked_last_turn(&t));
    }

    #[test]
    fn skill_before_the_last_user_message_does_not_suppress() {
        // The turn is over. A skill invoked two turns ago says nothing about
        // whether this turn is running the discipline.
        let t = format!(
            "{}\n{}\n{}\n{}\n{}\n",
            user_prompt("first request"),
            skill_call("drovr:tdd"),
            assistant_text("done"),
            user_prompt("second request"),
            assistant_text("sure")
        );
        assert!(!skill_invoked_last_turn(&t));
        assert!(gate_json(ReflexConfig::default().gate(), Some(&t)).is_some());
    }

    #[test]
    fn only_a_drovr_prefixed_skill_suppresses() {
        // `drovr:` is a PREFIX, not a substring: a third-party skill whose name
        // merely contains the string must not buy silence from the gate.
        for skill in [
            "claude-api",
            "notdrovr:tdd",
            "superpowers:drovr:tdd",
            "drovrish",
            "Drovr:tdd",
            " drovr:tdd",
        ] {
            let t = format!("{}\n{}\n", user_prompt("go"), skill_call(skill));
            assert!(
                !skill_invoked_last_turn(&t),
                "{skill:?} must not suppress the gate"
            );
        }
        // ...and the real thing still does.
        let t = format!(
            "{}\n{}\n",
            user_prompt("go"),
            skill_call("drovr:code-review")
        );
        assert!(skill_invoked_last_turn(&t));
    }

    #[test]
    fn talking_about_a_skill_does_not_suppress() {
        // Only an actual `Skill` tool_use counts. Naming one in prose, or
        // passing it as an argument to some other tool, does not.
        let mentions = format!(
            "{}\n{}\n",
            user_prompt("go"),
            assistant_text("I could use drovr:tdd here but I will not")
        );
        assert!(!skill_invoked_last_turn(&mentions));

        let other_tool = format!(
            "{}\n{}\n",
            user_prompt("go"),
            tool_use(
                "Bash",
                r#"{"command":"echo drovr:tdd","skill":"drovr:tdd"}"#
            )
        );
        assert!(!skill_invoked_last_turn(&other_tool));

        // A `tool_use` block sitting in a USER record is not an invocation
        // either — only the assistant invokes skills.
        let user_side = format!(
            "{}\n{}\n",
            user_prompt("go"),
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_use","name":"Skill","input":{"skill":"drovr:tdd"}}]}}"#
        );
        assert!(!skill_invoked_last_turn(&user_side));

        // ...nor does one in a record of any other type. Only `assistant`
        // records are searched: matching on the block alone would let a
        // `system` record — which the transcript also carries, and which no
        // agent authored as an invocation — buy silence from the gate.
        for kind in ["system", "summary", "progress"] {
            let record = format!(
                r#"{{"type":"{kind}","message":{{"role":"assistant","content":[{{"type":"tool_use","name":"Skill","input":{{"skill":"drovr:tdd"}}}}]}}}}"#
            );
            let foreign = format!("{}\n{}\n", user_prompt("go"), record);
            assert!(
                !skill_invoked_last_turn(&foreign),
                "a Skill tool_use in a {kind:?} record must not suppress the gate"
            );
        }
    }

    #[test]
    fn malformed_transcript_lines_are_never_fatal() {
        // Garbage anywhere must produce a decision, never a panic. Blank lines
        // are genuinely nothing and are stepped over; unparseable lines end the
        // turn (see the test below).
        //
        // The walk runs BACKWARDS, so a line only tests anything if it sits
        // between the call being detected and EOF. The blank line is placed
        // there deliberately: behind the call it would never be reached and
        // this assertion would hold no matter what blank lines did.
        let t = format!(
            "{}\n{}\n{}\n{}\n{}\n",
            "not json at all",
            user_prompt("go"),
            skill_call("drovr:tdd"),
            "   ",
            assistant_text("still inside the same turn")
        );
        assert!(
            skill_invoked_last_turn(&t),
            "a blank line between records must not end the scan"
        );
        // An entirely unparseable transcript reads as "no skill" → emit.
        assert!(!skill_invoked_last_turn("garbage\n{{{\n"));
        assert!(!skill_invoked_last_turn(""));
        assert!(!skill_invoked_last_turn("\n\n   \n"));
    }

    #[test]
    fn an_unparseable_line_ends_the_turn_rather_than_being_walked_past() {
        // The last remaining ambiguity that resolved toward SUPPRESSING. A line
        // that will not parse might have been this turn's user message; walking
        // past it reaches an earlier turn's skill call and silences a turn that
        // ran nothing. Unknowable means emit, exactly as every other ambiguity
        // in this scan is settled.
        let t = format!(
            "{}\n{}\n{}\n{}\n",
            user_prompt("older request"),
            skill_call("drovr:tdd"),
            r#"{"type":"user","message":{"role":"user","conte"#, // truncated mid-write
            assistant_text("this turn invoked nothing")
        );
        assert!(
            !skill_invoked_last_turn(&t),
            "an unparseable line must end the turn, not be stepped over"
        );
        assert!(gate_json(ReflexConfig::default().gate(), Some(&t)).is_some());
    }

    #[test]
    fn a_user_record_with_text_ends_the_turn() {
        // The boundary rule must accept both shapes a real prompt takes: a bare
        // string, and a content array carrying a `text` block (what a prompt
        // with an attachment looks like).
        let array_form = concat!(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t","name":"Skill","input":{"skill":"drovr:tdd"}}]}}"#,
            "\n",
            r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"next request"}]}}"#,
            "\n",
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"ok"}]}}"#,
        );
        assert!(
            !skill_invoked_last_turn(array_form),
            "a user record carrying a text block must end the turn"
        );
    }

    #[test]
    fn an_unreadable_user_record_ends_the_turn() {
        // A `user` record whose content is missing or an unexpected shape must
        // end the turn, not be walked past. Walking past it would let a skill
        // invoked in an EARLIER turn suppress this one — the drift direction.
        // Every ambiguity in this scan resolves toward emitting the card.
        for odd in [
            r#"{"type":"user","message":{}}"#,
            r#"{"type":"user"}"#,
            r#"{"type":"user","message":{"role":"user","content":42}}"#,
            r#"{"type":"user","message":{"role":"user","content":null}}"#,
            // The EMPTY array. `[].iter().any(…)` is vacuously false, so a
            // naive "does any block fail to be a tool_result" test reads this
            // as a tool-result record and walks past it — into an older turn,
            // whose skill call then suppresses a turn that never ran the
            // discipline. Vacuous truth, in the drift direction.
            r#"{"type":"user","message":{"role":"user","content":[]}}"#,
        ] {
            let t = format!(
                "{}\n{}\n{}\n",
                user_prompt("older request"),
                skill_call("drovr:tdd"),
                odd
            );
            assert!(
                !skill_invoked_last_turn(&t),
                "an unreadable user record must end the turn, got suppression on {odd}"
            );
            assert!(gate_json(ReflexConfig::default().gate(), Some(&t)).is_some());
        }
    }

    #[test]
    fn suppression_always_implies_a_successful_drovr_skill_call() {
        // The invariant, checked exhaustively against the shipped function
        // rather than argued for: over every ordering of the record shapes that
        // occur in real transcripts, `true` is returned ONLY when the sequence
        // contains a `drovr:*` `Skill` call with a successful result recorded
        // after it. Any other `true` is a turn silenced without the discipline
        // having run — the one failure mode that hides drift.
        //
        // **This checks one direction only.** It cannot fail on a change that
        // makes the gate suppress *less*; removing the `isMeta` transparency,
        // for instance, leaves it green. That direction is only ever a
        // redundant injection, and it is covered by the named tests above.
        const CALL: &str = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"x","name":"Skill","input":{"skill":"drovr:tdd"}}]}}"#;
        const OK: &str = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","is_error":false}]}}"#;
        let shapes: [&str; 12] = [
            CALL,
            OK,
            r#"{"type":"user","message":{"role":"user","content":"go"}}"#,
            r#"{"type":"user","isMeta":true,"sourceToolUseID":"x","message":{"role":"user","content":[{"type":"text","text":"body"}]}}"#,
            r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"Continue"}}"#,
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","is_error":true}]}}"#,
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","is_error":"true"}]}}"#,
            r#"{"type":"user","message":{"role":"user","content":[]}}"#,
            r#"{"type":"user","messa"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"x","name":"Read","input":{}}]}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"drovr:tdd"}]}}"#,
            r#"{"type":"system","message":{"role":"assistant","content":[{"type":"tool_use","id":"x","name":"Skill","input":{"skill":"drovr:tdd"}}]}}"#,
        ];
        let n = shapes.len();
        let mut suppressed = 0usize;
        for len in 2..=4 {
            for mut code in 0..n.pow(len as u32) {
                let mut seq: Vec<&str> = Vec::with_capacity(len);
                for _ in 0..len {
                    seq.push(shapes[code % n]);
                    code /= n;
                }
                if !skill_invoked_last_turn(&seq.join("\n")) {
                    continue;
                }
                suppressed += 1;
                // Legitimate only if a call is followed later by its success.
                let call_at = seq.iter().position(|r| *r == CALL);
                let legit = call_at.is_some_and(|i| seq[i + 1..].contains(&OK));
                assert!(
                    legit,
                    "suppressed with no successful drovr:* call:\n{}",
                    seq.join("\n")
                );
            }
        }
        assert!(
            suppressed > 0,
            "no sequence suppressed at all — the check proves nothing"
        );
    }

    /// Whitespace folded to single spaces, so a phrase is looked for in prose
    /// rather than in one particular hard-wrapping of it.
    ///
    /// **Without this the checks below are line-break-sensitive, and that is a
    /// false-negative machine.** `skills/using-drovr/SKILL.md` is hard-wrapped
    /// at ~82 columns; every phrase in [`GATE_CARD_PHRASES`] is a whole clause,
    /// so any edit that reflows a paragraph can put a newline inside one. The
    /// resulting failure says the phrase *is not in the file at all* — which is
    /// false, and which invites the repair of un-wrapping a line to appease a
    /// test rather than looking at what changed. Twice while writing this task
    /// a trim to a neighbouring sentence turned both checks red with the rule
    /// itself fully intact.
    ///
    /// It only folds; it does not lowercase or drop punctuation, so an ordinary
    /// rewording or deletion of an anchor still fails.
    ///
    /// **What it gives up, stated rather than glossed:** folding erases line and
    /// sentence boundaries, so text that has been genuinely rewritten can still
    /// contain an anchor's exact word run reassembled across a boundary the
    /// unfolded comparison would have seen. A reviewer built the case — rewriting
    /// the 1% rule's clause to *"Skipping one costs the discipline, silently,
    /// invoking / costs almost nothing about that"* keeps both checks green,
    /// because the folded run `invoking costs almost nothing` survives the
    /// rewrite by accident. That is a narrower hole than the false negative it
    /// replaces (a reflow reading as a deletion, which happened twice while this
    /// task was written), but it is a hole, and these checks are a drift guard
    /// rather than a proof that the two texts still say the same thing.
    ///
    /// **This is a second copy of `skills_valid.rs`'s `normalize_ws`, and the
    /// duplication is structural rather than careless.** `drovr` is a
    /// **bin-only** crate — there is no library target, which is why
    /// `cargo test --lib` reports *"no library targets found in package
    /// `drovr`"* — so `cli/tests/skills_valid.rs`, an integration test, has no
    /// public API to import this from, and this `#[cfg(test)]` module cannot
    /// reach into a test binary either. Neither direction is expressible today.
    ///
    /// So the gap is recorded instead of papered over: **if either side gains
    /// normalisation — apostrophe folding, case folding, punctuation stripping —
    /// change both, or the two "folded" vocabularies stop being one.**
    /// `skills_valid.rs`'s copy is the richer one (it backs the `Quote` and
    /// `FoldedBody` newtypes) and is the one to copy *from*. Giving `drovr` a
    /// lib target purely to share four tokens would be the larger change;
    /// whoever adds one for another reason should collapse these two.
    fn folded(text: &str) -> String {
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// A literal this module searches for, paired with the thing it stands for.
    ///
    /// **A named struct rather than a `(&str, &str)`, because this module had
    /// two such tuple tables with their fields in OPPOSITE order** —
    /// [`ROUTING_CORE`] read `(anchor, label)` while
    /// `gate_card_carries_every_required_item` read `(label, anchor)`. Both are
    /// `(&str, &str)`, so nothing distinguished them to the compiler: a row
    /// copied between the tables, or a new row written from memory of the wrong
    /// one, transposes silently and surfaces as a baffling assertion failure
    /// rather than as a type error. Named fields make the transposition
    /// unwriteable — a struct literal must spell both names, and spelling them
    /// is the check.
    ///
    /// The asymmetry is the whole point and is stated once, here: **`needle` is
    /// the only field ever searched for; `label` is diagnostic only.** Asserting
    /// on `label` would be asserting that this table describes itself.
    ///
    /// It guards the routing core — the invariant whose quiet failure produces
    /// an agent that believes it is running drovr and is not — so it is the last
    /// place in the run where two same-shaped tuples should be left to mean two
    /// different things.
    struct Anchor {
        /// What this stands for, for the failure message. **Never searched.**
        label: &'static str,
        /// The literal text searched for. Whitespace-folded before comparison
        /// wherever the target is hard-wrapped prose.
        needle: &'static str,
    }

    /// `skills/using-drovr/SKILL.md`, as the shipped file the checks below are
    /// about. One reader, so the two tests cannot end up looking at the router
    /// by two different paths.
    fn router_skill_md() -> String {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../skills/using-drovr/SKILL.md"
        );
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {path}: {e}"))
    }

    #[test]
    fn gate_card_phrases_present_in_router_skill() {
        // The drift guard (§4.2, §9.2). TWO-SIDED on purpose: asserting only
        // that the card contains a phrase lets the router drop it, and
        // asserting only the router lets the card drop it. Either half alone is
        // a guarantee with nothing keeping it.
        assert!(
            !GATE_CARD_PHRASES.is_empty(),
            "an empty phrase list makes this test vacuous"
        );
        let card = folded(GATE_CARD);
        let md = folded(&router_skill_md());
        for phrase in GATE_CARD_PHRASES {
            let phrase = folded(phrase);
            assert!(
                card.contains(&phrase),
                "GATE_CARD is missing shared phrase {phrase:?}"
            );
            assert!(
                md.contains(&phrase),
                "skills/using-drovr/SKILL.md is missing shared phrase {phrase:?} \
                 (whitespace is folded before comparing, so this is a rewording \
                 or a deletion, not a line wrap)"
            );
        }
    }

    #[test]
    fn routing_core_survives_section_subtraction() {
        // §9.2: `[reflex.sections]` may subtract advisory sections but must not
        // be able to delete the routing core. The section list is READ FROM THE
        // FILE rather than hardcoded, so a section added later that happens to
        // wrap the core fails this test instead of slipping past it.
        let md = router_skill_md();
        let names: Vec<String> = md
            .lines()
            .filter_map(|l| parse_open_marker(l.trim()).map(str::to_string))
            .collect();
        assert!(
            !names.is_empty(),
            "no reflex sections found — this test would subtract nothing"
        );
        let sections: BTreeMap<String, bool> = names.iter().map(|n| (n.clone(), false)).collect();
        let body = render_body(&md, &sections);
        // Non-vacuity: the subtraction must actually have removed something, or
        // this test would pass on a render that ignored `sections` entirely.
        assert!(
            body.len() < render_body(&md, &BTreeMap::new()).len(),
            "subtracting every section removed nothing — the test proves nothing"
        );
        // §4.1: items 2–6 sit OUTSIDE every marker, so every one of them has to
        // survive here. Each anchor is paired with the item it stands for, so a
        // failure says *which* part of the router a config toggle just deleted
        // rather than only quoting a string.
        //
        // The two assertions per anchor are not redundant. "Never written" and
        // "written inside a marker and subtracted away" are different bugs with
        // different fixes, and collapsing them into one `body.contains` would
        // report the second as the first — which is how someone repairs this by
        // pasting the text back in, still inside the marker.
        const ROUTING_CORE: &[Anchor] = &[
            Anchor {
                label: "§4.1 item 1 — the subagent stop",
                needle: "<SUBAGENT-STOP>",
            },
            Anchor {
                label: "the H1",
                needle: "# Using Drovr",
            },
            Anchor {
                label: "§4.1 item 2 — the 1% rule",
                needle: "even a 1% chance a drovr:* skill applies",
            },
            Anchor {
                label: "§4.1 item 2 — the 1% rule's cost-lowering clause",
                needle: "invoking costs almost nothing",
            },
            Anchor {
                label: "§4.1 item 3 — the per-turn rule",
                needle: "including clarifying questions and read-only exploration",
            },
            Anchor {
                label: "§4.1 item 4 — the instruction-priority ladder (rung 1)",
                needle: "The human's explicit instructions",
            },
            Anchor {
                label: "§4.1 item 5 — the gate-function flowchart",
                needle: "digraph drovr_gate",
            },
            Anchor {
                label: "§4.1 item 5's checklist branch — fix 3's canonical directive (§5 site 1)",
                needle: "one tracked item per step",
            },
            Anchor {
                label: "§4.1 item 6 — the router's own red-flag table",
                needle: "## Red flags — you are about to route nothing",
            },
        ];
        // Folded on both sides, for the reason [`folded`] gives: these anchors
        // are whole clauses in a hard-wrapped file, and a line-break-sensitive
        // comparison reports a reflow as a deletion.
        let folded_md = folded(&md);
        let folded_body = folded(&body);
        for Anchor { label, needle } in ROUTING_CORE {
            let core = folded(needle);
            assert!(
                folded_md.contains(&core),
                "{label}: {core:?} is not in skills/using-drovr/SKILL.md at all — \
                 this test can only tell you whether the routing core survives \
                 subtraction, not write it for you"
            );
            assert!(
                folded_body.contains(&core),
                "subtracting every section deleted {label}: {core:?}. It is inside \
                 a `<!-- reflex:section:NAME -->` pair, so `[reflex.sections]` can \
                 remove it — which yields an agent that believes it is running \
                 drovr and is not. Move it outside every marker; only \
                 `[reflex] enabled = false` may remove the routing core.\n{body}"
            );
        }
    }

    #[test]
    fn transcript_tail_is_bounded() {
        // This hook runs on EVERY user prompt, and live transcripts in
        // `~/.claude/projects/` reach 29 MB. Reading one whole would put tens
        // of megabytes of I/O directly in the path of every keystroke-to-first-
        // token. Only the tail can possibly matter: the scan stops at the last
        // real user message.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.jsonl");
        let filler = format!("{}\n", user_prompt("older"));
        let mut big = filler.repeat(1 + (TRANSCRIPT_TAIL_BYTES as usize / filler.len()));
        big.push_str(&format!("{}\n", user_prompt("the last line")));
        assert!(
            big.len() > TRANSCRIPT_TAIL_BYTES as usize,
            "fixture must exceed the window"
        );
        std::fs::write(&path, &big).unwrap();

        let tail = read_transcript_tail(&path).expect("a readable file must yield a tail");
        assert!(
            tail.len() <= TRANSCRIPT_TAIL_BYTES as usize,
            "tail is {} bytes, window is {TRANSCRIPT_TAIL_BYTES}",
            tail.len()
        );
        assert!(
            tail.ends_with("the last line\"}}\n"),
            "the tail must be the END of the file, not its start"
        );
    }

    #[test]
    fn transcript_tail_reads_a_short_file_whole() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("small.jsonl");
        let body = format!("{}\n{}\n", user_prompt("go"), skill_call("drovr:tdd"));
        std::fs::write(&path, &body).unwrap();
        assert_eq!(read_transcript_tail(&path).as_deref(), Some(body.as_str()));
        // ...and the suppression decision is unchanged by going through the file.
        assert!(skill_invoked_last_turn(
            &read_transcript_tail(&path).unwrap()
        ));
    }

    #[test]
    fn transcript_tail_is_none_when_unreadable() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read_transcript_tail(&dir.path().join("nope.jsonl")), None);
        // A directory is not a transcript.
        assert_eq!(read_transcript_tail(dir.path()), None);
    }

    #[test]
    #[cfg(unix)]
    fn transcript_tail_refuses_a_non_regular_file() {
        // A FIFO blocks on open until a writer appears. If the gate ever opened
        // one it would hang the hook — and the hook sits in front of every
        // reply, so a hang there is the user's session, not a background task.
        // The guard has to be a metadata check BEFORE the open, which is why
        // this test can exist at all: it would hang, not fail, without one.
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("pipe.jsonl");
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !made {
            eprintln!("skipping: mkfifo unavailable");
            return;
        }
        assert_eq!(
            read_transcript_tail(&fifo),
            None,
            "a FIFO is not a transcript and must not be opened"
        );
    }

    #[test]
    fn hook_stdin_is_capped() {
        // The payload is a small JSON object from the hook harness, but this
        // reads from a pipe on every turn; an unbounded read is a resource the
        // caller controls. Truncation degrades to an unparseable payload, which
        // is the fail-open path — no transcript, so the card is emitted.
        let huge = "x".repeat(HOOK_STDIN_MAX_BYTES as usize * 2);
        let got = read_hook_input(huge.as_bytes());
        assert_eq!(got.len(), HOOK_STDIN_MAX_BYTES as usize);
        assert_eq!(transcript_path_from_hook_input(&got), None);
        assert!(gate_json(ReflexConfig::default().gate(), None).is_some());

        // A normal payload is unaffected.
        let payload = r#"{"transcript_path":"/p/t.jsonl"}"#;
        assert_eq!(read_hook_input(payload.as_bytes()), payload);
        // Non-UTF-8 on stdin degrades rather than failing the turn.
        assert!(read_hook_input(&b"\xff\xfe"[..]).len() <= HOOK_STDIN_MAX_BYTES as usize);
    }

    #[test]
    fn a_tail_split_mid_character_does_not_panic() {
        // The window boundary lands wherever it lands — including inside a
        // multi-byte character. That must degrade to a skipped line, not a
        // crash in a hook that sits in front of every user prompt.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("utf8.jsonl");
        let mut body = "é".repeat(TRANSCRIPT_TAIL_BYTES as usize); // 2 bytes each
        body.push('\n');
        body.push_str(&format!("{}\n", user_prompt("go")));
        std::fs::write(&path, &body).unwrap();

        let tail = read_transcript_tail(&path).expect("must not fail on split characters");
        assert!(!skill_invoked_last_turn(&tail));
    }

    #[test]
    fn a_skill_call_older_than_the_window_fails_open() {
        // The documented cost of the bound: a `Skill` call further back than
        // the window is invisible, so the gate emits a redundant card. That is
        // the safe direction — the same one every other ambiguity resolves to.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("long-turn.jsonl");
        let mut body = format!("{}\n{}\n", user_prompt("go"), skill_call("drovr:tdd"));
        let bulk = format!("{}\n", tool_use("Read", r#"{"file_path":"/p/x.rs"}"#));
        body.push_str(&bulk.repeat(1 + (TRANSCRIPT_TAIL_BYTES as usize / bulk.len())));
        std::fs::write(&path, &body).unwrap();

        assert!(
            skill_invoked_last_turn(&body),
            "the whole file does contain the suppressing call"
        );
        let tail = read_transcript_tail(&path).unwrap();
        assert!(
            !skill_invoked_last_turn(&tail),
            "beyond the window the call is invisible — and that must EMIT, not suppress"
        );
    }

    #[test]
    fn transcript_path_is_read_from_hook_input() {
        assert_eq!(
            transcript_path_from_hook_input(
                r#"{"session_id":"s","transcript_path":"/p/t.jsonl","hook_event_name":"UserPromptSubmit"}"#
            ),
            Some(std::path::PathBuf::from("/p/t.jsonl"))
        );
        // Every shape that cannot yield a path reads as "no transcript", which
        // fails open toward emitting the card.
        for bad in [
            "",
            "not json",
            "{}",
            r#"{"transcript_path":null}"#,
            r#"{"transcript_path":42}"#,
            r#"{"transcript_path":""}"#,
            "[]",
        ] {
            assert_eq!(
                transcript_path_from_hook_input(bad),
                None,
                "{bad:?} must not yield a transcript path"
            );
        }
    }
}
