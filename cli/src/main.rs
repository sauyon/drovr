mod brief;
mod code_review;
mod config;
mod findings;
mod herdr;
mod mcp_findings;
mod phase;
mod reflex;
mod review;
mod run;
/// Dependency-free SHA-256, used only by build-time integrity tests that pin
/// embedded third-party assets to known-good digests.
#[cfg(test)]
mod sha256;
mod shell;
mod worktree;

use brief::compose_phase_brief;
use clap::{Parser, Subcommand};
use code_review::{ReviewOutcome, code_review_brief, code_review_run, head_sha};
use herdr::{Herdr, SystemHerdr};
use phase::{
    PhaseWaitOutcome, RehydrateOutcome, collect, diagnose_stuck_phase, phase_done,
    phase_rehydrate, phase_send, phase_start, phase_wait, triage_blocked_phase,
};
use review::{WaitOutcome, display_addr, review_summary, review_wait, serve};
use run::{PhaseStatus, RunState, run_dir};
use shell::shell_single_quote;
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

    /// Write an empty `<phase>-HANDOFF.md` — the fixed seven sections and nothing
    /// else — for the finishing agent to fill in from its own context.
    ///
    /// Structure only, by design: drovr does not guess which commits or files belong
    /// to your session. Refuses to overwrite an existing handoff unless `--force`.
    HandoffScaffold {
        run: String,
        phase_name: String,
        /// Overwrite an existing `<phase>-HANDOFF.md`.
        #[arg(long)]
        force: bool,
    },

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

    /// Serve the review panel's one-tool MCP findings channel on stdio.
    ///
    /// Spawned by `code-review run` for each reviewer, never by a human. Reviewers run
    /// read-only and so cannot write their own findings file; this exposes a single
    /// `submit_findings` tool and performs that one write for them. The reviewer names
    /// its angle, which is validated against the configured angles — it can never name
    /// a path, so its one write always lands inside the run dir.
    #[command(hide = true)]
    McpFindings {
        /// Run whose findings are being collected.
        run: String,
        /// Task under review, e.g. `task-3`.
        task: String,
        /// Review iteration this panel is serving. Scopes the findings file, so one
        /// pass can never harvest another's verdicts.
        iter: u64,
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
        /// Compose this phase's brief (see `phase brief`) and inject it once the
        /// agent is at its composer. This is how a phase should be briefed: drovr
        /// owns the frame, you supply only what it cannot know.
        #[arg(long, conflicts_with = "context_file")]
        context: Option<String>,
        #[arg(long)]
        context_file: Option<PathBuf>,
        /// Spawn the agent WITHOUT a brief (the pre-brief behavior). For a phase
        /// drovr has no template for, or when you will brief it by hand with
        /// `phase send`.
        #[arg(long)]
        no_brief: bool,
    },
    /// Send text to a running phase pane. Waits for the agent to attach first;
    /// exit 2 = it never became ready within the readiness timeout (likely parked
    /// on a first-run/permission prompt — see the diagnostic), 1 = io error.
    Send {
        run: String,
        phase_name: String,
        /// The text to send, or `-` to read it from stdin — which is how a composed
        /// brief reaches an already-running phase:
        /// `drovr phase brief <run> <phase> | drovr phase send <run> <phase> -`.
        text: String,
    },
    /// Wait for a phase to complete (polls for the `done` marker and the pane's
    /// herdr status). Exit 0 = done, 2 = timeout, 4 = blocked (agent hit a
    /// safety/permission prompt — see the triage diagnostic), 5 = superseded (a
    /// newer pass re-entered the phase; re-run the wait), 1 = io error.
    Wait {
        run: String,
        phase_name: String,
        #[arg(long, default_value_t = 30_000)]
        timeout_ms: u64,
    },
    /// Bring back a phase whose pane is gone, RESUMING its recorded agent
    /// session where the backend offers one (`claude --resume <id>`).
    ///
    /// Exit 0 = the pane is back AND the agent has this phase's context (its
    /// session was resumed, or its seed was re-sent). Exit 2 = the pane is back
    /// but the agent was NOT given its context — treat it like `phase send`'s
    /// exit 2 and act, never as success. Exit 1 = refused or failed.
    ///
    /// A fresh tab in the run's project dir, launched under the profile the
    /// phase originally ran with. When no session was captured — or the backend
    /// has no resume surface — a fresh agent is launched instead and the
    /// phase's seed re-sent, which recovers the artifacts but not the
    /// conversation. Refuses a phase that still holds a pane (attach to that
    /// instead) and never creates a phase that does not exist.
    Rehydrate { run: String, phase_name: String },
    /// Print a phase's composed brief and exit, spawning nothing.
    ///
    /// This is the text a phase agent should be given: drovr's template for that
    /// phase, its substitutions filled in, the run's task, and your `--context`.
    /// Pipe it into `phase send` (or read it to see exactly what a phase is told).
    /// Composed for `brainstorm`, `plan`, `implement-task-<N>` and `review`.
    Brief {
        run: String,
        phase_name: String,
        /// What this phase needs to know that drovr cannot compose: the task brief
        /// from `plan.md`, accumulated interfaces, why the last attempt failed.
        /// RECORDED, so a re-brief of this phase reuses it; `--context ''` clears it.
        #[arg(long, conflicts_with = "context_file")]
        context: Option<String>,
        #[arg(long)]
        context_file: Option<PathBuf>,
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
    ///
    /// Re-running after a timeout (exit 2) RESUMES: it re-attaches to the panel
    /// still in flight, banks the angles that finished, and keeps waiting on the
    /// stragglers. Pass `--fresh` to abandon them and open a new panel instead.
    Run {
        run: String,
        task: String,
        #[arg(long, default_value_t = 1_800_000)]
        timeout_ms: u64,
        /// Always start a new review iteration, even if a previous one is still
        /// in flight. Use when the pending reviewers are wedged or reviewing a
        /// diff you no longer care about.
        #[arg(long)]
        fresh: bool,
        /// What this change is about, in your words. drovr composes the brief; this
        /// is the one part of it you supply. Recorded in the run dir, so a resume
        /// briefs its reviewers identically.
        #[arg(long, conflicts_with = "context_file")]
        context: Option<String>,
        /// Read `--context` from a file (use for anything longer than a sentence).
        #[arg(long)]
        context_file: Option<PathBuf>,
    },
    /// Print one angle's reviewer brief and exit, spawning nothing.
    ///
    /// For whenever you spawn the reviewer yourself instead of through the panel: an
    /// in-harness read-only subagent, a host with no herdr integration for the review
    /// agent, or a wedged panel. Pass this text to that reviewer VERBATIM — it is the
    /// same brief `code-review run` injects, so the frame stays drovr's rather than
    /// one the driver improvised.
    Brief {
        run: String,
        task: String,
        #[arg(long)]
        angle: String,
        /// See `code-review run --context`. Supplying it here RECORDS it too, so a
        /// later `run` or `brief` for this task reuses it; `--context ''` clears it.
        #[arg(long, conflicts_with = "context_file")]
        context: Option<String>,
        #[arg(long)]
        context_file: Option<PathBuf>,
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

/// Create the run's herdr workspace and label its root shell pane. `None` if
/// herdr could not create it, which is a warning rather than a failure (the run
/// still exists; `phase start` is what needs the workspace).
///
/// Returns the [`herdr::Workspace`] it was handed rather than a pair of
/// same-typed `Option<String>`s: two positional `Option<String>`s are a
/// swap away from silently recording the pane id as the workspace id.
///
/// The workspace is created in the project dir so the root shell and every phase
/// tab start already `cd`'d into the project.
///
/// **The rename is why this is a function.** No phase ever runs in the root
/// pane, so it sits in the switcher for the whole run showing an idle shell
/// prompt with nothing to explain it. Labelling it says what it is and which run
/// it anchors. Best-effort: a failed rename is cosmetic and must not cost the
/// run its workspace.
///
/// The rename is wrapped in the `focused_workspace`/`workspace_focus`
/// capture-and-restore that `launch_in_pane` uses, for the same reason:
/// `pane_rename` has no `--no-focus` flag and yanks the user's focus onto the
/// pane it renames. `workspace_create` is deliberately called with
/// `focus: false` so `drovr new` never disturbs whatever the user is doing —
/// renaming without the guard would undo that one call later.
fn create_run_workspace<H: Herdr>(
    herdr: &H,
    name: &str,
    project_dir: &str,
) -> Option<herdr::Workspace> {
    let prev_focus = herdr.focused_workspace();
    let ws = match herdr.workspace_create(&format!("drovr:{name}"), project_dir) {
        Ok(ws) => ws,
        Err(e) => {
            eprintln!("drovr: warning: could not create herdr workspace: {e}");
            return None;
        }
    };
    if let Err(e) = herdr.pane_rename(&ws.root_pane, &format!("drovr:{name} (idle shell)")) {
        eprintln!("drovr: warning: could not label the run's root shell pane: {e}");
    }
    if let Some(prev) = prev_focus {
        // Warn rather than swallow. The capture/restore exists precisely so
        // `drovr new` does not move the user; if the restore fails they are left
        // sitting in a brand-new idle workspace with no clue why, and the fix
        // (switch back) is one they can only apply if they know it happened.
        if let Err(e) = herdr.workspace_focus(&prev) {
            eprintln!("drovr: warning: could not restore focus to workspace {prev}: {e}");
        }
    }
    Some(ws)
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

    let (workspace, root_pane) = match create_run_workspace(herdr, name, &project_dir) {
        Some(ws) => (Some(ws.id), Some(ws.root_pane)),
        None => (None, None),
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
        archived: false,
        retired_panes: vec![],
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

/// What `drovr attach` found to connect the human to.
///
/// The two variants are **not interchangeable**, and that is the whole point of
/// distinguishing them: `Phase` holds an agent, `RootShell` is a bare `sh`. The
/// only attach primitive herdr exposes is `herdr agent attach`, which requires
/// an attached agent — there is no `herdr pane attach` — so the second variant
/// cannot be handed to the same code path as the first. See [`AttachPlan`].
#[derive(Debug)]
enum AttachTarget<'a> {
    Phase { phase: &'a str, pane: &'a str },
    RootShell { pane: &'a str },
}

/// What `drovr attach` will actually DO about the target it found.
///
/// Split out from [`attach_target`] so the decision is testable without
/// spawning `herdr` or calling `process::exit`. It exists because those two were
/// once one function, and the fallback rung it added ("no phase pane → the idle
/// root shell") silently contradicted the `herdr agent attach` it then ran: the
/// root shell has no agent, so that attach could only ever fail with
/// `agent_not_found`. Nothing walked that path in a test.
#[derive(Debug)]
enum AttachPlan {
    /// Hand the terminal to `herdr agent attach <pane>`.
    AttachAgent { phase: String, pane: String },
    /// Print this to stderr and exit non-zero. There is no agent to attach to,
    /// and saying so is more use than an opaque herdr error.
    Refuse(String),
}

/// Decide what `drovr attach <name>` does, given the run's state.
///
/// A run with no live agent pane is **refused**, not silently redirected. The
/// idle root shell is a real pane and `drovr cleanup` still owns it, but it is
/// not a conversation: attaching there would drop the human at a `sh` prompt
/// under a command whose entire contract is "show me this run's agent". The
/// refusal names the workspace so they can open it in herdr themselves if the
/// shell is genuinely what they wanted.
fn attach_plan(run: &RunState, name: &str) -> AttachPlan {
    match attach_target(run) {
        Some(AttachTarget::Phase { phase, pane }) => AttachPlan::AttachAgent {
            phase: phase.to_owned(),
            pane: pane.to_owned(),
        },
        // The workspace and its anchor shell are alive; only the agents are gone.
        Some(AttachTarget::RootShell { pane }) => AttachPlan::Refuse(format!(
            "run '{name}' has no live agent pane — no phase holds one. Its workspace \
             {} is still open, anchored by the idle shell {pane} (a plain shell, not \
             an agent, so there is nothing to attach to).{recover} Start a phase with: \
             drovr phase start {quoted} <phase>",
            run.workspace.as_deref().unwrap_or("(unknown)"),
            quoted = shell_single_quote(name),
            recover = rehydrate_hint(run, name),
        )),
        None => AttachPlan::Refuse(format!(
            "run '{name}' has no live agent pane, and no herdr workspace either \
             (creation failed at `drovr new`, or the run was cleaned up). Start a \
             phase with: drovr phase start {} <phase>",
            shell_single_quote(name),
        )),
    }
}

/// Where a line goes and what it costs: stdout+exit 0, or stderr+a code.
#[derive(Debug, PartialEq, Eq)]
struct Report {
    code: i32,
    to_stderr: bool,
    line: String,
}

/// How a [`RehydrateOutcome`] is reported — the DECISION, split from the
/// printing and the `process::exit` so a test can reach it. Task 4's handoff
/// §3d: a decision that is tested while what the caller does with it is not is
/// how two halves come to contradict each other undetected.
///
/// **`Incomplete` is exit 2, not 0.** The pane is back, but the agent in it was
/// not confirmed to have this phase's context — it never became ready (so a
/// resume was never confirmed, or a seed never sent), there was no seed
/// recorded, or the delivery failed. `phase send` already reserves
/// exit 2 for exactly that ("so the driver can escalate rather than assume the
/// seed landed"), and a driver that only checks the status would otherwise run
/// `phase wait` against an agent nobody ever told what to do.
fn rehydrate_report(run: &str, phase: &str, outcome: &RehydrateOutcome) -> Report {
    match outcome {
        RehydrateOutcome::Resumed => Report {
            code: 0,
            to_stderr: false,
            line: format!("phase '{phase}' resumed with its recorded session"),
        },
        RehydrateOutcome::Reseeded => Report {
            code: 0,
            to_stderr: false,
            line: format!(
                "phase '{phase}' relaunched — its session was not recoverable, so a fresh \
                 agent was seeded from the handoff"
            ),
        },
        // The prose comes from the VARIANT (`Unfinished::note`), never the
        // other way round — so a caller that needs to know which failure this
        // was matches on the type instead of on the sentence.
        RehydrateOutcome::Incomplete(why) => Report {
            code: 2,
            to_stderr: true,
            line: format!(
                "drovr: phase '{phase}' relaunched INCOMPLETE — {}",
                why.note(run, phase)
            ),
        },
    }
}

/// The " Or bring back …" clause for a refusal, when the run has a phase whose
/// pane drovr closed. Empty otherwise — a refusal must never advertise a
/// recovery that would just error.
///
/// The LAST reaped phase, because a run's phases are appended in order and the
/// most recent one is the one a human losing their pane is asking about.
/// Reviewers are excluded: they live in `review_phases` and are not what
/// `drovr attach <run>` is looking for.
fn rehydrate_hint(run: &RunState, name: &str) -> String {
    match run.phases.iter().rev().find(|p| p.is_reaped()) {
        Some(p) => format!(
            " Phase '{}' was closed by drovr and can be brought back (resuming its \
             session where the agent supports it): drovr phase rehydrate {} {}.",
            p.name,
            shell_single_quote(name),
            shell_single_quote(&p.name),
        ),
        None => String::new(),
    }
}

/// Pick what `drovr attach <run>` should connect to:
///
/// 1. [`RunState::live_agent_pane`] — the run's current phase, if it holds one.
///    **The same call the review UI's mirror makes**, so `drovr attach` and the
///    UI can never point at different panes for the same run. There is no
///    fallback to an earlier phase; the reason is on `live_agent_pane`.
/// 2. otherwise the workspace's idle root shell, which no phase ever occupies
///    and which therefore outlives them all;
/// 3. otherwise nothing — a run whose workspace creation failed at `drovr new`.
///
/// **Rung 2 is not an attach target** — see [`attach_plan`], which refuses it.
/// It is reported rather than dropped because it distinguishes two refusals: a
/// run whose workspace is still open and anchored, and one with no workspace at
/// all. Those deserve different advice.
fn attach_target(run: &RunState) -> Option<AttachTarget<'_>> {
    match run.live_agent_pane() {
        Some((phase, pane)) => Some(AttachTarget::Phase { phase, pane }),
        None => run
            .root_pane
            .as_deref()
            .map(|pane| AttachTarget::RootShell { pane }),
    }
}

fn cmd_attach(name: &str) {
    if let Err(e) = validate_run_name(name) {
        eprintln!("drovr: {e}");
        process::exit(1);
    }
    let run = load_run(name);

    let (phase, pane_id) = match attach_plan(&run, name) {
        AttachPlan::AttachAgent { phase, pane } => (phase, pane),
        AttachPlan::Refuse(msg) => {
            eprintln!("drovr: {msg}");
            process::exit(1);
        }
    };
    // Name the phase: `attach_target`'s rung 2 can land somewhere other than the
    // phase the human had in mind, and a silent attach hides which.
    eprintln!("drovr: attaching to phase '{phase}' of run '{name}'");

    // Shell out: herdr agent attach <pane_id>
    let status = std::process::Command::new("herdr")
        .args(["agent", "attach", &pane_id])
        .status()
        .unwrap_or_else(|e| {
            eprintln!("drovr: failed to exec herdr: {e}");
            process::exit(1);
        });
    if !status.success() {
        process::exit(status.code().unwrap_or(1));
    }
}

/// Carry what a code-review panel recorded onto a freshly loaded run state.
///
/// **Transplant, never write `state` back wholesale.** A panel runs for many
/// minutes and `state` is a snapshot from before it started; saving the whole
/// thing would resurrect every pipeline phase's status as of panel-start —
/// including a `Done` that a `phase send` re-entry has since cleared, which
/// makes the next `phase wait` report success for an agent that is mid-work.
///
/// `code_review_run` mutates exactly two things, and BOTH have to come across:
///
/// * `review_phases` — the reviewers it spawned and their status.
/// * `retired_panes` — load-bearing, not bookkeeping. `drovr cleanup` closes
///   exactly the panes this file records and treats everything else in the
///   workspace as the human's, so a pane dropped from `review_phases` without
///   landing here is immortal AND blocks `workspace_close` for the whole run.
///   The resume path retires a replaced reviewer's pane immediately before
///   `spawn_reviewer`, which can then fail — leaving that retirement in memory
///   only. This used to drop it on exactly that path.
///
/// `retired_panes` is UNIONED, not assigned: the freshly loaded state may
/// already record retirements this snapshot never saw, and `retire_pane` is
/// idempotent, so neither side loses.
fn merge_panel_progress(merged: &mut RunState, state: &RunState) {
    merged.review_phases = state.review_phases.clone();
    for pane in &state.retired_panes {
        merged.retire_pane(pane.clone());
    }
}

/// Every pane id drovr created for `run`, in creation order and deduped: the
/// workspace's idle root pane, each phase's pane, each reviewer's pane, and
/// every pane retired from a phase but still drovr's (see
/// `RunState::retired_panes`).
///
/// `root_pane` is now an unconditional entry — no phase ever claims that id, so
/// it stays in exactly one place for the run's lifetime. (It used to move onto
/// the first phase, which is why the dedup below exists and why a `state.json`
/// written by an older build can still list it in both places.)
///
/// This list is the whole definition of "drovr's panes" at cleanup: a pane drovr
/// created but failed to record is indistinguishable from one the human opened,
/// and is therefore left alone.
fn drovr_pane_ids(run: &RunState) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let ids = run
        .root_pane
        .iter()
        .map(String::as_str)
        .chain(run.phases.iter().filter_map(|p| p.pane_id()))
        .chain(run.review_phases.iter().filter_map(|p| p.pane_id()))
        .chain(run.retired_panes.iter().map(String::as_str));
    for id in ids {
        if !out.iter().any(|seen| seen == id) {
            out.push(id.to_owned());
        }
    }
    out
}

/// Tear down the panes drovr created for `run`, and *only* those.
///
/// drovr creates the run's workspace at `drovr new`, but the human keeps working
/// in it — a shell to run the tests, an editor, their own agent in a spare tab.
/// So `workspace_close` is only correct once we have established that the
/// workspace holds nothing but drovr's own panes; otherwise it kills the human's
/// work as collateral. When anything foreign is present (or when we cannot tell
/// what is present), close the recorded panes one at a time and leave the
/// workspace standing. Leaving an empty workspace behind is a cosmetic mistake;
/// closing a pane that was not ours is not a recoverable one.
fn close_run_panes<H: Herdr>(run: &RunState, ws_id: &str, herdr: &H) {
    let ours = drovr_pane_ids(run);
    let targets = match herdr.workspace_panes(ws_id) {
        Ok(present) => {
            if present.iter().all(|p| ours.contains(p)) {
                // Nothing in there but ours: one call reaps every pane AND the
                // workspace, so no empty husk is left in the human's switcher.
                if let Err(e) = herdr.workspace_close(ws_id) {
                    eprintln!("drovr: warning: workspace_close({ws_id}) failed: {e}");
                }
                return;
            }
            let foreign: Vec<&String> = present.iter().filter(|p| !ours.contains(p)).collect();
            println!(
                "drovr: keeping workspace {ws_id} — {} pane(s) drovr did not create are still open ({})",
                foreign.len(),
                foreign
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            // Only panes still listed: one drovr recorded but that has since been
            // closed would just make `pane.close` fail and print a warning about
            // a pane the human already dealt with.
            ours.iter()
                .filter(|p| present.contains(p))
                .cloned()
                .collect()
        }
        Err(e) => {
            // "Cannot tell" — a daemon blip, a changed result shape — is not
            // "the workspace is all mine". Fall back to closing the recorded
            // panes, skipping the ones herdr can prove are already gone.
            eprintln!(
                "drovr: warning: could not list panes in workspace {ws_id}: {e}; \
                 closing only the panes drovr created"
            );
            ours.iter()
                .filter(|p| herdr.pane_exists(p))
                .cloned()
                .collect::<Vec<String>>()
        }
    };

    for pane in &targets {
        if let Err(e) = herdr.pane_close(pane) {
            eprintln!("drovr: warning: pane_close({pane}) failed: {e}");
        }
    }
}

// Generic over `Herdr` (rather than taking `&SystemHerdr`) so the archived-marking
// path can be driven with `FakeHerdr` in a test — it was untested while it was
// the one thing standing between a torn-down run and a permanently live-looking
// session row.
fn cmd_cleanup<H: Herdr>(name: &str, purge: bool, herdr: &H) {
    if let Err(e) = validate_run_name(name) {
        eprintln!("drovr: {e}");
        process::exit(1);
    }
    let run = load_run(name);

    // Reap the run's panes. Older runs without a recorded workspace id are
    // skipped gracefully.
    if let Some(ws_id) = &run.workspace {
        close_run_panes(&run, ws_id, herdr);
    }

    // Mark it archived HERE — immediately after the panes die, before any git
    // work. `archived` means "the workspace is torn down", and that becomes true
    // on the line above, so this is the moment it is honest.
    //
    // Not at the end of the function: the worktree prune below can `exit(1)` on a
    // dirty tree or a failed squash-commit, and every one of those paths leaves a
    // run whose panes are already gone. Marking archived last would let exactly
    // those runs keep displaying as live sessions — the stale-status bug this
    // field exists to kill. A run needing a second `drovr cleanup` is listed as
    // finished-but-still-on-disk, which is what it is; the prune error and the
    // kept-branch hint still print.
    //
    // Re-read rather than reusing the copy loaded above: `save()` rewrites the
    // whole file, and a phase agent may have written its own status between that
    // load and now. Re-reading shrinks the clobber window to this one call.
    if !purge {
        let mut latest = RunState::load(name).unwrap_or_else(|_| run.clone());
        latest.archived = true;
        if let Err(e) = latest.save() {
            eprintln!("drovr: warning: could not mark run '{name}' archived: {e}");
        }
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
                println!(
                    "drovr: kept branch {b} — merge it with: git merge {}",
                    shell_single_quote(b)
                );
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
                        println!(
                            "drovr: kept branch {b} — merge it with: git merge {}",
                            shell_single_quote(b)
                        );
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

/// `drovr resurrect`, minus the process plumbing: RESTORE the run — which means
/// making its herdr workspace real again — and then report where to resume.
///
/// The order is the whole point. This command's help says it "reloads a stopped
/// run and reports the resume point", and it used to print
/// `To resume: drovr phase start …` having restored nothing, so the resume it
/// advertised died on the next command with a raw `workspace_not_found`. A
/// recovery command that reports success it did not achieve is worse than one
/// that errors: it sends you looking for the fault somewhere else entirely.
/// Either the workspace is there when this returns `Ok`, or this returns `Err`.
fn resurrect_report<H: Herdr>(h: &H, run: &mut RunState) -> io::Result<String> {
    let Some(idx) = run.first_incomplete() else {
        // Nothing to resume into, so nothing to provision: a finished run does
        // not need a workspace conjured up to be told it is finished.
        return Ok(format!(
            "run '{}' is fully complete — nothing to resurrect",
            run.name
        ));
    };

    let mut out = String::new();
    if let phase::WorkspaceHealing::Reprovisioned { orphaned } = phase::ensure_workspace(h, run)? {
        // Shared wording, so a driver reading this and the stderr warning
        // `phase_start` prints does not have to work out that they are the same
        // event described twice.
        out.push_str("restored: ");
        out.push_str(&phase::healing_report(run, &orphaned));
        out.push('\n');
    }

    // Re-read the resume point: the repair above may have demoted the phase that
    // was `Running` to `Failed`, which does not move `first_incomplete` — but
    // reading it again keeps this honest if that ever changes.
    let idx = run.first_incomplete().unwrap_or(idx);
    out.push_str(&format!(
        "run '{}' — resume at phase {idx}: {}\n",
        run.name, run.phases[idx].name
    ));
    // Print all phases for context
    for (i, p) in run.phases.iter().enumerate() {
        out.push_str(&format!(
            "  [{i}] {} — {}\n",
            p.name,
            phase_status_str(&p.status)
        ));
    }
    out.push('\n');
    out.push_str(&format!(
        "To resume: drovr phase start {} {}",
        shell_single_quote(&run.name),
        shell_single_quote(&run.phases[idx].name)
    ));
    Ok(out)
}

fn cmd_resurrect<H: Herdr>(h: &H, name: &str) {
    if let Err(e) = validate_run_name(name) {
        eprintln!("drovr: {e}");
        process::exit(1);
    }
    let mut run = load_run(name);
    match resurrect_report(h, &mut run) {
        Ok(report) => println!("{report}"),
        Err(e) => {
            eprintln!("drovr: cannot resurrect run '{name}': {e}");
            process::exit(1);
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
        // A refused duplicate already reads as a full sentence naming the live
        // server; "serve failed:" in front of it only buries the point.
        if e.kind() == io::ErrorKind::AddrInUse {
            eprintln!("drovr: {e}");
        } else {
            eprintln!("drovr: serve failed: {e}");
        }
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
            context,
            context_file,
            no_brief,
        } => {
            if let Err(e) = validate_run_name(&run) {
                eprintln!("drovr: {e}");
                process::exit(1);
            }
            let context = read_context_arg(context, context_file);
            let mut state = load_run(&run);

            // Compose BEFORE spawning. A phase whose brief cannot be composed (no
            // template for that name) must not end up as a live agent sitting at an
            // empty composer waiting for a driver to improvise one — that is the
            // failure mode this whole mechanism exists to remove.
            let composed = if no_brief {
                None
            } else {
                match compose_phase_brief(&state, &phase_name, context.as_deref()) {
                    Ok(brief) => Some(brief),
                    Err(e) => {
                        eprintln!("drovr: {e}");
                        eprintln!(
                            "drovr: (or spawn it unbriefed with `drovr phase start {run} \
                             {phase_name} --no-brief`)"
                        );
                        process::exit(1);
                    }
                }
            };

            if let Err(e) = phase_start(&h, &mut state, &phase_name, seed.as_deref()) {
                eprintln!("drovr: phase start failed: {e}");
                process::exit(1);
            }
            println!("started phase '{phase_name}' for run '{run}'");

            if let Some(brief) = composed {
                if let Err(e) = phase_send(&h, &mut state, &phase_name, &brief) {
                    // The pane is up but unbriefed. Say so precisely: the phase is
                    // NOT running its task, and a driver that reads "started" alone
                    // would wait forever on an agent that was never asked anything.
                    if e.kind() == io::ErrorKind::TimedOut {
                        if let Some(diag) = diagnose_stuck_phase(&h, &state, &phase_name) {
                            eprintln!("drovr: {diag}");
                        } else {
                            eprintln!("drovr: {e}");
                        }
                    } else {
                        eprintln!("drovr: could not deliver the brief: {e}");
                    }
                    // Mark it Failed, exactly as the reviewer path does for the same
                    // condition. Left `Running`, a `phase wait` blocks forever on an
                    // agent that was never asked anything, and a re-entry believes the
                    // phase is live. The pane stays up (never closed mid-run) and is
                    // still recorded, so `drovr cleanup` reclaims it.
                    if let Some(i) = state.phases.iter().position(|p| p.name == phase_name) {
                        state.phases[i].status = run::PhaseStatus::Failed;
                        if let Err(e) = state.save() {
                            // The phase is still `Running` ON DISK, so a later
                            // `phase wait` would block forever on an agent that was never
                            // briefed. That is a worse state than the send failure itself,
                            // and it is not remediable by re-sending — exit 1, not the
                            // exit 2 that means "re-brief it".
                            eprintln!(
                                "drovr: could not record the failed phase ({e}) — phase \
                                 '{phase_name}' is still Running on disk with an UNBRIEFED \
                                 agent; fix the run dir before anything waits on it"
                            );
                            process::exit(1);
                        }
                    }
                    // The brief's context is recorded, so the remediation needs no
                    // --context: `phase brief` reuses it.
                    eprintln!(
                        "drovr: phase '{phase_name}' marked FAILED — its pane is alive but the \
                         agent was never briefed. Re-send with `drovr phase brief {run} \
                         {phase_name} | drovr phase send {run} {phase_name} -` once the pane is \
                         at its composer"
                    );
                    process::exit(2);
                }
                println!("briefed phase '{phase_name}' ({} bytes)", brief.len());
            }
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
            // `-` reads stdin, so a brief drovr composed can be piped straight in
            // without a driver retyping (or paraphrasing) it.
            let text = if text == "-" {
                // Bounded: a prompt is a message, and an accidental `cat huge.bin |` would
                // otherwise be read wholly into memory and then typed into a pane.
                //
                // Deliberately LARGER than MAX_CONTEXT: the canonical use of `send -` is
                // `drovr phase brief … | drovr phase send … -`, and a brief is the template
                // frame PLUS up to MAX_CONTEXT of context. Reusing the context cap here
                // would let a legitimate at-limit brief be rejected by the very remediation
                // drovr prints.
                const MAX_STDIN: u64 = 4 << 20; // 4 MiB
                let mut buf = String::new();
                if let Err(e) =
                    io::Read::read_to_string(&mut io::Read::take(io::stdin(), MAX_STDIN), &mut buf)
                {
                    eprintln!("drovr: cannot read the message from stdin: {e}");
                    process::exit(1);
                }
                if buf.trim().is_empty() {
                    eprintln!("drovr: refusing to send an empty message read from stdin");
                    process::exit(1);
                }
                if buf.len() as u64 == MAX_STDIN {
                    eprintln!(
                        "drovr: refusing to send {MAX_STDIN} bytes from stdin — that is not a \
                         brief; check what you piped in"
                    );
                    process::exit(1);
                }
                buf
            } else {
                text
            };
            if let Err(e) = phase_send(&h, &mut state, &phase_name, &text) {
                // A readiness timeout (agent never attached) is not a plain send
                // failure — the agent is almost certainly parked on a prompt with
                // no human at the pane. Raise it to the driver with the same
                // actionable, pane-quoting diagnostic the wait-timeout path uses,
                // and a distinct exit code (2) so the driver can escalate rather
                // than assume the seed landed.
                if e.kind() == io::ErrorKind::TimedOut {
                    // ALWAYS print the error itself. It names WHICH of the
                    // failures happened — never attached, seed swallowed, would
                    // not submit — and, for the swallowed one, why drovr refused
                    // to press a key on the user's behalf. `diagnose_stuck_phase`
                    // is additive context, a pane snapshot, not a replacement
                    // diagnosis: it used to REPLACE this text, which both lost the
                    // explanation and asserted the wrong cause, since it phrases
                    // every verdict as a `phase wait` timeout.
                    eprintln!("drovr: {e}");
                    if let Some(diag) = diagnose_stuck_phase(&h, &state, &phase_name) {
                        eprintln!("drovr: pane context — {diag}");
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
                    //
                    // stdout, like the benign timeout line above it: nothing is
                    // wrong with the PHASE. `phase_wait` has already explained on
                    // stderr which pass went away, so this line stays the action.
                    println!(
                        "phase '{phase_name}' was superseded by a newer pass — re-run the wait \
                         to follow the live one"
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
        PhaseCmd::Rehydrate { run, phase_name } => {
            if let Err(e) = validate_run_name(&run) {
                eprintln!("drovr: {e}");
                process::exit(1);
            }
            let mut state = load_run(&run);
            match phase_rehydrate(&h, &mut state, &phase_name) {
                // The decision lives in `rehydrate_report`; this is only the
                // doing. See there for why an incomplete rehydrate exits 2.
                Ok(outcome) => {
                    let r = rehydrate_report(&run, &phase_name, &outcome);
                    if r.to_stderr {
                        eprintln!("{}", r.line);
                    } else {
                        println!("{}", r.line);
                    }
                    if r.code != 0 {
                        process::exit(r.code);
                    }
                }
                Err(e) => {
                    eprintln!("drovr: phase rehydrate failed: {e}");
                    process::exit(1);
                }
            }
        }
        PhaseCmd::Brief {
            run,
            phase_name,
            context,
            context_file,
        } => {
            if let Err(e) = validate_run_name(&run) {
                eprintln!("drovr: {e}");
                process::exit(1);
            }
            let context = read_context_arg(context, context_file);
            let state = load_run(&run);
            match compose_phase_brief(&state, &phase_name, context.as_deref()) {
                Ok(brief) => print!("{brief}"),
                Err(e) => {
                    eprintln!("drovr: {e}");
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
                        "  watch: drovr review wait {}   # run this BACKGROUNDED, then end your turn",
                        shell_single_quote(&run)
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

/// Resolve `--context` / `--context-file` into the context text. clap enforces that
/// the two are mutually exclusive, so this only has to read the file. An unreadable
/// `--context-file` EXITS rather than proceeding contextless: the driver asked for that
/// context to be in the brief, and silently reviewing without it is the failure this
/// whole mechanism exists to prevent.
/// Read `--context-file`: a regular file, not through a symlink, size-bounded.
///
/// The path is often inside the run dir, which agents write to (handoffs live there), so it
/// gets the same treatment as a recorded context: a symlink there would inject an arbitrary
/// readable file into the brief, a FIFO would hang the driver on `read_to_string`, and a
/// metadata-only size check is a TOCTOU the read itself has to enforce.
fn read_context_file(path: &std::path::Path) -> String {
    use brief::MAX_CONTEXT;
    let bail = |msg: String| -> ! {
        eprintln!("drovr: {msg}");
        process::exit(1);
    };
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) => bail(format!(
            "cannot read --context-file {}: {e}",
            path.display()
        )),
    };
    if meta.file_type().is_symlink() {
        bail(format!(
            "--context-file {} is a symlink; pass the real path",
            path.display()
        ));
    }
    if !meta.file_type().is_file() {
        bail(format!(
            "--context-file {} is not a regular file",
            path.display()
        ));
    }
    if meta.len() > MAX_CONTEXT {
        bail(format!(
            "--context-file {} is {} bytes, over the {MAX_CONTEXT}-byte limit — that is not a \
             context; check what you passed",
            path.display(),
            meta.len()
        ));
    }
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) => bail(format!(
            "cannot read --context-file {}: {e}",
            path.display()
        )),
    };
    let mut text = String::new();
    if let Err(e) = io::Read::read_to_string(&mut io::Read::take(file, MAX_CONTEXT + 1), &mut text)
    {
        bail(format!(
            "cannot read --context-file {}: {e}",
            path.display()
        ));
    }
    if text.len() as u64 > MAX_CONTEXT {
        bail(format!(
            "--context-file {} grew past the {MAX_CONTEXT}-byte limit while being read",
            path.display()
        ));
    }
    text
}

fn read_context_arg(context: Option<String>, context_file: Option<PathBuf>) -> Option<String> {
    // One cap for every path context can arrive by, defined next to the record I/O that
    // enforces it on reuse.
    use brief::MAX_CONTEXT;
    match (context, context_file) {
        (Some(text), _) if text.len() as u64 > MAX_CONTEXT => {
            eprintln!(
                "drovr: --context is {} bytes, over the {MAX_CONTEXT}-byte limit — that is not \
                 a context; check what you passed",
                text.len()
            );
            process::exit(1);
        }
        (Some(text), _) => Some(text),
        (None, Some(path)) => Some(read_context_file(&path)),
        (None, None) => None,
    }
}

/// `drovr handoff-scaffold` — write the empty 7-section handoff for `phase`.
///
/// Never clobbers silently: a handoff already on disk is an agent's authored work (or a
/// half-written draft), and losing it costs the whole compression pass that produced it.
fn cmd_handoff_scaffold(run: &str, phase_name: &str, force: bool) {
    if let Err(e) = validate_run_name(run) {
        eprintln!("drovr: {e}");
        process::exit(1);
    }
    if let Err(e) = validate_label("phase", phase_name) {
        eprintln!("drovr: {e}");
        process::exit(1);
    }
    let dir = run::run_dir(run);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("drovr: cannot create run dir: {e}");
        process::exit(1);
    }
    let path = dir.join(format!("{phase_name}-HANDOFF.md"));
    if path.exists() && !force {
        eprintln!(
            "drovr: {} already exists — refusing to overwrite an authored handoff (pass \
             --force to replace it)",
            path.display()
        );
        process::exit(1);
    }
    if let Err(e) = std::fs::write(&path, brief::handoff_scaffold()) {
        eprintln!("drovr: cannot write {}: {e}", path.display());
        process::exit(1);
    }
    println!("scaffolded {}", path.display());
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
                eprintln!("drovr: {}", phase::missing_project_dir_error(&run));
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
        CodeReviewCmd::Brief {
            run,
            task,
            angle,
            context,
            context_file,
        } => {
            if let Err(e) = validate_run_name(&run) {
                eprintln!("drovr: {e}");
                process::exit(1);
            }
            if let Err(e) = validate_label("task", &task) {
                eprintln!("drovr: {e}");
                process::exit(1);
            }
            if let Err(e) = validate_label("angle", &angle) {
                eprintln!("drovr: {e}");
                process::exit(1);
            }
            let context = read_context_arg(context, context_file);
            let state = load_run(&run);
            match code_review_brief(&state, &task, &angle, context.as_deref()) {
                Ok(brief) => print!("{brief}"),
                Err(e) => {
                    eprintln!("drovr: cannot compose brief: {e}");
                    process::exit(1);
                }
            }
        }
        CodeReviewCmd::Run {
            run,
            task,
            timeout_ms,
            fresh,
            context,
            context_file,
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
            let context = read_context_arg(context, context_file);
            let outcome =
                code_review_run(&h, &mut state, &task, timeout_ms, fresh, context.as_deref());
            // Persist what the panel recorded, on EVERY path including the
            // `Err` early-exit below — `code_review_run` mutates state in memory
            // and can then fail with none of it saved.
            let mut merged = load_run(&run);
            merge_panel_progress(&mut merged, &state);
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
                // Exit 1 with `Error`: both are "stop and fix the setup", and the
                // pipeline's failure model already routes 1 to STOP-and-diagnose. The
                // variants stay distinct in the type so a caller reading the outcome
                // (rather than the exit code) can tell an empty range from a broken one;
                // the specific diagnosis is already on stderr.
                ReviewOutcome::EmptyRange => {
                    eprintln!("code-review: nothing to review for '{task}' (see message above)");
                    process::exit(1);
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
        Commands::Resurrect { name } => cmd_resurrect(&herdr, &name),
        Commands::Serve { host, port } => cmd_serve(host, port),
        Commands::Phase { sub } => cmd_phase(sub),
        Commands::Collect { run, phase_name } => cmd_collect(&run, &phase_name),
        Commands::HandoffScaffold {
            run,
            phase_name,
            force,
        } => cmd_handoff_scaffold(&run, &phase_name, force),
        Commands::Review { sub } => cmd_review(sub),
        Commands::CodeReview { sub } => cmd_code_review(sub),
        Commands::Reflex { skill } => cmd_reflex(&skill),
        Commands::McpFindings { run, task, iter } => {
            // Both reach the filesystem as path components. The panel always supplies
            // names it has already validated, but this is a write-capable entrypoint
            // and nothing stops it being invoked directly — so it validates its own
            // inputs rather than trusting its only intended caller.
            for (kind, value) in [("run name", &run), ("task", &task)] {
                if let Err(e) = validate_label(kind, value) {
                    eprintln!("drovr: mcp-findings: {e}");
                    process::exit(1);
                }
            }
            let angles = match config::load_config() {
                Ok(c) => c.angles,
                Err(e) => {
                    eprintln!("drovr: mcp-findings could not load config: {e}");
                    process::exit(1);
                }
            };
            if let Err(e) = mcp_findings::serve(&run_dir(&run), &task, iter, &angles) {
                eprintln!("drovr: mcp-findings failed: {e}");
                process::exit(1);
            }
        }
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

    // -- cleanup ----------------------------------------------------------------

    /// Point `XDG_DATA_HOME` at a scratch dir unique to this test, and return it
    /// so the caller can remove it. Callers must hold `ENV_LOCK`.
    #[cfg(test)]
    fn cleanup_scratch(tag: &str) -> std::path::PathBuf {
        let tmp = std::env::temp_dir().join(format!("drovr-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        unsafe {
            std::env::set_var("XDG_DATA_HOME", &tmp);
        }
        tmp
    }

    #[test]
    fn the_panel_merge_carries_retired_panes_not_just_review_phases() {
        // `retired_panes` is what tells `drovr cleanup` a pane is drovr's. Since
        // main's `8173f03`, cleanup closes only panes it can prove it opened and
        // leaves everything else standing as the human's — so a pane that is
        // dropped from `review_phases` but never lands in `retired_panes` is
        // immortal, and blocks `workspace_close` for the whole run.
        //
        // The resume path retires a replaced reviewer's pane immediately before
        // `spawn_reviewer`, which can then fail — so the retirement exists only
        // in the panel's in-memory copy when this merge runs. Transplanting just
        // `review_phases` dropped it on exactly that path.
        let mut on_disk = RunState {
            name: "r".into(),
            task: "t".into(),
            agent: None,
            phases: vec![run::Phase::new("plan")],
            review_phases: vec![],
            gate: "spec".into(),
            cursor: 0,
            workspace: None,
            root_pane: None,
            project_dir: "/tmp/p".into(),
            worktree_path: None,
            worktree_branch: None,
            archived: false,
            retired_panes: vec!["w:earlier".into()],
        };
        // What the panel held when it failed: a respawned reviewer registered,
        // the replaced one's pane retired — neither of them saved.
        let mut panel = on_disk.clone();
        panel.review_phases = vec![run::Phase::new("review:task-1:1:correctness")];
        panel.retire_pane("w:replaced");

        merge_panel_progress(&mut on_disk, &panel);

        assert_eq!(on_disk.review_phases.len(), 1, "reviewers come across");
        assert!(
            on_disk.retired_panes.contains(&"w:replaced".to_string()),
            "and so does the retirement, or that pane is immortal: {:?}",
            on_disk.retired_panes
        );
        assert!(
            on_disk.retired_panes.contains(&"w:earlier".to_string()),
            "unioned, not assigned — a retirement the panel never saw must survive"
        );
        // Pipeline phases are deliberately NOT transplanted: the panel's snapshot
        // is minutes old and would resurrect a status a re-entry has since cleared.
        assert_eq!(on_disk.phases.len(), 1);
    }

    /// A saved run in workspace `wAC` whose recorded drovr panes are `wAC:p1`
    /// (the brainstorm phase, mid-flight) and `wAC:p2` (a reviewer). No worktree,
    /// so `cmd_cleanup` runs to completion instead of exiting on the prune path.
    fn seed_paned_run(name: &str) -> RunState {
        let run = RunState {
            name: name.into(),
            task: "t".into(),
            agent: None,
            phases: vec![
                {
                    let mut p = run::Phase::new("brainstorm");
                    p.status = PhaseStatus::Running;
                    p
                }
                .with_pane("wAC:p1"),
            ],
            review_phases: vec![
                {
                    let mut p = run::Phase::new("review:brainstorm:1:correctness");
                    p.status = PhaseStatus::Running;
                    p
                }
                .with_pane("wAC:p2"),
            ],
            gate: "spec".into(),
            cursor: 0,
            workspace: Some("wAC".into()),
            root_pane: None,
            project_dir: "/tmp/p".into(),
            worktree_path: None,
            worktree_branch: None,
            archived: false,
            retired_panes: vec![],
        };
        run.save().expect("seed run");
        run
    }

    // -- resurrect --------------------------------------------------------------

    /// A run whose workspace is gone, mid-flight at `implement`.
    #[cfg(test)]
    fn seed_workspaceless_run(name: &str) -> RunState {
        let run = RunState {
            name: name.into(),
            task: "t".into(),
            agent: None,
            phases: vec![
                {
                    let mut p = run::Phase::new("brainstorm");
                    p.status = PhaseStatus::Done;
                    p
                },
                {
                    let mut p = run::Phase::new("implement");
                    p.status = PhaseStatus::Running;
                    p
                }
                .with_pane("wAG:p1"),
            ],
            review_phases: vec![],
            gate: "spec".into(),
            cursor: 0,
            workspace: Some("wAG".into()),
            root_pane: None,
            project_dir: "/tmp/p".into(),
            worktree_path: None,
            worktree_branch: None,
            archived: false,
            retired_panes: vec![],
        };
        run.save().expect("seed run");
        run
    }

    /// `resurrect`'s help says it "reloads a stopped run and reports the resume
    /// point". It used to print `To resume: drovr phase start …` having restored
    /// nothing at all, so the resume it advertised failed on the next command —
    /// worse than an error, because it reads as success.
    #[test]
    fn resurrect_restores_the_workspace_it_advertises_a_resume_into() {
        use crate::herdr::FakeHerdr;
        use crate::test_util::ENV_LOCK;

        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = cleanup_scratch("resurrect-restores");
        let mut run = seed_workspaceless_run("lost-ws");
        let fake = FakeHerdr::new();
        fake.kill_workspace("wAG", ["wAG:p1".to_string()]);

        let report = resurrect_report(&fake, &mut run).expect("resurrect must restore or refuse");

        assert!(
            fake.calls().iter().any(|c| c.contains("workspace_create")),
            "resurrect must actually restore the workspace: {:?}",
            fake.calls()
        );
        assert_ne!(
            RunState::load("lost-ws").unwrap().workspace.as_deref(),
            Some("wAG"),
            "and persist the new one, or the next command hits the same dead id"
        );
        assert!(
            report.contains("To resume: drovr phase start"),
            "having restored it, it may say how to resume: {report}"
        );
        assert!(
            report.contains("implement"),
            "the phase whose agent died is where you resume: {report}"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// The other half of "restore what you advertise, or refuse and say why".
    #[test]
    fn resurrect_that_cannot_restore_refuses_instead_of_advertising_a_resume() {
        use crate::herdr::FakeHerdr;
        use crate::test_util::ENV_LOCK;

        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = cleanup_scratch("resurrect-refuses");
        let mut run = seed_workspaceless_run("unfixable-ws");
        let fake = FakeHerdr::new();
        fake.kill_workspace("wAG", ["wAG:p1".to_string()]);
        fake.fail_workspace_create();

        let err = resurrect_report(&fake, &mut run)
            .expect_err("a resume it cannot deliver must be an error, not a printout");
        assert!(
            !err.to_string().contains("To resume"),
            "it must not smuggle the resume instruction into the failure: {err}"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// A finished run needs no workspace, and resurrect must not create one just
    /// to tell you there is nothing to do.
    #[test]
    fn resurrect_of_a_finished_run_provisions_nothing() {
        use crate::herdr::FakeHerdr;
        use crate::test_util::ENV_LOCK;

        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = cleanup_scratch("resurrect-complete");
        let mut run = seed_workspaceless_run("done-ws");
        for p in run.phases.iter_mut() {
            p.status = PhaseStatus::Done;
        }
        run.save().unwrap();
        let fake = FakeHerdr::new();
        fake.kill_workspace("wAG", ["wAG:p1".to_string()]);

        let report = resurrect_report(&fake, &mut run).unwrap();

        assert!(report.contains("fully complete"), "{report}");
        assert!(
            !fake.calls().iter().any(|c| c.contains("workspace_create")),
            "no workspace is needed to say a run is finished: {:?}",
            fake.calls()
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// The workspace drovr created for a run is still the human's to use — they
    /// open their own shell or editor tab in it while the run works. Cleanup must
    /// reap only the panes drovr created and leave that workspace standing;
    /// `workspace_close` would take the human's panes down with it.
    #[test]
    fn cleanup_keeps_a_workspace_holding_panes_drovr_did_not_create() {
        use crate::herdr::FakeHerdr;
        use crate::test_util::ENV_LOCK;

        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = cleanup_scratch("cleanup-foreign");
        seed_paned_run("keep-ws");

        let fake = FakeHerdr::new();
        // The human's own pane (wAC:p9) sits alongside drovr's two.
        fake.push_workspace_panes("wAC", ["wAC:p1", "wAC:p2", "wAC:p9"]);
        cmd_cleanup("keep-ws", false, &fake);

        let calls = fake.calls();
        assert!(
            !calls.iter().any(|c| c.contains("workspace_close")),
            "must not close a workspace holding the human's panes: {calls:?}"
        );
        assert!(
            calls.iter().any(|c| c == "pane_close pane=wAC:p1"),
            "the phase pane must be closed: {calls:?}"
        );
        assert!(
            calls.iter().any(|c| c == "pane_close pane=wAC:p2"),
            "the reviewer pane must be closed: {calls:?}"
        );
        assert!(
            !calls.iter().any(|c| c.contains("wAC:p9")),
            "a pane drovr did not create must be left alone: {calls:?}"
        );
        assert!(
            RunState::load("keep-ws").expect("run kept").archived,
            "the run is still archived when only its panes were reaped"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// When the workspace holds nothing but drovr's own panes, one
    /// `workspace_close` reaps them all and leaves no empty workspace behind.
    #[test]
    fn cleanup_closes_the_workspace_when_only_drovr_panes_remain() {
        use crate::herdr::FakeHerdr;
        use crate::test_util::ENV_LOCK;

        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = cleanup_scratch("cleanup-owned");
        seed_paned_run("close-ws");

        let fake = FakeHerdr::new();
        fake.push_workspace_panes("wAC", ["wAC:p1", "wAC:p2"]);
        cmd_cleanup("close-ws", false, &fake);

        let calls = fake.calls();
        assert!(
            calls.iter().any(|c| c.contains("workspace_close id=wAC")),
            "a workspace of only drovr panes must be closed outright: {calls:?}"
        );
        assert!(
            !calls.iter().any(|c| c.contains("pane_close")),
            "no per-pane close is needed once the workspace goes: {calls:?}"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// A failed pane listing means "cannot tell what is in there", never "it is
    /// all ours". Closing the workspace on that answer is how the human's panes
    /// get killed by a transient daemon blip, so fall back to closing only the
    /// panes drovr recorded — an empty workspace husk is the cheaper mistake.
    #[test]
    fn cleanup_closes_only_drovr_panes_when_the_listing_fails() {
        use crate::herdr::FakeHerdr;
        use crate::test_util::ENV_LOCK;

        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = cleanup_scratch("cleanup-blind");
        seed_paned_run("blind-ws");

        let fake = FakeHerdr::new();
        fake.fail_workspace_panes();
        cmd_cleanup("blind-ws", false, &fake);

        let calls = fake.calls();
        assert!(
            !calls.iter().any(|c| c.contains("workspace_close")),
            "must not close a workspace it could not inspect: {calls:?}"
        );
        assert!(
            calls.iter().any(|c| c == "pane_close pane=wAC:p1"),
            "the phase pane must still be closed: {calls:?}"
        );
        assert!(
            calls.iter().any(|c| c == "pane_close pane=wAC:p2"),
            "the reviewer pane must still be closed: {calls:?}"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// The blind fallback must still skip a pane herdr can prove is gone: closing
    /// it would only print a warning about a pane the human already dealt with.
    #[test]
    fn cleanup_blind_fallback_skips_panes_proven_gone() {
        use crate::herdr::FakeHerdr;
        use crate::test_util::ENV_LOCK;

        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = cleanup_scratch("cleanup-blind-dead");
        seed_paned_run("blind-dead-ws");

        let fake = FakeHerdr::new();
        fake.fail_workspace_panes();
        fake.kill_pane("wAC:p1");
        cmd_cleanup("blind-dead-ws", false, &fake);

        let calls = fake.calls();
        assert!(
            !calls.iter().any(|c| c == "pane_close pane=wAC:p1"),
            "a pane herdr reports gone must not be closed again: {calls:?}"
        );
        assert!(
            calls.iter().any(|c| c == "pane_close pane=wAC:p2"),
            "the surviving pane must still be closed: {calls:?}"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// A pane drovr recorded but that is already gone (the human closed the tab)
    /// must not be closed again — the failed `pane.close` would print a warning
    /// about a pane the human already dealt with.
    #[test]
    fn cleanup_skips_drovr_panes_that_are_already_gone() {
        use crate::herdr::FakeHerdr;
        use crate::test_util::ENV_LOCK;

        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = cleanup_scratch("cleanup-gone");
        seed_paned_run("gone-ws");

        let fake = FakeHerdr::new();
        // p1 is gone; p9 (the human's) keeps the workspace alive.
        fake.push_workspace_panes("wAC", ["wAC:p2", "wAC:p9"]);
        cmd_cleanup("gone-ws", false, &fake);

        let calls = fake.calls();
        assert!(
            !calls.iter().any(|c| c == "pane_close pane=wAC:p1"),
            "a pane that is no longer in the workspace must not be closed: {calls:?}"
        );
        assert!(
            calls.iter().any(|c| c == "pane_close pane=wAC:p2"),
            "the pane that is still there must be closed: {calls:?}"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// The set of panes drovr owns: `root_pane` (which no phase ever claims, so
    /// it is always drovr's to reclaim), every phase pane, and every reviewer
    /// pane.
    #[test]
    fn drovr_pane_ids_covers_phases_reviewers_and_the_root_shell() {
        let mut run = RunState {
            name: "r".into(),
            task: "t".into(),
            agent: None,
            phases: vec![
                {
                    let mut p = run::Phase::new("brainstorm");
                    p.status = PhaseStatus::Done;
                    p
                }
                .with_pane("w:p1"),
                {
                    let mut p = run::Phase::new("plan");
                    p.status = PhaseStatus::Pending;
                    p
                },
            ],
            review_phases: vec![
                {
                    let mut p = run::Phase::new("review:plan:1:correctness");
                    p.status = PhaseStatus::Running;
                    p
                }
                .with_pane("w:p2"),
            ],
            gate: "spec".into(),
            cursor: 0,
            workspace: Some("w".into()),
            root_pane: Some("w:root".into()),
            project_dir: "/tmp/p".into(),
            worktree_path: None,
            worktree_branch: None,
            archived: false,
            retired_panes: vec![],
        };
        assert_eq!(drovr_pane_ids(&run), vec!["w:root", "w:p1", "w:p2"]);

        // A retired pane — one drovr opened whose phase registration was replaced
        // (a respawned reviewer) — is still drovr's to reap.
        run.retired_panes = vec!["w:p0".into()];
        assert_eq!(drovr_pane_ids(&run), vec!["w:root", "w:p1", "w:p2", "w:p0"]);
        run.retired_panes = vec![];

        // A `state.json` written by an older build, where the first phase DID
        // claim the root pane: the id must still appear exactly once.
        run.root_pane = None;
        run.phases[1].set_pane("w:root");
        assert_eq!(drovr_pane_ids(&run), vec!["w:p1", "w:root", "w:p2"]);
    }

    /// A `RunState` carrying just the fields `attach_target` reads.
    fn attach_run(phases: Vec<(&str, PhaseStatus, Option<&str>)>, root: Option<&str>) -> RunState {
        RunState {
            name: "r".into(),
            task: "t".into(),
            agent: None,
            phases: phases
                .into_iter()
                .map(|(name, status, pane)| {
                    let mut p = run::Phase::new(name);
                    p.status = status;
                    if let Some(pane) = pane {
                        p.set_pane(pane);
                    }
                    p
                })
                .collect(),
            review_phases: vec![],
            gate: "spec".into(),
            cursor: 0,
            workspace: Some("w".into()),
            root_pane: root.map(str::to_owned),
            project_dir: "/tmp/p".into(),
            worktree_path: None,
            worktree_branch: None,
            archived: false,
            retired_panes: vec![],
        }
    }

    /// What `drovr attach` connects to, in preference order — and why the last
    /// two rungs exist: once phases are reaped, a finished run holds no phase
    /// pane at all, and exiting 1 on the human's "show me this run" is a worse
    /// answer than the workspace's idle shell.
    #[test]
    fn attach_prefers_the_current_phase_then_reports_the_root_shell() {
        use PhaseStatus::{Done, Running};

        // The phase actually being worked wins, even though a later one has a pane.
        let run = attach_run(
            vec![
                ("brainstorm", Done, Some("w:p1")),
                ("implement", Running, Some("w:p2")),
                ("review", PhaseStatus::Pending, Some("w:p3")),
            ],
            Some("w:root"),
        );
        assert!(matches!(
            attach_target(&run),
            Some(AttachTarget::Phase {
                phase: "implement",
                pane: "w:p2"
            })
        ));

        // All done → no current phase, so no phase target. It must NOT offer
        // the last pane it can find: under reaping those are the panes that get
        // closed, and a finished run's stale pane is not its current state.
        let run = attach_run(
            vec![
                ("brainstorm", Done, Some("w:p1")),
                ("implement", Done, Some("w:p2")),
            ],
            Some("w:root"),
        );
        assert!(matches!(
            attach_target(&run),
            Some(AttachTarget::RootShell { pane: "w:root" })
        ));

        // Same when the current phase simply has no pane yet: an EARLIER
        // phase's pane is not an answer to "attach me to this run".
        let run = attach_run(
            vec![
                ("brainstorm", Done, Some("w:p1")),
                ("implement", Running, None),
            ],
            Some("w:root"),
        );
        assert!(matches!(
            attach_target(&run),
            Some(AttachTarget::RootShell { pane: "w:root" })
        ));

        // No phase pane anywhere → the run's idle root shell, reported as such
        // so `attach_plan` can refuse it by name rather than attaching to it.
        let run = attach_run(vec![("brainstorm", Done, None)], Some("w:root"));
        assert!(matches!(
            attach_target(&run),
            Some(AttachTarget::RootShell { pane: "w:root" })
        ));

        // Nothing at all (a run whose workspace creation failed) → None.
        let run = attach_run(vec![("brainstorm", Done, None)], None);
        assert!(attach_target(&run).is_none());

        // No phases at all — the shape a caller that recovered from an
        // unreadable `state.json` holds, where `first_incomplete()` is vacuously
        // `None`. The root shell rung must still answer, and must not panic.
        let run = attach_run(vec![], Some("w:root"));
        assert!(matches!(
            attach_target(&run),
            Some(AttachTarget::RootShell { pane: "w:root" })
        ));
        let run = attach_run(vec![], None);
        assert!(attach_target(&run).is_none());
    }

    /// The root-shell rung must never end in `herdr agent attach`.
    ///
    /// This is the test whose absence let that ship: `attach_target` was unit
    /// tested, but nothing walked the decision `cmd_attach` makes *with* the
    /// target it gets back, so "fall back to the root shell" and "attach to an
    /// agent" could contradict each other undetected. `herdr agent attach`
    /// requires an attached agent and the root shell has none — there is no
    /// `herdr pane attach` to fall back to — so the honest answer is a refusal
    /// that says what is actually true.
    #[test]
    fn attach_refuses_the_root_shell_instead_of_attaching_to_a_nonexistent_agent() {
        use PhaseStatus::{Done, Running};

        // A live phase pane is the one case that really attaches.
        let run = attach_run(vec![("implement", Running, Some("w:p2"))], Some("w:root"));
        match attach_plan(&run, "r") {
            AttachPlan::AttachAgent { phase, pane } => {
                assert_eq!(phase, "implement");
                assert_eq!(pane, "w:p2");
            }
            AttachPlan::Refuse(msg) => panic!("a live phase pane must attach, got: {msg}"),
        }

        // No phase pane, but the workspace and its idle shell are still there.
        // Distinctive ids, so asserting they appear is not satisfied by the
        // surrounding prose — the fixture's default `"w"` is a substring of
        // "workspace" and would make the check vacuous.
        let mut run = attach_run(vec![("brainstorm", Done, None)], Some("ws-77:root"));
        run.workspace = Some("ws-77".into());
        let msg = match attach_plan(&run, "r") {
            AttachPlan::Refuse(msg) => msg,
            AttachPlan::AttachAgent { pane, .. } => {
                panic!("the root shell has no agent to attach to, but got pane {pane}")
            }
        };
        assert!(
            msg.contains("no live agent pane"),
            "must say plainly that there is no agent: {msg}"
        );
        assert!(
            msg.contains("ws-77") && !msg.contains("(unknown)"),
            "must name the REAL workspace so the user can go there themselves: {msg}"
        );
        assert!(
            msg.contains("ws-77:root"),
            "must name the idle shell it is refusing, not just describe one: {msg}"
        );
        assert!(
            msg.contains("drovr phase start"),
            "must say what to do next: {msg}"
        );

        // No workspace at all — a distinct situation, and a distinct message:
        // there is no idle shell to point the user at.
        let run = attach_run(vec![("brainstorm", Done, None)], None);
        let bare = match attach_plan(&run, "r") {
            AttachPlan::Refuse(msg) => msg,
            AttachPlan::AttachAgent { .. } => panic!("nothing exists to attach to"),
        };
        assert!(
            bare.contains("no live agent pane"),
            "must say plainly that there is no agent: {bare}"
        );
        assert_ne!(
            bare, msg,
            "a run with no workspace must not be described as if it had an idle shell"
        );

        // Nothing in this run was reaped, so neither refusal may advertise a
        // rehydrate: `phase_rehydrate` would refuse a phase that never had its
        // pane closed, and a refusal must not send the user at a second error.
        for m in [&msg, &bare] {
            assert!(
                !m.contains("rehydrate"),
                "nothing is reaped here, so nothing to bring back: {m}"
            );
        }
    }

    #[test]
    fn an_incomplete_rehydrate_never_reports_as_success() {
        use phase::RehydrateOutcome::*;
        // The failure class `docs/known-issues.md` keeps recording: a driver
        // reads an exit code as success and carries on. A rehydrate that
        // brought the pane back but never gave the agent its context is NOT
        // success — a `phase wait` after it would block on an agent nobody told
        // what to do — and `phase send` already reserves exit 2 for exactly
        // this. Assert the code AND the stream: a driver reads one, a human the
        // other.
        let done = rehydrate_report("r", "plan", &Resumed);
        assert_eq!(done.code, 0);
        assert!(!done.to_stderr);
        assert!(done.line.contains("resumed with its recorded session"), "{done:?}");

        let seeded = rehydrate_report("r", "plan", &Reseeded);
        assert_eq!(seeded.code, 0, "a reseeded agent DID get its context");
        assert!(!seeded.to_stderr);

        let partial = rehydrate_report(
            "r",
            "plan",
            &Incomplete(crate::phase::Unfinished::NeverReady {
                pane: "w:p1".into(),
                waited: std::time::Duration::from_secs(30),
                resuming: false,
                had_seed: true,
            }),
        );
        assert_eq!(
            partial.code, 2,
            "the pane is back but the agent was never told what it is doing: {partial:?}"
        );
        assert!(partial.to_stderr, "{partial:?}");
        assert!(partial.line.contains("INCOMPLETE"), "{partial:?}");
        assert!(partial.line.contains("Its seed was NOT re-sent"), "{partial:?}");
    }

    #[test]
    fn attach_offers_a_rehydrate_when_a_phase_was_reaped() {
        // The counterpart to the assertion above: once a pane HAS been closed,
        // the refusal has somewhere to send the user.
        use crate::run::PhaseStatus::Done;
        let mut run = attach_run(vec![("brainstorm", Done, None), ("plan", Done, None)], Some("ws-77:root"));
        run.workspace = Some("ws-77".into());
        // BOTH reaped, so "picks the last" is a real assertion rather than
        // "picks the only one" — the run has moved past brainstorm, and that is
        // the phase a human losing their pane is asking about.
        run.phases[0].set_pane("ws-77:p1");
        run.phases[0].mark_reaped();
        run.phases[1].set_pane("ws-77:p9");
        run.phases[1].mark_reaped();

        let msg = match attach_plan(&run, "r") {
            AttachPlan::Refuse(msg) => msg,
            AttachPlan::AttachAgent { pane, .. } => panic!("a reaped phase has no pane: {pane}"),
        };
        assert!(
            msg.contains("drovr phase rehydrate 'r' 'plan'"),
            "must name the run AND the phase, ready to paste: {msg}"
        );
        // The LAST reaped phase, not the first — both are reaped, and `plan` is
        // where the run got to.
        assert!(!msg.contains("'brainstorm'"), "{msg}");
        // It is an addition, not a replacement — starting a new phase is still
        // the answer for someone who does not want the old one back.
        assert!(msg.contains("drovr phase start"), "{msg}");
    }

    /// `drovr new` labels the workspace's root shell so the idle tab explains
    /// itself, and a failed rename is cosmetic — it must never cost the run its
    /// workspace.
    #[test]
    fn new_labels_the_idle_root_shell_and_survives_a_failed_rename() {
        use crate::herdr::FakeHerdr;

        let h = FakeHerdr::new();
        let ws = create_run_workspace(&h, "alpha", "/tmp/p").expect("workspace must be created");
        let root = ws.root_pane.clone();
        let calls = h.calls();
        let rename = calls
            .iter()
            .find(|c| c.contains("pane_rename"))
            .expect("the root shell must be renamed");
        assert!(
            rename.contains(&format!("pane={root}")),
            "the RUN's root pane is what gets renamed: {rename}"
        );
        assert!(
            rename.contains("alpha") && rename.to_lowercase().contains("idle"),
            "the label must name the run and say the shell is idle: {rename}"
        );

        // `pane_rename` has no `--no-focus` flag, and `workspace_create` is
        // called with `focus: false` precisely so `drovr new` never disturbs the
        // user. Renaming without capture/restore would undo that one call later,
        // yanking the user onto a brand-new idle workspace.
        let idx = |needle: &str| {
            calls
                .iter()
                .position(|c| c.contains(needle))
                .unwrap_or_else(|| panic!("missing {needle}: {calls:?}"))
        };
        assert!(
            idx("focused_workspace") < idx("pane_rename"),
            "focus must be captured before the rename: {calls:?}"
        );
        assert!(
            idx("pane_rename") < idx("workspace_focus id=ws-focused"),
            "focus must be restored after the rename: {calls:?}"
        );

        let h = FakeHerdr::new();
        h.fail_pane_rename();
        assert!(
            create_run_workspace(&h, "beta", "/tmp/p").is_some(),
            "a cosmetic rename failure must not discard the workspace"
        );

        // Same for a focus restore that fails: it is reported, not fatal. The
        // run must not lose the workspace it just created over where the user
        // happens to be looking.
        let h = FakeHerdr::new();
        h.fail_workspace_focus();
        assert!(
            create_run_workspace(&h, "gamma", "/tmp/p").is_some(),
            "a failed focus restore must not discard the workspace"
        );
    }

    /// `drovr cleanup` must leave the run marked archived. Without it the session
    /// list has no way to tell a torn-down run from a live one: the phase statuses
    /// stay frozen at their last write (`Running`, against a pane that no longer
    /// exists) and the review gate keeps whatever verdict slot it was parked in,
    /// so the row would advertise itself as an active session forever.
    #[test]
    fn cleanup_marks_the_run_archived() {
        use crate::herdr::FakeHerdr;
        use crate::test_util::ENV_LOCK;

        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = cleanup_scratch("cleanup-archived");

        // Cleaned up mid-brainstorm: the shape that used to strand a run on a
        // live-looking status. No worktree, so the prune path is a no-op and the
        // function runs to completion instead of exiting.
        let run = seed_paned_run("cleanup-me");
        assert!(
            !run.is_complete(),
            "precondition: not complete before cleanup"
        );

        let fake = FakeHerdr::new();
        cmd_cleanup("cleanup-me", false, &fake);

        assert!(
            fake.calls().iter().any(|c| c.contains("workspace_close")),
            "cleanup must close the workspace: {:?}",
            fake.calls()
        );
        let after = RunState::load("cleanup-me").expect("run dir is kept without --purge");
        assert!(after.archived, "cleanup must mark the run archived");
        assert!(
            after.is_complete(),
            "an archived run reads as complete even with its phases frozen at Running"
        );
        // The phase statuses are deliberately left alone — `archived` is the flag
        // that carries the meaning, and rewriting phase history would lose the
        // record of how far the run actually got.
        assert_eq!(after.phases[0].status, PhaseStatus::Running);

        let _ = std::fs::remove_dir_all(&tmp);
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
                        ..
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
                        fresh,
                        ..
                    },
            } => {
                assert_eq!(run, "myrun");
                assert_eq!(task, "task-1");
                // Generous default (30 min), matching `review wait`.
                assert_eq!(timeout_ms, 1_800_000);
                assert!(
                    !fresh,
                    "a plain re-run must default to resuming an in-flight panel"
                );
            }
            _ => panic!("wrong variant"),
        }
    }

    /// `--context` and `--context-file` are alternatives, not a pair. clap enforces
    /// it, so a driver cannot supply two different contexts and leave drovr guessing.
    /// A brief is composed by default now; `--no-brief` is the explicit opt-out, and
    /// `--context` is the only part of the brief a driver supplies.
    #[test]
    fn parse_phase_start_context_and_no_brief() {
        let cli = parse(&[
            "drovr",
            "phase",
            "start",
            "myrun",
            "implement-task-2",
            "--context",
            "task brief from plan.md",
        ])
        .unwrap();
        match cli.command {
            Commands::Phase {
                sub: PhaseCmd::Start {
                    context, no_brief, ..
                },
            } => {
                assert_eq!(context.as_deref(), Some("task brief from plan.md"));
                assert!(!no_brief, "briefing is the default");
            }
            _ => panic!("wrong variant"),
        }
        let cli = parse(&[
            "drovr",
            "phase",
            "start",
            "myrun",
            "verify-land",
            "--no-brief",
        ])
        .unwrap();
        match cli.command {
            Commands::Phase {
                sub: PhaseCmd::Start { no_brief, .. },
            } => assert!(no_brief),
            _ => panic!("wrong variant"),
        }
        assert!(
            parse(&[
                "drovr",
                "phase",
                "start",
                "myrun",
                "plan",
                "--context",
                "a",
                "--context-file",
                "/tmp/b",
            ])
            .is_err(),
            "--context with --context-file must be rejected"
        );
    }

    #[test]
    fn parse_phase_brief() {
        let cli = parse(&["drovr", "phase", "brief", "myrun", "plan"]).unwrap();
        match cli.command {
            Commands::Phase {
                sub: PhaseCmd::Brief {
                    run, phase_name, ..
                },
            } => {
                assert_eq!(run, "myrun");
                assert_eq!(phase_name, "plan");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn parse_code_review_context_args_are_mutually_exclusive() {
        let cli = parse(&[
            "drovr",
            "code-review",
            "run",
            "myrun",
            "task-1",
            "--context",
            "the retry loop is new",
        ])
        .unwrap();
        match cli.command {
            Commands::CodeReview {
                sub: CodeReviewCmd::Run { context, .. },
            } => assert_eq!(context.as_deref(), Some("the retry loop is new")),
            _ => panic!("wrong variant"),
        }
        assert!(
            parse(&[
                "drovr",
                "code-review",
                "run",
                "myrun",
                "task-1",
                "--context",
                "a",
                "--context-file",
                "/tmp/b",
            ])
            .is_err(),
            "--context with --context-file must be rejected, not silently resolved"
        );
    }

    #[test]
    fn parse_code_review_brief() {
        let cli = parse(&[
            "drovr",
            "code-review",
            "brief",
            "myrun",
            "task-1",
            "--angle",
            "security",
        ])
        .unwrap();
        match cli.command {
            Commands::CodeReview {
                sub:
                    CodeReviewCmd::Brief {
                        run, task, angle, ..
                    },
            } => {
                assert_eq!(run, "myrun");
                assert_eq!(task, "task-1");
                assert_eq!(angle, "security");
            }
            _ => panic!("wrong variant"),
        }
        assert!(
            parse(&["drovr", "code-review", "brief", "myrun", "task-1"]).is_err(),
            "an angle-less brief has no frame to compose"
        );
    }

    #[test]
    fn parse_code_review_run_fresh() {
        let cli = parse(&["drovr", "code-review", "run", "myrun", "task-1", "--fresh"]).unwrap();
        match cli.command {
            Commands::CodeReview {
                sub: CodeReviewCmd::Run { fresh, .. },
            } => assert!(fresh),
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

    /// `mcp-findings` turns `run` and `task` into path components, and it is the one
    /// write-capable entrypoint drovr exposes. It is spawned by the panel with names
    /// already validated, but nothing stops it being invoked directly, so it must
    /// enforce the same rule rather than inherit it from its intended caller.
    #[test]
    fn mcp_findings_run_and_task_are_the_labels_the_dispatch_validates() {
        let cmd = Cli::parse_from(["drovr", "mcp-findings", "../escape", "task-1", "2"]);
        let Commands::McpFindings { run, task, iter } = cmd.command else {
            panic!("expected McpFindings");
        };
        assert_eq!(iter, 2, "the iteration scopes the findings file");
        // Exactly the checks the dispatch arm applies before serving.
        assert!(
            validate_label("run name", &run).is_err(),
            "a traversing run name must be refused"
        );
        assert!(validate_label("task", &task).is_ok());
        for bad in ["../escape", "a/b", ".."] {
            assert!(
                validate_label("task", bad).is_err(),
                "{bad} must be refused"
            );
        }
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
            review_phases: vec![{
                let mut p = run::Phase::new("review:task-1:1:correctness");
                p.status = PhaseStatus::Running;
                p
            }],
            gate: "spec".into(),
            cursor: 0,
            workspace: None,
            root_pane: None,
            project_dir: "/tmp/proj".into(),
            worktree_path: None,
            worktree_branch: None,
            archived: false,
            retired_panes: vec![],
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
            phases: vec![{
                let mut p = run::Phase::new("brainstorm");
                p.status = PhaseStatus::Done;
                p
            }],
            review_phases: vec![],
            gate: "spec".into(),
            cursor: 0,
            workspace: None,
            root_pane: None,
            project_dir: "/tmp/proj".into(),
            worktree_path: None,
            worktree_branch: None,
            archived: false,
            retired_panes: vec![],
        };
        let s = format_progress(&run);
        assert!(s.contains("1/1"), "got: {s}");
        assert!(s.contains("all done"), "got: {s}");
    }
}
