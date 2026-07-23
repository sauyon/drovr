// The whole module is unused until Task 6 wires `drovr code-review run|base` into the
// CLI; suppress the dead-code noise until then (Task 6 drops this attribute).
#[allow(dead_code)]
mod code_review;
mod compress;
mod config;
mod findings;
mod herdr;
mod phase;
mod review;
mod run;

use clap::{Parser, Subcommand};
use compress::{SystemRunner, handoff_self, phase_compress};
use herdr::{Herdr, SystemHerdr};
use std::io::Read as _;
use phase::{collect, diagnose_stuck_phase, phase_done, phase_send, phase_start, phase_wait};
use review::{review_summary, review_wait, serve, WaitOutcome};
use run::{PhaseStatus, RunState, run_dir};
use std::io;
use std::path::PathBuf;
use std::process;

// ---------------------------------------------------------------------------
// CLI definition
// ---------------------------------------------------------------------------

#[derive(Debug, Parser)]
#[command(name = "drovr", about = "Drovr workflow manager")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// List all runs: name, phase progress, current phase.
    List,

    /// Create a new run.
    New {
        name: String,
        #[arg(long)]
        task: Option<String>,
        /// Project directory phases will run in (must exist; defaults to cwd).
        #[arg(long)]
        dir: Option<PathBuf>,
    },

    /// Print each phase + status + resume point for a run.
    Status { name: String },

    /// Attach to the current phase's pane.
    Attach { name: String },

    /// Stop the herdr session; optionally remove the run dir.
    Cleanup {
        name: String,
        #[arg(long)]
        purge: bool,
    },

    /// Reload a stopped run and report the resume point.
    Resurrect { name: String },

    /// Start the review HTTP server for a run.
    Serve {
        name: String,
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long, default_value_t = 8791)]
        port: u16,
    },

    /// Self-serve mid-task handoff (compress caller's own context).
    Handoff {
        #[command(subcommand)]
        sub: HandoffCmd,
    },

    /// Plumbing: phase lifecycle operations.
    Phase {
        #[command(subcommand)]
        sub: PhaseCmd,
    },

    /// Plumbing: collect the handoff doc for a finished phase.
    Collect {
        run: String,
        phase_name: String,
    },

    /// Plumbing: review subcommands.
    Review {
        #[command(subcommand)]
        sub: ReviewCmd,
    },
}

#[derive(Debug, Subcommand)]
enum PhaseCmd {
    /// Start a phase (spawn the agent pane).
    Start {
        run: String,
        phase_name: String,
        #[arg(long)]
        seed: Option<PathBuf>,
    },
    /// Send text to a running phase pane.
    Send {
        run: String,
        phase_name: String,
        text: String,
    },
    /// Wait for a phase to complete (polls for the `done` marker).
    Wait {
        run: String,
        phase_name: String,
        #[arg(long, default_value_t = 30_000)]
        timeout_ms: u64,
    },
    /// Mark a phase complete. Run by the phase AGENT itself as its final
    /// action — it drops the completion marker `drovr phase wait` polls for.
    Done {
        run: String,
        phase_name: String,
    },
    /// Compress a finished phase into a handoff doc.
    Compress {
        run: String,
        phase_name: String,
    },
}

#[derive(Debug, Subcommand)]
enum HandoffCmd {
    /// Compress the caller's own context into a HANDOFF doc and print the resume pointer.
    #[command(name = "self")] // `self` is a reserved word → rename the variant
    Own {
        #[arg(long)]
        objective: Option<String>,
        #[arg(long)]
        transcript: Option<PathBuf>,
        #[arg(long)]
        pane: Option<String>,
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
enum ReviewCmd {
    /// POST summary text to the running review server.
    Summary { run: String, text: String },
    /// Block until the reviewer acts, then exit with the outcome.
    ///
    /// Run in the background after posting a summary: the process exits when
    /// the reviewer acts (harness wakes the driver on exit) — no busy-poll.
    /// Exit codes: 0 = approved, 3 = changes requested (`feedback.json` holds
    /// the turn), 2 = timeout (re-run to resume), 1 = error.
    Wait {
        run: String,
        #[arg(long, default_value_t = 1_800_000)]
        timeout_ms: u64,
    },
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Reject run names that are empty or contain path-separator characters.
/// Prevents path traversal in commands that touch the filesystem.
fn validate_run_name(name: &str) -> io::Result<()> {
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid run name {:?}: must not be empty or contain '/', '\\\\', or '..'", name),
        ));
    }
    Ok(())
}

