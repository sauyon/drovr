mod code_review;
mod config;
mod findings;
mod herdr;
mod phase;
mod reflex;
mod review;
mod run;
mod worktree;
/// Dependency-free SHA-256, used only by build-time integrity tests that pin
/// embedded third-party assets to known-good digests.
#[cfg(test)]
mod sha256;

use clap::{Parser, Subcommand};
use code_review::{ReviewOutcome, code_review_run, head_sha};
use herdr::{Herdr, SystemHerdr};
use phase::{
    PhaseWaitOutcome, collect, diagnose_stuck_phase, phase_done, phase_send, phase_start,
    phase_wait, triage_blocked_phase,
};
use review::{WaitOutcome, display_addr, review_summary, review_wait, serve};
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
        /// Isolate the run in a git worktree (`.drovr/wt/<run>` on branch
        /// `drovr/<run>`). Overrides the `worktree` config default. `--no-worktree`
        /// forces it off. Requires the project dir to be a git repo.
        #[arg(long, overrides_with = "no_worktree")]
        worktree: bool,
        #[arg(long = "no-worktree")]
        no_worktree: bool,
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

    /// Start the always-on review server (serves every run). Runs in the
    /// foreground; normally auto-started on demand by `drovr review …`.
    Serve {
        /// Host/address to bind. Overrides `serve_host` from config
        /// (which itself defaults to `127.0.0.1`).
        #[arg(long)]
        host: Option<String>,
        #[arg(long, default_value_t = review::DEFAULT_PORT)]
        port: u16,
    },

    /// Plumbing: phase lifecycle operations.
    Phase {
        #[command(subcommand)]
        sub: PhaseCmd,
    },

    /// Plumbing: collect the handoff doc for a finished phase.
    Collect { run: String, phase_name: String },

    /// Plumbing: review subcommands.
    Review {
        #[command(subcommand)]
        sub: ReviewCmd,
    },

    /// Automatic review-until-clean panel (see drovr:code-review).
    CodeReview {
        #[command(subcommand)]
        sub: CodeReviewCmd,
    },

    /// Emit the SessionStart reflex context as Claude Code hook JSON.
    ///
    /// Run by the `session-start` hook. Reads `--skill`, shapes it per the
    /// `[reflex]` config (master switch, preamble override, section toggles),
    /// and prints the hook JSON — or nothing when the reflex is disabled.
    Reflex {
        /// Path to the router skill markdown to inject.
        #[arg(long)]
        skill: PathBuf,
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
    /// Send text to a running phase pane. Waits for the agent to attach first;
    /// exit 2 = it never became ready within the readiness timeout (likely parked
    /// on a first-run/permission prompt — see the diagnostic), 1 = io error.
    Send {
        run: String,
        phase_name: String,
        text: String,
    },
    /// Wait for a phase to complete (polls for the `done` marker and the pane's
    /// herdr status). Exit 0 = done, 2 = timeout, 4 = blocked (agent hit a
    /// safety/permission prompt — see the triage diagnostic), 1 = io error.
    Wait {
        run: String,
        phase_name: String,
        #[arg(long, default_value_t = 30_000)]
        timeout_ms: u64,
    },
    /// Mark a phase complete. Run by the phase AGENT itself as its final action —
    /// it drops the completion marker `drovr phase wait` polls for. Refuses for a
    /// pipeline phase until that phase has authored its `<phase>-HANDOFF.md`.
    Done { run: String, phase_name: String },
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
    /// the turn), 5 = cancelled by the reviewer (terminal — tear the run down),
    /// 2 = timeout (re-run to resume), 1 = error.
    ///
    /// Note 1 (error) is distinct from every outcome: a failed wait must never
    /// be read as an approval.
    Wait {
        run: String,
        #[arg(long, default_value_t = 1_800_000)]
        timeout_ms: u64,
    },
}

