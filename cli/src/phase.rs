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

    // Spawn a plain `claude` pane; seed injection happens via the first
    // agent_send (the skill reads handoff_doc and sends the seed text).
    let argv: Vec<String> = vec!["claude".into()];

    // Tag the spawned agent with DROVR_PHASE=<run>/<phase> so the reflex hook
    // detects a drovr-spawned phase and suppresses the human-facing reflex.
    let phase_id = format!("{}/{}", run.name, phase);
    let pane_id = h.agent_start(phase, &cwd, run.workspace.as_deref(), Some(phase_id.as_str()), &argv)?;

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
    run.phases[idx].pane_id = Some(pane_id);
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
            workspace: None,
            project_dir: "/tmp/drovr-proj-test".into(),
        }
    }

    fn make_run_with_workspace(name: &str, ws_id: &str) -> RunState {
        let mut run = make_run(name);
        run.workspace = Some(ws_id.to_owned());
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
        // agent_start was called
        assert!(h.calls()[0].contains("agent_start"));
        assert!(h.calls()[0].contains("brainstorm"));
    }

    #[test]
    fn agent_start_called_with_no_focus_and_workspace() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run_with_workspace("ws-isolation-test", "ws-42");

        phase_start(&h, &mut run, "brainstorm", None).unwrap();

        let calls = h.calls();
        let start_call = calls.iter().find(|c| c.contains("agent_start")).unwrap();
        // workspace id must be threaded through
        assert!(
            start_call.contains("workspace=Some(\"ws-42\")"),
            "workspace id not found in call: {start_call}"
        );
    }

    #[test]
    fn agent_start_no_workspace_passes_none() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("no-ws-test"); // workspace: None

        phase_start(&h, &mut run, "plan", None).unwrap();

        let calls = h.calls();
        let start_call = calls.iter().find(|c| c.contains("agent_start")).unwrap();
        assert!(
            start_call.contains("workspace=None"),
            "expected workspace=None in call: {start_call}"
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
        // (b) spawned argv must NOT contain "--seed" or the seed path —
        //     seed injection happens via the first agent_send, not the spawn argv
        let calls = h.calls();
        assert!(!calls[0].contains("--seed"), "argv must not contain --seed: {}", calls[0]);
        assert!(!calls[0].contains("/tmp/seed.md"), "argv must not contain seed path: {}", calls[0]);
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

        assert!(
            !h.calls().iter().any(|c| c.contains("pane_close")),
            "phase_start must never close a pane mid-run: {:?}",
            h.calls()
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

        phase_start(&h, &mut run, "brainstorm", None).unwrap();

        let calls = h.calls();
        let start_call = calls.iter().find(|c| c.contains("agent_start")).unwrap();
        assert!(
            start_call.contains("cwd=/home/user/my-project"),
            "agent_start must use project_dir as cwd, got: {start_call}"
        );
    }

    // -- A1: phase_start sets DROVR_PHASE=<run>/<phase> on the spawned agent --
    #[test]
    fn phase_start_sets_drovr_phase() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let mut run = make_run("start-test");

        phase_start(&h, &mut run, "brainstorm", None).unwrap();

        let calls = h.calls();
        let start_call = calls.iter().find(|c| c.contains("agent_start")).unwrap();
        assert!(
            start_call.contains("phase_id=Some(\"start-test/brainstorm\")"),
            "agent_start must carry phase_id=<run>/<phase>: {start_call}"
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
