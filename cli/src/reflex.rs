//! Renders the SessionStart reflex context from the router skill markdown.
//!
//! The `session-start` hook delegates here (`drovr reflex`) so the reflex is
//! shaped by `[reflex]` config rather than baked into the bash hook. Three pure
//! steps compose the output:
//!   1. [`render_body`] strips `<!-- reflex:section:NAME -->` markers and omits
//!      any section disabled in config.
//!   2. [`wrap`] frames the body in the `<EXTREMELY_IMPORTANT>` envelope with the
//!      configured (or default) preamble.
//!   3. [`envelope`] packages it as the Claude Code SessionStart hook JSON.
//!
//! [`reflex_json`] threads them together and honors the master `enabled` switch.

use crate::config::ReflexConfig;
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

/// Package `context` as the JSON a Claude Code hook consumes for `event`
/// (`SessionStart` for the full reflex, `UserPromptSubmit` for the gate).
pub fn envelope(event: &str, context: &str) -> String {
    let value = serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": event,
            "additionalContext": context,
        }
    });
    // Serializing a plain object with string leaves is infallible.
    serde_json::to_string_pretty(&value).expect("serialize reflex hook JSON")
}

/// The full JSON to emit for the reflex, or `None` when the reflex is disabled.
pub fn reflex_json(skill_md: &str, cfg: &ReflexConfig) -> Option<String> {
    if !cfg.enabled {
        return None;
    }
    let body = render_body(skill_md, &cfg.sections);
    let context = wrap(&body, cfg.preamble.as_deref());
    Some(envelope("SessionStart", &context))
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
/// phrases into the router adds them here.
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
const GATE_CARD_PHRASES: &[&str] = &["<SUBAGENT-STOP>", "Single writer", "drovr:code-review"];

/// The gate JSON, or `None` when the gate is off or the previous turn already
/// ran the discipline.
///
/// `transcript` is the transcript JSONL when it could be read. **`None` fails
/// open toward emitting**: an absent or unreadable transcript path is not
/// evidence that a skill was invoked, and silent drift costs more than a
/// redundant 600-byte injection.
pub fn gate_json(cfg: &ReflexConfig, transcript: Option<&str>) -> Option<String> {
    if !cfg.enabled || !cfg.per_turn {
        return None;
    }
    if transcript.is_some_and(skill_invoked_last_turn) {
        return None;
    }
    Some(envelope("UserPromptSubmit", GATE_CARD))
}

/// True if the assistant turn since the last user message invoked a `drovr:*`
/// skill.
///
/// Walks backwards from EOF and stops at the first record that is a **real user
/// message**. That qualifier is the whole subtlety: Claude Code writes tool
/// *results* as `type: "user"` records too, and every `Skill` call is
/// immediately followed by one — so stopping at the first `type == "user"` line
/// would end the scan before reaching the call it is looking for, and this
/// check would return `false` for every session that ever used a tool. A user
/// record counts as the turn boundary only when its content is a bare string or
/// carries at least one non-`tool_result` block.
///
/// Malformed lines are skipped, not fatal: a truncated tail is normal for a
/// file being appended to live.
pub fn skill_invoked_last_turn(transcript_jsonl: &str) -> bool {
    for line in transcript_jsonl.lines().rev() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(record) = serde_json::from_str::<serde_json::Value>(line) else {
            continue; // malformed line: skip, keep walking
        };
        match record.get("type").and_then(|t| t.as_str()) {
            Some("user") if is_turn_boundary(&record) => return false,
            Some("assistant") if invokes_drovr_skill(&record) => return true,
            _ => {}
        }
    }
    false
}

/// True if this `user` record is a real prompt rather than a tool result.
///
/// Only one shape is *not* a boundary: a content array whose every block is a
/// `tool_result`. Everything else — a bare string prompt, an array carrying a
/// `text` block, and any shape this function does not recognize — ends the
/// turn. That default is the fail-open direction: walking *past* an
/// unrecognized record would let a skill invoked in some earlier turn suppress
/// this one, which is silent drift, the failure the gate exists to catch.
/// Ending the turn early costs at worst one redundant 600-byte injection.
fn is_turn_boundary(record: &serde_json::Value) -> bool {
    match record["message"]["content"].as_array() {
        Some(blocks) => blocks
            .iter()
            .any(|b| b.get("type").and_then(|t| t.as_str()) != Some("tool_result")),
        None => true,
    }
}

/// True if this `assistant` record carries a `Skill` tool_use for a `drovr:*`
/// skill. The name must match exactly and the skill must have `drovr:` as a
/// **prefix** — a skill merely containing the string does not count, and
/// neither does naming one in prose.
fn invokes_drovr_skill(record: &serde_json::Value) -> bool {
    let Some(blocks) = record["message"]["content"].as_array() else {
        return false;
    };
    blocks.iter().any(|block| {
        block.get("type").and_then(|t| t.as_str()) == Some("tool_use")
            && block.get("name").and_then(|n| n.as_str()) == Some("Skill")
            && block["input"]["skill"]
                .as_str()
                .is_some_and(|s| s.starts_with("drovr:"))
    })
}

/// How much of the transcript's end the gate will read.
///
/// This hook runs on **every** user prompt, and live transcripts reach tens of
/// megabytes; reading one whole would put that I/O in front of every prompt.
/// Only the end can matter — [`skill_invoked_last_turn`] stops at the last real
/// user message — and 1 MiB covers any plausible single turn with room to
/// spare. A turn longer than the window simply isn't seen, which emits a
/// redundant card: the same safe direction every other ambiguity resolves to.
const TRANSCRIPT_TAIL_BYTES: u64 = 1 << 20;

