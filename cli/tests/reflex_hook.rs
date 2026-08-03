//! Tests drovr's two reflex hooks and their delegation to `drovr reflex`.
//!
//! `hooks/session-start` is the always-on reflex for human-facing agents: on a
//! normal Claude Code session it resolves `skills/using-drovr/SKILL.md` and execs
//! `drovr reflex --skill`, which renders the `SessionStart` additional context
//! shaped by the `[reflex]` config. Inside a drovr-spawned phase (signalled by
//! `DROVR_PHASE`) the hook no-ops *before* spawning drovr, so the phase runs
//! purely on its injected handoff.
//!
//! `hooks/user-prompt` is the per-turn gate: it execs `drovr reflex --gate` on
//! every `UserPromptSubmit`, passing the hook's stdin payload through so the CLI
//! can read the transcript and suppress a card the session does not need. Its two
//! deliberate differences from its sibling are both load-bearing and both tested
//! here: it does **not** suppress on `DROVR_PHASE` (a phase is exactly where the
//! discipline must hold), and stdin reaches the CLI (without it the suppression
//! rule can never fire, and the hook would look healthy while being useless).
//!
//! The scripts are exercised standalone via `bash`, with `CLAUDE_PLUGIN_ROOT`
//! pointed at the repo root (so it resolves the skill) and `DROVR_BIN` pointed at
//! the freshly built binary (so it doesn't depend on `drovr` being on PATH).
//! `XDG_CONFIG_HOME` is always pinned to a controlled dir so the reflex config is
//! hermetic — empty for the default-behavior tests, populated for the rest.
//! Every test is gated on `bash` being available so the suite stays green in odd
//! CI environments.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// The `SessionStart` reflex hook.
const SESSION_START: &str = "session-start";
/// The `UserPromptSubmit` per-turn gate hook.
const USER_PROMPT: &str = "user-prompt";

/// Repo root (one level up from this crate's manifest dir).
fn repo_root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
}

/// A hook script by file name — [`SESSION_START`] or [`USER_PROMPT`].
fn hook_script(name: &str) -> PathBuf {
    repo_root().join("hooks").join(name)
}

/// Locate the drovr binary built for this test. Points `DROVR_BIN` at it so the
/// hook execs the just-built CLI, not one on PATH. Uses cargo's
/// `CARGO_BIN_EXE_drovr` — the path to the built `drovr` bin, set for integration
/// tests — so it is hermetic across debug/release and the nix checkPhase sandbox
/// (where a hardcoded `target/debug/drovr` does not exist, so the hook execs a
/// missing binary and fails with 127).
fn drovr_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_drovr"))
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

