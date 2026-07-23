use std::io;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use crate::herdr::Herdr;
use crate::run::{Phase, PhaseStatus, RunState, run_dir};

/// How often `phase_wait` polls the filesystem for the completion marker.
const POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Path of the completion marker a phase agent drops via `drovr phase done`.
fn done_marker(run: &str, phase: &str) -> PathBuf {
    run_dir(run).join(format!("{phase}.done"))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn find_phase_idx(run: &RunState, phase: &str) -> Option<usize> {
    run.phases.iter().position(|p| p.name == phase)
}

/// POSIX single-quote `s` so it becomes exactly one literal shell word when
/// interpolated into a `herdr pane run` command. Neutralizes spaces and shell
/// metacharacters (`;`, `$()`, `&&`, …); the enclosing single quotes are stripped
/// by the shell, so the resulting env value is unchanged.
fn shell_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn require_pane_id(run: &RunState, phase: &str) -> io::Result<String> {
    let idx = find_phase_idx(run, phase).ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, format!("phase not found: {phase}"))
    })?;
    run.phases[idx].pane_id.clone().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("phase has no pane_id: {phase}"),
        )
    })
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Spawn a persistent claude pane for `phase`, record pane_id + herdr_session,
/// set status Running, and save.  If the phase already exists (e.g. resume) its
/// entry is reused; if not, it is appended.
pub fn phase_start<H: Herdr>(
    h: &H,
    run: &mut RunState,
    phase: &str,
    seed: Option<&Path>,
) -> io::Result<()> {
    if run.project_dir.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "run '{}' has no project_dir (created before this fix); \
                 please recreate the run with `drovr new`",
                run.name
            ),
        ));
    }
    let cwd = run.project_dir.clone();

    // Pick the pane this phase's `claude` will run in, WITHOUT splitting a new
    // pane beside an empty shell:
    //   * a restarting phase reuses its own recorded pane;
    //   * the first phase reuses the workspace's root shell pane (taken here so
    //     later phases don't);
    //   * every later phase gets its own fresh tab (whose auto shell pane it
    //     reuses).
    let existing_pane = find_phase_idx(run, phase).and_then(|i| run.phases[i].pane_id.clone());
    // `used_root` defers consuming `run.root_pane` until the launch actually
    // succeeds — if `pane_run` fails, the root pane stays available for a retry
    // instead of being silently forfeited to a fresh tab.
    let mut used_root = false;
    let target_pane = if let Some(pane) = existing_pane {
        pane
    } else if let Some(root) = run.root_pane.clone() {
        used_root = true;
        root
    } else if let Some(ws) = run.workspace.as_deref() {
        h.tab_create(ws, phase, &cwd)?
    } else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "run '{}' has no herdr workspace (creation failed at `drovr new`); \
                 please recreate the run with `drovr new`",
                run.name
            ),
        ));
    };

    // Capture focus so pane operations (which lack a --no-focus flag) don't steal
    // it from the user. Restored after the pane is launched.
    let prev_focus = h.focused_workspace();

    // Launch `claude` inside the target pane. DROVR_PHASE=<run>/<phase> tags it so
    // the reflex hook detects a drovr-spawned phase and suppresses the human
    // reflex. It is not a secret, so it rides inline on the command; auth secrets
    // travel via herdr `--env` at pane creation, never in this string. The value
    // is single-quoted so a run/phase name with a space or shell metacharacter
    // stays one literal word and cannot break out of the command.
    let command = format!(
        "DROVR_PHASE={} claude",
        shell_single_quote(&format!("{}/{}", run.name, phase))
    );
    h.pane_run(&target_pane, &command)?;
    // The launch succeeded, so this phase has now claimed the root pane (if it
    // used it); clear it so later phases don't try to reuse the same pane.
    if used_root {
        run.root_pane = None;
    }
    // Cosmetic pane label; best-effort (a rename failure must not fail the phase).
    let _ = h.pane_rename(&target_pane, phase);
    // Restore focus if a pane operation moved it.
    if let Some(prev) = prev_focus {
        let _ = h.workspace_focus(&prev);
    }

    // Find existing phase or append a new one
    let idx = match find_phase_idx(run, phase) {
        Some(i) => i,
        None => {
            run.phases.push(Phase {
                name: phase.to_owned(),
                status: PhaseStatus::Pending,
                handoff_doc: None,
                herdr_session: None,
                pane_id: None,
            });
            run.phases.len() - 1
        }
    };

    let seed_str = seed.map(|p| p.to_string_lossy().into_owned());
    run.phases[idx].handoff_doc = seed_str;
    // pane_id only — herdr_session is not used for cleanup (workspace_close handles that)
    run.phases[idx].herdr_session = None;
    run.phases[idx].pane_id = Some(target_pane);
    run.phases[idx].status = PhaseStatus::Running;

    // Panes are never closed mid-run: closing any pane makes herdr reassign
    // focus, disturbing the user. The run's workspace (root pane + every phase
    // pane) is torn down in one shot at the end by `drovr cleanup`
    // (`workspace_close`), once the user confirms.
    run.save()?;
    Ok(())
}