fn load_run(name: &str) -> RunState {
    RunState::load(name).unwrap_or_else(|e| {
        eprintln!("drovr: failed to load run '{name}': {e}");
        process::exit(1);
    })
}

fn save_run(run: &RunState) {
    run.save().unwrap_or_else(|e| {
        eprintln!("drovr: failed to save run '{}': {e}", run.name);
        process::exit(1);
    });
}

fn phase_status_str(status: &PhaseStatus) -> &'static str {
    match status {
        PhaseStatus::Pending => "pending",
        PhaseStatus::Running => "running",
        PhaseStatus::Done => "done",
        PhaseStatus::Failed => "FAILED",
    }
}

/// Format the phase list as `N/M phases done | current: <name>` (for `list`).
fn format_progress(run: &RunState) -> String {
    let done = run.phases.iter().filter(|p| p.status == PhaseStatus::Done).count();
    let total = run.phases.len();
    let current = run
        .first_incomplete()
        .and_then(|i| run.phases.get(i))
        .map(|p| p.name.as_str())
        .unwrap_or("(all done)");
    format!("{done}/{total} phases done | current: {current}")
}

// ---------------------------------------------------------------------------
// Porcelain command handlers
// ---------------------------------------------------------------------------

fn cmd_list() {
    let base = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(std::env::var("HOME").unwrap()).join(".local/share")
        });
    let runs_dir = base.join("drovr").join("runs");

    let entries = match std::fs::read_dir(&runs_dir) {
        Ok(e) => e,
        Err(_) => {
            println!("no runs found");
            return;
        }
    };

    let mut runs: Vec<RunState> = entries
        .flatten()
        .filter_map(|entry| {
            let state_path = entry.path().join("state.json");
            std::fs::read_to_string(&state_path)
                .ok()
                .and_then(|s| serde_json::from_str::<RunState>(&s).ok())
        })
        .collect();

    if runs.is_empty() {
        println!("no runs found");
        return;
    }

    runs.sort_by(|a, b| a.name.cmp(&b.name));
    for run in &runs {
        println!("{:20}  {}", run.name, format_progress(run));
    }
}

fn cmd_new(name: &str, task: Option<String>, dir: Option<PathBuf>, herdr: &SystemHerdr) {
    if let Err(e) = validate_run_name(name) {
        eprintln!("drovr: {e}");
        process::exit(1);
    }
    if !herdr.integration_present() {
        eprintln!("prerequisite missing: run 'herdr integration install claude'");
        process::exit(1);
    }

    let project_dir = match dir {
        Some(d) => {
            if !d.exists() {
                eprintln!("drovr: --dir path does not exist: {}", d.display());
                process::exit(1);
            }
            d.to_string_lossy().into_owned()
        }
        None => std::env::current_dir()
            .unwrap_or_else(|e| {
                eprintln!("drovr: cannot determine current directory: {e}");
                process::exit(1);
            })
            .to_string_lossy()
            .into_owned(),
    };

    let task_str = task.unwrap_or_else(|| "(no task specified)".to_string());

    // Create the workspace in the project dir so its root shell pane (reused by
    // the first phase) and every later tab start already `cd`'d into the project.
    let (workspace, root_pane) = match herdr.workspace_create(&format!("drovr:{name}"), &project_dir) {
        Ok(ws) => (Some(ws.id), Some(ws.root_pane)),
        Err(e) => {
            eprintln!("drovr: warning: could not create herdr workspace: {e}");
            (None, None)
        }
    };

    let run = RunState {
        name: name.to_owned(),
        task: task_str,
        project_dir,
        phases: vec![
            run::Phase {
                name: "brainstorm".into(),
                status: PhaseStatus::Pending,
                handoff_doc: None,
                herdr_session: None,
                pane_id: None,
            },
            run::Phase {
                name: "plan".into(),
                status: PhaseStatus::Pending,
                handoff_doc: None,
                herdr_session: None,
                pane_id: None,
            },
            run::Phase {
                name: "implement".into(),
                status: PhaseStatus::Pending,
                handoff_doc: None,
                herdr_session: None,
                pane_id: None,
            },
            run::Phase {
                name: "review".into(),
                status: PhaseStatus::Pending,
                handoff_doc: None,
                herdr_session: None,
                pane_id: None,
            },
        ],
        review_phases: vec![],
        gate: "spec".into(),
        cursor: 0,
        workspace,
        root_pane,
    };

    save_run(&run);
    println!("created run '{}' at {}", name, run_dir(name).display());
}

