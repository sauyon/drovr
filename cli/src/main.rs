mod compress;
mod herdr;
mod phase;
mod review;
mod run;

use clap::{Parser, Subcommand};
use compress::{SystemRunner, phase_compress};
use herdr::{Herdr, SystemHerdr};
use phase::{collect, phase_send, phase_start, phase_wait};
use review::{review_summary, serve};
use run::{PhaseStatus, RunState, run_dir};
use std::path::PathBuf;
use std::process;

// ---------------------------------------------------------------------------
// CLI definition
// ---------------------------------------------------------------------------

#[derive(Debug, Parser)]
#[command(name = "relay", about = "Relay workflow manager")]
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
    /// Wait for a phase to complete.
    Wait {
        run: String,
        phase_name: String,
        #[arg(long, default_value_t = 30_000)]
        timeout_ms: u64,
    },
    /// Compress a finished phase into a handoff doc.
    Compress {
        run: String,
        phase_name: String,
    },
}

#[derive(Debug, Subcommand)]
enum ReviewCmd {
    /// POST summary text to the running review server.
    Summary { run: String, text: String },
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn load_run(name: &str) -> RunState {
    RunState::load(name).unwrap_or_else(|e| {
        eprintln!("relay: failed to load run '{name}': {e}");
        process::exit(1);
    })
}

fn save_run(run: &RunState) {
    run.save().unwrap_or_else(|e| {
        eprintln!("relay: failed to save run '{}': {e}", run.name);
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
            PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/share")
        });
    let runs_dir = base.join("relay").join("runs");

    let entries = match std::fs::read_dir(&runs_dir) {
        Ok(e) => e,
        Err(_) => {
            println!("no runs found");
            return;
        }
    };

    let mut found = false;
    for entry in entries.flatten() {
        let state_path = entry.path().join("state.json");
        if let Some(run) = std::fs::read_to_string(&state_path)
            .ok()
            .and_then(|s| serde_json::from_str::<RunState>(&s).ok())
        {
            println!("{:20}  {}", run.name, format_progress(&run));
            found = true;
        }
    }
    if !found {
        println!("no runs found");
    }
}

fn cmd_new(name: &str, task: Option<String>, herdr: &SystemHerdr) {
    if !herdr.integration_present() {
        eprintln!("prerequisite missing: run 'herdr integration install claude'");
        process::exit(1);
    }

    let task_str = task.unwrap_or_else(|| "(no task specified)".to_string());

    let run = RunState {
        name: name.to_owned(),
        task: task_str,
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
        gate: "spec".into(),
        cursor: 0,
    };

    save_run(&run);
    println!("created run '{}' at {}", name, run_dir(name).display());
}

fn cmd_status(name: &str) {
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
                    eprintln!("relay: failed to exec herdr: {e}");
                    process::exit(1);
                });
            if !status.success() {
                process::exit(status.code().unwrap_or(1));
            }
        }
        None => {
            eprintln!("relay: no active pane for run '{name}'; try 'relay phase start {name} <phase>'");
            process::exit(1);
        }
    }
}

fn cmd_cleanup(name: &str, purge: bool, herdr: &SystemHerdr) {
    let run = load_run(name);

    // Stop the herdr session if any phase has one recorded
    let sessions: Vec<String> = run
        .phases
        .iter()
        .filter_map(|p| p.herdr_session.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    for session in &sessions {
        if let Err(e) = herdr.session_stop(session) {
            eprintln!("relay: warning: session_stop({session}) failed: {e}");
        }
    }

    if purge {
        let dir = run_dir(name);
        if let Err(e) = std::fs::remove_dir_all(&dir) {
            eprintln!("relay: failed to remove run dir {}: {e}", dir.display());
            process::exit(1);
        }
        println!("cleaned up and purged run '{name}'");
    } else {
        println!("cleaned up run '{name}' (run dir kept; use --purge to delete)");
    }
}

fn cmd_resurrect(name: &str) {
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
                "To resume: relay phase start {name} {}",
                run.phases[idx].name
            );
        }
        None => {
            println!("run '{name}' is fully complete — nothing to resurrect");
        }
    }
}

fn cmd_serve(name: &str, host: &str, port: u16) {
    if let Err(e) = serve(name, host, port) {
        eprintln!("relay: serve failed: {e}");
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
            let mut state = load_run(&run);
            if let Err(e) = phase_start(&h, &mut state, &phase_name, seed.as_deref()) {
                eprintln!("relay: phase start failed: {e}");
                process::exit(1);
            }
            println!("started phase '{phase_name}' for run '{run}'");
        }
        PhaseCmd::Send { run, phase_name, text } => {
            let state = load_run(&run);
            if let Err(e) = phase_send(&h, &state, &phase_name, &text) {
                eprintln!("relay: phase send failed: {e}");
                process::exit(1);
            }
        }
        PhaseCmd::Wait { run, phase_name, timeout_ms } => {
            let mut state = load_run(&run);
            match phase_wait(&h, &mut state, &phase_name, timeout_ms) {
                Ok(true) => println!("phase '{phase_name}' done"),
                Ok(false) => {
                    println!("phase '{phase_name}' still running (timeout)");
                    process::exit(2);
                }
                Err(e) => {
                    eprintln!("relay: phase wait failed: {e}");
                    process::exit(1);
                }
            }
        }
        PhaseCmd::Compress { run, phase_name } => {
            let state = load_run(&run);
            match phase_compress(&h, &r, &state, &phase_name) {
                Ok(path) => println!("handoff written to {}", path.display()),
                Err(e) => {
                    eprintln!("relay: phase compress failed: {e}");
                    process::exit(1);
                }
            }
        }
    }
}