/// Run `script` via `bash <script>` with a clean, controlled environment.
///
/// `drovr_phase` is `Some(value)` to set `DROVR_PHASE`, or `None` to guarantee it
/// is unset. `config_home` becomes `XDG_CONFIG_HOME`, so `drovr reflex` reads
/// `<config_home>/drovr/config.toml` (absent → reflex defaults). `stdin` is the
/// hook payload written to the child's stdin — `None` closes it, which is what a
/// `SessionStart` hook sees and is also the gate's "no payload" fail-open path.
fn run_hook_with_stdin(
    script: &str,
    drovr_phase: Option<&str>,
    config_home: &Path,
    stdin: Option<&str>,
) -> Output {
    let mut cmd = Command::new("bash");
    cmd.arg(hook_script(script))
        .env("CLAUDE_PLUGIN_ROOT", repo_root())
        .env("DROVR_BIN", drovr_binary())
        .env("XDG_CONFIG_HOME", config_home)
        // Force the deterministic Claude Code path: strip sibling-platform
        // markers that could branch the output elsewhere.
        .env_remove("CURSOR_PLUGIN_ROOT")
        .env_remove("COPILOT_CLI")
        .stdin(match stdin {
            Some(_) => Stdio::piped(),
            None => Stdio::null(),
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    match drovr_phase {
        Some(v) => {
            cmd.env("DROVR_PHASE", v);
        }
        None => {
            cmd.env_remove("DROVR_PHASE");
        }
    }

    let mut child = cmd
        .spawn()
        .unwrap_or_else(|e| panic!("failed to execute hooks/{script}: {e}"));
    if let Some(payload) = stdin {
        child
            .stdin
            .take()
            .expect("piped stdin")
            .write_all(payload.as_bytes())
            .expect("failed to write hook payload to stdin");
    }
    child
        .wait_with_output()
        .unwrap_or_else(|e| panic!("failed to collect hooks/{script} output: {e}"))
}

/// [`run_hook_with_stdin`] with no stdin payload.
fn run_hook(script: &str, drovr_phase: Option<&str>, config_home: &Path) -> Output {
    run_hook_with_stdin(script, drovr_phase, config_home, None)
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

/// Parse the hook stdout and return its injected `additionalContext`, asserting
/// the envelope names `expected_event`. The event name is a parameter because
/// Claude Code routes on it: a gate card labelled `SessionStart` would be
/// mis-delivered, and the two hooks share one envelope function.
fn injected_context(stdout: &str, expected_event: &str) -> String {
    let parsed: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("hook stdout must be valid JSON");
    assert_eq!(
        parsed["hookSpecificOutput"]["hookEventName"].as_str(),
        Some(expected_event),
        "hookEventName must be {expected_event}"
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
    let stdout = ok_stdout(run_hook(SESSION_START, Some("drovr-v2/plan"), cfg.path()));

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
    let stdout = ok_stdout(run_hook(SESSION_START, None, cfg.path()));
    let injected = injected_context(&stdout, "SessionStart");

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
    let stdout = ok_stdout(run_hook(SESSION_START, Some(""), cfg.path()));
    let injected = injected_context(&stdout, "SessionStart");
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
        .arg(hook_script(SESSION_START))
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
    let stdout = ok_stdout(run_hook(SESSION_START, None, cfg.path()));

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
    let stdout = ok_stdout(run_hook(SESSION_START, None, cfg.path()));
    let injected = injected_context(&stdout, "SessionStart");

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
    let stdout = ok_stdout(run_hook(SESSION_START, None, cfg.path()));
    let injected = injected_context(&stdout, "SessionStart");

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

// ---------------------------------------------------------------------------
// hooks/user-prompt — the per-turn gate
// ---------------------------------------------------------------------------

/// A `UserPromptSubmit` hook payload naming `transcript_path`. This is the only
/// way the gate can learn the previous turn already ran the discipline, so a
/// hook that drops stdin can never suppress — see
/// [`user_prompt_hook_passes_stdin_through_to_the_suppression_check`].
fn hook_payload(transcript_path: &Path) -> String {
    serde_json::json!({
        "session_id": "test-session",
        "transcript_path": transcript_path.to_str().expect("tempdir path is UTF-8"),
        "cwd": "/tmp",
        "hook_event_name": "UserPromptSubmit",
        "prompt": "do the thing",
    })
    .to_string()
}

/// The three records Claude Code writes for a successful `Skill` call: the
/// assistant's `tool_use`, its `tool_result`, and the `isMeta` record carrying
/// the skill's body. All three are required — the shape is documented in
/// `cli/src/reflex.rs`'s test module, and a fixture missing the third one tests
/// a transcript that does not exist.
fn skill_call_records(skill: &str) -> String {
    format!(
        concat!(
            r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"toolu_1","name":"Skill","input":{{"skill":"{skill}"}}}}]}}}}"#,
            "\n",
            r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"toolu_1","content":"ok"}}]}}}}"#,
            "\n",
            r#"{{"type":"user","isMeta":true,"sourceToolUseID":"toolu_1","message":{{"role":"user","content":[{{"type":"text","text":"Base directory for this skill: /x\n\n# TDD\n"}}]}}}}"#,
        ),
        skill = skill
    )
}