fn cmd_status(name: &str) {
    if let Err(e) = validate_run_name(name) {
        eprintln!("drovr: {e}");
        process::exit(1);
    }
    let run = load_run(name);
    println!("run: {}", run.name);
    println!("task: {}", run.task);
    for (i, p) in run.phases.iter().enumerate() {
        let marker = if run.first_incomplete() == Some(i) { " <-- resume" } else { "" };
        println!(
            "  [{:2}] {:15} {}{}",
            i,
            p.name,
            phase_status_str(&p.status),
            marker
        );
    }
    if let Some(idx) = run.first_incomplete() {
        println!("resume at phase {idx}: {}", run.phases[idx].name);
    } else {
        println!("all phases complete");
    }
}

fn cmd_attach(name: &str) {
    if let Err(e) = validate_run_name(name) {
        eprintln!("drovr: {e}");
        process::exit(1);
    }
    let run = load_run(name);
    // Find the current/last-running phase pane
    let pane_id = run
        .first_incomplete()
        .and_then(|i| run.phases.get(i))
        .and_then(|p| p.pane_id.as_deref())
        .or_else(|| {
            // If all done, use the last phase's pane
            run.phases.last().and_then(|p| p.pane_id.as_deref())
        });

    match pane_id {
        Some(id) => {
            // Shell out: herdr agent attach <pane_id>
            let status = std::process::Command::new("herdr")
                .args(["agent", "attach", id])
                .status()
                .unwrap_or_else(|e| {
                    eprintln!("drovr: failed to exec herdr: {e}");
                    process::exit(1);
                });
            if !status.success() {
                process::exit(status.code().unwrap_or(1));
            }
        }
        None => {
            eprintln!("drovr: no active pane for run '{name}'; try 'drovr phase start {name} <phase>'");
            process::exit(1);
        }
    }
}

fn cmd_cleanup(name: &str, purge: bool, herdr: &SystemHerdr) {
    if let Err(e) = validate_run_name(name) {
        eprintln!("drovr: {e}");
        process::exit(1);
    }
    let run = load_run(name);

    // Close the run's workspace (this closes all phase panes within it).
    // Older runs without a recorded workspace id are skipped gracefully.
    if let Some(ws_id) = &run.workspace
        && let Err(e) = herdr.workspace_close(ws_id)
    {
        eprintln!("drovr: warning: workspace_close({ws_id}) failed: {e}");
    }

    if purge {
        let dir = run_dir(name);
        if let Err(e) = std::fs::remove_dir_all(&dir) {
            eprintln!("drovr: failed to remove run dir {}: {e}", dir.display());
            process::exit(1);
        }
        println!("cleaned up and purged run '{name}'");
    } else {
        println!("cleaned up run '{name}' (run dir kept; use --purge to delete)");
    }
}

