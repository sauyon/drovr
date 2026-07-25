//! Tests the `hooks/session-start` reflex hook and its delegation to
//! `drovr reflex`.
//!
//! The hook is the always-on reflex for human-facing agents: on a normal Claude
//! Code session it resolves `skills/using-drovr/SKILL.md` and execs
//! `drovr reflex`, which renders the `SessionStart` additional context shaped by
//! the `[reflex]` config. Inside a drovr-spawned phase (signalled by
//! `DROVR_PHASE`) the hook no-ops *before* spawning drovr, so the phase runs
//! purely on its injected handoff.
//!
//! The script is exercised standalone via `bash`, with `CLAUDE_PLUGIN_ROOT`
//! pointed at the repo root (so it resolves the skill) and `DROVR_BIN` pointed at
//! the freshly built binary (so it doesn't depend on `drovr` being on PATH).
//! `XDG_CONFIG_HOME` is always pinned to a controlled dir so the reflex config is
//! hermetic — empty for the default-behavior tests, populated for the rest.
//! Every test is gated on `bash` being available so the suite stays green in odd
//! CI environments.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Repo root (one level up from this crate's manifest dir).
fn repo_root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
}

/// The reflex hook script under test.
fn hook_script() -> PathBuf {
    repo_root().join("hooks/session-start")
}

/// Locate the drovr binary built by `cargo test` (mirrors e2e.rs). Points
/// `DROVR_BIN` at it so the hook execs the just-built CLI, not one on PATH.
fn drovr_binary() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let bin = manifest.join("target/debug/drovr");
    if bin.exists() {
        return bin;
    }
    let ws_bin = manifest
        .parent()
        .unwrap_or(&manifest)
        .join("target/debug/drovr");
    if ws_bin.exists() {
        return ws_bin;
    }
    bin // will fail with a clear error if missing
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

/// Run the hook via `bash <script>` with a clean, controlled environment.
///
/// `drovr_phase` is `Some(value)` to set `DROVR_PHASE`, or `None` to guarantee it
/// is unset. `config_home` becomes `XDG_CONFIG_HOME`, so `drovr reflex` reads
/// `<config_home>/drovr/config.toml` (absent → reflex defaults).
fn run_hook(drovr_phase: Option<&str>, config_home: &Path) -> Output {
    let mut cmd = Command::new("bash");
    cmd.arg(hook_script())
        .env("CLAUDE_PLUGIN_ROOT", repo_root())
        .env("DROVR_BIN", drovr_binary())
        .env("XDG_CONFIG_HOME", config_home)
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

    cmd.output().expect("failed to execute hooks/session-start")
}