/// Send `text` to the running phase pane.
pub fn phase_send<H: Herdr>(
    h: &H,
    run: &RunState,
    phase: &str,
    text: &str,
) -> io::Result<()> {
    let pane_id = require_pane_id(run, phase)?;
    h.agent_send(&pane_id, text)
}

/// Mark a phase complete by dropping its completion marker. Run BY the phase
/// agent itself as its final action (via `drovr phase done`), NOT by the
/// orchestrator — it is the only reliable "the agent finished" signal, since
/// herdr's `idle` status also fires while an agent is merely parked awaiting a
/// subagent. Writing a marker file (rather than mutating `state.json`) keeps
/// the orchestrator the sole writer of run state.
pub fn phase_done(run: &RunState, phase: &str) -> io::Result<PathBuf> {
    find_phase_idx(run, phase).ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, format!("phase not found: {phase}"))
    })?;
    let marker = done_marker(&run.name, phase);
    if let Some(parent) = marker.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&marker, b"")?;
    Ok(marker)
}

/// Poll the filesystem for the phase's completion marker (dropped by the phase
/// agent via `drovr phase done`) until it appears or `timeout_ms` elapses.
/// Marks the phase Done (and saves) when found; leaves it Running on timeout.
/// Deliberately does NOT consult herdr status: `idle` is not a completion
/// signal (it also fires when an agent is parked awaiting its own subagent).
pub fn phase_wait(run: &mut RunState, phase: &str, timeout_ms: u64) -> io::Result<bool> {
    find_phase_idx(run, phase).ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, format!("phase not found: {phase}"))
    })?;
    let marker = done_marker(&run.name, phase);
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        if marker.exists() {
            let idx = find_phase_idx(run, phase).unwrap();
            run.phases[idx].status = PhaseStatus::Done;
            run.save()?;
            return Ok(true);
        }
        let now = Instant::now();
        if now >= deadline {
            return Ok(false);
        }
        thread::sleep(POLL_INTERVAL.min(deadline - now));
    }
}

// ---------------------------------------------------------------------------
// Liveness net — detect a phase pane parked on a first-run prompt
// ---------------------------------------------------------------------------
//
// A freshly-spawned interactive `claude` can PARK on a prompt with no human to
// answer it — a first-run upsell, a workspace-trust dialog, a future onboarding
// step — and then it never reaches its composer, never runs `drovr phase done`,
// and `phase wait` just times out with no explanation. The renderer upsell is
// handled at the source (`CLAUDE_CODE_NO_FLICKER=1`, see herdr::spawn_env_flags),
// but other prompts are unforeseeable, so this net catches the general case:
// when a wait times out, read the pane once and, if it matches a known
// "waiting on a prompt" signature, surface a clear, actionable diagnostic
// (naming the run/phase, quoting the pane, suggesting `drovr attach`) instead of
// a silent timeout.
//
// Design choices:
//   * SURFACE, never auto-dismiss. Guessing a keypress to clear an unknown
//     prompt is fragile — a wrong guess corrupts the agent's input — so we tell
//     the human exactly where to look rather than gambling.
//   * Read-only. `agent read` never moves focus (it is the same call `phase
//     compress` uses without any focus capture), so this is focus-safe by
//     construction and needs no capture/restore.
//   * No busy-poll. This runs exactly once, on the wait's timeout — not in a
//     loop.