#[derive(Debug, Subcommand)]
enum CodeReviewCmd {
    /// Record HEAD as the review base for `task` (run by the implement phase at
    /// task start, before any code is written, so HEAD is the pre-task SHA).
    Base { run: String, task: String },
    /// Spawn one review panel for `task`, wait, merge, exit 0/3/2/1.
    Run {
        run: String,
        task: String,
        #[arg(long, default_value_t = 1_800_000)]
        timeout_ms: u64,
    },
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Reject a label (run name, task, ...) that is empty or contains path-separator
/// characters. `kind` names the label in the error message. Prevents path traversal
/// in commands that touch the filesystem with the value as a path component.
fn validate_label(kind: &str, s: &str) -> io::Result<()> {
    if s.is_empty() || s.contains('/') || s.contains('\\') || s.contains("..") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid {kind} {s:?}: must not be empty or contain '/', '\\\\', or '..'"),
        ));
    }
    Ok(())
}

/// Reject run names that are empty or contain path-separator characters.
/// Thin wrapper over the shared [`validate_label`] predicate.
fn validate_run_name(name: &str) -> io::Result<()> {
    validate_label("run name", name)
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
    let done = run
        .phases
        .iter()
        .filter(|p| p.status == PhaseStatus::Done)
        .count();
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
        .unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap()).join(".local/share"));
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

