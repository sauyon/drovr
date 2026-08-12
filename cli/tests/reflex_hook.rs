//! Tests drovr's two reflex hooks and their delegation to `drovr reflex`.
//!
//! `hooks/session-start` is the always-on reflex for human-facing agents: on a
//! normal Claude Code session it resolves `skills/using-drovr/SKILL.md` and execs
//! `drovr reflex --skill`, which renders the `SessionStart` additional context
//! shaped by the `[reflex]` config. Inside a drovr-spawned phase (signalled by
//! `DROVR_PHASE`) the hook no-ops *before* spawning drovr, so the phase runs
//! purely on its injected handoff.
//!
//! `hooks/user-prompt` is the per-turn gate: it runs `drovr reflex --gate` on
//! every `UserPromptSubmit`, passing the hook's stdin payload through so the CLI
//! can read the transcript and suppress a card the session does not need.
//!
//! Several deliberate differences from its sibling, each load-bearing. No count
//! is given: two earlier attempts at one were both wrong, because the split
//! between the script and the CLI path it reaches is not crisp. These four live
//! in the script's own logic:
//!
//!   1. it does **not** suppress on `DROVR_PHASE` — a phase is exactly where the
//!      discipline must hold;
//!   2. stdin reaches the CLI — without it the suppression rule can never fire,
//!      and the hook would look healthy while being useless;
//!   3. a missing `drovr` degrades **silently**, where the SessionStart reflex
//!      fails loudly. The plugin installs without the CLI, and complaining once
//!      per session is right where complaining once per *prompt* is not;
//!   4. it never exits **2**. Measured on Claude Code 2.1.220, exit 2 from a
//!      `UserPromptSubmit` hook discards the user's prompt unprocessed, and clap
//!      exits 2 on a usage error — so an `exec` plus a `drovr` predating
//!      `--gate` would erase every prompt typed.
//!
//! …and these are also pinned here, though they are not all script-level:
//!
//!   - it resolves no plugin root, where session-start must
//!     (`user_prompt_hook_needs_no_plugin_root` — a script difference);
//!   - it fails OPEN on an unloadable `config.toml` and runs on defaults, where
//!     session-start exits 1 (`user_prompt_hook_emits_on_unloadable_config` /
//!     `session_start_hook_exits_nonzero_on_unloadable_config` — a CLI one).
//!
//! Other CLI-side differences are real but live in `cli/src/`: only the gate
//! reads stdin and a transcript, only session mode has a mandatory file read
//! that exits 1, and `preamble`/`[reflex.sections]` shape the session reflex
//! while merely switching the gate. **That last one is not asserted anywhere** —
//! nothing proves a `preamble` config leaves the gate card unchanged.
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
/// These transcript fixtures deliberately duplicate the shapes in
/// `cli/src/reflex.rs`'s test module rather than sharing a builder with it, and
/// the duplication is structural, not an oversight: `cli/Cargo.toml` declares a
/// `[[bin]]` target with no `[lib]`, so `cli/tests/*.rs` are separate
/// compilation units with **no** path to `src/`. `skills_valid.rs` reimplements
/// `parse_skill` for the same reason. Extracting a shared builder means adding a
/// library target — a bigger call than a test helper should make. If these ever
/// drift from the CLI's fixtures, the CLI's are authoritative: they are the ones
/// verified against live transcripts.
///
/// `id` is a parameter, not a constant: two calls sharing one id would let the
/// second's `tool_result` credit the first's `tool_use` as having succeeded,
/// quietly making a multi-call fixture assert something other than it reads.
fn skill_call_records(skill: &str, id: &str) -> String {
    format!(
        concat!(
            r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"{id}","name":"Skill","input":{{"skill":"{skill}"}}}}]}}}}"#,
            "\n",
            r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"{id}","content":"ok"}}]}}}}"#,
            "\n",
            r#"{{"type":"user","isMeta":true,"sourceToolUseID":"{id}","message":{{"role":"user","content":[{{"type":"text","text":"Base directory for this skill: /x\n\n# TDD\n"}}]}}}}"#,
        ),
        skill = skill,
        id = id
    )
}