/// The last [`TRANSCRIPT_TAIL_BYTES`] of the transcript at `path`, or `None`
/// when it cannot be read at all — which fails open toward emitting the card.
///
/// The window boundary can land inside a multi-byte character or mid-record;
/// both degrade to one unparseable line, which the scan skips.
pub fn read_transcript_tail(path: &std::path::Path) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    // Check the file type BEFORE opening. Opening a FIFO blocks until a writer
    // appears, and this runs in front of every reply — a hang here is the
    // user's session. Only a regular file can be a transcript.
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
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
        let json = envelope("SessionStart", "HELLO <EXTREMELY_IMPORTANT> \"quoted\"");
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
        assert_eq!(reflex_json(SAMPLE, &cfg), None);
    }

    #[test]
    fn reflex_json_some_when_enabled_carries_body() {
        let cfg = ReflexConfig::default();
        let json = reflex_json(SAMPLE, &cfg).expect("enabled reflex must emit");
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
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

    /// A `Skill` call for `skill`, plus the tool_result Claude Code records
    /// right after it — the pair that defeats a naive turn-boundary scan.
    fn skill_call(skill: &str) -> String {
        format!(
            "{}\n{}",
            tool_use("Skill", &format!(r#"{{"skill":"{skill}"}}"#)),
            tool_result("toolu_1")
        )
    }

    /// The rendered `additionalContext` of a gate emission, or `None`.
    fn gate_context(cfg: &ReflexConfig, transcript: Option<&str>) -> Option<String> {
        let json = gate_json(cfg, transcript)?;
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
        for (item, needle) in [
            ("the 1% rule", "1%"),
            ("the per-turn check", "before every response"),
            ("the announcement string", "Using drovr:"),
            ("the checklist-binding line", "tracked task"),
            ("the subagent-stop line", "<SUBAGENT-STOP>"),
            ("the router pointer", "drovr:using-drovr"),
        ] {
            assert!(
                ctx.contains(needle),
                "gate card is missing {item} (no {needle:?}):\n{ctx}"
            );
        }
    }

    #[test]
    fn envelope_carries_event_name() {
        let json = envelope("UserPromptSubmit", "BODY");
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
        assert_eq!(gate_json(&cfg, None), None);
        // The master switch outranks the per-turn key even when it is on.
        let cfg = ReflexConfig {
            enabled: false,
            per_turn: true,
            ..ReflexConfig::default()
        };
        assert_eq!(gate_json(&cfg, None), None);
    }

    #[test]
    fn gate_json_none_when_per_turn_false() {
        let cfg = ReflexConfig {
            per_turn: false,
            ..ReflexConfig::default()
        };
        assert!(cfg.enabled, "this test must isolate per_turn from enabled");
        assert_eq!(gate_json(&cfg, None), None);
    }

    #[test]
    fn gate_emitted_when_transcript_absent() {
        // Fail-open toward EMITTING: an absent or unreadable transcript_path
        // yields `None`, and drift is worse than a redundant injection.
        assert!(gate_json(&ReflexConfig::default(), None).is_some());
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
        assert_eq!(gate_json(&ReflexConfig::default(), Some(&t)), None);
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
        assert!(gate_json(&ReflexConfig::default(), Some(&t)).is_some());
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
        assert!(gate_json(&ReflexConfig::default(), Some(&t)).is_some());
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
    fn malformed_transcript_lines_are_skipped_not_fatal() {
        let t = format!(
            "{}\n{}\n{}\n{}\n{}\n",
            "not json at all",
            user_prompt("go"),
            r#"{"type":"assistant","message":"truncated"#,
            "",
            skill_call("drovr:tdd")
        );
        assert!(
            skill_invoked_last_turn(&t),
            "a malformed line must be skipped, not abort the scan"
        );
        // An entirely unparseable transcript reads as "no skill" → emit.
        assert!(!skill_invoked_last_turn("garbage\n{{{\n"));
        assert!(!skill_invoked_last_turn(""));
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
            assert!(gate_json(&ReflexConfig::default(), Some(&t)).is_some());
        }
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
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../skills/using-drovr/SKILL.md"
        );
        let md =
            std::fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {path}: {e}"));
        for phrase in GATE_CARD_PHRASES {
            assert!(
                GATE_CARD.contains(phrase),
                "GATE_CARD is missing shared phrase {phrase:?}"
            );
            assert!(
                md.contains(phrase),
                "skills/using-drovr/SKILL.md is missing shared phrase {phrase:?}"
            );
        }
    }

    #[test]
    fn routing_core_survives_section_subtraction() {
        // §9.2: `[reflex.sections]` may subtract advisory sections but must not
        // be able to delete the routing core. The section list is READ FROM THE
        // FILE rather than hardcoded, so a section added later that happens to
        // wrap the core fails this test instead of slipping past it.
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../skills/using-drovr/SKILL.md"
        );
        let md =
            std::fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {path}: {e}"));
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
        for core in ["<SUBAGENT-STOP>", "# Using Drovr"] {
            assert!(
                body.contains(core),
                "subtracting every section deleted the routing core {core:?}:\n{body}"
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
        assert!(gate_json(&ReflexConfig::default(), None).is_some());

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