fn cmd_resurrect(name: &str) {
    if let Err(e) = validate_run_name(name) {
        eprintln!("drovr: {e}");
        process::exit(1);
    }
    let run = load_run(name);
    match run.first_incomplete() {
        Some(idx) => {
            println!(
                "run '{name}' — resume at phase {idx}: {}",
                run.phases[idx].name
            );
            // Print all phases for context
            for (i, p) in run.phases.iter().enumerate() {
                println!("  [{i}] {} — {}", p.name, phase_status_str(&p.status));
            }
            println!();
            println!(
                "To resume: drovr phase start {name} {}",
                run.phases[idx].name
            );
        }
        None => {
            println!("run '{name}' is fully complete — nothing to resurrect");
        }
    }
}

fn cmd_serve(name: &str, host: &str, port: u16) {
    if let Err(e) = validate_run_name(name) {
        eprintln!("drovr: {e}");
        process::exit(1);
    }
    if let Err(e) = serve(name, host, port) {
        eprintln!("drovr: serve failed: {e}");
        process::exit(1);
    }
}

// ---------------------------------------------------------------------------
// Plumbing handlers
// ---------------------------------------------------------------------------

fn cmd_phase(sub: PhaseCmd) {
    let h = SystemHerdr::new();
    let r = SystemRunner;

    match sub {
        PhaseCmd::Start { run, phase_name, seed } => {
            if let Err(e) = validate_run_name(&run) {
                eprintln!("drovr: {e}");
                process::exit(1);
            }
            let mut state = load_run(&run);
            if let Err(e) = phase_start(&h, &mut state, &phase_name, seed.as_deref()) {
                eprintln!("drovr: phase start failed: {e}");
                process::exit(1);
            }
            println!("started phase '{phase_name}' for run '{run}'");
        }
        PhaseCmd::Send { run, phase_name, text } => {
            if let Err(e) = validate_run_name(&run) {
                eprintln!("drovr: {e}");
                process::exit(1);
            }
            let state = load_run(&run);
            if let Err(e) = phase_send(&h, &state, &phase_name, &text) {
                eprintln!("drovr: phase send failed: {e}");
                process::exit(1);
            }
        }
        PhaseCmd::Wait { run, phase_name, timeout_ms } => {
            if let Err(e) = validate_run_name(&run) {
                eprintln!("drovr: {e}");
                process::exit(1);
            }
            let mut state = load_run(&run);
            match phase_wait(&mut state, &phase_name, timeout_ms) {
                Ok(true) => println!("phase '{phase_name}' done"),
                Ok(false) => {
                    // Liveness net: a timeout can mean the agent is genuinely
                    // still working, OR it parked on a first-run prompt with no
                    // human to answer it. Read the pane once (read-only,
                    // focus-safe) and, if it matches a known "waiting on a
                    // prompt" signature, surface an actionable diagnostic instead
                    // of the bare timeout line.
                    if let Some(diag) = diagnose_stuck_phase(&h, &state, &phase_name) {
                        eprintln!("drovr: {diag}");
                    } else {
                        println!("phase '{phase_name}' still running (timeout)");
                    }
                    process::exit(2);
                }
                Err(e) => {
                    eprintln!("drovr: phase wait failed: {e}");
                    process::exit(1);
                }
            }
        }
        PhaseCmd::Done { run, phase_name } => {
            if let Err(e) = validate_run_name(&run) {
                eprintln!("drovr: {e}");
                process::exit(1);
            }
            let state = load_run(&run);
            match phase_done(&state, &phase_name) {
                Ok(path) => println!("marked phase '{phase_name}' done ({})", path.display()),
                Err(e) => {
                    eprintln!("drovr: phase done failed: {e}");
                    process::exit(1);
                }
            }
        }
        PhaseCmd::Compress { run, phase_name } => {
            if let Err(e) = validate_run_name(&run) {
                eprintln!("drovr: {e}");
                process::exit(1);
            }
            let state = load_run(&run);
            match phase_compress(&h, &r, &state, &phase_name) {
                Ok(path) => println!("handoff written to {}", path.display()),
                Err(e) => {
                    eprintln!("drovr: phase compress failed: {e}");
                    process::exit(1);
                }
            }
        }
    }
}

