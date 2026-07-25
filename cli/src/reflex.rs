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

/// Package `context` as the JSON the Claude Code SessionStart hook consumes.
pub fn envelope(context: &str) -> String {
    let value = serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "SessionStart",
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
    Some(envelope(&context))
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
        let json = envelope("HELLO <EXTREMELY_IMPORTANT> \"quoted\"");
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
}