/// A real user prompt — the record that ends the previous turn.
fn user_prompt_record(text: &str) -> String {
    format!(r#"{{"type":"user","message":{{"role":"user","content":"{text}"}}}}"#)
}

/// An assistant record carrying one `text` block — the previous turn's reply.
fn assistant_text_record(text: &str) -> String {
    format!(
        r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"{text}"}}]}}}}"#
    )
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
    // The hook's whole job beyond invoking drovr is handing the payload to it,
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
            skill_call_records("drovr:tdd", "toolu_1")
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
    //
    // THE TRAILING `user` RECORD HERE IS AN EARLIER TURN'S PROMPT, NOT THIS
    // TURN'S. It does not model the transcript as it looks when the hook fires
    // — at that moment the submitting prompt is NOT yet a `type: "user"` record
    // (see `user_prompt_hook_suppresses_on_the_real_hook_time_tail`). Reading
    // this fixture as the hook-time shape suggests suppression can never fire
    // in production, which is the opposite of what it does.
    let cfg = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let transcript = write_transcript(
        work.path(),
        &format!(
            "{}\n{}\n{}\n",
            user_prompt_record("fix the parser"),
            skill_call_records("drovr:tdd", "toolu_1"),
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

/// The records Claude Code appends after the assistant's reply, which are what
/// actually sit at the tail when `UserPromptSubmit` fires.
///
/// **Captured from a live 2-turn session, not invented.** The submitting prompt
/// is NOT written as a `type: "user"` record before the hook runs — it arrives
/// as `type: "last-prompt"`, followed by a `type: "mode"` record. Neither is a
/// turn boundary, so the backward walk passes them and reaches the previous
/// turn. Everything downstream of the suppression rule depends on that.
fn hook_time_tail(prompt: &str) -> String {
    format!(
        concat!(
            r#"{{"type":"last-prompt","lastPrompt":"{prompt}","leafUuid":"u1","sessionId":"s1"}}"#,
            "\n",
            r#"{{"type":"mode","mode":"normal","sessionId":"s1"}}"#,
        ),
        prompt = prompt
    )
}

#[test]
fn user_prompt_hook_suppresses_on_the_real_hook_time_tail() {
    if !bash_available() {
        eprintln!("skipping: bash not available");
        return;
    }
    // THE PRODUCTION CASE, and the one every other suppression test was missing:
    // a transcript shaped exactly as it is at the moment the hook fires.
    //
    // Every other fixture here ends at the previous turn's last record, which
    // quietly assumes the submitting prompt is not yet on disk. That assumption
    // is true, but it was never tested — and a reviewer reading the fixture in
    // `..._emits_when_last_turn_invoked_no_skill` as the hook-time shape
    // concluded suppression could never fire in production at all. It does; this
    // pins why. Measured on a live 2-turn session: at fire time the tail is the
    // previous turn's assistant record, then `last-prompt`, then `mode`.
    //
    // If Claude Code ever starts writing the submitting prompt as a `user`
    // record before the hook, this test goes red — and it should, because the
    // suppression rule and the cost bound the whole mechanism claims would both
    // be gone.
    let cfg = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let transcript = write_transcript(
        work.path(),
        &format!(
            "{}\n{}\n{}\n{}\n",
            user_prompt_record("fix the parser"),
            skill_call_records("drovr:tdd", "toolu_1"),
            assistant_text_record("done"),
            hook_time_tail("now ship it")
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
        "with the REAL hook-time tail, a previous turn that ran a drovr:* skill \
         must suppress the card, but stdout was:\n{stdout}"
    );
}

#[test]
fn user_prompt_hook_emits_on_the_real_hook_time_tail_without_a_skill() {
    if !bash_available() {
        eprintln!("skipping: bash not available");
        return;
    }
    // Non-vacuity partner for the case above: same hook-time tail, no skill call
    // in the previous turn. Without this, the suppression test would pass
    // equally against a gate that had gone silent for some unrelated reason —
    // an unreadable transcript, say, which also produces empty stdout.
    let cfg = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let transcript = write_transcript(
        work.path(),
        &format!(
            "{}\n{}\n{}\n",
            user_prompt_record("fix the parser"),
            assistant_text_record("done"),
            hook_time_tail("now ship it")
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
        "with the real hook-time tail and no skill last turn, the card must be \
         emitted, got:\n{injected}"
    );
}

/// Run `hooks/user-prompt` with `DROVR_BIN` pointed at `bin`.
fn run_gate_with_binary(bin: &Path, config_home: &Path) -> Output {
    Command::new("bash")
        .arg(hook_script(USER_PROMPT))
        .env("CLAUDE_PLUGIN_ROOT", repo_root())
        .env("DROVR_BIN", bin)
        .env("XDG_CONFIG_HOME", config_home)
        .env_remove("DROVR_PHASE")
        // Kept identical to `run_hook_with_stdin` so the two helpers cannot
        // drift. Nothing in `cli/src` or `hooks/` reads these today; the
        // scrubbing is defensive, not load-bearing.
        .env_remove("CURSOR_PLUGIN_ROOT")
        .env_remove("COPILOT_CLI")
        .stdin(Stdio::null())
        .output()
        .expect("failed to execute hooks/user-prompt")
}

/// Write an executable stub at `<dir>/<name>` that exits `code`.
fn write_stub(dir: &Path, name: &str, code: i32) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(
        &path,
        // `#!/bin/sh`, NOT `#!/usr/bin/env bash`. This stub is a fake `drovr`,
        // not a shipped script, and its body is POSIX (`echo`, `exit`). A nix
        // build sandbox has no `/usr/bin/env`, so an `env` shebang here made
        // the stub unexecutable and the assertion below reported it as "drovr
        // did not let its own stderr through" — a real packaging failure
        // wearing the costume of a drovr defect. `/bin/sh` is present wherever
        // this suite can run at all. The SHIPPED hooks keep their
        // `#!/usr/bin/env bash` (pinned by `hook_shebangs_are_env_bash`); this
        // is only the test's own throwaway binary.
        format!("#!/bin/sh\necho boom >&2\nexit {code}\n"),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    path
}

#[test]
fn user_prompt_hook_degrades_silently_without_drovr() {
    if !bash_available() {
        eprintln!("skipping: bash not available");
        return;
    }
    // The plugin installs on its own while the CLI is a separate build-and-PATH
    // step, so "plugin without binary" is a normal state. There is no discipline
    // to re-assert when drovr is not installed, and complaining here would
    // complain before EVERY prompt of EVERY session. hooks/session-start still
    // fails loudly — once per session, which is the right number of times.
    let cfg = tempfile::tempdir().unwrap();
    let out = run_gate_with_binary(Path::new("/nonexistent/definitely-not-drovr"), cfg.path());

    assert!(
        out.status.success(),
        "a missing drovr must let the turn through cleanly, got status={:?} stderr={}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.stdout.is_empty(),
        "a missing drovr must inject nothing, got stdout:\n{}",
        String::from_utf8_lossy(&out.stdout)
    );
    // "Silently" is half the name of this test, and stdout alone does not test
    // it. Adding an `echo … >&2` beside the guard's `exit 0` would keep every
    // other assertion green while putting a line in front of EVERY prompt of
    // EVERY session for anyone who installed the plugin without the CLI — the
    // exact outcome the guard exists to prevent.
    assert!(
        out.stderr.is_empty(),
        "a missing drovr must say nothing at all, got stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn session_start_hook_still_fails_loudly_without_drovr() {
    if !bash_available() {
        eprintln!("skipping: bash not available");
        return;
    }
    // The asymmetry's other half, asserted so the silent-degrade above cannot be
    // "harmonised" onto the hook that should stay loud. `missing_binary_fails_
    // loudly` already pins the exit code; what this adds is the LOUD half —
    // stderr must actually reach the user, which is the whole reason
    // session-start is allowed to be noisy where the gate is not.
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
        "the SessionStart reflex must still fail loudly when drovr is missing"
    );
    // The diagnostic here is bash's own exec failure, not something the script
    // prints — what this pins is that session-start does not SWALLOW it (an added
    // `2>/dev/null` on the exec line is the mutation), where the gate
    // deliberately produces nothing at all.
    assert!(
        !out.stderr.is_empty(),
        "\"loudly\" means the user is told: session-start must let the failure \
         reach stderr when drovr is missing, where the gate says nothing"
    );
}

#[test]
fn user_prompt_hook_never_exits_two() {
    if !bash_available() {
        eprintln!("skipping: bash not available");
        return;
    }
    // MEASURED on Claude Code 2.1.220: exit 2 from a UserPromptSubmit hook is a
    // BLOCKING error — the user's prompt is discarded unprocessed. clap exits 2
    // on a usage error, and `drovr` is installed by hand independently of the
    // plugin's hooks/, so a binary predating `--gate` is the expected skew. With
    // `exec`, that skew would erase every prompt the user typed. Every failure
    // must therefore map to a non-blocking code.
    let cfg = tempfile::tempdir().unwrap();
    let stub_dir = tempfile::tempdir().unwrap();

    for code in [2, 1, 127] {
        let stub = write_stub(stub_dir.path(), &format!("drovr-exit-{code}"), code);
        let out = run_gate_with_binary(&stub, cfg.path());
        // Exactly 1, not merely "not 2". The measurement behind this covers
        // two codes only (2 blocks, 127 does not); nothing establishes that an
        // arbitrary code is non-blocking on this harness, so the mapping must
        // land on the one code that was actually observed to be safe. `!= 2`
        // alone would stay green if the mapping drifted to `exit 3`.
        assert_eq!(
            out.status.code(),
            Some(1),
            "a drovr exiting {code} must be mapped to exit 1, never to a \
             prompt-blocking 2 and never to an unmeasured code"
        );
        // The failure must stay LOUD. The whole argument for tolerating noise
        // on the version-skew path is that the user gets a clue; adding a
        // `2>/dev/null` to the invocation would leave this suite green and
        // produce exactly the quiet-and-broken state the design rejects.
        // Pin whose stderr it is. `2>/dev/null || { echo "drovr failed" >&2; …}`
        // would keep this non-empty while destroying clap's actual message —
        // and clap's message is the version-skew clue the entire
        // tolerate-the-noise argument rests on.
        let stderr = String::from_utf8_lossy(&out.stderr);
        // Separate "the stub never ran" from "drovr's stderr was swallowed".
        // They fail this assertion identically, and conflating them cost a
        // packaging outage: a nix sandbox with no `/usr/bin/env` made the stub
        // unexecutable, and this assertion reported it as drovr suppressing
        // stderr, so `main` was unbuildable while the message pointed at the
        // wrong subsystem entirely.
        assert!(
            !stderr.contains("bad interpreter") && !stderr.contains("No such file or directory"),
            "the stub itself failed to execute, so this test measured nothing about drovr's \
             exit-code mapping — fix the stub's interpreter, not drovr: {stderr:?}"
        );
        assert!(
            stderr.contains("boom"),
            "a drovr exiting {code} must let ITS OWN stderr reach the user, got: {stderr:?}"
        );
    }
}

#[test]
fn user_prompt_hook_emits_when_transcript_path_is_absent() {
    if !bash_available() {
        eprintln!("skipping: bash not available");
        return;
    }
    // The fail-open bullet that had no end-to-end coverage: a payload naming a
    // transcript that cannot be read must still emit. Approaching it from the
    // `None` side inside the CLI misses it — a "defensive" early return in
    // `cmd_reflex_gate` for an unreadable transcript would leave the whole suite
    // green while the gate went permanently silent for those users.
    let cfg = tempfile::tempdir().unwrap();
    let out = run_hook_with_stdin(
        USER_PROMPT,
        None,
        cfg.path(),
        Some(&hook_payload(Path::new(
            "/nonexistent/dir/transcript.jsonl",
        ))),
    );
    let injected = injected_context(&ok_stdout(out), "UserPromptSubmit");

    assert!(
        injected.contains("DROVR GATE"),
        "an unreadable transcript path must not suppress the card, got:\n{injected}"
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
    // Bound the INNER array too: duplicating the command object leaves every
    // assertion below passing while injecting the card twice per turn — 1094
    // bytes, straight through the ~60 KB/100-turn ceiling the design budgets.
    assert_eq!(
        user_prompt[0]["hooks"].as_array().map(Vec::len),
        Some(1),
        "the UserPromptSubmit entry must declare exactly one command hook"
    );
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
    // Without this, mutating `type` to anything else makes Claude Code skip the
    // hook entirely while the only guard on the wiring stays green.
    assert_eq!(
        user_prompt[0]["hooks"][0]["type"].as_str(),
        Some("command"),
        "the UserPromptSubmit hook must be a command hook"
    );
    // A hook that runs before every prompt must be bounded: the default is 60s,
    // and a stalled filesystem under `transcript_path` would hold the user's
    // prompt for all of it. Measured cost is 8-10 ms, so 5s is ample headroom.
    assert_eq!(
        user_prompt[0]["hooks"][0]["timeout"].as_u64(),
        Some(5),
        "the UserPromptSubmit hook must carry a timeout"
    );
    // Same class as `type`: flipping this to true means the hook's stdout is no
    // longer merged into the prompt, so the card silently stops reaching the
    // model while every bash-driven test in this file stays green.
    assert_eq!(
        user_prompt[0]["hooks"][0]["async"].as_bool(),
        Some(false),
        "the UserPromptSubmit hook must be synchronous, or its stdout is dropped"
    );

    // ...and the sibling keeps its matcher: this test exists to pin the
    // DIFFERENCE, so asserting only the new entry would stay green if someone
    // "harmonised" the two by deleting SessionStart's.
    let session_start = parsed["hooks"]["SessionStart"]
        .as_array()
        .expect("hooks.json must still declare a SessionStart array");
    assert!(
        !session_start.is_empty(),
        "hooks.json must still declare a SessionStart entry"
    );
    assert_eq!(
        session_start[0]["matcher"].as_str(),
        Some("startup|clear|compact"),
        "SessionStart must keep its matcher"
    );
}

#[test]
fn user_prompt_hook_is_executable_in_the_index() {
    // hooks.json invokes the script directly, not via `bash <script>` as these
    // tests do, so the mode bit is part of the contract.
    //
    // Assert on the INDEX, not the working tree. `git update-index --chmod=-x`
    // ships a non-executable hook while leaving the checkout untouched: a
    // working-tree assertion stays green in the very checkout where the mistake
    // is made and breaks only for downstream users on a fresh clone. What is
    // distributed is what must be checked.
    let out = Command::new("git")
        .arg("-C")
        .arg(repo_root())
        .args(["ls-files", "-s", "hooks/user-prompt"])
        .output();
    let Ok(out) = out else {
        eprintln!("skipping: git not available");
        return;
    };
    if !out.status.success() {
        eprintln!("skipping: git ls-files failed (not a repo?)");
        return;
    }
    let listing = String::from_utf8_lossy(&out.stdout);
    assert!(
        listing.starts_with("100755 "),
        "hooks/user-prompt must be mode 100755 in the index, got: {listing:?}"
    );
}

#[test]
fn user_prompt_hook_needs_no_plugin_root() {
    if !bash_available() {
        eprintln!("skipping: bash not available");
        return;
    }
    // The gate card is a const in the CLI, so unlike its sibling this hook needs
    // no plugin root — and must not acquire a resolution step that can fail.
    // `hooks/session-start`'s fallback (`SCRIPT_DIR="$(cd "$(dirname "$0")" &&
    // pwd)"` then `PLUGIN_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"`) aborts under
    // `set -e` when either directory is unreachable, which would silence the
    // gate for a reason unrelated to the gate.
    //
    // THE BEHAVIOURAL HALF CANNOT PIN THIS, and on its own it is a test that
    // passes while checking nothing: the resolution block SUCCEEDS with
    // CLAUDE_PLUGIN_ROOT unset in any healthy checkout, so restoring it verbatim
    // leaves this green. An absence is only expressible as an assertion about
    // the source text, so assert both — the behaviour, and the absence itself.
    let script = std::fs::read_to_string(hook_script(USER_PROMPT))
        .expect("hooks/user-prompt must be readable");
    let operative: String = script
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    for forbidden in [
        "CLAUDE_PLUGIN_ROOT",
        "PLUGIN_ROOT",
        "dirname",
        "BASH_SOURCE",
        "realpath",
        "readlink",
        "${0%",
    ] {
        assert!(
            !operative.contains(forbidden),
            "hooks/user-prompt must not resolve a plugin root, but its code \
             contains {forbidden:?}:\n{operative}"
        );
    }

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

#[test]
fn user_prompt_hook_emits_on_unloadable_config() {
    if !bash_available() {
        eprintln!("skipping: bash not available");
        return;
    }
    // The fail-open bullet with no coverage until now. `load_config` fails for
    // reasons that have nothing to do with `[reflex]` — a typo anywhere in
    // config.toml — and the SessionStart reflex deliberately exits 1 there.
    // The gate must do the opposite: warn and run on defaults, because going
    // quiet for the rest of the session over an unrelated typo is exactly the
    // silent drift it exists to prevent.
    let cfg = tempfile::tempdir().unwrap();
    write_config(cfg.path(), "[reflex\nenabled = = false\n");
    let out = run_hook(USER_PROMPT, None, cfg.path());
    let out_stderr = out.stderr.clone();
    let injected = injected_context(&ok_stdout(out), "UserPromptSubmit");

    assert!(
        injected.contains("DROVR GATE"),
        "an unloadable config must not silence the gate, got:\n{injected}"
    );
    // Running on defaults is only half the contract; the user must also be told
    // their config.toml is broken. Deleting the eprintln in `cmd_reflex` would
    // otherwise leave the gate quietly ignoring a typo forever.
    let stderr = String::from_utf8_lossy(&out_stderr);
    assert!(
        stderr.contains("failed to load config"),
        "an unloadable config must warn on stderr with the CLI's own message, \
         got: {stderr:?}"
    );
}

#[test]
fn session_start_hook_exits_nonzero_on_unloadable_config() {
    if !bash_available() {
        eprintln!("skipping: bash not available");
        return;
    }
    // The other half of the asymmetry above, asserted so the two hooks cannot
    // quietly converge: the SessionStart reflex injects a whole skill body, and
    // a half-read config would frame it wrongly, so it fails LOUD.
    let cfg = tempfile::tempdir().unwrap();
    write_config(cfg.path(), "[reflex\nenabled = = false\n");
    let out = run_hook(SESSION_START, None, cfg.path());

    assert!(
        !out.status.success(),
        "the SessionStart reflex must exit non-zero on an unloadable config, got stdout:\n{}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn user_prompt_hook_resolves_drovr_from_path() {
    if !bash_available() {
        eprintln!("skipping: bash not available");
        return;
    }
    // THE SHIPPING CONFIGURATION, which no other test in this file exercises:
    // `DROVR_BIN` unset, `drovr` found on PATH. Every other case pins
    // `DROVR_BIN` to an absolute path, so the whole `${DROVR_BIN:-drovr}`
    // default and the PATH lookup were untested — and combined with the silent
    // degrade, a break there is invisible. Two one-character mutations silence
    // the gate for every real user while leaving the rest of the suite green:
    //
    //   DROVR="${DROVR_BIN:-drovr}"  ->  DROVR="${DROVR_BIN:-}"
    //       `command -v ""` returns 1, so the guard exits 0. Forever, silently.
    //   command -v "$DROVR" ...      ->  [ -x "$DROVR" ] ...
    //       a bare name is not a path, so `-x` fails and the guard exits 0.
    //
    // This is the exact failure the design says it fears — a hook that looks
    // healthy while being useless — and `exit 0` guarantees no diagnostics.
    let cfg = tempfile::tempdir().unwrap();
    let bin_dir = tempfile::tempdir().unwrap();

    // A stub *named* `drovr`, reachable only via PATH.
    let stub = bin_dir.path().join("drovr");
    std::fs::copy(drovr_binary(), &stub).expect("failed to stage a drovr on PATH");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    // Prepended, not replaced: `bash` and its own helpers must stay reachable.
    // Being first is what matters — it shadows any real `drovr` on the ambient
    // PATH, so the stub is the one that answers.
    let path = match std::env::var_os("PATH") {
        Some(existing) => {
            let mut dirs = vec![bin_dir.path().to_path_buf()];
            dirs.extend(std::env::split_paths(&existing));
            std::env::join_paths(dirs).expect("failed to build PATH")
        }
        None => bin_dir.path().as_os_str().to_owned(),
    };

    let out = Command::new("bash")
        .arg(hook_script(USER_PROMPT))
        .env("CLAUDE_PLUGIN_ROOT", repo_root())
        // The point of the test: DROVR_BIN is NOT set.
        .env_remove("DROVR_BIN")
        .env("PATH", path)
        .env("XDG_CONFIG_HOME", cfg.path())
        .env_remove("DROVR_PHASE")
        .env_remove("CURSOR_PLUGIN_ROOT")
        .env_remove("COPILOT_CLI")
        .stdin(Stdio::null())
        .output()
        .expect("failed to execute hooks/user-prompt");
    let injected = injected_context(&ok_stdout(out), "UserPromptSubmit");

    assert!(
        injected.contains("DROVR GATE"),
        "the gate must emit when drovr is resolved from PATH, got:\n{injected}"
    );
}

#[test]
fn user_prompt_hook_runs_as_hooks_json_invokes_it() {
    // Every other test in this file spawns `bash <script>`, which bypasses the
    // shebang entirely — so the interpreter line was stated in comments and
    // covered by nothing. `hooks.json` runs the file DIRECTLY, so the shebang,
    // the exec bit and the real invocation path are all part of the contract.
    //
    // The mutation that matters: `#!/usr/bin/env bash` -> `#!/usr/bin/env sh`.
    // Where `sh` is dash, `set -o pipefail` is unrecognised and the shell exits
    // **2** — the one code this whole design exists never to emit, arriving
    // before any of the script's own logic runs. `#!/bin/bash` is a quieter
    // version of the same class: it breaks NixOS and Guix outright.
    // The behavioural half below CANNOT catch the shebang mutation on every
    // machine, and does not catch it on a machine where `/bin/sh` is bash — as
    // it is on Arch, where this was written. The breakage is real but shows up
    // only where `sh` is dash. So assert the interpreter as TEXT, which is
    // deterministic everywhere, and keep the behavioural half for the exec bit
    // and the real invocation path.
    let script = std::fs::read_to_string(hook_script(USER_PROMPT))
        .expect("hooks/user-prompt must be readable");
    assert_eq!(
        script.lines().next(),
        Some("#!/usr/bin/env bash"),
        "hooks/user-prompt must declare bash via env: a bare `#!/bin/bash` \
         breaks NixOS/Guix, and `sh` (dash) dies on `set -o pipefail` with \
         exit 2 — the one code this hook must never emit"
    );

    #[cfg(unix)]
    {
        // Bound to a local, like every other test here: inlining the tempdir
        // relies on the temporary outliving the statement. It does today, but
        // splitting the builder across statements would silently point
        // XDG_CONFIG_HOME at a deleted directory — and the failure mode (no
        // config -> defaults -> still green) is exactly why that would go
        // unnoticed.
        let cfg = tempfile::tempdir().unwrap();
        let out = Command::new(hook_script(USER_PROMPT))
            .env("CLAUDE_PLUGIN_ROOT", repo_root())
            .env("DROVR_BIN", drovr_binary())
            .env("XDG_CONFIG_HOME", cfg.path())
            .env_remove("DROVR_PHASE")
            .stdin(Stdio::null())
            .output();

        let out = match out {
            Ok(out) => out,
            Err(e) => {
                eprintln!("skipping: cannot exec hooks/user-prompt directly ({e})");
                return;
            }
        };
        assert_ne!(
            out.status.code(),
            Some(2),
            "invoked directly, the hook must not exit 2 — stderr:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let injected = injected_context(&ok_stdout(out), "UserPromptSubmit");
        assert!(
            injected.contains("DROVR GATE"),
            "the hook must emit when run the way hooks.json runs it, got:\n{injected}"
        );
    }
}

#[test]
fn session_only_config_keys_do_not_reach_the_gate_card() {
    if !bash_available() {
        eprintln!("skipping: bash not available");
        return;
    }
    // The reviewer's exact gap: `preamble` and `[reflex.sections]` shape the
    // SessionStart reflex and must not touch the gate card, and nothing
    // asserted it. The `session()` / `gate()` split now makes reading them
    // from the gate a compile error — but a type cannot prove the END-TO-END
    // behaviour, only that one function cannot name one field. So drive both
    // hooks through the SAME config and assert the split holds where a user
    // would see it.
    let cfg = tempfile::tempdir().unwrap();
    write_config(
        cfg.path(),
        "[reflex]\npreamble = \"BESPOKE REFLEX FRAMING LINE\"\n\n\
         [reflex.sections]\nescalation = false\n",
    );

    let gate = injected_context(
        &ok_stdout(run_hook(USER_PROMPT, None, cfg.path())),
        "UserPromptSubmit",
    );
    let baseline = {
        let plain = tempfile::tempdir().unwrap();
        injected_context(
            &ok_stdout(run_hook(USER_PROMPT, None, plain.path())),
            "UserPromptSubmit",
        )
    };
    assert_eq!(
        gate, baseline,
        "session-only keys must leave the gate card byte-identical"
    );
    assert!(
        !gate.contains("BESPOKE REFLEX FRAMING LINE"),
        "the preamble must not reach the gate card, got:\n{gate}"
    );

    // ...and the same config demonstrably DOES reach the SessionStart reflex,
    // so this is a test about routing rather than a test that the config file
    // was ignored altogether.
    let session = injected_context(
        &ok_stdout(run_hook(SESSION_START, None, cfg.path())),
        "SessionStart",
    );
    assert!(
        session.contains("BESPOKE REFLEX FRAMING LINE"),
        "the preamble must still reach the SessionStart reflex, got:\n{session}"
    );
    assert!(
        !session.contains("Escalation contract"),
        "the disabled section must still be subtracted from the SessionStart reflex"
    );
}

#[test]
fn gate_only_config_keys_do_not_reach_the_session_reflex() {
    if !bash_available() {
        eprintln!("skipping: bash not available");
        return;
    }
    // The mirror of `session_only_config_keys_do_not_reach_the_gate_card`, and
    // the half that had no test at all. `per_turn` governs the gate; it must not
    // touch the SessionStart reflex.
    //
    // The mutation this exists to catch is a one-liner in `cmd_reflex`: folding
    // `per_turn` into the `enabled` check, or an early
    // `if !cfg.reflex.per_turn { return; }`. Every user who set
    // `per_turn = false` to quiet the per-turn card would silently lose the
    // router injection too — and before this test the whole suite stayed green,
    // because the unit tests cover `session()`/`gate()` rather than `cmd_reflex`.
    let cfg = tempfile::tempdir().unwrap();
    write_config(cfg.path(), "[reflex]\nper_turn = false\n");

    let session = injected_context(
        &ok_stdout(run_hook(SESSION_START, None, cfg.path())),
        "SessionStart",
    );
    let baseline = {
        let plain = tempfile::tempdir().unwrap();
        injected_context(
            &ok_stdout(run_hook(SESSION_START, None, plain.path())),
            "SessionStart",
        )
    };
    assert_eq!(
        session, baseline,
        "per_turn is gate-only: it must leave the SessionStart reflex byte-identical"
    );

    // ...and the same config demonstrably DOES silence the gate, so this is a
    // test about routing rather than one that proves the key was ignored.
    let gate = ok_stdout(run_hook(USER_PROMPT, None, cfg.path()));
    assert!(
        gate.trim().is_empty(),
        "per_turn = false must still silence the gate, got:\n{gate}"
    );
}
