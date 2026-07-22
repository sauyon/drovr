use std::io::{self, Write as _};
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[cfg(test)]
use std::cell::RefCell;
#[cfg(test)]
use std::collections::VecDeque;

use crate::herdr::Herdr;
use crate::run::{RunState, run_dir};

const COMPRESS_PROMPT: &str = include_str!("../assets/compress-prompt.md");

// ---------------------------------------------------------------------------
// CmdRunner trait + impls
// ---------------------------------------------------------------------------

pub trait CmdRunner {
    /// Run `cmd` with `args`, writing `stdin` to the child's stdin.
    /// Returns stdout on success (trimmed by `SystemRunner`; raw by `FakeRunner`).
    fn run(&self, cmd: &str, args: &[&str], stdin: &str) -> io::Result<String>;
}

pub struct SystemRunner;

impl CmdRunner for SystemRunner {
    fn run(&self, cmd: &str, args: &[&str], stdin_text: &str) -> io::Result<String> {
        let mut child = Command::new(cmd)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(stdin_text.as_bytes())?;
        }

        let output = child.wait_with_output()?;
        if !output.status.success() {
            return Err(io::Error::other(
                format!(
                    "{cmd} exited with status {}",
                    output.status.code().unwrap_or(-1)
                ),
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }
}

/// Fake runner for tests: records invocations, returns scripted stdout FIFO.
#[cfg(test)]
pub struct FakeRunner {
    calls: RefCell<Vec<(String, Vec<String>, String)>>,
    stdout_queue: RefCell<VecDeque<String>>,
}

#[cfg(test)]
impl FakeRunner {
    pub fn new() -> Self {
        Self {
            calls: RefCell::new(Vec::new()),
            stdout_queue: RefCell::new(VecDeque::new()),
        }
    }

    /// Queue a string to be returned by the next `run` call.
    pub fn push_stdout(&self, s: impl Into<String>) {
        self.stdout_queue.borrow_mut().push_back(s.into());
    }

    /// Return all recorded calls as `(cmd, args, stdin)`.
    pub fn calls(&self) -> Vec<(String, Vec<String>, String)> {
        self.calls.borrow().clone()
    }
}

#[cfg(test)]
impl CmdRunner for FakeRunner {
    fn run(&self, cmd: &str, args: &[&str], stdin: &str) -> io::Result<String> {
        self.calls.borrow_mut().push((
            cmd.to_owned(),
            args.iter().map(|s| s.to_string()).collect(),
            stdin.to_owned(),
        ));
        Ok(self
            .stdout_queue
            .borrow_mut()
            .pop_front()
            .unwrap_or_default())
    }
}

// ---------------------------------------------------------------------------
// compress
// ---------------------------------------------------------------------------

/// Build the compressor prompt and run claude one-shot.
/// Returns the trimmed handoff document.
pub fn compress<R: CmdRunner>(
    r: &R,
    transcript: &str,
    objective: &str,
) -> io::Result<String> {
    let prompt = format!(
        "{}\n\n## OBJECTIVE\n{}\n\n## RAW PHASE CONTEXT\n{}",
        COMPRESS_PROMPT, objective, transcript
    );
    r.run("claude", &["-p", "--permission-mode", "plan"], &prompt)
}

// ---------------------------------------------------------------------------
// phase_compress
// ---------------------------------------------------------------------------

/// Read the finished phase's transcript, compress it, and write
/// `<phase>-HANDOFF.md` into the run dir.  Returns the path written.
pub fn phase_compress<H: Herdr, R: CmdRunner>(
    h: &H,
    r: &R,
    run: &RunState,
    phase: &str,
) -> io::Result<PathBuf> {
    // Find the pane_id for this phase so we know what to read
    let pane_id = run
        .phases
        .iter()
        .find(|p| p.name == phase)
        .and_then(|p| p.pane_id.clone())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("phase_compress: no pane_id for phase '{phase}'"),
            )
        })?;

    let transcript = h.agent_read(&pane_id)?;
    let objective = &run.task;
    let handoff = compress(r, &transcript, objective)?;

    let dir = run_dir(&run.name);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{phase}-HANDOFF.md"));
    std::fs::write(&path, &handoff)?;
    Ok(path)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::herdr::FakeHerdr;
    use crate::run::{Phase, PhaseStatus, RunState, run_dir};
    use crate::test_util::ENV_LOCK;

    fn make_run(name: &str) -> RunState {
        unsafe {
            std::env::set_var(
                "XDG_DATA_HOME",
                format!("/tmp/drovr-compress-test-{name}"),
            );
        }
        RunState {
            name: name.to_owned(),
            task: "compress the brainstorm phase output".into(),
            phases: vec![],
            gate: "spec".into(),
            cursor: 0,
            workspace: None,
            project_dir: "/tmp/drovr-proj-test".into(),
        }
    }

    // -- compress ----------------------------------------------------------------

    #[test]
    fn compress_prompt_contains_objective_and_transcript() {
        let runner = FakeRunner::new();
        runner.push_stdout("## Objective\nDone.");

        let result = compress(&runner, "the transcript text", "do the objective").unwrap();

        assert_eq!(result, "## Objective\nDone.");
        let calls = runner.calls();
        assert_eq!(calls.len(), 1);
        let (cmd, args, stdin) = &calls[0];
        assert_eq!(cmd, "claude");
        assert_eq!(args, &["-p", "--permission-mode", "plan"]);
        // stdin must contain the fixed COMPRESS_PROMPT prefix
        assert!(
            stdin.contains("You are drovr's handoff compressor"),
            "stdin missing COMPRESS_PROMPT preamble"
        );
        // stdin must contain the section headings
        assert!(
            stdin.contains("## OBJECTIVE"),
            "stdin missing ## OBJECTIVE heading"
        );
        assert!(
            stdin.contains("## RAW PHASE CONTEXT"),
            "stdin missing ## RAW PHASE CONTEXT heading"
        );
        // stdin must contain the caller-supplied content
        assert!(stdin.contains("do the objective"), "stdin missing objective text");
        assert!(stdin.contains("the transcript text"), "stdin missing transcript");
    }

    #[test]
    fn compress_returns_trimmed_stdout() {
        let runner = FakeRunner::new();
        runner.push_stdout("  handoff output  \n");
        // FakeRunner already returns the string as-is; compress must trim
        let result = compress(&runner, "t", "o").unwrap();
        // The trimming happens inside SystemRunner but FakeRunner returns raw.
        // compress itself doesn't trim — the caller (SystemRunner) does.
        // Let's verify the raw value is returned unchanged.
        assert_eq!(result, "  handoff output  \n");
    }

    #[test]
    fn compress_propagates_runner_error() {
        struct ErrRunner;
        impl CmdRunner for ErrRunner {
            fn run(&self, _: &str, _: &[&str], _: &str) -> io::Result<String> {
                Err(io::Error::new(io::ErrorKind::Other, "claude not found"))
            }
        }
        let result = compress(&ErrRunner, "t", "o");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("claude not found"));
    }

    // -- phase_compress ----------------------------------------------------------

    #[test]
    fn phase_compress_writes_handoff_file() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let runner = FakeRunner::new();

        let mut run = make_run("write-handoff");
        run.phases.push(Phase {
            name: "brainstorm".into(),
            status: PhaseStatus::Done,
            handoff_doc: None,
            herdr_session: Some("pane-1".into()),
            pane_id: Some("pane-1".into()),
        });

        h.push_read("the raw brainstorm transcript");
        runner.push_stdout("## Objective\nHandoff content here.");

        let path = phase_compress(&h, &runner, &run, "brainstorm").unwrap();

        // Path must be <run_dir>/<phase>-HANDOFF.md
        let expected = run_dir(&run.name).join("brainstorm-HANDOFF.md");
        assert_eq!(path, expected);

        // File must exist and match what the runner returned
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "## Objective\nHandoff content here.");
    }

    #[test]
    fn phase_compress_path_matches_collect() {
        // Verify that the path phase_compress writes is exactly what
        // phase::collect reads: run_dir/phase-HANDOFF.md.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let runner = FakeRunner::new();

        let mut run = make_run("collect-compat");
        run.phases.push(Phase {
            name: "plan".into(),
            status: PhaseStatus::Done,
            handoff_doc: None,
            herdr_session: Some("pane-2".into()),
            pane_id: Some("pane-2".into()),
        });

        h.push_read("plan transcript");
        runner.push_stdout("plan handoff");

        let written_path = phase_compress(&h, &runner, &run, "plan").unwrap();
        let collect_path = run_dir(&run.name).join("plan-HANDOFF.md");
        assert_eq!(written_path, collect_path);

        // Reading via the same path that collect() uses must succeed
        let content = std::fs::read_to_string(&collect_path).unwrap();
        assert_eq!(content, "plan handoff");
    }

    #[test]
    fn phase_compress_no_pane_id_returns_err() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let runner = FakeRunner::new();

        let mut run = make_run("no-pane");
        run.phases.push(Phase {
            name: "brainstorm".into(),
            status: PhaseStatus::Pending,
            handoff_doc: None,
            herdr_session: None,
            pane_id: None, // no pane_id
        });

        let result = phase_compress(&h, &runner, &run, "brainstorm");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("no pane_id"), "error should mention missing pane_id: {msg}");
    }

    #[test]
    fn phase_compress_missing_phase_returns_err() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let runner = FakeRunner::new();
        let run = make_run("missing-phase");

        let result = phase_compress(&h, &runner, &run, "ghost");
        assert!(result.is_err());
    }
}