/// Resolve the transcript to compress, by precedence:
/// `transcript` file > `pane` (or `$HERDR_PANE_ID`) > the `stdin` reader.
/// Returns the transcript text; returns an error rather than exiting so the
/// precedence logic is unit-testable (the caller maps errors to `process::exit`).
fn resolve_transcript<H: Herdr, R: io::Read>(
    transcript: Option<&std::path::Path>,
    pane: Option<String>,
    herdr: &H,
    mut stdin: R,
) -> io::Result<String> {
    if let Some(path) = transcript {
        std::fs::read_to_string(path).map_err(|e| {
            io::Error::new(e.kind(), format!("cannot read transcript {}: {e}", path.display()))
        })
    } else if let Some(pane_id) = pane.or_else(|| std::env::var("HERDR_PANE_ID").ok()) {
        herdr
            .agent_read(&pane_id)
            .map_err(|e| io::Error::new(e.kind(), format!("cannot read pane '{pane_id}': {e}")))
    } else {
        let mut buf = String::new();
        stdin
            .read_to_string(&mut buf)
            .map_err(|e| io::Error::new(e.kind(), format!("cannot read stdin: {e}")))?;
        Ok(buf)
    }
}

fn cmd_handoff(sub: HandoffCmd, herdr: &SystemHerdr) {
    match sub {
        HandoffCmd::Own { objective, transcript, pane, out } => {
            // Resolve transcript by precedence:
            //   --transcript file > --pane (or $HERDR_PANE_ID) > stdin
            let transcript_text =
                resolve_transcript(transcript.as_deref(), pane, herdr, io::stdin())
                    .unwrap_or_else(|e| {
                        eprintln!("drovr: {e}");
                        process::exit(1);
                    });

            let objective =
                objective.unwrap_or_else(|| "(self-serve mid-task handoff)".to_string());
            let out = out.unwrap_or_else(|| PathBuf::from("./HANDOFF.md"));

            match handoff_self(&SystemRunner, &transcript_text, &objective, &out) {
                Ok(path) => {
                    // Print an absolute resume pointer. canonicalize succeeds on
                    // the success path (the file was just written); fall back to
                    // joining cwd so a relative --out still prints absolute.
                    let abs = std::fs::canonicalize(&path).unwrap_or_else(|_| {
                        if path.is_absolute() {
                            path.clone()
                        } else {
                            std::env::current_dir()
                                .map(|d| d.join(&path))
                                .unwrap_or_else(|_| path.clone())
                        }
                    });
                    println!("handoff written to {}", abs.display());
                    println!(
                        "resume: start a fresh agent and read {} as its only briefing",
                        abs.display()
                    );
                }
                Err(e) => {
                    eprintln!("drovr: handoff self failed: {e}");
                    process::exit(1);
                }
            }
        }
    }
}

fn cmd_collect(run: &str, phase_name: &str) {
    if let Err(e) = validate_run_name(run) {
        eprintln!("drovr: {e}");
        process::exit(1);
    }
    let state = load_run(run);
    match collect(&state, phase_name) {
        Ok(content) => print!("{content}"),
        Err(e) => {
            eprintln!("drovr: collect failed: {e}");
            process::exit(1);
        }
    }
}

