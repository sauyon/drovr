//! Panel orchestration for `drovr:code-review`.
//!
//! One call to [`code_review_run`] runs a single review pass for a task: read the
//! `base..head` scope, load config, seed + spawn one read-only reviewer per angle,
//! wait (bounded) for every reviewer to finish, read + union-merge the per-angle
//! findings, write the merged `<task>-review.json`, and return a [`ReviewOutcome`]
//! (→ exit 0 / 3 / 2 / 1). It is BLOCKING; the pipeline driver — never a skill — calls
//! it and reacts to the outcome.
//!
//! # Read-only findings path
//!
//! Reviewers emit fenced findings JSON in their transcript and exit. Drovr
//! observes herdr's `done` status, extracts the JSON, and writes all artifacts.
//! Legacy file output and `.done` markers remain accepted for compatibility.

use std::io;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use crate::config::load_config;
use crate::findings::{Review, is_clean, merge_reviews, parse_review};
use crate::herdr::Herdr;
use crate::phase::{done_marker, phase_send, spawn_reviewer};
use crate::run::{PhaseStatus, RunState, run_dir};

/// How often the private wait loop polls the filesystem for a reviewer's marker.
/// Mirrors `phase::POLL_INTERVAL` (that one is private; the panel does its own poll
/// because reviewer phases live in `review_phases`, which `phase_wait` never touches).
const POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Outcome of one review pass. Maps to the CLI exit codes the driver reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewOutcome {
    /// No blocking (Critical|Important) findings. → exit 0.
    Clean,
    /// At least one blocking finding; see `<task>-review.json`. → exit 3.
    Findings,
    /// Not every reviewer dropped its marker before `timeout_ms`. → exit 2.
    Timeout,
    /// Setup failure (e.g. base SHA not recorded, or HEAD unreadable). → exit 1.
    Error,
}

/// One-paragraph brief per angle, embedded in the reviewer seed. Unknown/custom
/// angles fall back to [`GENERIC_BRIEF`].
const ANGLE_BRIEFS: &[(&str, &str)] = &[
    (
        "correctness",
        "Focus on logic errors: off-by-ones, wrong conditions, broken invariants, \
         incorrect state transitions, concurrency hazards, and behavior that diverges \
         from the task's stated intent. Prefer a failing case over a hunch.",
    ),
    (
        "security",
        "Focus on injection (shell/SQL/path), unsanitized input crossing a trust \
         boundary, secret handling, authz/authn gaps, unsafe deserialization, and \
         resource-exhaustion vectors introduced by the change.",
    ),
    (
        "error-handling",
        "Focus on unhandled errors, swallowed failures, panics on untrusted input, \
         missing cleanup on the error path, and misleading or lost error context. \
         Check that fallible IO and subprocess calls are actually checked.",
    ),
    (
        "type-design",
        "Focus on the shape of the types and APIs: illegal states made \
         representable, stringly-typed data that should be an enum, leaky \
         abstractions, and signatures that push invariants onto every caller.",
    ),
];

/// Brief used for an angle not present in [`ANGLE_BRIEFS`].
const GENERIC_BRIEF: &str = "Review the change for issues relevant to this angle; report anything a careful \
     reviewer of that concern would flag.";

fn angle_brief(angle: &str) -> &'static str {
    ANGLE_BRIEFS
        .iter()
        .find(|(a, _)| *a == angle)
        .map(|(_, b)| *b)
        .unwrap_or(GENERIC_BRIEF)
}

/// The findings JSON schema, embedded verbatim in every reviewer seed so a reviewer
/// writes exactly what `findings::parse_review` accepts.
const FINDINGS_SCHEMA: &str = r#"{
  "verdict": "clean" | "changes",
  "findings": [
    {
      "file": "cli/src/foo.rs",
      "line": 42,                      // optional
      "severity": "critical" | "important" | "nit",
      "summary": "one-line what",
      "rationale": "why it matters"    // optional
    }
  ],
  "impact": "low | medium | high"      // optional
}"#;