/// Assert the hook exited 0 and return its stdout as a `String`.
fn ok_stdout(out: Output) -> String {
    assert!(
        out.status.success(),
        "hook exited non-zero: status={:?} stderr={}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("hook stdout is not UTF-8")
}

/// Write `<dir>/drovr/config.toml` with `contents`. The caller owns `dir` (a
/// tempdir) so it outlives the hook process.
fn write_config(dir: &Path, contents: &str) {
    let cfg_dir = dir.join("drovr");
    std::fs::create_dir_all(&cfg_dir).unwrap();
    std::fs::write(cfg_dir.join("config.toml"), contents).unwrap();
}

/// Parse the hook stdout and return its injected `additionalContext`.
fn injected_context(stdout: &str) -> String {
    let parsed: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("hook stdout must be valid JSON");
    assert_eq!(
        parsed["hookSpecificOutput"]["hookEventName"].as_str(),
        Some("SessionStart"),
        "hookEventName must be SessionStart"
    );
    parsed["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect("hookSpecificOutput.additionalContext must be a string")
        .to_string()
}

#[test]
fn suppressed_when_drovr_phase_set() {
    if !bash_available() {
        eprintln!("skipping: bash not available");
        return;
    }
    let cfg = tempfile::tempdir().unwrap();
    let stdout = ok_stdout(run_hook(Some("drovr-v2/plan"), cfg.path()));

    assert!(
        stdout.trim().is_empty(),
        "reflex must be suppressed under DROVR_PHASE, but stdout was:\n{stdout}"
    );
}

#[test]
fn injects_reflex_when_drovr_phase_unset() {
    if !bash_available() {
        eprintln!("skipping: bash not available");
        return;
    }
    // Empty config dir → reflex defaults (fully on).
    let cfg = tempfile::tempdir().unwrap();
    let stdout = ok_stdout(run_hook(None, cfg.path()));
    let injected = injected_context(&stdout);

    assert!(
        injected.contains("using-drovr"),
        "injected context must carry the using-drovr reflex skill, got:\n{injected}"
    );
    // A phrase that lives only in SKILL.md's *body* — proves the file was read
    // and injected, not merely that the name substring appears.
    assert!(
        injected.contains("Single writer, read-only explorers"),
        "injected context must carry the SKILL.md body, got:\n{injected}"
    );
    // The section markers must never leak into the injected context.
    assert!(
        !injected.contains("reflex:section:"),
        "section markers must be stripped, got:\n{injected}"
    );
}

#[test]
fn empty_drovr_phase_does_not_suppress() {
    if !bash_available() {
        eprintln!("skipping: bash not available");
        return;
    }
    // `DROVR_PHASE=""` is not a phase: `[ -n "$DROVR_PHASE" ]` is false, so the
    // reflex must still be injected (suppression requires a non-empty value).
    let cfg = tempfile::tempdir().unwrap();
    let stdout = ok_stdout(run_hook(Some(""), cfg.path()));
    let injected = injected_context(&stdout);
    assert!(
        injected.contains("Single writer, read-only explorers"),
        "an empty DROVR_PHASE must not suppress the reflex, got:\n{injected}"
    );
}

#[test]
fn missing_binary_fails_loudly() {
    if !bash_available() {
        eprintln!("skipping: bash not available");
        return;
    }
    // A missing drovr binary must fail loudly (non-zero) rather than emit a
    // partial or empty context that looks like a legitimate no-op.
    let cfg = tempfile::tempdir().unwrap();
    let out = Command::new("bash")
        .arg(hook_script())
        .env("CLAUDE_PLUGIN_ROOT", repo_root())
        .env("DROVR_BIN", "/nonexistent/definitely-not-drovr")
        .env("XDG_CONFIG_HOME", cfg.path())
        .env_remove("DROVR_PHASE")
        .output()
        .expect("failed to execute hooks/session-start");
    assert!(
        !out.status.success(),
        "a missing binary must make the hook exit non-zero, got stdout:\n{}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn reflex_disabled_emits_nothing() {
    if !bash_available() {
        eprintln!("skipping: bash not available");
        return;
    }
    let cfg = tempfile::tempdir().unwrap();
    write_config(cfg.path(), "[reflex]\nenabled = false\n");
    let stdout = ok_stdout(run_hook(None, cfg.path()));

    assert!(
        stdout.trim().is_empty(),
        "a disabled reflex must inject nothing, but stdout was:\n{stdout}"
    );
}

#[test]
fn section_toggle_omits_disabled_section() {
    if !bash_available() {
        eprintln!("skipping: bash not available");
        return;
    }
    let cfg = tempfile::tempdir().unwrap();
    write_config(cfg.path(), "[reflex.sections]\nescalation = false\n");
    let stdout = ok_stdout(run_hook(None, cfg.path()));
    let injected = injected_context(&stdout);

    // The escalation section is gone...
    assert!(
        !injected.contains("Escalation contract"),
        "disabled escalation section must be omitted, got:\n{injected}"
    );
    // ...but enabled siblings remain.
    assert!(
        injected.contains("Single writer, read-only explorers"),
        "enabled sections must survive a sibling being disabled, got:\n{injected}"
    );
}

#[test]
fn custom_preamble_replaces_default_framing() {
    if !bash_available() {
        eprintln!("skipping: bash not available");
        return;
    }
    let cfg = tempfile::tempdir().unwrap();
    write_config(
        cfg.path(),
        "[reflex]\npreamble = \"BESPOKE REFLEX FRAMING LINE\"\n",
    );
    let stdout = ok_stdout(run_hook(None, cfg.path()));
    let injected = injected_context(&stdout);

    assert!(
        injected.contains("BESPOKE REFLEX FRAMING LINE"),
        "custom preamble must appear in the injected context, got:\n{injected}"
    );
    // A phrase unique to the default preamble (not the skill body) must be gone.
    assert!(
        !injected.contains("For all other skills, use the 'Skill' tool"),
        "custom preamble must replace the default framing, got:\n{injected}"
    );
    // The skill body still rides along under the custom framing.
    assert!(
        injected.contains("Single writer, read-only explorers"),
        "skill body must still be injected under a custom preamble, got:\n{injected}"
    );
}