fn cmd_review(sub: ReviewCmd) {
    match sub {
        ReviewCmd::Summary { run, text } => {
            if let Err(e) = validate_run_name(&run) {
                eprintln!("drovr: {e}");
                process::exit(1);
            }
            if let Err(e) = review_summary(&run, &text) {
                eprintln!("drovr: review summary failed: {e}");
                process::exit(1);
            }
        }
        ReviewCmd::Wait { run, timeout_ms } => {
            if let Err(e) = validate_run_name(&run) {
                eprintln!("drovr: {e}");
                process::exit(1);
            }
            match review_wait(&run, timeout_ms) {
                Ok(WaitOutcome::Approved) => {
                    println!("review approved for run '{run}'");
                }
                Ok(WaitOutcome::ChangesRequested) => {
                    println!("review: changes requested for run '{run}' (see feedback.json)");
                    process::exit(3);
                }
                Ok(WaitOutcome::Timeout) => {
                    println!("review: no reviewer action for run '{run}' within timeout (re-run to resume)");
                    process::exit(2);
                }
                Err(e) => {
                    eprintln!("drovr: review wait failed: {e}");
                    process::exit(1);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

fn main() {
    let cli = Cli::parse();
    let herdr = SystemHerdr::new();

    match cli.command {
        Commands::List => cmd_list(),
        Commands::New { name, task, dir } => cmd_new(&name, task, dir, &herdr),
        Commands::Status { name } => cmd_status(&name),
        Commands::Attach { name } => cmd_attach(&name),
        Commands::Cleanup { name, purge } => cmd_cleanup(&name, purge, &herdr),
        Commands::Resurrect { name } => cmd_resurrect(&name),
        Commands::Serve { name, host, port } => cmd_serve(&name, &host, port),
        Commands::Handoff { sub } => cmd_handoff(sub, &herdr),
        Commands::Phase { sub } => cmd_phase(sub),
        Commands::Collect { run, phase_name } => cmd_collect(&run, &phase_name),
        Commands::Review { sub } => cmd_review(sub),
    }
}

// ---------------------------------------------------------------------------
// Shared env-var lock for tests that mutate `XDG_DATA_HOME`.
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod test_util {
    use std::sync::Mutex;
    pub static ENV_LOCK: Mutex<()> = Mutex::new(());
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(args)
    }

    // -- clap parse tests -------------------------------------------------------

    #[test]
    fn parse_list() {
        let cli = parse(&["drovr", "list"]).unwrap();
        assert!(matches!(cli.command, Commands::List));
    }

    #[test]
    fn parse_new_no_task() {
        let cli = parse(&["drovr", "new", "myrun"]).unwrap();
        assert!(matches!(cli.command, Commands::New { name, task: None, .. } if name == "myrun"));
    }

    #[test]
    fn parse_new_with_task() {
        let cli = parse(&["drovr", "new", "myrun", "--task", "build a thing"]).unwrap();
        match cli.command {
            Commands::New { name, task, .. } => {
                assert_eq!(name, "myrun");
                assert_eq!(task.as_deref(), Some("build a thing"));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn parse_status() {
        let cli = parse(&["drovr", "status", "myrun"]).unwrap();
        assert!(matches!(cli.command, Commands::Status { name } if name == "myrun"));
    }

    #[test]
    fn parse_attach() {
        let cli = parse(&["drovr", "attach", "myrun"]).unwrap();
        assert!(matches!(cli.command, Commands::Attach { name } if name == "myrun"));
    }

    #[test]
    fn parse_cleanup_no_purge() {
        let cli = parse(&["drovr", "cleanup", "myrun"]).unwrap();
        assert!(matches!(cli.command, Commands::Cleanup { name, purge: false } if name == "myrun"));
    }

    #[test]
    fn parse_cleanup_purge() {
        let cli = parse(&["drovr", "cleanup", "myrun", "--purge"]).unwrap();
        assert!(matches!(cli.command, Commands::Cleanup { name, purge: true } if name == "myrun"));
    }

    #[test]
    fn parse_resurrect() {
        let cli = parse(&["drovr", "resurrect", "myrun"]).unwrap();
        assert!(matches!(cli.command, Commands::Resurrect { name } if name == "myrun"));
    }

    #[test]
    fn parse_serve_defaults() {
        let cli = parse(&["drovr", "serve", "myrun"]).unwrap();
        match cli.command {
            Commands::Serve { name, host, port } => {
                assert_eq!(name, "myrun");
                assert_eq!(host, "127.0.0.1");
                assert_eq!(port, 8791);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn parse_serve_custom_port() {
        let cli = parse(&["drovr", "serve", "demo", "--port", "9000"]).unwrap();
        match cli.command {
            Commands::Serve { name, port, .. } => {
                assert_eq!(name, "demo");
                assert_eq!(port, 9000);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn parse_phase_start() {
        let cli = parse(&["drovr", "phase", "start", "myrun", "brainstorm"]).unwrap();
        match cli.command {
            Commands::Phase { sub: PhaseCmd::Start { run, phase_name, seed } } => {
                assert_eq!(run, "myrun");
                assert_eq!(phase_name, "brainstorm");
                assert!(seed.is_none());
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn parse_phase_start_with_seed() {
        let cli = parse(&["drovr", "phase", "start", "myrun", "brainstorm", "--seed", "/tmp/seed.md"]).unwrap();
        match cli.command {
            Commands::Phase { sub: PhaseCmd::Start { seed, .. } } => {
                assert_eq!(seed.as_deref(), Some(std::path::Path::new("/tmp/seed.md")));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn parse_phase_send() {
        let cli = parse(&["drovr", "phase", "send", "myrun", "plan", "hello"]).unwrap();
        match cli.command {
            Commands::Phase { sub: PhaseCmd::Send { run, phase_name, text } } => {
                assert_eq!(run, "myrun");
                assert_eq!(phase_name, "plan");
                assert_eq!(text, "hello");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn parse_phase_wait_default_timeout() {
        let cli = parse(&["drovr", "phase", "wait", "myrun", "plan"]).unwrap();
        match cli.command {
            Commands::Phase { sub: PhaseCmd::Wait { timeout_ms, .. } } => {
                assert_eq!(timeout_ms, 30_000);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn parse_phase_compress() {
        let cli = parse(&["drovr", "phase", "compress", "demo", "plan"]).unwrap();
        match cli.command {
            Commands::Phase { sub: PhaseCmd::Compress { run, phase_name } } => {
                assert_eq!(run, "demo");
                assert_eq!(phase_name, "plan");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn parse_handoff_self() {
        let cli = parse(&[
            "drovr", "handoff", "self", "--objective", "o", "--out", "/tmp/h.md",
        ])
        .unwrap();
        match cli.command {
            Commands::Handoff { sub: HandoffCmd::Own { objective, transcript, pane, out } } => {
                assert_eq!(objective.as_deref(), Some("o"));
                assert_eq!(out.as_deref(), Some(std::path::Path::new("/tmp/h.md")));
                assert!(transcript.is_none());
                assert!(pane.is_none());
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn parse_handoff_self_all_none() {
        let cli = parse(&["drovr", "handoff", "self"]).unwrap();
        match cli.command {
            Commands::Handoff { sub: HandoffCmd::Own { objective, transcript, pane, out } } => {
                assert!(objective.is_none());
                assert!(transcript.is_none());
                assert!(pane.is_none());
                assert!(out.is_none());
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn parse_collect() {
        let cli = parse(&["drovr", "collect", "myrun", "brainstorm"]).unwrap();
        match cli.command {
            Commands::Collect { run, phase_name } => {
                assert_eq!(run, "myrun");
                assert_eq!(phase_name, "brainstorm");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn parse_review_summary() {
        let cli = parse(&["drovr", "review", "summary", "myrun", "the text"]).unwrap();
        match cli.command {
            Commands::Review { sub: ReviewCmd::Summary { run, text } } => {
                assert_eq!(run, "myrun");
                assert_eq!(text, "the text");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn parse_review_wait_default_timeout() {
        let cli = parse(&["drovr", "review", "wait", "myrun"]).unwrap();
        match cli.command {
            Commands::Review { sub: ReviewCmd::Wait { run, timeout_ms } } => {
                assert_eq!(run, "myrun");
                // Generous default (30 min) — not a short silent cap.
                assert_eq!(timeout_ms, 1_800_000);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn parse_review_wait_custom_timeout() {
        let cli = parse(&["drovr", "review", "wait", "myrun", "--timeout-ms", "5000"]).unwrap();
        match cli.command {
            Commands::Review { sub: ReviewCmd::Wait { run, timeout_ms } } => {
                assert_eq!(run, "myrun");
                assert_eq!(timeout_ms, 5000);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn unknown_subcommand_errors() {
        assert!(parse(&["drovr", "bogus"]).is_err());
    }

    // -- validate_run_name tests -----------------------------------------------

    #[test]
    fn validate_run_name_accepts_normal_name() {
        assert!(validate_run_name("my-feature-run").is_ok());
        assert!(validate_run_name("run1").is_ok());
        assert!(validate_run_name("abc").is_ok());
    }

    #[test]
    fn validate_run_name_rejects_path_traversal() {
        assert!(validate_run_name("../x").is_err());
        assert!(validate_run_name("a/b").is_err());
        assert!(validate_run_name("a\\b").is_err());
        assert!(validate_run_name("").is_err());
    }

    // -- format_progress helper -------------------------------------------------

    #[test]
    fn format_progress_none_done() {
        let run = RunState {
            name: "r".into(),
            task: "t".into(),
            phases: vec![
                run::Phase { name: "brainstorm".into(), status: PhaseStatus::Pending,
                    handoff_doc: None, herdr_session: None, pane_id: None },
                run::Phase { name: "plan".into(), status: PhaseStatus::Pending,
                    handoff_doc: None, herdr_session: None, pane_id: None },
            ],
            // A populated review_phases list must not shift the "0/2" progress or
            // the "current" phase — format_progress walks `phases` only.
            review_phases: vec![
                run::Phase { name: "review:task-1:1:correctness".into(), status: PhaseStatus::Running,
                    handoff_doc: None, herdr_session: None, pane_id: None },
            ],
            gate: "spec".into(),
            cursor: 0,
            workspace: None,
            root_pane: None,
            project_dir: "/tmp/proj".into(),
        };
        let s = format_progress(&run);
        assert!(s.contains("0/2"), "got: {s}");
        assert!(s.contains("brainstorm"), "got: {s}");
    }

    #[test]
    fn format_progress_all_done() {
        let run = RunState {
            name: "r".into(),
            task: "t".into(),
            phases: vec![
                run::Phase { name: "brainstorm".into(), status: PhaseStatus::Done,
                    handoff_doc: None, herdr_session: None, pane_id: None },
            ],
            review_phases: vec![],
            gate: "spec".into(),
            cursor: 0,
            workspace: None,
            root_pane: None,
            project_dir: "/tmp/proj".into(),
        };
        let s = format_progress(&run);
        assert!(s.contains("1/1"), "got: {s}");
        assert!(s.contains("all done"), "got: {s}");
    }

    // -- resolve_transcript: the `drovr handoff self` source-precedence helper ----
    // (transcript file > pane / $HERDR_PANE_ID > stdin). ENV_LOCK guards the tests
    // that depend on HERDR_PANE_ID being unset.

    #[test]
    fn resolve_transcript_prefers_file_over_pane_and_stdin() {
        let _lock = crate::test_util::ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.txt");
        std::fs::write(&p, "FILE CONTENT").unwrap();
        let h = herdr::FakeHerdr::new();
        h.push_read("PANE CONTENT");
        let out =
            resolve_transcript(Some(p.as_path()), Some("w1:p1".into()), &h, &b"STDIN"[..]).unwrap();
        assert_eq!(out, "FILE CONTENT");
    }

    #[test]
    fn resolve_transcript_uses_pane_when_no_file() {
        let _lock = crate::test_util::ENV_LOCK.lock().unwrap();
        let h = herdr::FakeHerdr::new();
        h.push_read("PANE CONTENT");
        let out = resolve_transcript(None, Some("w1:p1".into()), &h, &b"STDIN"[..]).unwrap();
        assert_eq!(out, "PANE CONTENT");
    }

    #[test]
    fn resolve_transcript_falls_back_to_stdin() {
        let _lock = crate::test_util::ENV_LOCK.lock().unwrap();
        // The pane branch consults $HERDR_PANE_ID; unset it so we reach stdin.
        unsafe { std::env::remove_var("HERDR_PANE_ID") };
        let h = herdr::FakeHerdr::new();
        let out = resolve_transcript(None, None, &h, &b"STDIN CONTENT"[..]).unwrap();
        assert_eq!(out, "STDIN CONTENT");
    }
}