/// `git -C <project_dir> rev-parse HEAD`, trimmed. `pub(crate)` so the
/// `drovr code-review base` handler (Task 6) records the base with the same helper.
pub(crate) fn head_sha(project_dir: &str) -> io::Result<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(project_dir)
        .args(["rev-parse", "HEAD"])
        .output()?;
    if !out.status.success() {
        return Err(io::Error::other(format!(
            "git rev-parse HEAD failed in {project_dir}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// Read the recorded review base for `task` from `<dir>/<task>-base.sha` (trimmed).
/// A missing file is the caller's `Error` outcome (base not recorded at task start).
fn base_sha(dir: &Path, task: &str) -> io::Result<String> {
    let p = dir.join(format!("{task}-base.sha"));
    Ok(std::fs::read_to_string(&p)?.trim().to_owned())
}

/// One greater than the max existing iteration among `run.review_phases` named
/// `review:<task>:<iter>:<angle>`. First pass = 1. This is what makes a timed-out
/// pass resumable: a re-run bumps the iter, so the new markers/phase names never
/// collide with the previous (still-`Running`) reviewers.
fn next_iter(run: &RunState, task: &str) -> u64 {
    let prefix = format!("review:{task}:");
    run.review_phases
        .iter()
        .filter_map(|p| p.name.strip_prefix(&prefix))
        .filter_map(|rest| rest.split_once(':').map(|(it, _angle)| it))
        .filter_map(|it| it.parse::<u64>().ok())
        .max()
        .map(|m| m + 1)
        .unwrap_or(1)
}

/// Build the per-angle reviewer seed.
fn build_seed(
    _run_name: &str,
    task: &str,
    angle: &str,
    base: &str,
    head: &str,
    task_desc: &str,
    _iter: u64,
) -> String {
    format!(
        "# Review angle: {angle}\n\n\
         You are a READ-ONLY reviewer on the drovr review panel for task `{task}`.\n\
         You are NOT a writer of project source or `state.json`. Do not edit either.\n\n\
         ## Your angle\n\n{brief}\n\n\
         ## Scope\n\n\
         Review the diff `git diff {base}..{head}` **plus** the current working tree in\n\
         the project. You may read any file and run tests. Base = `{base}`, head = `{head}`.\n\n\
         ## Task under review\n\n{task_desc}\n\n\
         ## Output\n\n\
         Return your findings in a fenced JSON block matching:\n\n\
         ```json\n{schema}\n```\n\n\
         `severity` is one of `critical` | `important` | `nit`. Omit `angle` in each\n\
         finding — drovr stamps it from this file's angle (`{angle}`). Report only issues\n\
         introduced or exposed by this change; a clean review is `{{\"verdict\":\"clean\",\"findings\":[]}}`.\n\n\
         ## Finish\n\n\
         Emit the fenced JSON, then exit. Do not modify any files or run `drovr phase done`.\n",
        brief = angle_brief(angle),
        schema = FINDINGS_SCHEMA,
    )
}

/// Extract the findings JSON from a reviewer's pane transcript: the LAST fenced code
/// block whose trimmed body starts with `{`. Used only on the fallback path (the
/// reviewer's readonly flag blocked the file write). Pure, so it is unit-testable.
fn extract_findings_json(transcript: &str) -> Option<String> {
    let mut result = None;
    let mut rest = transcript;
    while let Some(open) = rest.find("```") {
        let after_open = &rest[open + 3..];
        // Skip an optional language tag on the fence's opening line.
        let Some(nl) = after_open.find('\n') else {
            break;
        };
        let body = &after_open[nl + 1..];
        let Some(close) = body.find("```") else {
            break;
        };
        let block = body[..close].trim();
        if block.starts_with('{') {
            result = Some(block.to_string());
        }
        rest = &body[close + 3..];
    }
    result
}

/// Obtain one reviewer's findings JSON: read the file it wrote (primary), else fall
/// back to extracting the fenced JSON from its pane transcript and writing the file
/// on the reviewer's behalf. See the module doc for why.
fn obtain_findings_json<H: Herdr>(
    h: &H,
    run: &RunState,
    dir: &Path,
    task: &str,
    angle: &str,
    phase_name: &str,
) -> io::Result<String> {
    let path = dir.join(format!("{task}-review-{angle}.json"));
    // Prefer this iteration's transcript so a canonical file left by an earlier
    // pass cannot make resolved findings persist forever.
    let pane = run
        .find_phase(phase_name)
        .and_then(|p| p.pane_id.clone())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("reviewer '{phase_name}' has no pane to read findings from"),
            )
        })?;
    let transcript = h.agent_read(&pane)?;
    if let Some(json) = extract_findings_json(&transcript) {
        std::fs::write(&path, &json)?;
        return Ok(json);
    }
    // Compatibility path for reviewers that wrote the canonical file.
    std::fs::read_to_string(&path).map_err(|_| {
        io::Error::other(format!(
            "reviewer '{phase_name}' produced no findings JSON (no file written and \
             none found in its transcript)"
        ))
    })
}

/// Run ONE review panel for `task` and return the outcome. Blocking.
///
/// - base SHA: read `<run_dir>/<task>-base.sha` (missing → `Error`: base not recorded).
/// - head SHA: `git -C <run.project_dir> rev-parse HEAD` (unreadable → `Error`).
/// - iter: [`next_iter`] (first pass = 1; a re-run after timeout bumps it).
/// - per configured angle: write `<task>-review-<angle>-seed.md`, [`spawn_reviewer`]
///   read-only, then [`phase_send`] the seed brief.
/// - wait: private poll loop on each `review:<task>:<iter>:<angle>.done` marker (reuses
///   `phase::done_marker`; never `phase_wait`, which is `phases`-only). Each marker that
///   lands flips that reviewer's `review_phases` status to `Done`; saved once.
/// - merge: [`obtain_findings_json`] + [`parse_review`] per angle, [`merge_reviews`]
///   (angle stamped from the filename; verdict RECOMPUTED, per-angle verdicts ignored),
///   pretty-write `<task>-review.json`; `Clean` if [`is_clean`] else `Findings`.
/// - timeout: `Timeout` (resumable — the still-`Running` reviewers stay, a re-run bumps
///   iter so its markers don't collide).
pub fn code_review_run<H: Herdr>(
    h: &H,
    run: &mut RunState,
    task: &str,
    timeout_ms: u64,
) -> io::Result<ReviewOutcome> {
    let dir = run_dir(&run.name);

    // Scope first: without a recorded base or a readable HEAD there is nothing to
    // review. Base is read before HEAD so "base not recorded" is reported precisely.
    let base = match base_sha(&dir, task) {
        Ok(b) => b,
        // Surface *why* (missing file vs. read error): a bare `Error` exit with no
        // explanation is painful to debug from the driver.
        Err(e) => {
            eprintln!(
                "code-review: no review base for '{task}' ({e}); run `drovr code-review base` at task start"
            );
            return Ok(ReviewOutcome::Error);
        }
    };
    let head = match head_sha(&run.project_dir) {
        Ok(h) => h,
        Err(e) => {
            eprintln!(
                "code-review: cannot read HEAD in '{}' ({e})",
                run.project_dir
            );
            return Ok(ReviewOutcome::Error);
        }
    };

    let cfg = load_config()?;
    let auto_cursor_integrated = cfg.review_agent.is_none() && h.integration_present("cursor");
    let review_agent = cfg.review_agent_for(run.agent.as_deref(), auto_cursor_integrated)?;
    let review_agent_integrated = if review_agent == "cursor" && auto_cursor_integrated {
        true
    } else {
        h.integration_present(&review_agent)
    };
    if !review_agent_integrated {
        return Err(std::io::Error::other(format!(
            "review agent '{review_agent}' has no herdr integration; run \
             `herdr integration install {review_agent}`"
        )));
    }
    let launch = cfg.launch(&review_agent, &run.project_dir, true)?;
    let iter = next_iter(run, task);
    std::fs::create_dir_all(&dir)?;

    // Seed + spawn one read-only reviewer per angle, then inject its brief. Every
    // reviewer exits (drops its marker) before the implementer fixes anything, so the
    // single-writer invariant holds — the panel never has a reviewer alive while a
    // writer runs.
    for angle in &cfg.angles {
        let seed_path = dir.join(format!("{task}-review-{angle}-seed.md"));
        let seed_text = build_seed(&run.name, task, angle, &base, &head, &run.task, iter);
        std::fs::write(&seed_path, &seed_text)?;

        let phase = format!("review:{task}:{iter}:{angle}");
        spawn_reviewer(h, run, &phase, Some(&seed_path), &launch)?;
        // A `phase_send` failure ABORTS the pass (`?` → Err → the CLI's `Error`
        // exit) rather than continuing: a spawned-but-unseeded reviewer would never
        // write findings or drop a marker, so pressing on would only guarantee a
        // timeout. Any reviewer panes already spawned this pass are left running and
        // reclaimed by the single `workspace_close` at `drovr cleanup` — the codebase
        // invariant is "never close a pane mid-run" (mirrors `phase_start`).
        phase_send(h, run, &phase, &seed_text)?;
    }

    // Private, `review_phases`-aware wait: poll every angle's marker until all present
    // or the deadline passes. Each landed marker flips the reviewer's status directly.
    let phases: Vec<String> = cfg
        .angles
        .iter()
        .map(|a| format!("review:{task}:{iter}:{a}"))
        .collect();
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let mut pending: Vec<String> = phases.clone();
    loop {
        pending.retain(|name| {
            let finished = done_marker(&run.name, name).exists()
                || run
                    .find_phase(name)
                    .and_then(|phase| phase.pane_id.as_deref())
                    .and_then(|pane| h.agent_status(pane))
                    .as_deref()
                    == Some("done");
            if finished {
                if let Some(i) = run.review_phases.iter().position(|p| &p.name == name) {
                    run.review_phases[i].status = PhaseStatus::Done;
                }
                false
            } else {
                true
            }
        });
        if pending.is_empty() {
            break;
        }
        let now = Instant::now();
        if now >= deadline {
            // Leave the timed-out reviewers `Running`; a re-run bumps iter and waits on
            // fresh markers, so this pass's leftovers never collide with the next.
            run.save()?;
            return Ok(ReviewOutcome::Timeout);
        }
        thread::sleep(POLL_INTERVAL.min(deadline - now));
    }
    run.save()?;

    // All markers present → collect, merge, write.
    let mut per_angle: Vec<(String, Review)> = Vec::with_capacity(cfg.angles.len());
    for (angle, phase) in cfg.angles.iter().zip(phases.iter()) {
        let json = obtain_findings_json(h, run, &dir, task, angle, phase)?;
        per_angle.push((angle.clone(), parse_review(&json)?));
    }
    let merged = merge_reviews(per_angle);
    let out_path = dir.join(format!("{task}-review.json"));
    std::fs::write(
        &out_path,
        serde_json::to_string_pretty(&merged).map_err(io::Error::other)?,
    )?;

    Ok(if is_clean(&merged) {
        ReviewOutcome::Clean
    } else {
        ReviewOutcome::Findings
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::herdr::FakeHerdr;
    use crate::phase::done_marker;
    use crate::run::{Phase, PhaseStatus};
    use crate::test_util::ENV_LOCK;
    use std::process::Command;

    /// A run whose `project_dir` is a fresh git repo with one commit (so `head_sha`
    /// resolves), and whose run dir is a clean, unique `XDG_DATA_HOME`. Caller holds
    /// ENV_LOCK. Also writes a config that pins reviews to Claude so tests do not
    /// depend on whether Cursor's `agent` executable is installed on the host.
    fn make_run(name: &str) -> (RunState, tempfile::TempDir) {
        let data = std::path::PathBuf::from(format!("/tmp/drovr-cr-test-{name}"));
        let _ = std::fs::remove_dir_all(&data);
        unsafe {
            std::env::set_var("XDG_DATA_HOME", &data);
        }
        // Pin the review backend; all other fields use built-in defaults.
        let cfg_home = data.join("config-home");
        std::fs::create_dir_all(cfg_home.join("drovr")).unwrap();
        std::fs::write(
            cfg_home.join("drovr/config.toml"),
            "review_agent = \"claude\"\n",
        )
        .unwrap();
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", &cfg_home);
        }

        let repo = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            let out = Command::new("git")
                .arg("-C")
                .arg(repo.path())
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "t@t.t"]);
        git(&["config", "user.name", "t"]);
        std::fs::write(repo.path().join("f.txt"), "hi").unwrap();
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "init"]);

        let run = RunState {
            name: name.to_owned(),
            task: "implement the widget".into(),
            agent: Some("claude".into()),
            phases: vec![],
            review_phases: vec![],
            gate: "spec".into(),
            cursor: 0,
            workspace: Some("ws-cr".into()),
            root_pane: Some("ws-cr:root".into()),
            project_dir: repo.path().to_string_lossy().into_owned(),
            worktree_path: None,
            worktree_branch: None,
        };
        (run, repo)
    }

    fn write_base(run: &RunState, task: &str) {
        let dir = run_dir(&run.name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{task}-base.sha")), "deadbeef\n").unwrap();
    }

    fn seed_angle_file(run: &RunState, task: &str, angle: &str, body: &str) {
        let dir = run_dir(&run.name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{task}-review-{angle}.json")), body).unwrap();
    }

    /// Pre-drop the done markers for iter=1's four default angles (the panel spawns
    /// them, then the very first wait poll sees the markers and completes).
    fn drop_markers(run: &RunState, task: &str, iter: u64) {
        for a in ["correctness", "security", "error-handling", "type-design"] {
            let name = format!("review:{task}:{iter}:{a}");
            let m = done_marker(&run.name, &name);
            std::fs::create_dir_all(m.parent().unwrap()).unwrap();
            std::fs::write(&m, b"").unwrap();
        }
    }

    const CLEAN: &str = r#"{"verdict":"clean","findings":[]}"#;

    #[test]
    fn clean_pass_writes_merged_and_returns_clean() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-clean");
        write_base(&run, "task-1");
        for a in ["correctness", "security", "error-handling", "type-design"] {
            seed_angle_file(&run, "task-1", a, CLEAN);
        }
        // Simulate every reviewer having dropped its marker.
        drop_markers(&run, "task-1", 1);

        let outcome = code_review_run(&h, &mut run, "task-1", 5_000).unwrap();
        assert_eq!(outcome, ReviewOutcome::Clean);

        // Merged file exists and is clean.
        let merged = run_dir(&run.name).join("task-1-review.json");
        let parsed = parse_review(&std::fs::read_to_string(&merged).unwrap()).unwrap();
        assert_eq!(parsed.verdict, "clean");
        assert!(parsed.findings.is_empty());

        // Isolation: pipeline `phases` untouched; four iter-1 reviewers registered.
        assert!(run.phases.is_empty(), "pipeline phases must stay empty");
        assert_eq!(run.review_phases.len(), 4);
        assert!(
            run.review_phases
                .iter()
                .all(|p| p.name.starts_with("review:task-1:1:")),
            "all reviewers are iter 1: {:?}",
            run.review_phases
                .iter()
                .map(|p| &p.name)
                .collect::<Vec<_>>()
        );
        assert!(
            run.review_phases
                .iter()
                .all(|p| p.status == PhaseStatus::Done),
            "every reviewer whose marker landed must be marked Done"
        );
    }

    #[test]
    fn readonly_reviewers_complete_from_herdr_status_and_transcript() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-readonly-done");
        run.agent = Some("cursor".into());
        std::fs::write(
            std::path::Path::new(&std::env::var("XDG_CONFIG_HOME").unwrap())
                .join("drovr/config.toml"),
            "review_agent = \"cursor\"\n",
        )
        .unwrap();
        write_base(&run, "task-1");
        // The seed path calls `phase_send`, whose readiness gate polls
        // `agent_status` once per angle; feed it an `idle` per angle so those polls
        // don't consume the `done` statuses the wait loop below relies on.
        for _ in 0..4 {
            h.push_status(Some("idle"));
        }
        for _ in 0..4 {
            h.push_status(Some("done"));
            h.push_read(format!("```json\n{CLEAN}\n```"));
        }

        let outcome = code_review_run(&h, &mut run, "task-1", 5_000).unwrap();
        assert_eq!(outcome, ReviewOutcome::Clean);
        assert!(
            run.review_phases
                .iter()
                .all(|phase| phase.status == PhaseStatus::Done)
        );
        let calls = h.calls();
        assert!(
            calls.iter().any(|call| {
                call.contains("agent --mode plan --model 'composer-2.5' --workspace")
            })
        );
    }

    #[test]
    fn important_finding_returns_findings_and_changes_verdict() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-findings");
        write_base(&run, "task-1");
        seed_angle_file(
            &run,
            "task-1",
            "correctness",
            r#"{"verdict":"clean","findings":[{"file":"a.rs","severity":"important","summary":"bug"}]}"#,
        );
        for a in ["security", "error-handling", "type-design"] {
            seed_angle_file(&run, "task-1", a, CLEAN);
        }
        drop_markers(&run, "task-1", 1);

        let outcome = code_review_run(&h, &mut run, "task-1", 5_000).unwrap();
        assert_eq!(outcome, ReviewOutcome::Findings);

        let merged = run_dir(&run.name).join("task-1-review.json");
        let parsed = parse_review(&std::fs::read_to_string(&merged).unwrap()).unwrap();
        assert_eq!(parsed.verdict, "changes");
        assert_eq!(parsed.findings.len(), 1);
        // The angle is stamped from the source filename, not the JSON.
        assert_eq!(parsed.findings[0].angle, "correctness");
    }

    #[test]
    fn later_clean_pass_replaces_stale_finding_files() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-stale-findings");
        write_base(&run, "task-1");
        seed_angle_file(
            &run,
            "task-1",
            "correctness",
            r#"{"verdict":"changes","findings":[{"file":"a.rs","severity":"important","summary":"fixed later"}]}"#,
        );
        for angle in ["security", "error-handling", "type-design"] {
            seed_angle_file(&run, "task-1", angle, CLEAN);
        }
        drop_markers(&run, "task-1", 1);
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 5_000).unwrap(),
            ReviewOutcome::Findings
        );

        for _ in 0..4 {
            h.push_read(format!("```json\n{CLEAN}\n```"));
        }
        drop_markers(&run, "task-1", 2);
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 5_000).unwrap(),
            ReviewOutcome::Clean
        );
        let merged = run_dir(&run.name).join("task-1-review.json");
        assert!(
            parse_review(&std::fs::read_to_string(merged).unwrap())
                .unwrap()
                .findings
                .is_empty()
        );
    }

    #[test]
    fn missing_base_sha_is_error() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-nobase");
        // No base.sha written.
        let outcome = code_review_run(&h, &mut run, "task-1", 5_000).unwrap();
        assert_eq!(outcome, ReviewOutcome::Error);
        // Nothing spawned.
        assert!(run.review_phases.is_empty());
    }

    #[test]
    fn timeout_leaves_running_and_next_call_bumps_iter() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-timeout");
        write_base(&run, "task-1");

        // No markers dropped → tiny timeout → Timeout.
        let outcome = code_review_run(&h, &mut run, "task-1", 40).unwrap();
        assert_eq!(outcome, ReviewOutcome::Timeout);
        assert_eq!(run.review_phases.len(), 4);
        assert!(
            run.review_phases
                .iter()
                .all(|p| p.name.starts_with("review:task-1:1:")),
            "first pass phases are iter 1"
        );
        assert!(
            run.review_phases
                .iter()
                .all(|p| p.status == PhaseStatus::Running),
            "timed-out reviewers stay Running (resumable)"
        );

        // Second call bumps to iter 2 and waits on the fresh markers.
        let outcome = code_review_run(&h, &mut run, "task-1", 40).unwrap();
        assert_eq!(outcome, ReviewOutcome::Timeout);
        assert_eq!(
            run.review_phases.len(),
            8,
            "iter-1 leftovers remain + iter-2 added"
        );
        assert!(
            run.review_phases
                .iter()
                .any(|p| p.name == "review:task-1:2:correctness"),
            "second pass must produce iter-2 phase names: {:?}",
            run.review_phases
                .iter()
                .map(|p| &p.name)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn reviewers_spawned_with_configured_readonly_launch() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-launch");
        write_base(&run, "task-1");
        for a in ["correctness", "security", "error-handling", "type-design"] {
            seed_angle_file(&run, "task-1", a, CLEAN);
        }
        drop_markers(&run, "task-1", 1);

        code_review_run(&h, &mut run, "task-1", 5_000).unwrap();

        let calls = h.calls();
        let run_calls: Vec<&String> = calls.iter().filter(|c| c.contains("pane_run")).collect();
        assert_eq!(run_calls.len(), 4, "one launch per angle: {run_calls:?}");
        for c in &run_calls {
            assert!(
                c.contains("--permission-mode plan"),
                "reviewer must launch with the configured read-only flag: {c}"
            );
        }
    }

    #[test]
    fn fallback_extracts_findings_from_transcript_when_file_absent() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-fallback");
        write_base(&run, "task-1");
        // Only three angles get a file; the fourth (type-design) does not, forcing the
        // transcript fallback. Queue one transcript read carrying fenced JSON.
        for a in ["correctness", "security", "error-handling"] {
            seed_angle_file(&run, "task-1", a, CLEAN);
            h.push_read("");
        }
        h.push_read(
            "reviewer output...\n```json\n{\"verdict\":\"clean\",\"findings\":[]}\n```\ndone",
        );
        drop_markers(&run, "task-1", 1);

        let outcome = code_review_run(&h, &mut run, "task-1", 5_000).unwrap();
        assert_eq!(outcome, ReviewOutcome::Clean);
        // drovr wrote the missing per-angle file from the transcript.
        let recovered = run_dir(&run.name).join("task-1-review-type-design.json");
        assert!(
            recovered.exists(),
            "fallback must persist the recovered findings file"
        );
        // The pane was read (agent_read) exactly for the missing angle.
        assert!(h.calls().iter().any(|c| c.contains("agent_read")));
    }

    #[test]
    fn head_sha_reads_temp_repo() {
        let _lock = ENV_LOCK.lock().unwrap();
        let (run, _repo) = make_run("cr-headsha");
        let sha = head_sha(&run.project_dir).unwrap();
        assert_eq!(sha.len(), 40, "a full HEAD sha: {sha}");
        assert!(sha.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn head_sha_errors_on_non_repo() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(head_sha(&tmp.path().to_string_lossy()).is_err());
    }

    #[test]
    fn next_iter_starts_at_one_then_increments() {
        let base = RunState {
            name: "r".into(),
            task: "t".into(),
            agent: None,
            phases: vec![],
            review_phases: vec![],
            gate: "spec".into(),
            cursor: 0,
            workspace: None,
            root_pane: None,
            project_dir: String::new(),
            worktree_path: None,
            worktree_branch: None,
        };
        assert_eq!(next_iter(&base, "task-1"), 1);

        let mut run = base.clone();
        let mk = |name: &str| Phase {
            name: name.into(),
            status: PhaseStatus::Running,
            ..Default::default()
        };
        run.review_phases = vec![
            mk("review:task-1:1:correctness"),
            mk("review:task-1:1:security"),
            // A different task's phases must not affect task-1's counter.
            mk("review:task-2:5:correctness"),
        ];
        assert_eq!(next_iter(&run, "task-1"), 2);
        assert_eq!(next_iter(&run, "task-2"), 6);
    }

    #[test]
    fn extract_findings_json_picks_last_json_fence() {
        let t = "prose\n```\nnot json\n```\nmore\n```json\n{\"verdict\":\"clean\"}\n```\ntail";
        assert_eq!(
            extract_findings_json(t).as_deref(),
            Some("{\"verdict\":\"clean\"}")
        );
        assert!(extract_findings_json("no fences here").is_none());
        assert!(extract_findings_json("```\njust text\n```").is_none());
    }

    #[test]
    fn seed_contains_scope_schema_and_readonly_finish_instruction() {
        let seed = build_seed(
            "myrun",
            "task-1",
            "security",
            "aaa",
            "bbb",
            "do the thing",
            3,
        );
        assert!(
            seed.contains("git diff aaa..bbb"),
            "seed must state the diff scope"
        );
        assert!(
            seed.contains("do the thing"),
            "seed must carry the task description"
        );
        assert!(
            seed.contains("fenced JSON block"),
            "seed must request transcript JSON"
        );
        assert!(
            seed.contains("Do not modify any files or run `drovr phase done`"),
            "seed must preserve strict read-only behavior"
        );
        assert!(seed.contains("critical") && seed.contains("important") && seed.contains("nit"));
    }
}