fn cmd_new(
    name: &str,
    task: Option<String>,
    dir: Option<PathBuf>,
    worktree_flag: bool,
    no_worktree_flag: bool,
    herdr: &SystemHerdr,
) {
    if let Err(e) = validate_run_name(name) {
        eprintln!("drovr: {e}");
        process::exit(1);
    }
    let cfg = config::load_config().unwrap_or_else(|e| {
        eprintln!("drovr: failed to load config: {e}");
        process::exit(1);
    });
    let agent = config::invoking_agent(&cfg);
    if !cfg.agents.contains_key(&agent) {
        eprintln!("drovr: detected unknown agent '{agent}': add it to the config agent map");
        process::exit(1);
    }
    if !herdr.integration_present(&agent) {
        eprintln!("prerequisite missing: run 'herdr integration install {agent}'");
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

    // Resolve worktree isolation: --no-worktree wins, then --worktree, else the
    // config default. When on, redirect project_dir into a fresh worktree so the
    // whole run (panes, code-review base, phase edits) lands there, never in the
    // invoking checkout.
    let use_worktree = if no_worktree_flag {
        false
    } else {
        worktree_flag || cfg.worktree
    };
    let (project_dir, worktree_path, worktree_branch) = if use_worktree {
        match worktree::create(std::path::Path::new(&project_dir), name) {
            Ok((abs, branch)) => {
                let p = abs.to_string_lossy().into_owned();
                println!("drovr: worktree {p} on branch {branch}");
                (p.clone(), Some(p), Some(branch))
            }
            Err(e) => {
                eprintln!("drovr: {e}");
                process::exit(1);
            }
        }
    } else {
        (project_dir, None, None)
    };

    let task_str = task.unwrap_or_else(|| "(no task specified)".to_string());

    // Create the workspace in the project dir so its root shell pane (reused by
    // the first phase) and every later tab start already `cd`'d into the project.
    let (workspace, root_pane) =
        match herdr.workspace_create(&format!("drovr:{name}"), &project_dir) {
            Ok(ws) => (Some(ws.id), Some(ws.root_pane)),
            Err(e) => {
                eprintln!("drovr: warning: could not create herdr workspace: {e}");
                (None, None)
            }
        };

    let run = RunState {
        name: name.to_owned(),
        task: task_str,
        agent: Some(agent),
        project_dir,
        phases: vec![
            run::Phase::new("brainstorm"),
            run::Phase::new("plan"),
            run::Phase::new("implement"),
            run::Phase::new("review"),
        ],
        review_phases: vec![],
        gate: "spec".into(),
        cursor: 0,
        workspace,
        root_pane,
        worktree_path,
        worktree_branch,
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
        let marker = if run.first_incomplete() == Some(i) {
            " <-- resume"
        } else {
            ""
        };
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
            eprintln!(
                "drovr: no active pane for run '{name}'; try 'drovr phase start {name} <phase>'"
            );
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

    // Prune the run's worktree, if any. Without --purge we keep the branch (locked
    // decision 1: drovr never merges — it hands back a reviewable branch) and let
    // git refuse a dirty tree rather than discard uncommitted work. --purge force-
    // removes and deletes the branch.
    if let Some(wt) = &run.worktree_path {
        let branch = run.worktree_branch.as_deref();
        let wt_path = std::path::Path::new(wt);
        if !wt_path.exists() {
            // The worktree is already gone (manually removed, or a prior
            // interrupted cleanup). Nothing to prune, and git ops from a missing
            // dir would fail — so skip pruning rather than abort. This keeps a run
            // recoverable/purge-able instead of wedged on a vanished worktree.
            eprintln!("drovr: worktree {wt} no longer exists; skipping prune");
            if !purge && let Some(b) = branch {
                println!("drovr: kept branch {b} — merge it with: git merge {b}");
            }
        } else {
            // Non-purge: land the run's accumulated work as one squash commit on
            // the branch before pruning (locked decision 4a), so the branch is
            // mergeable and the worktree removes cleanly. --purge discards.
            if !purge {
                let msg = format!("drovr({name}): {}", run.task);
                match worktree::commit_all(wt_path, &msg) {
                    Ok(true) => {
                        println!(
                            "drovr: committed run work to {}",
                            branch.unwrap_or("branch")
                        )
                    }
                    Ok(false) => {}
                    Err(e) => {
                        eprintln!("drovr: could not commit worktree {wt}: {e}");
                        process::exit(1);
                    }
                }
            }
            let delete_branch = if purge { branch } else { None };
            match worktree::remove(wt_path, delete_branch, purge) {
                Ok(()) => {
                    if !purge && let Some(b) = branch {
                        println!("drovr: kept branch {b} — merge it with: git merge {b}");
                    }
                }
                Err(e) => {
                    eprintln!("drovr: could not prune worktree {wt}: {e}");
                    if purge {
                        // --purge means "force it gone": warn but keep going so the
                        // run dir is still removed and the run isn't left un-purgeable.
                        eprintln!("drovr: continuing purge despite the prune failure");
                    } else {
                        eprintln!(
                            "drovr: commit or discard the changes and re-run cleanup, \
                             or use --purge to force-remove and delete the branch"
                        );
                        process::exit(1);
                    }
                }
            }
        }
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

fn cmd_serve(host: Option<String>, port: u16) {
    // `--host` omitted → fall back to the `serve_host` config field.
    let host = host.unwrap_or_else(|| {
        config::load_config()
            .map(|cfg| cfg.serve_host)
            .unwrap_or_else(|e| {
                eprintln!("drovr: failed to load config: {e}");
                process::exit(1);
            })
    });
    if let Err(e) = serve(&host, port) {
        eprintln!("drovr: serve failed: {e}");
        process::exit(1);
    }
}

// ---------------------------------------------------------------------------
// Plumbing handlers
// ---------------------------------------------------------------------------

fn cmd_phase(sub: PhaseCmd) {
    let h = SystemHerdr::new();

    match sub {
        PhaseCmd::Start {
            run,
            phase_name,
            seed,
        } => {
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
        PhaseCmd::Send {
            run,
            phase_name,
            text,
        } => {
            if let Err(e) = validate_run_name(&run) {
                eprintln!("drovr: {e}");
                process::exit(1);
            }
            let mut state = load_run(&run);
            if let Err(e) = phase_send(&h, &mut state, &phase_name, &text) {
                // A readiness timeout (agent never attached) is not a plain send
                // failure — the agent is almost certainly parked on a prompt with
                // no human at the pane. Raise it to the driver with the same
                // actionable, pane-quoting diagnostic the wait-timeout path uses,
                // and a distinct exit code (2) so the driver can escalate rather
                // than assume the seed landed.
                if e.kind() == io::ErrorKind::TimedOut {
                    if let Some(diag) = diagnose_stuck_phase(&h, &state, &phase_name) {
                        eprintln!("drovr: {diag}");
                    } else {
                        eprintln!("drovr: {e}");
                    }
                    process::exit(2);
                }
                eprintln!("drovr: phase send failed: {e}");
                process::exit(1);
            }
        }
        PhaseCmd::Wait {
            run,
            phase_name,
            timeout_ms,
        } => {
            if let Err(e) = validate_run_name(&run) {
                eprintln!("drovr: {e}");
                process::exit(1);
            }
            let mut state = load_run(&run);
            match phase_wait(&h, &mut state, &phase_name, timeout_ms) {
                Ok(PhaseWaitOutcome::Done) => println!("phase '{phase_name}' done"),
                Ok(PhaseWaitOutcome::Blocked) => {
                    // Proactive triage: herdr reported the phase pane as `blocked`
                    // (a Claude Code safety/permission prompt with no human at the
                    // pane). Classify it and either escalate (destructive/unknown)
                    // or conservatively auto-answer a routine, allow-listed prompt.
                    let t = triage_blocked_phase(&h, &state, &phase_name);
                    if t.auto_answered {
                        // A routine prompt was auto-answered; the agent should
                        // continue. Report on stdout and still exit 4 so the driver
                        // re-waits (the phase is not done yet).
                        println!("drovr: [{:?}] {}", t.class, t.diagnostic);
                    } else {
                        eprintln!("drovr: [{:?}] {}", t.class, t.diagnostic);
                    }
                    process::exit(4);
                }
                Ok(PhaseWaitOutcome::Superseded) => {
                    // NOT a timeout, and deliberately not exit 2. Another pass
                    // re-entered this phase while the wait ran, so this wait is
                    // obsolete and the agent it was watching is not the live one.
                    // Re-arming the same wait is the RIGHT move (the new pass's
                    // completion will satisfy it) — the wrong move is triaging a
                    // stuck agent that does not exist, which exit 2 invites.
                    eprintln!(
                        "drovr: phase '{phase_name}' was superseded by a newer pass while this \
                         wait was running — nothing is wrong with the phase, but this wait was \
                         watching a pass that is gone. Re-run the wait to follow the live pass."
                    );
                    process::exit(5);
                }
                Ok(PhaseWaitOutcome::TimedOut) => {
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
            match review_summary(&run, &text) {
                Ok(addr) => {
                    // The gate is now open. Serving is global and run-agnostic,
                    // so this is the only run-scoped moment that can hand the
                    // driver both halves of the gate — the page to show the
                    // human, and the watch that reports their decision.
                    println!("review: run '{run}' is ready for the reviewer");
                    println!("  page:  http://{}/#/runs/{run}", display_addr(&addr));
                    println!(
                        "  watch: drovr review wait {run}   # run this BACKGROUNDED, then end your turn"
                    );
                }
                Err(e) => {
                    eprintln!("drovr: review summary failed: {e}");
                    process::exit(1);
                }
            }
        }
        ReviewCmd::Wait { run, timeout_ms } => {
            if let Err(e) = validate_run_name(&run) {
                eprintln!("drovr: {e}");
                process::exit(1);
            }
            match review_wait(&run, timeout_ms) {
                Ok(WaitOutcome::Approved) => {
                    // Approval can carry answers: the reviewer may have answered
                    // the spec's open questions on the way to approving, and
                    // feedback.json is the only place those land. Say so, or the
                    // agent moves on and re-asks the human what they just told us.
                    println!(
                        "review approved for run '{run}' (any answers to open questions are in feedback.json)"
                    );
                }
                Ok(WaitOutcome::ChangesRequested) => {
                    println!("review: changes requested for run '{run}' (see feedback.json)");
                    process::exit(3);
                }
                Ok(WaitOutcome::Cancelled) => {
                    println!(
                        "review: run '{run}' was CANCELLED by the reviewer — stop work and tear the run down"
                    );
                    process::exit(5);
                }
                Ok(WaitOutcome::Timeout) => {
                    println!(
                        "review: no reviewer action for run '{run}' within timeout (re-run to resume)"
                    );
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

fn cmd_code_review(sub: CodeReviewCmd) {
    match sub {
        CodeReviewCmd::Base { run, task } => {
            if let Err(e) = validate_run_name(&run) {
                eprintln!("drovr: {e}");
                process::exit(1);
            }
            if let Err(e) = validate_label("task", &task) {
                eprintln!("drovr: {e}");
                process::exit(1);
            }
            let state = load_run(&run);
            // A run created before project_dir existed can't resolve a HEAD to
            // record; mirror `phase_start`'s guidance rather than recording a
            // base from the wrong directory.
            if state.project_dir.is_empty() {
                eprintln!(
                    "drovr: run '{run}' has no project_dir (created before this fix); \
                     please recreate the run with `drovr new`"
                );
                process::exit(1);
            }
            let sha = head_sha(&state.project_dir).unwrap_or_else(|e| {
                eprintln!("drovr: cannot read HEAD in '{}': {e}", state.project_dir);
                process::exit(1);
            });
            let dir = run_dir(&run);
            if let Err(e) = std::fs::create_dir_all(&dir) {
                eprintln!("drovr: cannot create run dir: {e}");
                process::exit(1);
            }
            let path = dir.join(format!("{task}-base.sha"));
            if let Err(e) = std::fs::write(&path, format!("{sha}\n")) {
                eprintln!("drovr: cannot write {}: {e}", path.display());
                process::exit(1);
            }
            println!(
                "recorded review base for '{task}' = {sha} ({})",
                path.display()
            );
        }
        CodeReviewCmd::Run {
            run,
            task,
            timeout_ms,
        } => {
            if let Err(e) = validate_run_name(&run) {
                eprintln!("drovr: {e}");
                process::exit(1);
            }
            if let Err(e) = validate_label("task", &task) {
                eprintln!("drovr: {e}");
                process::exit(1);
            }
            let h = SystemHerdr::new();
            let mut state = load_run(&run);
            let outcome = code_review_run(&h, &mut state, &task, timeout_ms);
            // Persist the review_phases progress the panel recorded (spawned
            // reviewers, done/running status) BEFORE handling the result:
            // `code_review_run` appends reviewer phases as it spawns them and can
            // then fail (e.g. a mid-spawn `phase_send` error) with those phases
            // only in memory, so an unconditional save here keeps disk and memory
            // in sync on every path — including the `Err` early-exit below.
            //
            // Transplant ONLY `review_phases` onto a freshly-loaded state. A panel
            // runs for many minutes and `state` is a snapshot from before it
            // started; writing the whole thing back would resurrect every pipeline
            // phase's status as of panel-start — including a `Done` that a
            // `phase send` re-entry has since cleared, which makes the next
            // `phase wait` report success for an agent that is mid-work.
            // `code_review_run` only ever mutates `review_phases`, so nothing else
            // in the snapshot is worth keeping.
            let mut merged = load_run(&run);
            merged.review_phases = state.review_phases.clone();
            save_run(&merged);
            let outcome = outcome.unwrap_or_else(|e| {
                eprintln!("drovr: code-review run failed: {e}");
                process::exit(1);
            });
            match outcome {
                ReviewOutcome::Clean => {
                    println!("code-review: clean for '{task}' — no blocking findings");
                }
                ReviewOutcome::Findings => {
                    let merged = run_dir(&run).join(format!("{task}-review.json"));
                    println!(
                        "code-review: changes requested for '{task}' (see {})",
                        merged.display()
                    );
                    process::exit(3);
                }
                ReviewOutcome::Timeout => {
                    println!(
                        "code-review: reviewers did not finish for '{task}' within timeout (re-run to resume)"
                    );
                    process::exit(2);
                }
                ReviewOutcome::Error => {
                    eprintln!("code-review: could not run panel for '{task}' (see message above)");
                    process::exit(1);
                }
            }
        }
    }
}

/// Emit the SessionStart reflex context, or nothing when the reflex is disabled.
///
/// The `DROVR_PHASE` phase-suppression lives in the bash hook (it gates before
/// this runs). Here we honor the config master switch and, when enabled, read
/// the skill file — failing loudly on a read error rather than injecting a
/// poisoned or partial context.
fn cmd_reflex(skill: &std::path::Path) {
    let cfg = config::load_config().unwrap_or_else(|e| {
        eprintln!("drovr: failed to load config: {e}");
        process::exit(1);
    });
    let skill_md = std::fs::read_to_string(skill).unwrap_or_else(|e| {
        eprintln!("drovr: cannot read reflex skill {}: {e}", skill.display());
        process::exit(1);
    });
    // `reflex_json` is the single authority on the `enabled` switch: it returns
    // `None` (emit nothing) when the reflex is disabled, `Some(json)` otherwise.
    if let Some(json) = reflex::reflex_json(&skill_md, &cfg.reflex) {
        println!("{json}");
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
        Commands::New {
            name,
            task,
            dir,
            worktree,
            no_worktree,
        } => cmd_new(&name, task, dir, worktree, no_worktree, &herdr),
        Commands::Status { name } => cmd_status(&name),
        Commands::Attach { name } => cmd_attach(&name),
        Commands::Cleanup { name, purge } => cmd_cleanup(&name, purge, &herdr),
        Commands::Resurrect { name } => cmd_resurrect(&name),
        Commands::Serve { host, port } => cmd_serve(host, port),
        Commands::Phase { sub } => cmd_phase(sub),
        Commands::Collect { run, phase_name } => cmd_collect(&run, &phase_name),
        Commands::Review { sub } => cmd_review(sub),
        Commands::CodeReview { sub } => cmd_code_review(sub),
        Commands::Reflex { skill } => cmd_reflex(&skill),
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
        // No run arg anymore — the always-on server serves every run.
        let cli = parse(&["drovr", "serve"]).unwrap();
        match cli.command {
            Commands::Serve { host, port } => {
                assert_eq!(host, None);
                assert_eq!(port, 8791);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn parse_serve_custom_port() {
        let cli = parse(&["drovr", "serve", "--port", "9000"]).unwrap();
        match cli.command {
            Commands::Serve { port, .. } => {
                assert_eq!(port, 9000);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn parse_serve_explicit_host() {
        // An explicit `--host` parses to `Some(..)`, which `cmd_serve` uses
        // verbatim (bypassing the `serve_host` config fallback).
        let cli = parse(&["drovr", "serve", "--host", "0.0.0.0"]).unwrap();
        match cli.command {
            Commands::Serve { host, .. } => assert_eq!(host, Some("0.0.0.0".to_string())),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn parse_phase_start() {
        let cli = parse(&["drovr", "phase", "start", "myrun", "brainstorm"]).unwrap();
        match cli.command {
            Commands::Phase {
                sub:
                    PhaseCmd::Start {
                        run,
                        phase_name,
                        seed,
                    },
            } => {
                assert_eq!(run, "myrun");
                assert_eq!(phase_name, "brainstorm");
                assert!(seed.is_none());
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn parse_phase_start_with_seed() {
        let cli = parse(&[
            "drovr",
            "phase",
            "start",
            "myrun",
            "brainstorm",
            "--seed",
            "/tmp/seed.md",
        ])
        .unwrap();
        match cli.command {
            Commands::Phase {
                sub: PhaseCmd::Start { seed, .. },
            } => {
                assert_eq!(seed.as_deref(), Some(std::path::Path::new("/tmp/seed.md")));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn parse_phase_send() {
        let cli = parse(&["drovr", "phase", "send", "myrun", "plan", "hello"]).unwrap();
        match cli.command {
            Commands::Phase {
                sub:
                    PhaseCmd::Send {
                        run,
                        phase_name,
                        text,
                    },
            } => {
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
            Commands::Phase {
                sub: PhaseCmd::Wait { timeout_ms, .. },
            } => {
                assert_eq!(timeout_ms, 30_000);
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
            Commands::Review {
                sub: ReviewCmd::Summary { run, text },
            } => {
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
            Commands::Review {
                sub: ReviewCmd::Wait { run, timeout_ms },
            } => {
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
            Commands::Review {
                sub: ReviewCmd::Wait { run, timeout_ms },
            } => {
                assert_eq!(run, "myrun");
                assert_eq!(timeout_ms, 5000);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn parse_code_review_base() {
        let cli = parse(&["drovr", "code-review", "base", "myrun", "task-1"]).unwrap();
        match cli.command {
            Commands::CodeReview {
                sub: CodeReviewCmd::Base { run, task },
            } => {
                assert_eq!(run, "myrun");
                assert_eq!(task, "task-1");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn parse_code_review_run_default_timeout() {
        let cli = parse(&["drovr", "code-review", "run", "myrun", "task-1"]).unwrap();
        match cli.command {
            Commands::CodeReview {
                sub:
                    CodeReviewCmd::Run {
                        run,
                        task,
                        timeout_ms,
                    },
            } => {
                assert_eq!(run, "myrun");
                assert_eq!(task, "task-1");
                // Generous default (30 min), matching `review wait`.
                assert_eq!(timeout_ms, 1_800_000);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn parse_code_review_run_custom_timeout() {
        let cli = parse(&[
            "drovr",
            "code-review",
            "run",
            "myrun",
            "task-1",
            "--timeout-ms",
            "5000",
        ])
        .unwrap();
        match cli.command {
            Commands::CodeReview {
                sub: CodeReviewCmd::Run { timeout_ms, .. },
            } => {
                assert_eq!(timeout_ms, 5000);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn parse_reflex() {
        let cli = parse(&["drovr", "reflex", "--skill", "/p/SKILL.md"]).unwrap();
        match cli.command {
            Commands::Reflex { skill } => {
                assert_eq!(skill, std::path::PathBuf::from("/p/SKILL.md"));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn parse_reflex_requires_skill() {
        // `--skill` is mandatory: the hook must always name the source markdown.
        assert!(parse(&["drovr", "reflex"]).is_err());
    }

    #[test]
    fn unknown_subcommand_errors() {
        assert!(parse(&["drovr", "bogus"]).is_err());
    }

    // -- validate_run_name / validate_label tests ------------------------------

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

    #[test]
    fn validate_label_rejects_unsafe_filename_components() {
        assert!(validate_label("task", "task-1").is_ok());
        assert!(validate_label("task", "..").is_err());
        assert!(validate_label("task", "a/b").is_err());
        assert!(validate_label("task", "a\\b").is_err());
        assert!(validate_label("task", "").is_err());
    }

    // -- format_progress helper -------------------------------------------------

    #[test]
    fn format_progress_none_done() {
        let run = RunState {
            name: "r".into(),
            task: "t".into(),
            agent: None,
            phases: vec![run::Phase::new("brainstorm"), run::Phase::new("plan")],
            // A populated review_phases list must not shift the "0/2" progress or
            // the "current" phase — format_progress walks `phases` only.
            review_phases: vec![run::Phase {
                name: "review:task-1:1:correctness".into(),
                status: PhaseStatus::Running,
                ..Default::default()
            }],
            gate: "spec".into(),
            cursor: 0,
            workspace: None,
            root_pane: None,
            project_dir: "/tmp/proj".into(),
            worktree_path: None,
            worktree_branch: None,
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
            agent: None,
            phases: vec![run::Phase {
                name: "brainstorm".into(),
                status: PhaseStatus::Done,
                ..Default::default()
            }],
            review_phases: vec![],
            gate: "spec".into(),
            cursor: 0,
            workspace: None,
            root_pane: None,
            project_dir: "/tmp/proj".into(),
            worktree_path: None,
            worktree_branch: None,
        };
        let s = format_progress(&run);
        assert!(s.contains("1/1"), "got: {s}");
        assert!(s.contains("all done"), "got: {s}");
    }

}