/// A real user prompt — the record that ends the previous turn.
fn user_prompt_record(text: &str) -> String {
    format!(r#"{{"type":"user","message":{{"role":"user","content":"{text}"}}}}"#)
}

/// Write `contents` to `<dir>/transcript.jsonl` and return the path.
fn write_transcript(dir: &Path, contents: &str) -> PathBuf {
    let path = dir.join("transcript.jsonl");
    std::fs::write(&path, contents).unwrap();
    path
}

#[test]
fn user_prompt_hook_emits_gate_json() {
    if !bash_available() {
        eprintln!("skipping: bash not available");
        return;
    }
    // Empty config dir → reflex defaults, per_turn on.
    let cfg = tempfile::tempdir().unwrap();
    let stdout = ok_stdout(run_hook(USER_PROMPT, None, cfg.path()));
    let injected = injected_context(&stdout, "UserPromptSubmit");

    // §4.2's budget is on the RENDERED additionalContext, not on the const.
    assert!(
        injected.len() <= 600,
        "gate card must be <= 600 rendered bytes, got {}:\n{injected}",
        injected.len()
    );
    // Prove the card is the card, not merely some well-formed envelope: two
    // phrases that exist only in GATE_CARD's body.
    assert!(
        injected.contains("<SUBAGENT-STOP>"),
        "gate card must carry its unconditional subagent-stop line, got:\n{injected}"
    );
    assert!(
        injected.contains("DROVR GATE"),
        "gate card must carry the gate header, got:\n{injected}"
    );
}

#[test]
fn user_prompt_hook_not_suppressed_in_phase() {
    if !bash_available() {
        eprintln!("skipping: bash not available");
        return;
    }
    // The deliberate asymmetry vs `suppressed_when_drovr_phase_set`: the
    // SessionStart reflex no-ops inside a drovr phase, the per-turn gate does
    // not. A phase is exactly where the discipline has to hold, and the phase
    // agent's briefing scrolls out of the window like anything else.
    let cfg = tempfile::tempdir().unwrap();
    let stdout = ok_stdout(run_hook(USER_PROMPT, Some("run/plan"), cfg.path()));
    let injected = injected_context(&stdout, "UserPromptSubmit");

    assert!(
        injected.contains("DROVR GATE"),
        "DROVR_PHASE must NOT suppress the per-turn gate, got:\n{injected}"
    );
}

#[test]
fn user_prompt_hook_respects_reflex_disabled() {
    if !bash_available() {
        eprintln!("skipping: bash not available");
        return;
    }
    let cfg = tempfile::tempdir().unwrap();
    write_config(cfg.path(), "[reflex]\nenabled = false\n");
    let stdout = ok_stdout(run_hook(USER_PROMPT, None, cfg.path()));

    assert!(
        stdout.trim().is_empty(),
        "the reflex master switch must silence the gate, but stdout was:\n{stdout}"
    );
}

#[test]
fn user_prompt_hook_respects_per_turn_false() {
    if !bash_available() {
        eprintln!("skipping: bash not available");
        return;
    }
    let cfg = tempfile::tempdir().unwrap();
    write_config(cfg.path(), "[reflex]\nper_turn = false\n");
    let stdout = ok_stdout(run_hook(USER_PROMPT, None, cfg.path()));

    assert!(
        stdout.trim().is_empty(),
        "per_turn = false must silence the gate, but stdout was:\n{stdout}"
    );
}

#[test]
fn user_prompt_hook_passes_stdin_through_to_the_suppression_check() {
    if !bash_available() {
        eprintln!("skipping: bash not available");
        return;
    }
    // The hook's whole job beyond exec'ing is handing the payload to the CLI,
    // and nothing else in this file can tell a dropped stdin from a working
    // one: with no payload the gate fails open and emits, which is also what a
    // healthy hook does on a drifted session. So drive the ONE input that
    // makes the CLI go silent — a transcript whose last turn invoked a
    // `drovr:*` skill — and assert silence. If `exec` ever stopped forwarding
    // stdin, or the script consumed it (a `read`, a heredoc), this is the test
    // that goes red.
    let cfg = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let transcript = write_transcript(
        work.path(),
        &format!(
            "{}\n{}\n",
            user_prompt_record("fix the parser"),
            skill_call_records("drovr:tdd")
        ),
    );
    let out = run_hook_with_stdin(
        USER_PROMPT,
        None,
        cfg.path(),
        Some(&hook_payload(&transcript)),
    );
    let stdout = ok_stdout(out);

    assert!(
        stdout.trim().is_empty(),
        "a previous turn that invoked a drovr:* skill must suppress the card \
         (empty stdout here proves the payload reached the CLI), but stdout was:\n{stdout}"
    );
}

#[test]
fn user_prompt_hook_emits_when_last_turn_invoked_no_skill() {
    if !bash_available() {
        eprintln!("skipping: bash not available");
        return;
    }
    // The other half of the pair above. Same payload plumbing, same transcript
    // records — but the skill call sits BEFORE the last user prompt, so it
    // belongs to an earlier turn and must not suppress. Without this case the
    // suppression test would pass just as well against a hook that emitted
    // nothing at all, ever.
    let cfg = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let transcript = write_transcript(
        work.path(),
        &format!(
            "{}\n{}\n{}\n",
            user_prompt_record("fix the parser"),
            skill_call_records("drovr:tdd"),
            user_prompt_record("now ship it")
        ),
    );
    let out = run_hook_with_stdin(
        USER_PROMPT,
        None,
        cfg.path(),
        Some(&hook_payload(&transcript)),
    );
    let injected = injected_context(&ok_stdout(out), "UserPromptSubmit");

    assert!(
        injected.contains("DROVR GATE"),
        "a skill call in an EARLIER turn must not suppress the card, got:\n{injected}"
    );
}

#[test]
fn user_prompt_hook_missing_binary_fails_loudly() {
    if !bash_available() {
        eprintln!("skipping: bash not available");
        return;
    }
    // `exec` is what makes this true: a missing binary surfaces as a non-zero
    // exit rather than an empty stdout that Claude Code would read as a
    // legitimate "nothing to inject".
    let cfg = tempfile::tempdir().unwrap();
    let out = Command::new("bash")
        .arg(hook_script(USER_PROMPT))
        .env("CLAUDE_PLUGIN_ROOT", repo_root())
        .env("DROVR_BIN", "/nonexistent/definitely-not-drovr")
        .env("XDG_CONFIG_HOME", cfg.path())
        .env_remove("DROVR_PHASE")
        .stdin(Stdio::null())
        .output()
        .expect("failed to execute hooks/user-prompt");
    assert!(
        !out.status.success(),
        "a missing binary must make the gate hook exit non-zero, got stdout:\n{}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn hooks_json_user_prompt_entry_has_no_matcher() {
    let raw = std::fs::read_to_string(repo_root().join("hooks/hooks.json"))
        .expect("hooks/hooks.json must be readable");
    let parsed: serde_json::Value =
        serde_json::from_str(&raw).expect("hooks/hooks.json must be valid JSON");

    let user_prompt = parsed["hooks"]["UserPromptSubmit"]
        .as_array()
        .expect("hooks.json must declare a UserPromptSubmit array");
    assert_eq!(
        user_prompt.len(),
        1,
        "expected exactly one UserPromptSubmit entry, got {user_prompt:#?}"
    );
    // `UserPromptSubmit` takes no matcher. Copying SessionStart's
    // `startup|clear|compact` across would be silently accepted and would gate
    // the hook on an event kind that never fires here.
    assert!(
        user_prompt[0].get("matcher").is_none(),
        "the UserPromptSubmit entry must carry no matcher key, got {:#?}",
        user_prompt[0]
    );
    assert_eq!(
        user_prompt[0]["hooks"][0]["command"].as_str(),
        Some("\"${CLAUDE_PLUGIN_ROOT}/hooks/user-prompt\""),
        "the UserPromptSubmit entry must run hooks/user-prompt"
    );

    // ...and the sibling keeps its matcher: this test exists to pin the
    // DIFFERENCE, so asserting only the new entry would stay green if someone
    // "harmonised" the two by deleting SessionStart's.
    let session_start = parsed["hooks"]["SessionStart"]
        .as_array()
        .expect("hooks.json must still declare a SessionStart array");
    assert_eq!(
        session_start[0]["matcher"].as_str(),
        Some("startup|clear|compact"),
        "SessionStart must keep its matcher"
    );
}

#[test]
fn user_prompt_hook_is_executable() {
    // hooks.json invokes the script directly, not via `bash <script>` as these
    // tests do, so the mode bit is part of the contract and nothing else here
    // would notice it missing.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(hook_script(USER_PROMPT))
            .expect("hooks/user-prompt must exist")
            .permissions()
            .mode();
        assert!(
            mode & 0o111 != 0,
            "hooks/user-prompt must be executable, mode is {mode:o}"
        );
    }
}

#[test]
fn user_prompt_hook_needs_no_plugin_root() {
    if !bash_available() {
        eprintln!("skipping: bash not available");
        return;
    }
    // The gate card is a const in the CLI, so unlike its sibling this hook needs
    // no plugin root — and must not acquire a resolution step that could fail.
    // `hooks/session-start`'s fallback (`cd "$(dirname "$0")/.." && pwd`) aborts
    // under `set -e` when that directory is unreachable, which would silence the
    // gate for a reason unrelated to the gate. This pins the absence.
    let cfg = tempfile::tempdir().unwrap();
    let out = Command::new("bash")
        .arg(hook_script(USER_PROMPT))
        .env_remove("CLAUDE_PLUGIN_ROOT")
        .env("DROVR_BIN", drovr_binary())
        .env("XDG_CONFIG_HOME", cfg.path())
        .env_remove("DROVR_PHASE")
        .stdin(Stdio::null())
        .output()
        .expect("failed to execute hooks/user-prompt");
    let injected = injected_context(&ok_stdout(out), "UserPromptSubmit");
    assert!(
        injected.contains("DROVR GATE"),
        "the gate must emit with no CLAUDE_PLUGIN_ROOT set, got:\n{injected}"
    );
}
