//! Tests the `hooks/session-start` reflex hook script's decision logic.
//!
//! The hook is the always-on reflex for human-initiated agents: on a normal
//! Claude Code session it injects the `using-drovr` router skill as
//! `SessionStart` additional context; inside a drovr-spawned phase (signalled
//! by the `DROVR_PHASE` env var) it must no-op so the phase runs purely on its
//! injected handoff.
//!
//! The script is exercised standalone via `bash`, with `CLAUDE_PLUGIN_ROOT`
//! pointed at the repo root so it resolves `skills/using-drovr/SKILL.md`.
//! Every test is gated on `bash` being available so the suite stays green in
//! odd CI environments.

use std::path::PathBuf;
use std::process::Command;

/// Repo root (one level up from this crate's manifest dir).
fn repo_root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
}

/// The reflex hook script under test.
fn hook_script() -> PathBuf {
    repo_root().join("hooks/session-start")
}

/// True if `bash` can be executed. Tests skip (pass) when it is absent.
fn bash_available() -> bool {
    Command::new("bash")
        .arg("-c")
        .arg("exit 0")
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Run the hook via `bash <script>` with a clean, controlled environment and
/// return its stdout. `drovr_phase` is `Some(value)` to set `DROVR_PHASE`, or
/// `None` to guarantee it is unset in the child.
fn run_hook(drovr_phase: Option<&str>) -> String {
    let mut cmd = Command::new("bash");
    cmd.arg(hook_script())
        .env("CLAUDE_PLUGIN_ROOT", repo_root())
        // Force the deterministic Claude Code path: strip sibling-platform
        // markers that could branch the output elsewhere.
        .env_remove("CURSOR_PLUGIN_ROOT")
        .env_remove("COPILOT_CLI");

    match drovr_phase {
        Some(v) => {
            cmd.env("DROVR_PHASE", v);
        }
        None => {
            cmd.env_remove("DROVR_PHASE");
        }
    }

    let out = cmd.output().expect("failed to execute hooks/session-start");
    assert!(
        out.status.success(),
        "hook exited non-zero: status={:?} stderr={}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("hook stdout is not UTF-8")
}

#[test]
fn suppressed_when_drovr_phase_set() {
    if !bash_available() {
        eprintln!("skipping: bash not available");
        return;
    }

    let stdout = run_hook(Some("drovr-v2/plan"));

    assert!(
        !stdout.contains("using-drovr"),
        "reflex must be suppressed under DROVR_PHASE, but stdout mentioned the skill:\n{stdout}"
    );
    assert!(
        !stdout.contains("additionalContext"),
        "reflex must inject no additionalContext under DROVR_PHASE, but stdout had it:\n{stdout}"
    );
}

#[test]
fn injects_reflex_when_drovr_phase_unset() {
    if !bash_available() {
        eprintln!("skipping: bash not available");
        return;
    }

    let stdout = run_hook(None);

    // Must be a single valid JSON object shaped for Claude Code SessionStart.
    let parsed: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("hook stdout must be valid JSON");

    let injected = parsed["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect("hookSpecificOutput.additionalContext must be a string");

    assert_eq!(
        parsed["hookSpecificOutput"]["hookEventName"].as_str(),
        Some("SessionStart"),
        "hookEventName must be SessionStart"
    );
    assert!(
        stdout.contains("hookSpecificOutput"),
        "stdout must carry the hookSpecificOutput envelope"
    );
    assert!(
        injected.contains("using-drovr"),
        "injected context must carry the using-drovr reflex skill, got:\n{injected}"
    );
    // A phrase that lives only in SKILL.md's *body*, not in its path or any
    // read-error string. This proves the file was actually read and injected —
    // guarding against a vacuous pass where a bad read emits a message that
    // merely happens to contain the substring "using-drovr" (the dir name).
    assert!(
        injected.contains("Single writer, read-only explorers"),
        "injected context must carry the SKILL.md body, not just its name, got:\n{injected}"
    );
}