fn cmd_collect(run: &str, phase_name: &str) {
    let state = load_run(run);
    match collect(&state, phase_name) {
        Ok(content) => print!("{content}"),
        Err(e) => {
            eprintln!("relay: collect failed: {e}");
            process::exit(1);
        }
    }
}

fn cmd_review(sub: ReviewCmd) {
    match sub {
        ReviewCmd::Summary { run, text } => {
            if let Err(e) = review_summary(&run, &text) {
                eprintln!("relay: review summary failed: {e}");
                process::exit(1);
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
        Commands::New { name, task } => cmd_new(&name, task, &herdr),
        Commands::Status { name } => cmd_status(&name),
        Commands::Attach { name } => cmd_attach(&name),
        Commands::Cleanup { name, purge } => cmd_cleanup(&name, purge, &herdr),
        Commands::Resurrect { name } => cmd_resurrect(&name),
        Commands::Serve { name, host, port } => cmd_serve(&name, &host, port),
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
        let cli = parse(&["relay", "list"]).unwrap();
        assert!(matches!(cli.command, Commands::List));
    }

    #[test]
    fn parse_new_no_task() {
        let cli = parse(&["relay", "new", "myrun"]).unwrap();
        assert!(matches!(cli.command, Commands::New { name, task: None } if name == "myrun"));
    }

    #[test]
    fn parse_new_with_task() {
        let cli = parse(&["relay", "new", "myrun", "--task", "build a thing"]).unwrap();
        match cli.command {
            Commands::New { name, task } => {
                assert_eq!(name, "myrun");
                assert_eq!(task.as_deref(), Some("build a thing"));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn parse_status() {
        let cli = parse(&["relay", "status", "myrun"]).unwrap();
        assert!(matches!(cli.command, Commands::Status { name } if name == "myrun"));
    }

    #[test]
    fn parse_attach() {
        let cli = parse(&["relay", "attach", "myrun"]).unwrap();
        assert!(matches!(cli.command, Commands::Attach { name } if name == "myrun"));
    }

    #[test]
    fn parse_cleanup_no_purge() {
        let cli = parse(&["relay", "cleanup", "myrun"]).unwrap();
        assert!(matches!(cli.command, Commands::Cleanup { name, purge: false } if name == "myrun"));
    }

    #[test]
    fn parse_cleanup_purge() {
        let cli = parse(&["relay", "cleanup", "myrun", "--purge"]).unwrap();
        assert!(matches!(cli.command, Commands::Cleanup { name, purge: true } if name == "myrun"));
    }

    #[test]
    fn parse_resurrect() {
        let cli = parse(&["relay", "resurrect", "myrun"]).unwrap();
        assert!(matches!(cli.command, Commands::Resurrect { name } if name == "myrun"));
    }

    #[test]
    fn parse_serve_defaults() {
        let cli = parse(&["relay", "serve", "myrun"]).unwrap();
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
        let cli = parse(&["relay", "serve", "demo", "--port", "9000"]).unwrap();
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
        let cli = parse(&["relay", "phase", "start", "myrun", "brainstorm"]).unwrap();
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
        let cli = parse(&["relay", "phase", "start", "myrun", "brainstorm", "--seed", "/tmp/seed.md"]).unwrap();
        match cli.command {
            Commands::Phase { sub: PhaseCmd::Start { seed, .. } } => {
                assert_eq!(seed.as_deref(), Some(std::path::Path::new("/tmp/seed.md")));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn parse_phase_send() {
        let cli = parse(&["relay", "phase", "send", "myrun", "plan", "hello"]).unwrap();
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
        let cli = parse(&["relay", "phase", "wait", "myrun", "plan"]).unwrap();
        match cli.command {
            Commands::Phase { sub: PhaseCmd::Wait { timeout_ms, .. } } => {
                assert_eq!(timeout_ms, 30_000);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn parse_phase_compress() {
        let cli = parse(&["relay", "phase", "compress", "demo", "plan"]).unwrap();
        match cli.command {
            Commands::Phase { sub: PhaseCmd::Compress { run, phase_name } } => {
                assert_eq!(run, "demo");
                assert_eq!(phase_name, "plan");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn parse_collect() {
        let cli = parse(&["relay", "collect", "myrun", "brainstorm"]).unwrap();
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
        let cli = parse(&["relay", "review", "summary", "myrun", "the text"]).unwrap();
        match cli.command {
            Commands::Review { sub: ReviewCmd::Summary { run, text } } => {
                assert_eq!(run, "myrun");
                assert_eq!(text, "the text");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn unknown_subcommand_errors() {
        assert!(parse(&["relay", "bogus"]).is_err());
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
            gate: "spec".into(),
            cursor: 0,
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
            gate: "spec".into(),
            cursor: 0,
        };
        let s = format_progress(&run);
        assert!(s.contains("1/1"), "got: {s}");
        assert!(s.contains("all done"), "got: {s}");
    }
}