/// Substrings that mark a pane as parked on an interactive prompt awaiting a
/// human. Kept deliberately specific to avoid matching normal agent output.
/// Matching is case-insensitive.
const STUCK_PROMPT_SIGNATURES: &[&str] = &[
    // claude's first-run fullscreen-renderer upsell (belt-and-suspenders: it is
    // suppressed at spawn via CLAUDE_CODE_NO_FLICKER, but a future variant might
    // slip through).
    "try the new fullscreen",
    // Generic numbered-choice prompt cursor, e.g. "❯ 1. Yes".
    "❯ 1.",
    // Workspace / directory trust dialog.
    "do you trust",
    // Common confirmation affordances.
    "enter to confirm",
    "press enter to continue",
    "1. yes",
];

/// If `pane` looks like it is parked on an interactive prompt awaiting a human,
/// return the signature that matched (for the diagnostic); otherwise `None`.
/// Pure and case-insensitive so it is trivially unit-testable.
fn detect_stuck_prompt(pane: &str) -> Option<&'static str> {
    let haystack = pane.to_lowercase();
    STUCK_PROMPT_SIGNATURES
        .iter()
        .copied()
        .find(|sig| haystack.contains(&sig.to_lowercase()))
}

/// The last `n` non-blank lines of `pane`, joined — a compact snippet to quote
/// in the diagnostic so the human can recognize the prompt without attaching.
fn tail_snippet(pane: &str, n: usize) -> String {
    let lines: Vec<&str> = pane.lines().map(str::trim_end).filter(|l| !l.trim().is_empty()).collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

/// Read the phase pane once and, if it is parked on a known interactive prompt,
/// return a clear, actionable diagnostic (naming the run/phase, quoting the
/// pane tail, suggesting `drovr attach`). Returns `None` when the pane is not
/// recognizably stuck (it may just be mid-work) or has no pane_id yet.
///
/// Read-only and focus-safe: `agent_read` never moves focus, so no capture /
/// restore is needed. Intended to be called ONCE on a `phase wait` timeout, not
/// in a poll loop. A failed pane read is swallowed (returns `None`) — a
/// best-effort diagnostic must never mask the underlying timeout with a new
/// error.
pub fn diagnose_stuck_phase<H: Herdr>(
    h: &H,
    run: &RunState,
    phase: &str,
) -> Option<String> {
    let idx = find_phase_idx(run, phase)?;
    let pane_id = run.phases[idx].pane_id.clone()?;
    let pane = h.agent_read(&pane_id).ok()?;
    let matched = detect_stuck_prompt(&pane)?;
    let snippet = tail_snippet(&pane, 6);
    Some(format!(
        "phase '{phase}' of run '{run_name}' appears STUCK on an interactive prompt \
         (matched \"{matched}\") rather than working — it will never signal `drovr phase done`, \
         so `phase wait` timed out.\n\
         Pane {pane_id}:\n{snippet}\n\
         Attach to answer the prompt: drovr attach {run_name}",
        run_name = run.name,
    ))
}

/// Read `HANDOFF.md` written by the compressor into the run directory.
pub fn collect(run: &RunState, phase: &str) -> io::Result<String> {
    let path: PathBuf = run_dir(&run.name).join(format!("{phase}-HANDOFF.md"));
    std::fs::read_to_string(&path).map_err(|e| {
        io::Error::new(
            e.kind(),
            format!("collect({phase}): cannot read {}: {e}", path.display()),
        )
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::herdr::FakeHerdr;
    use crate::run::{Phase, PhaseStatus, RunState};
    use crate::test_util::ENV_LOCK;

    fn make_run(name: &str) -> RunState {
        // Caller must hold ENV_LOCK before calling.
        unsafe {
            std::env::set_var(
                "XDG_DATA_HOME",
                format!("/tmp/drovr-phase-test-{name}"),
            );
        }
        // Start each test from a clean run dir so a stale `.done` marker or
        // state.json from a prior run can't leak across test invocations.
        let _ = std::fs::remove_dir_all(run_dir(name));
        RunState {
            name: name.to_owned(),
            task: "test task".into(),
            phases: vec![],
            gate: "spec".into(),
            cursor: 0,
            // `drovr new` always creates a workspace + root shell pane; the first
            // phase reuses the root pane, later phases each get their own tab.
            workspace: Some("ws-mk".into()),
            root_pane: Some("root-mk".into()),
            project_dir: "/tmp/drovr-proj-test".into(),
        }
    }

    fn make_run_with_workspace(name: &str, ws_id: &str) -> RunState {
        let mut run = make_run(name);
        run.workspace = Some(ws_id.to_owned());
        run.root_pane = Some(format!("{ws_id}:root"));
        run
    }

    // -- RED: write failing test first, then implement -----------------------

    #[test]
    fn start_records_pane_id_and_status_running() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("start-test");

        phase_start(&h, &mut run, "brainstorm", None).unwrap();

        // Phase should be appended and marked Running
        assert_eq!(run.phases.len(), 1);
        let p = &run.phases[0];
        assert_eq!(p.name, "brainstorm");
        assert_eq!(p.status, PhaseStatus::Running);
        assert!(p.pane_id.is_some(), "pane_id must be recorded");
        // herdr_session is no longer written (cleanup uses workspace_close, not session_stop)
        assert!(p.herdr_session.is_none(), "herdr_session must be None");
        // claude is launched via `pane run`, NOT a split-creating `agent start`.
        let calls = h.calls();
        assert!(
            !calls.iter().any(|c| c.contains("agent_start")),
            "must not use agent_start (it splits a new pane): {calls:?}"
        );
        let run_call = calls.iter().find(|c| c.contains("pane_run")).unwrap();
        assert!(run_call.contains("claude"), "pane_run must launch claude: {run_call}");
    }

    #[test]
    fn first_phase_reuses_root_pane() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run_with_workspace("ws-isolation-test", "ws-42");

        phase_start(&h, &mut run, "brainstorm", None).unwrap();

        let calls = h.calls();
        // The first phase reuses the workspace root pane — no tab is created,
        // and no empty shell is left dangling.
        assert!(
            !calls.iter().any(|c| c.contains("tab_create")),
            "first phase must not create a tab: {calls:?}"
        );
        let run_call = calls.iter().find(|c| c.contains("pane_run")).unwrap();
        assert!(
            run_call.contains("pane=ws-42:root"),
            "first phase must run claude in the root pane: {run_call}"
        );
        assert_eq!(run.phases[0].pane_id.as_deref(), Some("ws-42:root"));
        assert!(run.root_pane.is_none(), "root_pane must be consumed after first use");
    }

    #[test]
    fn later_phase_creates_its_own_tab() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run_with_workspace("later-tab-test", "ws-7");

        phase_start(&h, &mut run, "brainstorm", None).unwrap(); // consumes root pane
        phase_start(&h, &mut run, "plan", None).unwrap(); // must get its own tab

        let calls = h.calls();
        let tab_call = calls.iter().find(|c| c.contains("tab_create")).unwrap();
        assert!(tab_call.contains("workspace=ws-7"), "tab must be in the run workspace: {tab_call}");
        assert!(tab_call.contains("label=plan"), "tab must be labelled with the phase: {tab_call}");
        // claude runs in the new tab's pane
        let plan_pane = run.phases[1].pane_id.clone().unwrap();
        assert!(
            calls.iter().any(|c| c.contains(&format!("pane_run pane={plan_pane}"))),
            "claude must run in the new tab's pane: {calls:?}"
        );
    }

    #[test]
    fn no_workspace_and_no_root_pane_errors() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("no-ws-test");
        run.workspace = None;
        run.root_pane = None;

        let res = phase_start(&h, &mut run, "plan", None);
        assert!(res.is_err(), "must error when there is no workspace or root pane");
        assert!(res.unwrap_err().to_string().contains("workspace"));
    }

    #[test]
    fn phase_start_preserves_focus() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("focus-test");

        phase_start(&h, &mut run, "brainstorm", None).unwrap();

        let calls = h.calls();
        let capture = calls.iter().position(|c| c.contains("focused_workspace"));
        let run_at = calls.iter().position(|c| c.contains("pane_run"));
        let restore = calls.iter().position(|c| c.contains("workspace_focus"));
        let (capture, run_at, restore) = (capture.unwrap(), run_at.unwrap(), restore.unwrap());
        assert!(capture < run_at, "focus must be captured before pane_run: {calls:?}");
        assert!(restore > run_at, "focus must be restored after pane_run: {calls:?}");
        assert!(
            calls[restore].contains("workspace_focus id=ws-focused"),
            "focus must be restored to the previously-focused workspace: {}",
            calls[restore]
        );
    }

    #[test]
    fn wait_sees_marker_and_marks_phase_done() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("wait-done-test");

        phase_start(&h, &mut run, "plan", None).unwrap();
        // The phase agent signals completion by dropping the marker.
        let marker = phase_done(&run, "plan").unwrap();
        assert!(marker.exists(), "marker should exist at {}", marker.display());

        let done = phase_wait(&mut run, "plan", 5000).unwrap();
        assert!(done);
        assert_eq!(run.phases[0].status, PhaseStatus::Done);
    }

    #[test]
    fn wait_timeout_leaves_running() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("wait-timeout-test");

        phase_start(&h, &mut run, "plan", None).unwrap();
        // No marker dropped → wait times out quickly and leaves the phase Running.
        let done = phase_wait(&mut run, "plan", 50).unwrap();
        assert!(!done);
        assert_eq!(run.phases[0].status, PhaseStatus::Running);
    }

    #[test]
    fn done_on_unknown_phase_errors() {
        let _lock = ENV_LOCK.lock().unwrap();
        let run = make_run("done-unknown-test");
        // No phases registered → phase_done must reject rather than write a
        // stray marker.
        assert!(phase_done(&run, "nonexistent").is_err());
    }

    #[test]
    fn send_routes_to_pane() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("send-test");

        phase_start(&h, &mut run, "code", None).unwrap();
        phase_send(&h, &run, "code", "hello agent").unwrap();

        // Last call should be agent_send
        let calls = h.calls();
        let send_call = calls.iter().find(|c| c.contains("agent_send")).unwrap();
        assert!(send_call.contains("hello agent"));
        // Target should match the pane_id recorded
        let pane_id = run.phases[0].pane_id.as_ref().unwrap();
        assert!(send_call.contains(pane_id.as_str()));
    }

    #[test]
    fn start_with_seed_records_handoff_doc() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("seed-test");
        let seed = Path::new("/tmp/seed.md");

        phase_start(&h, &mut run, "brainstorm", Some(seed)).unwrap();

        // (a) handoff_doc stores the seed path for later injection via agent_send
        let p = &run.phases[0];
        assert_eq!(p.handoff_doc.as_deref(), Some("/tmp/seed.md"));
        // (b) the launch command must NOT contain "--seed" or the seed path —
        //     seed injection happens via the first agent_send, not the launch command
        let calls = h.calls();
        let run_call = calls.iter().find(|c| c.contains("pane_run")).unwrap();
        assert!(!run_call.contains("--seed"), "command must not contain --seed: {run_call}");
        assert!(!run_call.contains("/tmp/seed.md"), "command must not contain seed path: {run_call}");
    }

    // Panes are never closed mid-run (herdr reassigns focus on any close);
    // cleanup is a single `workspace_close` at end-of-run. `phase_start` must
    // therefore never close a pane.
    #[test]
    fn phase_start_never_closes_a_pane() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("no-mid-run-close-test");

        phase_start(&h, &mut run, "brainstorm", None).unwrap();
        phase_start(&h, &mut run, "plan", None).unwrap();

        let calls = h.calls();
        assert!(
            !calls.iter().any(|c| c.contains("pane_close")),
            "phase_start must never close a pane mid-run: {calls:?}"
        );
        assert!(
            !calls.iter().any(|c| c.contains("agent_start")),
            "phase_start must not use agent_start: {calls:?}"
        );
    }

    #[test]
    fn start_reuses_existing_phase_entry() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("reuse-test");

        // Pre-populate a Pending phase
        run.phases.push(Phase {
            name: "plan".into(),
            status: PhaseStatus::Pending,
            handoff_doc: None,
            herdr_session: None,
            pane_id: None,
        });

        phase_start(&h, &mut run, "plan", None).unwrap();
        // Still only one phase
        assert_eq!(run.phases.len(), 1);
        assert_eq!(run.phases[0].status, PhaseStatus::Running);
    }

    #[test]
    fn collect_reads_handoff_file() {
        let _lock = ENV_LOCK.lock().unwrap();
        let mut run = make_run("collect-reads-test");
        let dir = run_dir(&run.name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("brainstorm-HANDOFF.md"), "## handoff content").unwrap();
        run.phases = vec![];  // phases not relevant for collect

        let content = collect(&run, "brainstorm").unwrap();
        assert_eq!(content, "## handoff content");
    }

    #[test]
    fn collect_missing_file_returns_err() {
        let _lock = ENV_LOCK.lock().unwrap();
        let run = make_run("collect-missing-test");
        let result = collect(&run, "nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn phase_start_uses_project_dir_as_cwd() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("proj-cwd-test");
        run.project_dir = "/home/user/my-project".into();

        // First phase reuses the root pane (whose cwd was set at workspace
        // create); a later phase's tab must carry project_dir as its cwd.
        phase_start(&h, &mut run, "brainstorm", None).unwrap();
        phase_start(&h, &mut run, "plan", None).unwrap();

        let calls = h.calls();
        let tab_call = calls.iter().find(|c| c.contains("tab_create")).unwrap();
        assert!(
            tab_call.contains("cwd=/home/user/my-project"),
            "tab_create must use project_dir as cwd, got: {tab_call}"
        );
    }

    // -- A1: phase_start tags the launch with DROVR_PHASE=<run>/<phase> --------
    #[test]
    fn phase_start_sets_drovr_phase() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("start-test");

        phase_start(&h, &mut run, "brainstorm", None).unwrap();

        let calls = h.calls();
        let run_call = calls.iter().find(|c| c.contains("pane_run")).unwrap();
        assert!(
            run_call.contains(r"DROVR_PHASE='start-test/brainstorm' claude"),
            "pane_run command must carry a single-quoted DROVR_PHASE=<run>/<phase>: {run_call}"
        );
        // Auth secrets must never be inlined into the launch command.
        assert!(!run_call.contains("ANTHROPIC_API_KEY"), "no secret in command: {run_call}");
        assert!(!run_call.contains("CLAUDE_CONFIG_DIR"), "no secret in command: {run_call}");
    }

    // -- F1 (agy security): a phase/run name with shell metacharacters must be
    //    quoted into one literal word, not break out of the pane_run command.
    #[test]
    fn phase_start_shell_quotes_unsafe_phase_name() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("inject-test");

        // A phase name carrying a shell injection attempt.
        phase_start(&h, &mut run, "p; rm -rf ~", None).unwrap();

        let calls = h.calls();
        let run_call = calls.iter().find(|c| c.contains("pane_run")).unwrap();
        // The value is a single quoted word; the metacharacters are inert.
        assert!(
            run_call.contains(r"DROVR_PHASE='inject-test/p; rm -rf ~' claude"),
            "unsafe phase name must be single-quoted: {run_call}"
        );
    }

    #[test]
    fn shell_single_quote_neutralizes_metacharacters() {
        assert_eq!(shell_single_quote("a/b"), "'a/b'");
        assert_eq!(shell_single_quote("a; rm -rf ~"), "'a; rm -rf ~'");
        assert_eq!(shell_single_quote("$(id)"), "'$(id)'");
        // An embedded single quote is escaped, not terminated.
        assert_eq!(shell_single_quote("a'b"), "'a'\\''b'");
    }

    // -- F2 (agy correctness): a failed launch must NOT consume the root pane, so
    //    a retry can still reuse it rather than forfeiting it to a fresh tab.
    #[test]
    fn first_phase_keeps_root_pane_on_launch_failure() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        h.fail_pane_run();
        let mut run = make_run_with_workspace("launch-fail-test", "ws-9");

        let res = phase_start(&h, &mut run, "brainstorm", None);
        assert!(res.is_err(), "phase_start must propagate the pane_run failure");
        assert_eq!(
            run.root_pane.as_deref(),
            Some("ws-9:root"),
            "root_pane must be retained when the launch fails"
        );
    }

    // -- Task B: liveness net -------------------------------------------------

    #[test]
    fn detect_stuck_prompt_matches_renderer_upsell() {
        let pane = "some output\nTry the new fullscreen renderer?\n❯ 1. Yes\n  2. Not now";
        assert_eq!(detect_stuck_prompt(pane), Some("try the new fullscreen"));
    }

    #[test]
    fn detect_stuck_prompt_matches_trust_dialog() {
        let pane = "Do you trust the files in this folder?\n❯ 1. Yes, proceed";
        // The first signature scanned that matches wins; both are present here.
        assert!(detect_stuck_prompt(pane).is_some());
    }

    #[test]
    fn detect_stuck_prompt_is_case_insensitive() {
        assert!(detect_stuck_prompt("TRY THE NEW FULLSCREEN RENDERER?").is_some());
        assert!(detect_stuck_prompt("Enter To Confirm").is_some());
    }

    #[test]
    fn detect_stuck_prompt_none_on_normal_output() {
        // Ordinary agent working output must NOT be flagged as a stuck prompt.
        let pane = "Reading files...\nEditing src/main.rs\nRunning cargo test\nAll tests pass.";
        assert_eq!(detect_stuck_prompt(pane), None);
    }

    #[test]
    fn tail_snippet_returns_last_nonblank_lines() {
        let pane = "a\n\nb\n\n\nc\nd\n\n";
        assert_eq!(tail_snippet(pane, 2), "c\nd");
        // Fewer lines than requested returns all of them.
        assert_eq!(tail_snippet("only\n", 6), "only");
    }

    #[test]
    fn diagnose_stuck_phase_surfaces_actionable_diagnostic() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("stuck-test");

        phase_start(&h, &mut run, "brainstorm", None).unwrap();
        // The pane read on the next timeout returns a parked prompt.
        h.push_read("Try the new fullscreen renderer?\n❯ 1. Yes\n  2. Not now");

        let diag = diagnose_stuck_phase(&h, &run, "brainstorm")
            .expect("a parked prompt must yield a diagnostic");
        // Names the run + phase, quotes the pane, and points at `drovr attach`.
        assert!(diag.contains("stuck-test"), "diag must name the run: {diag}");
        assert!(diag.contains("brainstorm"), "diag must name the phase: {diag}");
        assert!(diag.contains("Try the new fullscreen"), "diag must quote the pane: {diag}");
        assert!(diag.contains("drovr attach stuck-test"), "diag must suggest attach: {diag}");
    }

    #[test]
    fn diagnose_stuck_phase_none_when_working() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("working-test");

        phase_start(&h, &mut run, "brainstorm", None).unwrap();
        // The pane shows ordinary working output — not a stuck prompt.
        h.push_read("Editing src/main.rs\nRunning cargo test\nAll tests pass.");

        assert!(
            diagnose_stuck_phase(&h, &run, "brainstorm").is_none(),
            "working output must not be reported as stuck"
        );
    }

    #[test]
    fn diagnose_stuck_phase_none_without_pane_id() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let run = make_run("no-pane-test"); // no phase started → no pane_id
        // No phase registered at all: nothing to diagnose, no pane read.
        assert!(diagnose_stuck_phase(&h, &run, "brainstorm").is_none());
        assert!(
            !h.calls().iter().any(|c| c.contains("agent_read")),
            "must not read a pane that does not exist yet"
        );
    }

    #[test]
    fn phase_start_empty_project_dir_returns_error() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("empty-proj-dir-test");
        run.project_dir = String::new();

        let result = phase_start(&h, &mut run, "brainstorm", None);
        assert!(result.is_err(), "must error when project_dir is empty");
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("project_dir"), "error should mention project_dir: {msg}");
    }
}
