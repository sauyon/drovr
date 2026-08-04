//! Panel orchestration for `drovr:code-review`.
//!
//! One call to [`code_review_run`] runs a single review pass for a task: read the
//! `base..head` scope, load config, seed + spawn one read-only reviewer per angle,
//! wait (bounded) for every reviewer to finish, read + union-merge the per-angle
//! findings, write the merged `<task>-review.json`, and return a [`ReviewOutcome`]
//! (→ exit 0 / 3 / 2 / 1). It is BLOCKING.
//!
//! Two roles share this one entry point, and it cannot tell them apart — there is no
//! caller identity here, by design. A task agent may run a pass on its own work as
//! often as it likes, as iteration feedback. Only the pipeline driver's pass is the
//! acceptance gate: the driver reacts to the outcome and decides whether the task
//! advances. That separation lives in the skill docs (`drovr:pipeline`,
//! `drovr:code-review`), not in this module — see `docs/known-issues.md` for the run
//! that made the distinction necessary.
//!
//! # Resuming a slow panel
//!
//! Reviewers can outlive `timeout_ms`. A timeout is therefore not a failure but a
//! pause: the outstanding reviewers stay `Running`, every angle that *did* finish is
//! already banked in `<task>-review-<iter>-<angle>.json`, and a plain re-run RESUMES —
//! re-attaching to the same panel and waiting only on the stragglers. Nothing is
//! re-reviewed, so a slow panel costs one reviewer per angle no matter how many
//! resumes it takes. A new panel is opened only when the caller passes `fresh`, when
//! HEAD has moved since the pending reviewers were seeded (their diff no longer
//! stands), or when the previous pass ran to completion (the fix loop asking for a
//! genuinely new review). See [`resumable_iter`].
//!
//! Resume must never wait forever on a reviewer that can no longer deliver, so an
//! angle is REPLACED rather than waited on when its pane is gone
//! ([`Herdr::pane_exists`] — which, unlike `agent_status`, separates "pane gone" from
//! "status unparseable") or when it is marked [`PhaseStatus::Failed`]. `Failed` is
//! recorded for the two ways a reviewer ends up alive but useless: its brief could
//! not be delivered (`phase_send` failed after the pane launched), or it finished
//! having emitted output that cannot be parsed. Both would otherwise reproduce
//! identically on every resume — the pre-resume code masked them by always spawning a
//! new panel, and only `Failed` preserves that self-healing.
//!
//! # Read-only findings path
//!
//! Each reviewer delivers by calling `submit_findings`, the single tool of the MCP
//! server drovr starts for it ([`crate::mcp_findings`]); that server writes
//! `<task>-review-<iter>-<angle>.json`. The file is the ONLY channel drovr reads; pane
//! transcripts are never parsed.
//!
//! The reviewer does not write the file itself because it cannot: read-only mode
//! refuses the write. Rather than widen reviewer permissions, drovr performs that
//! one write on its behalf, so the carve-out is exactly one file — and the panel
//! provisions the server (see [`write_mcp_config`]) before it spawns anyone.
//!
//! Scraping a transcript cannot be made correct, because it is a rendered terminal
//! view rather than a data channel: renderers hard-wrap long lines, inserting raw
//! newlines *inside* JSON string literals; they collapse long tool output behind
//! "N lines hidden"; and they need not show fence markers at all.
//!
//! Herdr still spawns reviewer panes and reports liveness — it just does not carry
//! their output.

use std::io;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use crate::config::load_config;
use crate::findings::{Review, is_clean, merge_reviews, parse_review};
use crate::herdr::{AgentStatus, Herdr};
use crate::mcp_findings::findings_path;
use crate::phase::{
    archived_run_error, done_marker, phase_send, poll_phase_pane, spawn_reviewer,
};
use crate::run::{PhaseStatus, RunState, run_dir};

/// How often the private wait loop polls the filesystem for a reviewer's marker.
/// Mirrors `phase::POLL_INTERVAL` (that one is private; the panel does its own poll
/// because reviewer phases live in `review_phases`, which `phase_wait` never touches).
const POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Outcome of one review pass. Maps to the CLI exit codes the driver reads.
///
/// `Clean` is the outcome the pipeline advances a task on, so it must mean exactly one
/// thing: reviewers looked at a real range and found nothing blocking. It used to mean
/// two — that, or nobody looked at anything, because the range was empty. Those are
/// indistinguishable downstream and the vacuous one is the more dangerous, since it
/// arrives faster and never disagrees with you. [`EmptyRange`](Self::EmptyRange) exists
/// so that state cannot be spelled `Clean`.
///
/// What this type still does NOT carry is WHO ran the pass. An author-run pass and the
/// driver's gate produce the same outcome, deliberately: caller identity here could only
/// be self-declared, and a forgeable role is worse than an absent one. That separation is
/// held by the skill docs (`drovr:pipeline`, `drovr:code-review`) and by the driver
/// re-running the panel itself, unconditionally. See `docs/known-issues.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewOutcome {
    /// Reviewers examined a non-empty `base..head` and reported no blocking
    /// (Critical|Important) findings. → exit 0.
    ///
    /// Read this narrowly. It is a statement about **the committed range at the moment
    /// the panel ran**, and about nothing else. Specifically, `Clean` does NOT mean:
    ///
    /// - **that the range covers the task's work.** `base..head` is whatever was
    ///   committed; anything left uncommitted is outside it. The reviewer's brief also
    ///   names the working tree, but that is prose in a prompt, not scope this code
    ///   computes or can vouch for — untracked files never appear in a `git diff`. A
    ///   partial commit yields an honest verdict on a subset nobody declared.
    /// - **that the work is done, or may be reported done.** `Clean` is one panel's
    ///   result, not an adjudication. Who is entitled to treat it as a gate is a
    ///   question this type deliberately cannot answer (see below).
    /// - **that nothing is wrong.** Reviewers are sampled and non-deterministic; a
    ///   second panel over the identical head has returned an Important where the first
    ///   returned clean. `Clean` is evidence, and one draw of it.
    ///
    /// What it does mean is exact: a real range was looked at, and nothing blocking came
    /// back. That narrow guarantee is worth more than a broad one, and callers must not
    /// infer past it.
    Clean,
    /// At least one blocking finding; see `<task>-review.json`. → exit 3.
    Findings,
    /// Not every reviewer dropped its marker before `timeout_ms`. → exit 2.
    Timeout,
    /// `git diff base..head` contains **no change** — the two commits have identical
    /// trees — so there is nothing to review and no verdict about it could mean
    /// anything. Refused before any reviewer is spawned. → exit 1.
    ///
    /// **Not "the two SHAs are equal."** That is the common cause and it is not the
    /// condition: `git commit --allow-empty` advances `head` without touching the tree,
    /// so the SHAs differ while the range is still empty, and that is refused here too.
    /// The discriminant is what the range CONTAINS. (An earlier version of this variant
    /// was documented, and implemented, as the SHA comparison; both were wrong.)
    ///
    /// Separate from [`Error`](Self::Error) because the cause is specific and the fix is
    /// specific (commit, or re-record the base), and separate from
    /// [`Clean`](Self::Clean) because that is the confusion it exists to prevent.
    EmptyRange,
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

/// The findings shape as a reviewer sees it, embedded in every seed.
///
/// RENDERED from [`crate::mcp_findings::review_schema`] — the same definition the MCP
/// tool advertises and `findings::parse_review` enforces. A JSON Schema is precise but
/// unreadable in a brief, so this renders the friendly form; deriving the field names
/// and the closed value sets from the schema is what stops the two drifting. They used
/// to be independent copies, and a drift there tells a reviewer to send something
/// validation then rejects — which reads exactly like a lazy reviewer.
fn findings_schema() -> String {
    let schema = crate::mcp_findings::review_schema();
    // `"a" | "b"` from a schema `enum`. Empty if the shape ever changes underneath —
    // `seed_schema_is_rendered_from_the_one_definition` is what catches that.
    let alt = |v: &serde_json::Value| {
        v["enum"]
            .as_array()
            .map(|vals| {
                vals.iter()
                    .map(|x| x.to_string())
                    .collect::<Vec<_>>()
                    .join(" | ")
            })
            .unwrap_or_default()
    };
    format!(
        "{{\n  \"verdict\": {verdict},\n  \"findings\": [\n    {{\n      \
         \"file\": \"cli/src/foo.rs\",\n      \"line\": 42,                      // optional\n      \
         \"severity\": {severity},\n      \"summary\": \"one-line what\",\n      \
         \"rationale\": \"why it matters\"    // optional\n    }}\n  ],\n  \
         \"impact\": {impact}      // optional\n}}",
        verdict = alt(&schema["verdict"]),
        severity = alt(&schema["findings"]["items"]["properties"]["severity"]),
        impact = alt(&schema["impact"]),
    )
}

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

/// Does `base..head` contain any change at all?
///
/// `Ok(true)` = the range is EMPTY: the two commits have identical trees, so there is
/// nothing for a reviewer to look at.
///
/// Comparing the two SHAs is NOT this property, which is the trap the first version of
/// this guard fell into. `git commit --allow-empty` advances HEAD without changing the
/// tree, so `base != head` while `git diff base..head` is empty — a vacuous review that
/// a hash comparison waves straight through. Ask git what the range CONTAINS, never
/// whether two names for it differ.
///
/// `git diff --quiet` is the direct question: exit 0 = no differences, exit 1 =
/// differences found. Any other status means git could not answer (an unresolvable
/// base, say), which is [`Err`] — "could not tell" is not "empty".
fn range_is_empty(project_dir: &str, base: &str, head: &str) -> io::Result<bool> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(project_dir)
        .args(["diff", "--quiet", &format!("{base}..{head}")])
        .output()?;
    match out.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        other => Err(io::Error::other(format!(
            "git diff --quiet {base}..{head} in {project_dir} exited {other:?}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))),
    }
}

/// Read the recorded review base for `task` from `<dir>/<task>-base.sha` (trimmed).
/// A missing file is the caller's `Error` outcome (base not recorded at task start), and
/// so is a file whose contents are not a plausible object name — see [`validated_sha`].
fn base_sha(dir: &Path, task: &str) -> io::Result<String> {
    let p = dir.join(format!("{task}-base.sha"));
    let raw = std::fs::read_to_string(&p)?;
    validated_sha(raw.trim())
        .map_err(|e| io::Error::other(format!("{}: {e}", p.display())))
        .map(|s| s.to_owned())
}

/// Reject a recorded base that is not a bare git object id, using the SAME predicate the
/// review server applies to the same file ([`crate::review::safe_sha`]) rather than a
/// second opinion that could drift from it.
///
/// `<task>-base.sha` is an ordinary file, and whatever is in it reaches `git` as an
/// argument composed into `{base}..{head}` — so a value like `-C` or `--output=x` is git
/// OPTION injection, and one containing `..` or whitespace silently redefines the range.
/// Restricting the alphabet to hex closes all of that at once.
///
/// This is a validity check, not a resolution check: `deadbeef` passes here and then fails
/// in the repository. Both halves are needed — validation stops a crafted value from
/// reaching git, and refusing when the range check errors stops an unresolvable one from
/// waving the guard through.
fn validated_sha(s: &str) -> Result<&str, String> {
    if crate::review::safe_sha(s) {
        Ok(s)
    } else {
        Err(format!(
            "{s:?} is not a git object name (expected bare hex); \
             re-record it with `drovr code-review base`"
        ))
    }
}

/// Resolve this pass's context via the shared recorder in [`crate::brief`], keyed
/// `<task>-review` (file `<task>-review-context.md`). Shared so the reviewer and phase
/// briefs cannot drift: `--context` records, `--context ''` clears, absent reuses and
/// says so.
fn resolve_context(dir: &Path, task: &str, supplied: Option<&str>) -> io::Result<Option<String>> {
    crate::brief::resolve_context(dir, &format!("{task}-review"), supplied)
}

/// Compose one angle's reviewer brief and return it, spawning NOTHING.
///
/// This is the same text `code_review_run` injects, exposed for every case where the
/// driver spawns the reviewer instead of the panel: an in-harness read-only subagent, a
/// host with no herdr integration for the review agent, a run with no workspace, or a
/// panel that is wedged. That reviewer's prompt must still be drovr's brief rather than
/// one the driver wrote, or the frame is agent-authored again.
pub fn code_review_brief(
    run: &RunState,
    task: &str,
    angle: &str,
    context: Option<&str>,
) -> io::Result<String> {
    let dir = run_dir(&run.name);
    let base = base_sha(&dir, task).map_err(|e| {
        io::Error::new(
            e.kind(),
            format!(
                "no review base recorded for '{task}' ({e}); run \
                 `drovr code-review base {} {task}` at task start",
                run.name
            ),
        )
    })?;
    let head = head_sha(&run.project_dir)?;
    // `Err` here, where `code_review_run` returns `ReviewOutcome::Error` for the same class
    // of failure. Not an oversight: this function's only job is to produce a brief, so it
    // has no outcome to report — the CLI prints the error and exits 1, which is the same
    // observable result the outcome path produces.
    let context = resolve_context(&dir, task, context)?;
    Ok(build_seed(
        task,
        angle,
        &base,
        &head,
        &run.task,
        &run.project_dir,
        context.as_deref(),
    ))
}

/// The iteration a re-run should RESUME, if any: the newest one, and only while a
/// reviewer for a **currently configured** angle is still `Running`.
///
/// Deliberately restricted to the *newest* iteration. An older iteration with
/// `Running` leftovers is a superseded pass (a `--fresh` re-run abandoned it), and
/// reviving those zombies would review a diff nobody asked about. A newest
/// iteration that is fully `Done` has already produced `<task>-review.json`, so a
/// re-run there is the fix loop asking for a genuinely new pass.
///
/// Restricted to configured `angles` for the same reason: an angle dropped from
/// config mid-run leaves a reviewer nothing will ever wait on again (the pass only
/// iterates configured angles), so counting it as "still running" would hold the
/// iteration open forever and keep re-banking a finished pass's results.
fn resumable_iter(run: &RunState, task: &str, angles: &[String]) -> Option<u64> {
    let prefix = format!("{}{task}:", crate::run::REVIEWER_PREFIX);
    let newest = run
        .review_phases
        .iter()
        .filter_map(|p| p.name.strip_prefix(&prefix))
        .filter_map(|rest| rest.split_once(':').map(|(it, _angle)| it))
        .filter_map(|it| it.parse::<u64>().ok())
        .max()?;
    let running = angles.iter().any(|angle| {
        run.review_phases.iter().any(|p| {
            p.name == format!("{prefix}{newest}:{angle}") && p.status == PhaseStatus::Running
        })
    });
    running.then_some(newest)
}

/// `<task>-review-<iter>.head` — the head SHA an iteration's reviewers were seeded
/// against. A resume compares it with the current HEAD: if the implementer has
/// committed since, those reviewers are reading a diff that no longer stands and
/// resuming them would launder a stale review, so the pass starts fresh instead.
fn iter_head_path(dir: &Path, task: &str, iter: u64) -> std::path::PathBuf {
    dir.join(format!("{task}-review-{iter}.head"))
}

/// One greater than the max existing iteration among `run.review_phases` named
/// `review:<task>:<iter>:<angle>`. First pass = 1. Used for a brand-new panel
/// (`--fresh`, a moved HEAD, or a previous pass that ran to completion) so its
/// markers/phase names never collide with an earlier iteration's leftovers.
fn next_iter(run: &RunState, task: &str) -> u64 {
    let prefix = format!("{}{task}:", crate::run::REVIEWER_PREFIX);
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
///
/// `project_dir` is named in the seed on purpose: the diff is what changed, but
/// whether the change is *right* only shows in the code around it — callers,
/// invariants, existing tests. A reviewer handed a diff and no repo grant reviews
/// the hunks and stops, so the seed spells out that the whole checkout is readable.
///
/// The reviewer runs read-only and so cannot write its own findings file; the
/// seed therefore routes the whole review through the `submit_findings` tool,
/// which drovr serves (see [`crate::mcp_findings`]) and performs the write for.
fn build_seed(
    task: &str,
    angle: &str,
    base: &str,
    head: &str,
    task_desc: &str,
    project_dir: &str,
    context: Option<&str>,
) -> String {
    // Always emitted, matching the phase briefs: a section that appears only sometimes is
    // one a brief cannot refer to, and its absence is indistinguishable from a delivery
    // failure. Say "none supplied" instead of saying nothing.
    let context_section = match context.map(str::trim).filter(|c| !c.is_empty()) {
        Some(c) => format!("## Context from the driver\n\n{c}\n\n"),
        None => "## Context from the driver\n\n*(none supplied — review the diff and the \
                 repository on their own terms.)*\n\n"
            .to_string(),
    };
    format!(
        "# Review angle: {angle}\n\n\
         You are a READ-ONLY reviewer on the drovr review panel for task `{task}`.\n\
         You are NOT a writer of project source or `state.json`. Do not edit either.\n\n\
         ## Your angle\n\n{brief}\n\n\
         ## Scope\n\n\
         The change under review is `git diff {base}..{head}` **plus** the current\n\
         working tree. Base = `{base}`, head = `{head}`.\n\n\
         You also have the WHOLE REPOSITORY to read: it is a full checkout at\n\
         `{project_dir}`. Do not review the diff in isolation — you may read any file\n\
         in it, follow the change's callers and callees outside the diff, check the\n\
         invariants and neighbouring code it has to hold up against, and run the tests.\n\
         Reading is unrestricted; only writing is not.\n\n\
         ## Task under review\n\n{task_desc}\n\n\
         {context_section}\
         ## Output\n\n\
         Deliver your findings with the `{tool}` tool. Your backend may list it\n\
         as `{qualified_tool}`, and may defer it — load its schema\n\
         before calling if so. Its `angle` argument is `{angle}` — YOUR angle, and only ever\n\
         that one: submitting under a panel-mate's angle overwrites their verdict. The\n\
         remaining arguments are:\n\n\
         ```json\n{schema}\n```\n\n\
         `severity` is one of `critical` | `important` | `nit`. Omit `angle` inside each\n\
         finding — drovr stamps it from the angle you submit under. Report only issues\n\
         introduced or exposed by this change; a clean review is `{{\"verdict\":\"clean\",\"findings\":[]}}`.\n\n\
         ## Finish\n\n\
         **That tool call IS your review, and it is the only channel drovr reads.**\n\
         Your pane output is never parsed, so a review you only print is a review you did\n\
         not deliver: it is discarded and your reviewer is respawned from scratch. Call\n\
         `submit_findings` exactly once, as soon as your review is complete. If it comes\n\
         back with an error, read it, fix the arguments and call it again — you are still\n\
         running and can still correct yourself. Afterwards you may summarise your\n\
         reasoning in prose, for the human.\n\n\
         You cannot write files, and do not need to: the tool performs drovr's one write\n\
         on your behalf. That call is the sanctioned way to deliver a review from\n\
         read-only mode — drovr provisioned the tool for exactly this and expects it, so\n\
         do not stop to ask permission for it.\n\
         Do not modify any files or run `drovr phase done`.\n",
        brief = angle_brief(angle),
        schema = findings_schema(),
        tool = crate::mcp_findings::TOOL_NAME,
        qualified_tool = crate::mcp_findings::qualified_tool_name(),
    )
}

/// The one server every reviewer of `task` is given: `drovr mcp-findings <run>
/// <task>` over stdio, exposing `submit_findings` and nothing else.
///
/// All four angles share it — cursor has no per-launch MCP scoping, and the angle
/// is a validated tool argument rather than argv precisely because of that. The
/// ITERATION is on the command line, though: it is drovr's, not the reviewer's, and it
/// is what keeps one pass's verdicts out of the next one's harvest.
fn findings_server(run_name: &str, task: &str, iter: u64) -> serde_json::Value {
    // Same binary that spawned the panel, so a reviewer cannot end up talking to a
    // different drovr on `$PATH`. The bare name is a last resort.
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "drovr".to_owned());
    serde_json::json!({
        "command": exe,
        "args": ["mcp-findings", run_name, task, iter.to_string()],
    })
}

/// Where the original of a replaced project config is kept.
fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".drovr-backup");
    PathBuf::from(name)
}

/// Write drovr's findings server into `path` as the **only** server there.
///
/// # Why it replaces rather than merges
///
/// This used to merge, preserving whatever else the file held. That is wrong for the
/// mechanism it serves: a `ProjectFile` backend is handed `--approve-mcps`, which
/// auto-approves **every** server in the file — drovr cannot approve selectively. So
/// any entry drovr preserved would be silently approved for a read-only reviewer, and
/// `.cursor/mcp.json` is a path a hostile repository can simply commit. That hands the
/// reviewer arbitrary extra tools and defeats the one-tool carve-out this whole
/// mechanism exists to enforce.
///
/// The user's own config is not destroyed: if the file held anything other than
/// drovr's server, the original is moved to `<path>.drovr-backup` first (never
/// overwriting an existing backup, so repeated passes cannot lose it) and the
/// displacement is reported. For a `ConfigFlag` backend the path is inside drovr's own
/// run dir, which drovr owns outright — the same rule costs nothing there.
fn write_mcp_config(path: &Path, run_name: &str, task: &str, iter: u64) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    refuse_if_not_a_regular_file(path)?;

    // NotFound is the ordinary case; anything else is a real failure. Collapsing them
    // would let a permissions or IO error look like "no file here", and drovr would
    // then replace a config it could not read — and could not have backed up.
    let existing = match std::fs::read_to_string(path) {
        Ok(s) => Some(s),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => {
            return Err(io::Error::new(
                e.kind(),
                format!(
                    "cannot read the existing MCP config at {}: {e}",
                    path.display()
                ),
            ));
        }
    };

    if let Some(body) = &existing
        && holds_more_than_drovrs_server(body)
    {
        let backup = backup_path(path);
        if backup.exists() {
            eprintln!(
                "code-review: {} already exists; leaving it alone and replacing {} again",
                backup.display(),
                path.display()
            );
        } else {
            std::fs::rename(path, &backup)?;
            eprintln!(
                "code-review: {} configured MCP servers that a read-only reviewer must \
                 not be given (`--approve-mcps` approves every server in that file). \
                 The original is at {}; drovr's findings server is the only one the \
                 reviewers see.",
                path.display(),
                backup.display()
            );
        }
    }

    let doc = serde_json::json!({
        "mcpServers": {crate::mcp_findings::SERVER_NAME: findings_server(run_name, task, iter)},
    });
    std::fs::write(
        path,
        serde_json::to_string_pretty(&doc).map_err(io::Error::other)?,
    )
}

/// True when `body` configures anything beyond drovr's own findings server — the
/// signal that replacing the file would displace something worth keeping. An
/// unparseable file counts: it is not drovr's, and it is not ours to discard silently.
fn holds_more_than_drovrs_server(body: &str) -> bool {
    let Ok(doc) = serde_json::from_str::<serde_json::Value>(body) else {
        return !body.trim().is_empty();
    };
    let Some(servers) = doc.get("mcpServers").and_then(|s| s.as_object()) else {
        // A JSON document with no `mcpServers` at all is not an MCP config; if it has
        // any content, it is something else the user cared about.
        return doc.as_object().is_some_and(|o| !o.is_empty());
    };
    servers
        .keys()
        .any(|k| k != crate::mcp_findings::SERVER_NAME)
}

/// Refuse to write through a symlink (or anything that is not a regular file).
///
/// `.cursor/mcp.json` sits inside the checkout under review, and a repository can
/// commit a symlink at that path — `fs::write` would follow it and drop drovr's config
/// wherever it points, outside the project entirely. The parent is checked too, since
/// a symlinked `.cursor/` redirects the write just as effectively. The same
/// untrusted-repo boundary as `docs/supply-chain.md`.
fn refuse_if_not_a_regular_file(path: &Path) -> io::Result<()> {
    let mut suspects = vec![path.to_path_buf()];
    if let Some(parent) = path.parent() {
        suspects.push(parent.to_path_buf());
    }
    for p in suspects {
        match std::fs::symlink_metadata(&p) {
            // Only the final component may be absent; a missing parent was just
            // created by `create_dir_all`.
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
            Ok(md) => {
                if md.file_type().is_symlink() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!(
                            "refusing to write drovr's MCP config through the symlink at \
                             {} — a repository must not be able to redirect it",
                            p.display()
                        ),
                    ));
                }
                if p == path && !md.is_file() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!("{} exists and is not a regular file", p.display()),
                    ));
                }
            }
        }
    }
    Ok(())
}

/// `git -C <project_dir> rev-parse --git-common-dir`, absolutised. The *common*
/// dir, not `--git-dir`: in a linked worktree the per-worktree gitdir is not where
/// git reads `info/exclude` from.
fn git_common_dir(project_dir: &str) -> Option<std::path::PathBuf> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(project_dir)
        .args(["rev-parse", "--git-common-dir"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let raw = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    if raw.is_empty() {
        return None;
    }
    let p = std::path::PathBuf::from(&raw);
    Some(if p.is_absolute() {
        p
    } else {
        Path::new(project_dir).join(p)
    })
}

/// Keep a file drovr wrote into the project out of git.
///
/// `.git/info/exclude` rather than the tracked `.gitignore`: this is drovr's own
/// plumbing, not a change the user asked for, so it must not show up in their
/// diff — as an untracked file OR as an edit to a tracked one.
///
/// Best-effort: a stray untracked file is cosmetic, and refusing to review over it
/// would be worse than the mess. Failures are reported, not fatal.
///
/// APPENDED, never rewritten. This file belongs to the whole repository — the common
/// dir is shared by every worktree, so concurrent drovr runs write it — and a
/// read-modify-write would drop whatever another run (or the user) added in between.
/// The worst an append race can do is duplicate a line, which git does not mind.
fn exclude_locally(project_dir: &str, rel: &str) {
    let Some(git_dir) = git_common_dir(project_dir) else {
        return;
    };
    let info = git_dir.join("info");
    let path = info.join("exclude");
    let current = std::fs::read_to_string(&path).unwrap_or_default();
    if current.lines().any(|l| l.trim() == rel) {
        return;
    }
    // A file that does not end in a newline would otherwise absorb `rel` into its
    // last line, turning two patterns into one nonsense pattern.
    let entry = if current.is_empty() || current.ends_with('\n') {
        format!("{rel}\n")
    } else {
        format!("\n{rel}\n")
    };
    let appended = std::fs::create_dir_all(&info).and_then(|()| {
        use std::io::Write as _;
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .and_then(|mut f| f.write_all(entry.as_bytes()))
    });
    if let Err(e) = appended {
        eprintln!(
            "code-review: could not add '{rel}' to {} ({e}); it will show up as an \
             untracked file",
            path.display()
        );
    }
}

/// Obtain one reviewer's findings: read the file its `submit_findings` call had
/// drovr write. That file is the ONLY channel findings enter drovr through.
///
/// The panel deliberately does NOT read pane transcripts. A transcript is a rendered
/// terminal view, not a data channel: renderers hard-wrap long lines — inserting raw
/// newlines *inside* JSON string literals, which no parser can accept — collapse long
/// tool output behind "N lines hidden", and need not show fence markers at all. So a
/// reviewer that finished correctly can be discarded as unparseable, while the schema
/// example echoed in every seed can be harvested as a verdict. Scraping cannot be made
/// correct, so it is not attempted.
///
/// A missing file is a real failure — the reviewer finished without ever calling the
/// tool: the caller marks that angle `Failed`, so the next resume replaces the
/// reviewer instead of waiting on it forever. The content is re-validated even though
/// the server validates before writing, because the file outlives the call that made
/// it (a truncated write, a stale leftover) and a bad verdict must never merge.
fn obtain_findings_json(
    dir: &Path,
    task: &str,
    iter: u64,
    angle: &str,
    phase_name: &str,
) -> io::Result<String> {
    let path = findings_path(dir, task, iter, angle);
    // Only NotFound means "never submitted". A file that exists but cannot be read
    // (permissions, EIO) is a different failure with a different remedy, and reporting
    // it as a silent reviewer sends whoever is debugging to the wrong place entirely.
    let json = std::fs::read_to_string(&path).map_err(|e| match e.kind() {
        io::ErrorKind::NotFound => io::Error::other(format!(
            "reviewer '{phase_name}' produced no findings (it never called \
             submit_findings, so nothing reached {})",
            path.display()
        )),
        _ => io::Error::new(
            e.kind(),
            format!(
                "reviewer '{phase_name}' submitted findings to {}, but they could not \
                 be read back: {e}",
                path.display()
            ),
        ),
    })?;
    // Validate here so a truncated or half-written file is reported against the
    // reviewer that produced it, rather than as a confusing merge error later.
    parse_review(&json).map_err(|e| {
        io::Error::other(format!(
            "reviewer '{phase_name}' left unparseable findings at {}: {e}",
            path.display()
        ))
    })?;
    Ok(json)
}

/// One angle's delivered verdict, if it has actually been delivered.
///
/// `Some` is the panel's definition of "this angle is finished", and it is deliberately
/// the ONLY definition that can complete an angle. The pane is not a data channel and it
/// is not a completion channel either: herdr reports `done` as a momentary EDGE at the
/// end of a turn and then settles to `idle`, so a poll that misses that instant could
/// never recover it — the angle waited on a signal that would never fire again while its
/// finished review sat on disk. Observed live, 2026-07-27: three of four angles noticed,
/// the fourth stuck `Running` across two whole passes.
///
/// `None` means "nothing delivered *yet*", and it covers a file that exists but does not
/// parse as well as one that is absent. That is what stops a write in flight being read
/// as a completion: an unparseable file is not evidence of anything, so the caller keeps
/// waiting rather than banking a torn verdict. (The server writes atomically — see
/// [`crate::mcp_findings`] — so this is the second guard, not the only one.)
fn delivered_review(dir: &Path, task: &str, iter: u64, angle: &str) -> Option<Review> {
    let json = std::fs::read_to_string(findings_path(dir, task, iter, angle)).ok()?;
    parse_review(&json).ok()
}

/// Clear an angle's findings file when its reviewer is REPLACED.
///
/// A replacement runs in the SAME iteration as the reviewer it replaces (that is what
/// makes it a respawn rather than a new panel), so the two share a filename and
/// whatever the outgoing one left behind is indistinguishable from what the new one
/// writes. Clearing at respawn stops a dead reviewer's verdict being passed off as its
/// replacement's. Cross-iteration staleness is handled by the filename itself — see
/// [`crate::mcp_findings::findings_path`].
///
/// **Failure is fatal to the pass, not ignorable.** This delete is the only thing
/// standing between a leftover verdict and a replacement that never submits; if it
/// cannot be removed, spawning the replacement would set up exactly the
/// misattribution the respawn exists to avoid. A `NotFound` file is already in the
/// state we want.
fn clear_findings_file(dir: &Path, task: &str, iter: u64, angle: &str) -> io::Result<()> {
    let path = findings_path(dir, task, iter, angle);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(io::Error::new(
            e.kind(),
            format!(
                "cannot clear the replaced reviewer's findings at {} ({e}); its \
                 replacement would inherit them",
                path.display()
            ),
        )),
    }
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
    fresh: bool,
    context: Option<&str>,
) -> io::Result<ReviewOutcome> {
    let dir = run_dir(&run.name);

    // Archived means the human filed this run away and `workspace_close` destroyed
    // its panes. Refuse before anything is spawned. This check is LOAD-BEARING, not
    // defence in depth: `spawn_reviewer` re-provisions a destroyed workspace since
    // 2026-08-02 (`phase::ensure_workspace`), so nothing downstream would stop a
    // panel from running on an archived run — and a run whose close merely failed
    // still has live panes, where we would happily resume, harvest findings and
    // flip phases to Done on a run the UI shows as archived.
    //
    // `ensure_workspace` carries its own archived guard for the same reason. Both
    // are wanted: this one refuses BEFORE the panel does any work, and reports it
    // as a review outcome rather than an io error.
    //
    // `refresh_archived`, not `run.archived` — the flag's authority is `state.json`
    // (see `RunState::archived`), and this function in particular is why: it holds
    // one `RunState` across a 30-minute wait, so its copy is exactly the one most
    // likely to be stale. Reading the field directly here while `ensure_workspace`
    // re-read disk would make `RunState.archived` mean different things depending
    // on which launch API you entered through. A read failure refuses too: an
    // unreadable state.json is not permission to spawn a panel.
    match run.refresh_archived() {
        Ok(true) => {
            eprintln!("code-review: {}", archived_run_error(&run.name));
            return Ok(ReviewOutcome::Error);
        }
        Ok(false) => {}
        Err(e) => {
            eprintln!(
                "code-review: run '{}': cannot read state.json to check whether it was \
                 archived, so no panel will be spawned: {e}",
                run.name
            );
            return Ok(ReviewOutcome::Error);
        }
    }

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

    // An empty range is refused, not reviewed. Every angle would come back clean
    // having examined no committed change — exit 0, the code the pipeline advances a
    // task on. That vacuous pass is indistinguishable from a real one downstream, which
    // is why it is refused here rather than annotated: a warning printed next to a
    // `clean` verdict and a 0 exit is read as the verdict.
    //
    // The seed does put the working tree in scope alongside the diff, so "empty range"
    // is not always "nothing exists to review" — but it is always a mistake worth
    // stopping for, because the committed scope the panel is built around is empty and
    // uncommitted work is not reliably reached (untracked files never appear in a
    // `git diff`, and this is exactly how the observed vacuous pass happened).
    //
    // Refused BEFORE any reviewer is spawned: four panes that can only report on
    // nothing are pure cost, and the caller needs the diagnosis, not a verdict.
    match range_is_empty(&run.project_dir, &base, &head) {
        Ok(true) => {
            let same = if base == head {
                " (base == HEAD)"
            } else {
                " (the commits differ but their trees do not — an empty commit)"
            };
            eprintln!(
                "code-review: empty review range for '{task}': `git diff {base}..{head}` \
                 contains no change{same}. There is nothing to review, so no verdict about \
                 it would mean anything. Either commit this task's work (uncommitted \
                 changes are not reliably reviewed) or re-record the base with \
                 `drovr code-review base` if it was recorded after the work landed."
            );
            return Ok(ReviewOutcome::EmptyRange);
        }
        Ok(false) => {}
        // "Could not tell" is REFUSED, not waved through. An earlier version of this
        // guard warned and proceeded, which made it bypassable: any base git cannot
        // resolve skipped the check entirely and the panel returned the same vacuous
        // verdict the guard exists to prevent. A guard that fails open is not a guard —
        // and this one failed open into precisely the class it was built to close.
        //
        // So the only paths out of here are: proven non-empty (review it), proven empty
        // (EmptyRange), or unknown (Error). There is no fourth path in which a verdict
        // gets produced without establishing that there is something to have a verdict
        // about.
        Err(e) => {
            eprintln!(
                "code-review: cannot determine whether {base}..{head} contains any \
                 change for '{task}' ({e}). Refusing rather than reviewing: an \
                 unverifiable range cannot produce a meaningful verdict. Check that the \
                 recorded base exists in this repository (`git -C <project> cat-file -e \
                 {base}`), and re-record it with `drovr code-review base` if it does not."
            );
            return Ok(ReviewOutcome::Error);
        }
    }

    // Resolved once per pass, so every angle in this panel — and every angle a later
    // resume respawns — is briefed identically.
    //
    // A failure here is a SETUP failure in the same class as a missing base or an
    // unreadable HEAD, so it takes the same channel: print why, return `Error` (exit 1 via
    // the outcome) rather than an `Err` that would exit 1 through a different path and read
    // as an internal fault. It happens BEFORE any reviewer is spawned, so nothing is left
    // half-started.
    let context = match resolve_context(&dir, task, context) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("code-review: cannot resolve the review context for '{task}' ({e})");
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
    std::fs::create_dir_all(&dir)?;

    // Check the findings channel BEFORE deciding anything else: a reviewer is
    // read-only, so this tool is the only way it can deliver anything at all.
    // Without a mechanism to hand it over, every reviewer would run to completion
    // and then be discarded — fail here instead, while the reason is still legible.
    let mcp = cfg.mcp_delivery(&review_agent)?.ok_or_else(|| {
        io::Error::other(format!(
            "review agent '{review_agent}' has no `mcp` mechanism configured, so its \
             reviewers would have no way to submit findings; configure \
             `[agents.{review_agent}.mcp]` or pick another review_agent"
        ))
    })?;

    // Resume, or open a new panel? A plain re-run after a timeout re-attaches to the
    // reviewers still in flight — spawning a second panel over the same diff would
    // double the token spend and throw away every angle that had already finished.
    let resumed = match if fresh {
        None
    } else {
        resumable_iter(run, task, &cfg.angles)
    } {
        Some(prev) => {
            // NotFound is the only readable "cannot verify the scope" — an old panel
            // that predates the head record, which legitimately starts fresh. Any other
            // IO error is a real failure, and collapsing it into the same `None` made
            // drovr announce "HEAD moved" and abandon a panel of four live reviewers for
            // a reason that was never true. Say what actually happened instead.
            let head_path = iter_head_path(&dir, task, prev);
            let seeded = match std::fs::read_to_string(&head_path) {
                Ok(s) => Some(s.trim().to_owned()),
                Err(e) if e.kind() == io::ErrorKind::NotFound => None,
                Err(e) => {
                    return Err(io::Error::new(
                        e.kind(),
                        format!(
                            "cannot read the head record for review iteration {prev} at \
                             {} ({e}); refusing to guess whether its reviewers are still \
                             reviewing the current diff",
                            head_path.display()
                        ),
                    ));
                }
            };
            if seeded.as_deref() == Some(head.as_str()) {
                Some(prev)
            } else {
                // The implementer committed while the panel was pending, so those
                // reviewers are reading a diff that no longer stands.
                println!(
                    "code-review: HEAD moved since review iteration {prev} was seeded \
                     — starting a fresh panel instead of resuming it"
                );
                None
            }
        }
        None => None,
    };
    let iter = resumed.unwrap_or_else(|| next_iter(run, task));
    if resumed.is_none() {
        crate::brief::write_no_follow(&iter_head_path(&dir, task, iter), &format!("{head}\n"))?;
    }

    // Provision the findings channel now that the iteration is known — the server
    // writes into `<task>-review-<iter>-<angle>.json`, so it has to be told which
    // pass it is serving. A resume rewrites the same config; a fresh panel points its
    // reviewers at a new iteration's files, which is what stops them harvesting the
    // last pass's verdicts. Reviewers still alive from a superseded iteration keep the
    // server they were launched with, so their late writes land in their own pass's
    // files and can never reach this one.
    let mcp_path = mcp.config_path(&dir, Path::new(&run.project_dir), task);
    write_mcp_config(&mcp_path, &run.name, task, iter)?;
    if let Some(rel) = mcp.project_relative_path() {
        exclude_locally(&run.project_dir, rel);
        exclude_locally(&run.project_dir, &format!("{rel}.drovr-backup"));
    }
    let launch = cfg.launch(&review_agent, &run.project_dir, true, Some(&mcp_path))?;

    // Split the angles: what is already banked from an earlier pass of this same
    // iteration, versus what still needs a reviewer waited on (or respawned).
    let mut banked: Vec<(String, Review)> = Vec::new();
    let mut pending: Vec<(String, String)> = Vec::new();
    for angle in &cfg.angles {
        let phase = crate::run::reviewer_phase_name(task, iter, angle);
        if resumed.is_some() {
            // A delivered verdict is banked on its own evidence — NOT gated on the
            // recorded status. Requiring `Done` here was half of a livelock: an angle
            // whose completion edge was missed stays `Running` forever, so this branch
            // never fired and the wait loop below then waited on a pane signal that
            // could never fire again. The file is the contract; if it parses, that
            // angle is in. An unreadable one banks nothing and falls through to wait
            // again, which self-heals rather than hard-failing.
            if let Some(review) = delivered_review(&dir, task, iter, angle) {
                // Make the record agree with what was delivered, so the phase does not
                // read `Running` forever in `state.json` and the web UI.
                if let Some(i) = run.review_phases.iter().position(|p| p.name == phase) {
                    run.review_phases[i].status = PhaseStatus::Done;
                }
                banked.push((angle.clone(), review));
                continue;
            }
            // Keep waiting only on a reviewer that can still deliver: registered,
            // pane present, and not already known to be unusable. A `Failed` angle
            // has a reviewer that was never seeded or whose output could not be
            // parsed — its pane may well still exist, but waiting on it again just
            // reproduces the same failure, so it needs a REPLACEMENT, not patience.
            let existing = run.find_phase(&phase);
            let failed = existing.is_some_and(|p| p.status == PhaseStatus::Failed);
            let alive = existing
                .and_then(|p| p.pane_id())
                .is_some_and(|pane| h.pane_exists(pane));
            if alive && !failed {
                pending.push((angle.clone(), phase));
                continue;
            }
            // Respawn in place, same iteration, below. Drop the stale registration
            // first so `find_phase` cannot resolve to the replaced pane — the spawn
            // must mint a new one rather than re-adopt the reviewer being replaced.
            let reason = match (existing.is_some(), failed) {
                (false, _) => "was never spawned this iteration",
                (true, true) => "produced nothing usable",
                (true, false) => "is gone",
            };
            // The dropped pane may well still be alive (a `Failed` angle is one
            // whose reviewer produced nothing usable, not necessarily one whose
            // pane died). Retire it so `drovr cleanup` still knows it is drovr's:
            // cleanup reaps only the panes this state file records, and treats
            // everything else in the workspace as the human's.
            if let Some(pane) = existing.and_then(|p| p.pane_id().map(str::to_owned)) {
                run.retire_pane(pane);
            }
            run.review_phases.retain(|p| p.name != phase);
            // Drop the outgoing reviewer's findings file so the replacement cannot
            // inherit it — a respawn stays in THIS iteration, so the two share a
            // filename. `?`: if the stale verdict cannot be removed, spawning a
            // replacement that might never submit would credit it with the dead
            // reviewer's conclusion.
            clear_findings_file(&dir, task, iter, angle)?;
            println!("code-review: reviewer for angle '{angle}' {reason} — respawning it");
        }

        // Seed + spawn one read-only reviewer, then inject its brief. Every reviewer
        // exits (drops its marker) before the implementer fixes anything, so the
        // single-writer invariant holds — the panel never has a reviewer alive while a
        // writer runs.
        let seed_path = dir.join(format!("{task}-review-{angle}-seed.md"));
        let seed_text = build_seed(
            task,
            angle,
            &base,
            &head,
            &run.task,
            &run.project_dir,
            context.as_deref(),
        );
        crate::brief::write_no_follow(&seed_path, &seed_text)?;
        // `launch` carries its own backend, so the reviewer cannot be recorded
        // as running an agent it was not launched with — that mismatch used to be
        // expressible, and it silently defeated session capture for any reviewer
        // `review_agent_for` put on a different agent than the run.
        spawn_reviewer(h, run, &phase, Some(&seed_path), &launch)?;
        // A `phase_send` failure ABORTS the pass (`?` → Err → the CLI's `Error`
        // exit) rather than continuing: a spawned-but-unseeded reviewer would never
        // write findings or drop a marker, so pressing on would only guarantee a
        // timeout. Any reviewer panes already spawned this pass are left running and
        // reclaimed at `drovr cleanup` (they are recorded in `review_phases`, so it
        // knows they are drovr's) — the codebase invariant is "never close a pane
        // mid-run" (mirrors `phase_start`).
        //
        // Mark it `Failed` first, though. `spawn_reviewer` has already registered the
        // phase as `Running` with a live pane, and the caller saves state even on the
        // error path — leaving it `Running` would make every later resume patiently
        // wait on an agent that was never given a task. `Failed` makes the next
        // resume replace it.
        if let Err(e) = phase_send(h, run, &phase, &seed_text) {
            if let Some(i) = run.review_phases.iter().position(|p| p.name == phase) {
                run.review_phases[i].status = PhaseStatus::Failed;
            }
            // Give every reviewer already seeded this pass one last poll before
            // abandoning the pass. They are live, working agents that this
            // function is about to stop looking at: the wait loop below never
            // runs, so without this their only capture was the readiness gate —
            // which routinely returns before herdr publishes the session. They
            // then exit, herdr drops it, and a later resume finds a reviewer it
            // cannot rehydrate.
            //
            // Best-effort by construction: `poll_phase_pane` cannot fail, and the
            // error being propagated is the one that matters.
            for (_, seeded) in &pending {
                poll_phase_pane(h, run, seeded);
            }
            return Err(e);
        }
        pending.push((angle.clone(), phase));
    }
    if let Some(prev) = resumed {
        println!(
            "code-review: resuming review iteration {prev} for '{task}' \
             ({} of {} angles already in — waiting on {})",
            banked.len(),
            cfg.angles.len(),
            pending
                .iter()
                .map(|(a, _)| a.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    // Private, `review_phases`-aware wait: poll every pending angle until all have
    // finished or the deadline passes. Each reviewer is harvested the moment it
    // finishes — banking it on disk BEFORE the status flips to `Done`, so a later
    // timeout can never leave a `Done` angle whose findings were never captured.
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let mut harvested: Vec<(String, Review)> = banked;
    loop {
        let mut still_pending: Vec<(String, String)> = Vec::new();
        for (angle, phase) in std::mem::take(&mut pending) {
            // POLL FIRST, unconditionally — before any completion decision, and
            // never as part of one. This loop is a REVIEWER's only long-lived poll,
            // and `poll_phase_pane` captures the session on the way past. herdr
            // publishes `agent_status` before `agent_session`, so `phase_send`'s
            // readiness gate routinely returns before the session exists — this loop
            // is where it shows up. Let any completion test run first and the capture
            // becomes reachable only on the SLOW path: an angle that has already
            // delivered, or that finished before this loop's first visit — entirely
            // normal, since angles are spawned and seeded one at a time — `continue`s
            // out with its one capture skipped, and herdr drops the session when the
            // agent exits. A capture a fast agent can outrun is not a capture.
            //
            // This is a session capture, NOT a completion signal: the status it
            // returns is consulted below for one question only (see there).
            let status = poll_phase_pane(h, run, &phase).and_then(|info| info.agent_status);
            // DELIVERY FIRST, and on its own. A parseable findings file for this
            // iteration is complete evidence that the angle is done — it is the one
            // thing a reviewer is asked to produce, and drovr wrote it itself. Asking
            // the pane first is what hung the panel: herdr's `done` is a momentary edge
            // at the end of a turn, and a reviewer sitting at its prompt reports
            // `idle`, so a missed edge stranded a finished review forever.
            if let Some(review) = delivered_review(&dir, task, iter, &angle) {
                if let Some(i) = run.review_phases.iter().position(|p| p.name == phase) {
                    run.review_phases[i].status = PhaseStatus::Done;
                }
                harvested.push((angle, review));
                continue;
            }
            // Nothing delivered. NOW the pane matters — but only to answer a different
            // question: has this reviewer finished WITHOUT delivering? That is the one
            // thing the artifact cannot tell us, and the only reason to consult herdr.
            let finished =
                done_marker(&run.name, &phase).exists() || status == Some(AgentStatus::Done);
            if !finished {
                still_pending.push((angle, phase));
                continue;
            }
            // It finished and delivered nothing usable, and re-reading the same file
            // will fail identically forever. Record `Failed` so the next resume
            // replaces the reviewer, then surface the error — an angle that delivered
            // nothing must not pass for a clean one.
            let harvest = obtain_findings_json(&dir, task, iter, &angle, &phase)
                .and_then(|json| parse_review(&json));
            if let Some(i) = run.review_phases.iter().position(|p| p.name == phase) {
                run.review_phases[i].status = PhaseStatus::Failed;
            }
            harvested.push((angle, harvest?));
        }
        pending = still_pending;
        if pending.is_empty() {
            break;
        }
        let now = Instant::now();
        if now >= deadline {
            // Leave the outstanding reviewers `Running` and their findings banked: a
            // plain re-run resumes this same iteration and waits only on these.
            run.save_preserving_archived()?;
            println!(
                "code-review: {} of {} angles finished; still waiting on {}",
                harvested.len(),
                cfg.angles.len(),
                pending
                    .iter()
                    .map(|(a, _)| a.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            return Ok(ReviewOutcome::Timeout);
        }
        thread::sleep(POLL_INTERVAL.min(deadline - now));
    }
    run.save_preserving_archived()?;

    // Every angle in → merge in configured order (harvest order is completion order,
    // which is nondeterministic) and write the merged review.
    let mut per_angle: Vec<(String, Review)> = Vec::with_capacity(cfg.angles.len());
    for angle in &cfg.angles {
        if let Some(i) = harvested.iter().position(|(a, _)| a == angle) {
            per_angle.push(harvested.remove(i));
        }
    }
    let merged = merge_reviews(per_angle);
    let out_path = dir.join(format!("{task}-review.json"));
    crate::brief::write_no_follow(
        &out_path,
        &serde_json::to_string_pretty(&merged).map_err(io::Error::other)?,
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
    use crate::findings::Verdict;
    use crate::herdr::{FakeHerdr, PaneInfo, SessionId};
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
            archived: false,
            retired_panes: vec![],
        };
        (run, repo)
    }

    /// Record a review base the way a real task does: capture HEAD, then commit work on
    /// top, so `base..HEAD` is a resolvable, NON-EMPTY range.
    ///
    /// This used to write the literal `deadbeef`, which git cannot resolve. That made
    /// every test using it exercise a range the panel could not compute — invisible
    /// while an unresolvable base was quietly reviewed anyway, and a mass failure the
    /// moment the range guard started refusing what it cannot verify. The fixture was
    /// asserting against a state that cannot occur in a healthy run.
    fn write_base(run: &RunState, task: &str) {
        let dir = run_dir(&run.name);
        std::fs::create_dir_all(&dir).unwrap();
        let base = head_sha(&run.project_dir).unwrap();
        std::fs::write(dir.join(format!("{task}-base.sha")), format!("{base}\n")).unwrap();
        commit_more(run);
    }

    /// Stand in for a reviewer of `iter` having called `submit_findings`. The
    /// iteration is explicit because the file is scoped to one: seeding the wrong
    /// iteration is the bug this naming exists to make impossible, so a fixture must
    /// not be able to elide it.
    fn seed_angle_file(run: &RunState, task: &str, iter: u64, angle: &str, body: &str) {
        let dir = run_dir(&run.name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            crate::mcp_findings::findings_path(&dir, task, iter, angle),
            body,
        )
        .unwrap();
    }

    /// Pre-drop the done markers for iter=1's four default angles (the panel spawns
    /// them, then the very first wait poll sees the markers and completes).
    fn drop_markers(run: &RunState, task: &str, iter: u64) {
        for a in ["correctness", "security", "error-handling", "type-design"] {
            drop_marker(run, task, iter, a);
        }
    }

    /// Drop the done marker for ONE angle — models a reviewer that finished while
    /// its panel-mates are still working (the resume path's whole reason to exist).
    fn drop_marker(run: &RunState, task: &str, iter: u64, angle: &str) {
        let name = crate::run::reviewer_phase_name(task, iter, angle);
        let m = done_marker(&run.name, &name);
        std::fs::create_dir_all(m.parent().unwrap()).unwrap();
        std::fs::write(&m, b"").unwrap();
    }

    /// Advance HEAD in the run's project dir, so a resume must notice the diff it
    /// would be resuming into is no longer the one the reviewers were seeded with.
    fn commit_more(run: &RunState) {
        let git = |args: &[&str]| {
            let out = Command::new("git")
                .arg("-C")
                .arg(&run.project_dir)
                .args(args)
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?}");
        };
        // APPEND, never overwrite: `write_base` now calls this too, so a test can reach
        // it twice, and re-writing identical content produces "nothing to commit" and a
        // failed fixture rather than a moved HEAD.
        let p = std::path::Path::new(&run.project_dir).join("g.txt");
        let prev = std::fs::read_to_string(&p).unwrap_or_default();
        std::fs::write(&p, format!("{prev}more work\n")).unwrap();
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "more"]);
    }

    fn pane_of(run: &RunState, phase: &str) -> String {
        run.find_phase(phase)
            .and_then(|p| p.pane_id().map(str::to_owned))
            .unwrap_or_else(|| panic!("phase {phase} has no pane"))
    }

    fn spawn_count(h: &FakeHerdr) -> usize {
        h.calls().iter().filter(|c| c.contains("pane_run")).count()
    }

    const CLEAN: &str = r#"{"verdict":"clean","findings":[]}"#;

    #[test]
    fn an_archived_run_is_refused_before_any_reviewer_is_spawned() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-archived");
        write_base(&run, "task-1");
        // The human filed this run away: `workspace_close` already destroyed its
        // workspace, and nothing recreates a closed one.
        run.archived = true;

        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Error,
            "an archived run's workspace is gone; a review must not start against it"
        );
        assert_eq!(
            spawn_count(&h),
            0,
            "no reviewer may be spawned into an archived run"
        );
        assert!(
            run.review_phases.is_empty(),
            "a refused review must not record phases either"
        );
    }

    /// Write the run's ACTUAL current HEAD as the review base, reproducing the state
    /// that produced the vacuous pass: `drovr code-review base` recorded, then nothing
    /// committed before the panel ran.
    fn write_base_at_head(run: &RunState, task: &str) {
        let dir = run_dir(&run.name);
        std::fs::create_dir_all(&dir).unwrap();
        let head = head_sha(&run.project_dir).unwrap();
        std::fs::write(dir.join(format!("{task}-base.sha")), format!("{head}\n")).unwrap();
    }

    /// The defect this whole branch documents, in its sharpest form: on
    /// `skill-stickiness` task 3, `task-3-base.sha` and `task-3-review-1.head` were both
    /// `5c8a7da`, four angles returned `clean`, and the agent nearly shipped on it.
    ///
    /// A clean verdict is what the pipeline advances a task on, so a verdict about an
    /// empty range must not be spellable as one.
    #[test]
    fn an_empty_review_range_is_refused_before_any_reviewer_is_spawned() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-empty-range");
        write_base_at_head(&run, "task-1");

        let outcome = code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap();

        // Deliberately NOT also asserting `outcome != Clean`: it cannot fail once the
        // line above passes, and an assertion that cannot fail is the exact class of
        // defect this test exists for.
        assert_eq!(
            outcome,
            ReviewOutcome::EmptyRange,
            "base == HEAD is an empty range; it must be refused, not reviewed — and \
             never reported as Clean, the outcome the pipeline advances a task on"
        );
        assert_eq!(
            spawn_count(&h),
            0,
            "no reviewer may be spawned for a range that contains nothing"
        );
        assert!(
            run.review_phases.is_empty(),
            "a refused pass must not record phases either"
        );
    }

    /// `<task>-base.sha` is an ordinary file whose contents become a `git` argument. A
    /// value crafted to look like an option must never reach git — and because the
    /// composed argument is `{base}..{head}`, `--output=<path>` would have git WRITE to a
    /// path the file chose. The assertion is therefore not just "refused" but "that file
    /// does not exist": proof the value never got as far as git.
    #[test]
    fn a_crafted_base_is_rejected_before_it_reaches_git() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, repo) = make_run("cr-crafted-base");
        let pwned = repo.path().join("pwned");
        let dir = run_dir(&run.name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("task-1-base.sha"),
            format!("--output={}\n", pwned.display()),
        )
        .unwrap();

        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Error,
            "a base that is not an object name must be refused, not passed to git"
        );
        assert!(
            !pwned.exists(),
            "git was invoked with the crafted value: it wrote {}",
            pwned.display()
        );
        assert_eq!(spawn_count(&h), 0, "nothing may be spawned for a bad base");
    }

    /// The bypass: a base git cannot resolve makes the emptiness check ERROR, and an
    /// earlier version of the guard warned and carried on — so any unresolvable base
    /// skipped the guard entirely and the panel produced the vacuous verdict the guard
    /// exists to prevent. A guard that fails open is not a guard.
    ///
    /// The value here is well-formed hex (so it passes validation) but absent from the
    /// repository, which is exactly the gap between "looks like an object name" and "is
    /// one" — and the reason both halves of the fix are needed.
    #[test]
    fn an_unresolvable_base_is_refused_rather_than_reviewed() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-unresolvable-base");
        let dir = run_dir(&run.name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("task-1-base.sha"), "a".repeat(40)).unwrap();

        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Error,
            "if emptiness cannot be established the panel must refuse; assuming \
             non-empty is how the guard becomes bypassable"
        );
        assert_eq!(
            spawn_count(&h),
            0,
            "no reviewer may be spawned for a range that cannot be computed"
        );
    }

    /// Advance HEAD without changing the tree. `git commit --allow-empty` is the case
    /// that separates "the range contains nothing" from "the two SHAs are equal" — the
    /// property the first version of this guard got wrong.
    fn commit_empty(run: &RunState) {
        let out = Command::new("git")
            .arg("-C")
            .arg(&run.project_dir)
            .args(["commit", "-q", "--allow-empty", "-m", "empty"])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git commit --allow-empty: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// `base != head` and yet there is NOTHING to review. A guard that compares the two
    /// SHAs waves this straight through and returns the same vacuous `Clean` the whole
    /// entry is about; only a guard that asks what the range CONTAINS catches it.
    ///
    /// This is the test that proves the guard checks the right property, so it is worth
    /// more than the equal-SHA case it generalises.
    #[test]
    fn an_empty_commit_is_refused_even_though_the_shas_differ() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-empty-commit");
        write_base_at_head(&run, "task-1");
        commit_empty(&run);

        let base = std::fs::read_to_string(run_dir(&run.name).join("task-1-base.sha")).unwrap();
        let head = head_sha(&run.project_dir).unwrap();
        assert_ne!(
            base.trim(),
            head,
            "fixture must actually move HEAD, or this test proves nothing"
        );

        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::EmptyRange,
            "an empty commit advances HEAD without adding anything to review; \
             differing SHAs are not a non-empty range"
        );
        assert_eq!(
            spawn_count(&h),
            0,
            "no reviewer may be spawned for a range that contains nothing"
        );
    }

    /// The guard must key on what the range contains, not on "did the caller commit
    /// recently" — and it must let a real range through untouched. Mutation check: this
    /// is the test that goes red if the emptiness check is inverted or widened.
    #[test]
    fn a_non_empty_range_still_reaches_the_reviewers() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-nonempty-range");
        write_base_at_head(&run, "task-1");
        // One commit is the entire difference between the refused case above and this
        // one; nothing else about the fixture changes.
        commit_more(&run);
        drop_markers(&run, "task-1", 1);
        for a in ["correctness", "security", "error-handling", "type-design"] {
            seed_angle_file(&run, "task-1", 1, a, CLEAN);
        }

        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Clean,
            "a real range with no blocking findings is still Clean"
        );
        assert_eq!(
            spawn_count(&h),
            4,
            "one reviewer per configured angle, as before the guard"
        );
    }

    #[test]
    fn archiving_mid_run_survives_every_save_the_review_makes() {
        // `into_inner` on poison: this test's assert is the whole point, and a
        // real failure here must not cascade PoisonError into every other test
        // sharing the lock — that turns one honest failure into ~11 misleading ones.
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-archive-mid-run");
        write_base(&run, "task-1");
        run.save().unwrap();

        // The human clicks Archive in the web UI while reviewers are being
        // spawned — i.e. after `code_review_run`'s entry guard has already passed
        // and while it holds a copy of the state that still says `archived: false`.
        h.archive_on_call("tab_create", "cr-archive-mid-run");

        let _ = code_review_run(&h, &mut run, "task-1", 40, false, None);

        assert!(
            RunState::load("cr-archive-mid-run").unwrap().archived,
            "no save made by a review pass may un-archive a run the human filed \
             away mid-flight — its workspace is already destroyed"
        );
    }

    #[test]
    fn archiving_during_a_resumed_poll_survives_the_deadline_save() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-archive-resumed-poll");
        write_base(&run, "task-1");
        run.save().unwrap();

        // First pass spawns the reviewers and times out, leaving them Running.
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );
        let spawned = spawn_count(&h);

        // The resume respawns NOTHING — every angle is still alive — so the poll
        // loop's own `agent_status` fallback (it fires whenever the done-marker is
        // absent) is the only herdr call in the whole pass. No `spawn_reviewer`
        // save runs to rescue the flag first, which makes the deadline save the
        // one that has to preserve it.
        //
        // This is the case an earlier docs claim said could not exist. It can: a
        // human archiving a run while a resumed review polls is ordinary use.
        h.archive_on_call("agent_status", "cr-archive-resumed-poll");

        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );
        assert_eq!(
            spawn_count(&h),
            spawned,
            "the resume must not respawn, or this exercises the spawn save instead"
        );
        assert!(
            RunState::load("cr-archive-resumed-poll").unwrap().archived,
            "the deadline save must not un-archive a run filed away during the poll"
        );
    }

    #[test]
    fn archiving_during_a_resumed_pass_survives_the_final_save_too() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-archive-resumed-final");
        write_base(&run, "task-1");
        run.save().unwrap();

        // Pass one spawns and times out.
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );

        // Pass two resumes with no respawn, and this time the angles finish — so
        // the run reaches the FINAL save rather than the deadline one.
        //
        // The archive is hooked on `integration_present`, not the poll loop's
        // `agent_status`: a resume whose angles have all DELIVERED banks them from
        // their files and never polls a pane at all, so `agent_status` is no longer
        // reachable here. `integration_present` still lands where this test needs it —
        // after `code_review_run`'s archived entry guard has passed, and before every
        // save the pass makes. The race being guarded is unchanged: the human clicks
        // Archive while a pass holds state that still says `archived: false`.
        for a in ["correctness", "security", "error-handling", "type-design"] {
            seed_angle_file(&run, "task-1", 1, a, CLEAN);
        }
        h.archive_on_call("integration_present", "cr-archive-resumed-final");

        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 5_000, false, None).unwrap(),
            ReviewOutcome::Clean,
            "the pass must actually COMPLETE, or it exercises the deadline save instead"
        );
        assert!(
            RunState::load("cr-archive-resumed-final").unwrap().archived,
            "the final save must not un-archive a run filed away during the poll"
        );
    }

    #[test]
    fn rerun_after_timeout_resumes_the_same_iter_without_respawning() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-resume-same-iter");
        write_base(&run, "task-1");

        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );
        assert_eq!(
            spawn_count(&h),
            4,
            "first pass spawns one reviewer per angle"
        );
        let panes: Vec<String> = run
            .review_phases
            .iter()
            .map(|p| p.pane_id().map(str::to_owned).unwrap())
            .collect();

        // A plain re-run must RESUME iter 1 — not open a second panel on the same diff.
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );
        assert_eq!(
            run.review_phases.len(),
            4,
            "resume must not add phases: {:?}",
            run.review_phases
                .iter()
                .map(|p| &p.name)
                .collect::<Vec<_>>()
        );
        assert!(
            run.review_phases
                .iter()
                .all(|p| p.name.starts_with("review:task-1:1:")),
            "resume stays on iter 1"
        );
        assert_eq!(
            spawn_count(&h),
            4,
            "resume must not launch another reviewer while the live ones still exist"
        );
        let panes_after: Vec<String> = run
            .review_phases
            .iter()
            .map(|p| p.pane_id().map(str::to_owned).unwrap())
            .collect();
        assert_eq!(panes, panes_after, "resume re-attaches to the same panes");
    }

    #[test]
    fn resume_harvests_angles_that_already_finished() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-resume-harvest");
        write_base(&run, "task-1");

        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );

        // Two of the four reviewers have since finished.
        drop_marker(&run, "task-1", 1, "correctness");
        drop_marker(&run, "task-1", 1, "security");
        seed_angle_file(&run, "task-1", 1, "correctness", CLEAN);
        seed_angle_file(
            &run,
            "task-1",
            1,
            "security",
            r#"{"verdict":"changes","findings":[{"file":"a.rs","severity":"important","summary":"leak"}]}"#,
        );

        // Still Timeout (two stragglers), but the finished work is banked on disk.
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );

        let dir = run_dir(&run.name);
        for angle in ["correctness", "security"] {
            let p = crate::mcp_findings::findings_path(&dir, "task-1", 1, angle);
            assert!(
                p.exists(),
                "a finished angle's findings must be harvested on resume, not re-run: {}",
                p.display()
            );
        }
        assert!(
            parse_review(
                &std::fs::read_to_string(crate::mcp_findings::findings_path(
                    &dir, "task-1", 1, "security"
                ))
                .unwrap()
            )
            .unwrap()
            .findings
            .len()
                == 1,
            "harvest must persist the actual findings, not an empty stub"
        );
        let status = |name: &str| run.find_phase(name).map(|p| p.status.clone());
        assert_eq!(
            status("review:task-1:1:correctness"),
            Some(PhaseStatus::Done)
        );
        assert_eq!(status("review:task-1:1:security"), Some(PhaseStatus::Done));
        assert_eq!(
            status("review:task-1:1:type-design"),
            Some(PhaseStatus::Running),
            "a straggler stays Running so the next resume keeps waiting on it"
        );
    }

    #[test]
    fn resume_completing_the_last_stragglers_merges_every_angle() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-resume-completes");
        write_base(&run, "task-1");

        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );

        // First resume banks two angles, then times out on the other two.
        drop_marker(&run, "task-1", 1, "correctness");
        drop_marker(&run, "task-1", 1, "security");
        seed_angle_file(&run, "task-1", 1, "correctness", CLEAN);
        seed_angle_file(&run, "task-1", 1, "security", CLEAN);
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );

        // Second resume: the stragglers land. The merge must cover ALL FOUR angles,
        // including the two harvested during the earlier resume.
        drop_marker(&run, "task-1", 1, "error-handling");
        drop_marker(&run, "task-1", 1, "type-design");
        seed_angle_file(
            &run,
            "task-1",
            1,
            "error-handling",
            r#"{"verdict":"changes","findings":[{"file":"b.rs","severity":"critical","summary":"panic"}]}"#,
        );
        seed_angle_file(&run, "task-1", 1, "type-design", CLEAN);
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Findings
        );

        let merged = parse_review(
            &std::fs::read_to_string(run_dir(&run.name).join("task-1-review.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(merged.verdict, Verdict::Changes);
        assert_eq!(merged.findings.len(), 1);
        assert_eq!(merged.findings[0].angle, "error-handling");
        assert_eq!(spawn_count(&h), 4, "no angle was ever re-reviewed");
    }

    #[test]
    fn resume_respawns_a_reviewer_whose_pane_died() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-resume-respawn");
        write_base(&run, "task-1");

        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );
        let dead = pane_of(&run, "review:task-1:1:type-design");
        let survivor = pane_of(&run, "review:task-1:1:correctness");
        h.kill_pane(dead.clone());

        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );

        assert_eq!(
            run.review_phases.len(),
            4,
            "a respawn replaces the dead reviewer in place — same iter, same angle"
        );
        let respawned = pane_of(&run, "review:task-1:1:type-design");
        assert_ne!(
            respawned, dead,
            "the dead reviewer must be respawned into a fresh pane"
        );
        assert_eq!(
            pane_of(&run, "review:task-1:1:correctness"),
            survivor,
            "live reviewers must not be disturbed"
        );
        assert_eq!(
            spawn_count(&h),
            5,
            "exactly one extra launch: only the dead angle is respawned"
        );
    }

    /// A reviewer that launched but could never be given its brief must not be left
    /// `Running`: a `Running` phase with a live pane is exactly what resume waits on,
    /// so it would wait on an agent that was never asked anything — forever.
    #[test]
    fn a_reviewer_that_could_not_be_seeded_is_marked_failed() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-send-fails");
        write_base(&run, "task-1");
        h.fail_agent_send();

        let err = code_review_run(&h, &mut run, "task-1", 40, false, None)
            .expect_err("a reviewer that cannot be seeded must fail the pass loudly");
        assert!(err.to_string().contains("agent_send"), "surfaced: {err}");

        let phase = run
            .find_phase("review:task-1:1:correctness")
            .expect("the spawned reviewer stays registered so its pane is reclaimed");
        assert_eq!(
            phase.status,
            PhaseStatus::Failed,
            "an unseeded reviewer must be Failed, never Running — otherwise resume \
             waits on an agent that was never given a task"
        );
    }

    /// Unusable output is not a transient condition: the reviewer has finished, so
    /// re-reading the file it left fails identically every time. Such an angle must be
    /// marked `Failed` so a resume replaces the reviewer instead of retrying forever.
    #[test]
    fn an_unparseable_reviewer_result_marks_the_angle_failed() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-bad-json");
        write_base(&run, "task-1");
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );

        // correctness finishes, but writes a file that is not a Review.
        drop_marker(&run, "task-1", 1, "correctness");
        seed_angle_file(&run, "task-1", 1, "correctness", r#"{"not":"a review"}"#);

        let err = code_review_run(&h, &mut run, "task-1", 40, false, None)
            .expect_err("unparseable findings must fail the pass loudly");
        assert!(!err.to_string().is_empty());
        assert_eq!(
            run.find_phase("review:task-1:1:correctness")
                .unwrap()
                .status,
            PhaseStatus::Failed,
            "an angle whose output cannot be parsed must be Failed, so the next \
             resume respawns it rather than re-reading the same unusable file"
        );
    }

    #[test]
    fn resume_respawns_an_angle_whose_reviewer_failed() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-resume-failed");
        write_base(&run, "task-1");
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );

        // Model the wedged angle: its pane is alive, but it is marked Failed.
        let wedged = pane_of(&run, "review:task-1:1:security");
        let i = run
            .review_phases
            .iter()
            .position(|p| p.name == "review:task-1:1:security")
            .unwrap();
        run.review_phases[i].status = PhaseStatus::Failed;

        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );

        assert_eq!(
            run.review_phases.len(),
            4,
            "the failed reviewer is replaced in place, not added alongside"
        );
        assert_ne!(
            pane_of(&run, "review:task-1:1:security"),
            wedged,
            "a Failed angle must get a NEW reviewer even though its pane still exists"
        );
        assert_eq!(spawn_count(&h), 5, "only the failed angle is respawned");
        // The replaced reviewer's pane is still alive but no longer registered under
        // any phase. It must be retired, not forgotten: `drovr cleanup` reaps exactly
        // the panes the run records, so an unrecorded pane of drovr's would be left
        // running forever AND read as the human's, keeping the workspace open.
        assert!(
            run.retired_panes.contains(&wedged),
            "the replaced reviewer's pane must be retired for cleanup to reap: {:?}",
            run.retired_panes
        );
    }

    /// A leftover `Running` reviewer for an angle no longer in config must not make
    /// a finished iteration look resumable forever.
    #[test]
    fn a_leftover_for_an_unconfigured_angle_does_not_make_an_iter_resumable() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-unconfigured-leftover");
        write_base(&run, "task-1");
        for a in ["correctness", "security", "error-handling", "type-design"] {
            seed_angle_file(&run, "task-1", 1, a, CLEAN);
        }
        drop_markers(&run, "task-1", 1);
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 5_000, false, None).unwrap(),
            ReviewOutcome::Clean
        );

        // An angle that was dropped from config mid-run, still Running from an
        // earlier pass. The configured angles are all Done, so this pass is over.
        run.review_phases.push(
            {
                let mut p = Phase::new("review:task-1:1:performance");
                p.status = PhaseStatus::Running;
                p
            }
            .with_pane("pane-stale"),
        );

        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );
        assert!(
            run.review_phases
                .iter()
                .any(|p| p.name == "review:task-1:2:correctness"),
            "a completed pass must still start fresh; an unconfigured angle's \
             leftover must not hold the iteration open: {:?}",
            run.review_phases
                .iter()
                .map(|p| &p.name)
                .collect::<Vec<_>>()
        );
    }

    /// Self-heal: a `Done` angle whose banked JSON is unreadable must be waited on
    /// again rather than trusted or hard-failed.
    #[test]
    fn resume_rewaits_an_angle_whose_banked_findings_are_unreadable() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-banked-corrupt");
        write_base(&run, "task-1");
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );

        // Mark one angle Done but corrupt its banked file.
        let i = run
            .review_phases
            .iter()
            .position(|p| p.name == "review:task-1:1:correctness")
            .unwrap();
        run.review_phases[i].status = PhaseStatus::Done;
        seed_angle_file(&run, "task-1", 1, "correctness", "{ this is not json");

        // It must be waited on again (so: Timeout, still 4 phases, no respawn since
        // its pane is alive) — not trusted, and not a hard error.
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );
        assert_eq!(run.review_phases.len(), 4);
        assert_eq!(
            spawn_count(&h),
            4,
            "a live pane is re-waited on, not respawned"
        );
    }

    /// Without a recorded head we cannot prove the pending reviewers are reading the
    /// current diff, so the safe move is a fresh panel.
    #[test]
    fn a_missing_iter_head_record_starts_a_fresh_panel() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-no-head-record");
        write_base(&run, "task-1");
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );

        std::fs::remove_file(run_dir(&run.name).join("task-1-review-1.head")).unwrap();

        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );
        assert!(
            run.review_phases
                .iter()
                .any(|p| p.name == "review:task-1:2:correctness"),
            "an unverifiable scope must start fresh rather than resume blind"
        );
    }

    #[test]
    fn fresh_flag_starts_a_new_iteration() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-fresh-flag");
        write_base(&run, "task-1");

        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, true, None).unwrap(),
            ReviewOutcome::Timeout
        );

        assert_eq!(
            run.review_phases.len(),
            8,
            "--fresh abandons the iter-1 leftovers and opens iter 2"
        );
        assert!(
            run.review_phases
                .iter()
                .any(|p| p.name == "review:task-1:2:correctness"),
            "--fresh must bump the iteration: {:?}",
            run.review_phases
                .iter()
                .map(|p| &p.name)
                .collect::<Vec<_>>()
        );
        assert_eq!(spawn_count(&h), 8, "--fresh launches a whole new panel");
    }

    #[test]
    fn head_moving_forces_a_fresh_iteration_instead_of_resuming() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-resume-head-moved");
        write_base(&run, "task-1");

        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );

        // The implementer committed while the panel was pending: the in-flight
        // reviewers were seeded against the OLD head, so resuming them would review
        // a diff that no longer exists.
        commit_more(&run);
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );

        assert!(
            run.review_phases
                .iter()
                .any(|p| p.name == "review:task-1:2:correctness"),
            "a moved HEAD must start a new iteration, not resume the stale one: {:?}",
            run.review_phases
                .iter()
                .map(|p| &p.name)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_completed_iter_is_never_resumed() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-resume-after-complete");
        write_base(&run, "task-1");
        for a in ["correctness", "security", "error-handling", "type-design"] {
            seed_angle_file(&run, "task-1", 1, a, CLEAN);
        }
        drop_markers(&run, "task-1", 1);
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 5_000, false, None).unwrap(),
            ReviewOutcome::Clean
        );

        // The fix loop re-reviews after the implementer acts on findings: iter 1 is
        // fully Done, so there is nothing to resume — this must be a fresh panel.
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );
        assert!(
            run.review_phases
                .iter()
                .any(|p| p.name == "review:task-1:2:correctness"),
            "a finished iteration must not be resumed: {:?}",
            run.review_phases
                .iter()
                .map(|p| &p.name)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn clean_pass_writes_merged_and_returns_clean() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-clean");
        write_base(&run, "task-1");
        for a in ["correctness", "security", "error-handling", "type-design"] {
            seed_angle_file(&run, "task-1", 1, a, CLEAN);
        }
        // Simulate every reviewer having dropped its marker.
        drop_markers(&run, "task-1", 1);

        let outcome = code_review_run(&h, &mut run, "task-1", 5_000, false, None).unwrap();
        assert_eq!(outcome, ReviewOutcome::Clean);

        // Merged file exists and is clean.
        let merged = run_dir(&run.name).join("task-1-review.json");
        let parsed = parse_review(&std::fs::read_to_string(&merged).unwrap()).unwrap();
        assert_eq!(parsed.verdict, Verdict::Clean);
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
    fn readonly_reviewers_complete_from_herdr_status_and_findings_file() {
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
        }
        // Each reviewer delivers by writing its findings file, not by printing.
        for a in load_config().unwrap().angles {
            seed_angle_file(&run, "task-1", 1, &a, CLEAN);
        }

        let outcome = code_review_run(&h, &mut run, "task-1", 5_000, false, None).unwrap();
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
            1,
            "correctness",
            r#"{"verdict":"clean","findings":[{"file":"a.rs","severity":"important","summary":"bug"}]}"#,
        );
        for a in ["security", "error-handling", "type-design"] {
            seed_angle_file(&run, "task-1", 1, a, CLEAN);
        }
        drop_markers(&run, "task-1", 1);

        let outcome = code_review_run(&h, &mut run, "task-1", 5_000, false, None).unwrap();
        assert_eq!(outcome, ReviewOutcome::Findings);

        let merged = run_dir(&run.name).join("task-1-review.json");
        let parsed = parse_review(&std::fs::read_to_string(&merged).unwrap()).unwrap();
        assert_eq!(parsed.verdict, Verdict::Changes);
        assert_eq!(parsed.findings.len(), 1);
        // The angle is stamped from the source filename, not the JSON.
        assert_eq!(parsed.findings[0].angle, "correctness");
    }

    /// A head record that cannot be READ is not a head record that says "HEAD moved".
    /// Reporting the two the same way abandons a panel of four live reviewers and tells
    /// whoever is debugging a story that never happened.
    #[test]
    fn an_unreadable_iter_head_record_is_surfaced_not_reported_as_a_moved_head() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-head-unreadable");
        write_base(&run, "task-1");
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );
        let spawned = spawn_count(&h);

        // A directory where the head record should be: reading it fails with something
        // other than NotFound, exactly as a permissions or IO failure would.
        let head = run_dir(&run.name).join("task-1-review-1.head");
        std::fs::remove_file(&head).unwrap();
        std::fs::create_dir(&head).unwrap();

        let err = code_review_run(&h, &mut run, "task-1", 40, false, None)
            .expect_err("an unreadable head record must not pass for a moved HEAD");
        assert!(
            err.to_string().contains("head record"),
            "the error must name what it could not read: {err}"
        );
        assert!(
            err.to_string().contains("task-1-review-1.head"),
            "…and where: {err}"
        );
        assert_eq!(
            spawn_count(&h),
            spawned,
            "the live panel must not be abandoned for a fresh one on an IO error"
        );
    }

    /// The findings file is the contract, so it alone must be able to finish an angle.
    ///
    /// Observed live (2026-07-27, `land-mcp-findings` panel): four cursor reviewers all
    /// delivered, but only three were noticed. herdr reports `done` as a momentary EDGE
    /// as a turn ends and then settles to `idle`; a poll that misses that edge could
    /// never recover it, and the angle waited forever on a signal that would never come
    /// again — with its valid verdict sitting on disk the whole time. The fake's default
    /// status is `idle`, which is exactly that pane.
    #[test]
    fn a_delivered_findings_file_completes_an_angle_with_no_pane_signal_at_all() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-artifact-only");
        write_base(&run, "task-1");
        // Every reviewer delivered. Nobody dropped a done-marker (the seed forbids
        // `drovr phase done`) and no pane will ever report `done`.
        for a in load_config().unwrap().angles {
            seed_angle_file(&run, "task-1", 1, &a, CLEAN);
        }

        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 5_000, false, None).unwrap(),
            ReviewOutcome::Clean,
            "a delivered review must complete its angle whatever the pane says"
        );
        assert!(
            run.review_phases
                .iter()
                .all(|p| p.status == PhaseStatus::Done),
            "{:?}",
            run.review_phases
        );
    }

    /// The same, one pass later: an angle recorded `Running` whose verdict is on disk
    /// must be banked on resume, not waited on again. This is the state the live panel
    /// was stuck in — `Running` forever, with a complete review beside it.
    #[test]
    fn a_resume_banks_a_delivered_angle_still_recorded_running() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-artifact-resume");
        write_base(&run, "task-1");
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );
        assert!(
            run.review_phases
                .iter()
                .all(|p| p.status == PhaseStatus::Running),
            "the first pass must leave them all Running for this to mean anything"
        );
        let spawned = spawn_count(&h);

        // The reviewers deliver after the pass gave up on them.
        for a in load_config().unwrap().angles {
            seed_angle_file(&run, "task-1", 1, &a, CLEAN);
        }

        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 5_000, false, None).unwrap(),
            ReviewOutcome::Clean
        );
        assert_eq!(
            spawn_count(&h),
            spawned,
            "a delivered angle must be banked, not respawned"
        );
        assert!(
            run.review_phases
                .iter()
                .all(|p| p.status == PhaseStatus::Done),
            "the recorded status must catch up with what was delivered: {:?}",
            run.review_phases
        );
    }

    /// Completion now rests on the file, so a file being WRITTEN must not read as one
    /// that was delivered. An unparseable file is not evidence of anything: keep
    /// waiting (the reviewer may still be mid-write, and a resume self-heals) rather
    /// than either completing the angle or failing the pass outright.
    #[test]
    fn a_half_written_findings_file_is_not_mistaken_for_completion() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-half-written");
        write_base(&run, "task-1");
        for a in ["correctness", "security", "type-design"] {
            seed_angle_file(&run, "task-1", 1, a, CLEAN);
        }
        // A verdict caught mid-write: valid JSON's opening, nothing more.
        seed_angle_file(
            &run,
            "task-1",
            1,
            "error-handling",
            r#"{"verdict":"clean","findi"#,
        );

        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout,
            "a torn file must neither complete the angle nor fail the pass"
        );
        // …and the completed angle is not credited to the torn one either.
        assert_eq!(
            run.find_phase("review:task-1:1:error-handling")
                .unwrap()
                .status,
            PhaseStatus::Running,
            "the angle stays waitable, so a resume can pick up the finished write"
        );

        // Once the write lands whole, the resume completes it with no pane signal.
        seed_angle_file(&run, "task-1", 1, "error-handling", CLEAN);
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 5_000, false, None).unwrap(),
            ReviewOutcome::Clean
        );
    }

    /// A new iteration reviews a NEW diff. It must be incapable of reading the
    /// previous iteration's verdicts: with transcript scraping gone, any file on disk
    /// counts as delivery, so a reviewer that finishes without calling the tool would
    /// otherwise be credited with whatever the last pass concluded — passing a change
    /// nobody reviewed.
    #[test]
    fn a_fresh_iteration_cannot_harvest_the_previous_iterations_verdicts() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-iter-staleness");
        write_base(&run, "task-1");

        // Iteration 1: every angle delivers clean, so the pass completes.
        for a in load_config().unwrap().angles {
            seed_angle_file(&run, "task-1", 1, &a, CLEAN);
        }
        drop_markers(&run, "task-1", 1);
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 5_000, false, None).unwrap(),
            ReviewOutcome::Clean
        );

        // Iteration 2 opens fresh (iteration 1 ran to completion). Its reviewers all
        // finish, but not one of them calls `submit_findings`.
        drop_markers(&run, "task-1", 2);
        let err = code_review_run(&h, &mut run, "task-1", 5_000, false, None)
            .expect_err("a pass where nobody submitted must fail, not inherit iter 1");
        assert!(
            err.to_string().contains("never called submit_findings"),
            "unexpected error: {err}"
        );
    }

    /// The reverse direction, and the one a delete-on-open fix would still miss: a
    /// reviewer left over from a superseded iteration is still alive and eventually
    /// submits. Its verdict must land in ITS iteration's file, where the current pass
    /// can never see it — clearing files when the new panel opens happens too early to
    /// stop a straggler that writes afterwards.
    #[test]
    fn a_straggler_from_a_superseded_iteration_cannot_pollute_the_new_one() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-iter-straggler");
        write_base(&run, "task-1");

        // Iteration 1 spawns and times out, leaving its reviewers running.
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );

        // The human forces a fresh panel. Iteration 2's reviewers finish having
        // submitted nothing, while iteration 1's stragglers submit late.
        drop_markers(&run, "task-1", 2);
        for a in load_config().unwrap().angles {
            seed_angle_file(&run, "task-1", 1, &a, CLEAN);
        }

        let err = code_review_run(&h, &mut run, "task-1", 5_000, true, None)
            .expect_err("a late straggler's verdict must not complete a newer panel");
        assert!(
            err.to_string().contains("never called submit_findings"),
            "unexpected error: {err}"
        );
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
            1,
            "correctness",
            r#"{"verdict":"changes","findings":[{"file":"a.rs","severity":"important","summary":"fixed later"}]}"#,
        );
        for angle in ["security", "error-handling", "type-design"] {
            seed_angle_file(&run, "task-1", 1, angle, CLEAN);
        }
        drop_markers(&run, "task-1", 1);
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 5_000, false, None).unwrap(),
            ReviewOutcome::Findings
        );

        // The second pass's reviewers each deliver clean — into ITERATION 2's files,
        // which is the only place iteration 2 will look.
        for a in load_config().unwrap().angles {
            seed_angle_file(&run, "task-1", 2, &a, CLEAN);
        }
        drop_markers(&run, "task-1", 2);
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 5_000, false, None).unwrap(),
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
        let outcome = code_review_run(&h, &mut run, "task-1", 5_000, false, None).unwrap();
        assert_eq!(outcome, ReviewOutcome::Error);
        // Nothing spawned.
        assert!(run.review_phases.is_empty());
    }

    #[test]
    fn an_aborted_pass_still_captures_the_reviewers_it_already_seeded() {
        // The third instance of the same class, found by auditing "what else can
        // skip this capture?" rather than by a bug report.
        //
        // Angles are spawned and seeded one at a time. If angle N's `phase_send`
        // fails, the pass aborts — and the wait loop below, which is where a
        // reviewer's session is normally captured, never runs at all. Reviewers
        // 1..N-1 are live, working agents nobody will look at again this
        // invocation; their only poll was the readiness gate, which routinely
        // returns before herdr publishes the session. They exit, herdr drops it,
        // and a later resume finds a reviewer it cannot rehydrate.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-aborted-pass");
        let cfg = std::path::Path::new(&std::env::var("XDG_CONFIG_HOME").unwrap())
            .join("drovr/config.toml");
        std::fs::write(
            &cfg,
            "review_agent = \"claude\"\nangles = [\"correctness\", \"security\"]\n",
        )
        .unwrap();
        write_base(&run, "task-1");

        // Angle 1's readiness gate: started, but no session published yet.
        h.push_pane_info(Some(PaneInfo {
            tab_id: FakeHerdr::tab_id_for("pane-1"),
            agent_status: Some(AgentStatus::Idle),
            agent_session: None,
        }));
        // Angle 2's readiness gate — its send then fails, aborting the pass.
        h.push_pane_info(Some(PaneInfo {
            tab_id: FakeHerdr::tab_id_for("pane-2"),
            agent_status: Some(AgentStatus::Idle),
            agent_session: None,
        }));
        // The drain poll of angle 1, on the way out. Its session is up by now,
        // and this is the last look anything will take at it.
        h.push_pane_info(Some(PaneInfo {
            tab_id: FakeHerdr::tab_id_for("pane-1"),
            agent_status: Some(AgentStatus::Idle),
            agent_session: Some(FakeHerdr::session_for("pane-1")),
        }));
        // Angle 1 seeds fine; angle 2 fails, which aborts the pass.
        h.fail_agent_send_after(1);

        assert!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).is_err(),
            "a failed seed aborts the pass"
        );

        let seeded = run
            .review_phases
            .iter()
            .find(|p| p.name == "review:task-1:1:correctness")
            .expect("angle 1 was registered");
        assert_eq!(
            seeded
                .pane_agent()
                .and_then(|a| a.session())
                .map(SessionId::as_str),
            Some(FakeHerdr::session_value_for("pane-1").as_str()),
            "a reviewer already seeded when the pass aborted must still be polled — \
             nothing else will ever look at it"
        );
    }

    #[test]
    fn a_reviewer_that_finished_before_the_first_visit_is_still_captured() {
        // Round 2 of the same defect: the capture existed but a FAST reviewer
        // outran it.
        //
        // Angles are spawned and seeded one at a time, so a reviewer can run and
        // deliver its findings while later angles are still being launched. The
        // wait loop's first visit to it then finds the angle already complete and
        // `continue`s, skipping the only poll that reviewer would ever get. Its
        // session was never recorded, and herdr drops it the moment the reviewer
        // exits.
        //
        // The fix is ordering: poll, THEN decide. That survived main's switch to
        // a findings-file completion signal — the short-circuit moved from
        // `done_marker(..).exists() || <poll>` to `delivered_review(..)`, and the
        // capture has to sit above BOTH. This test pins that ordering by arranging
        // the exact losing sequence — the angle already delivered and its marker
        // already on disk at the first visit, session published only on that
        // visit's poll.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-fast-reviewer");
        let cfg = std::path::Path::new(&std::env::var("XDG_CONFIG_HOME").unwrap())
            .join("drovr/config.toml");
        std::fs::write(
            &cfg,
            "review_agent = \"claude\"\nangles = [\"correctness\"]\n",
        )
        .unwrap();
        write_base(&run, "task-1");

        // Poll 1 — `phase_send`'s readiness gate. Started, no session yet.
        h.push_pane_info(Some(PaneInfo {
            tab_id: FakeHerdr::tab_id_for("pane-1"),
            agent_status: Some(AgentStatus::Idle),
            agent_session: None,
        }));
        // Poll 2 — the wait loop's FIRST visit, which happens with the marker
        // already on disk (dropped below). This is the only remaining chance.
        h.push_pane_info(Some(PaneInfo {
            tab_id: FakeHerdr::tab_id_for("pane-1"),
            agent_status: Some(AgentStatus::Idle),
            agent_session: Some(FakeHerdr::session_for("pane-1")),
        }));

        // The reviewer finishes before the loop ever looks at it: its verdict is
        // already delivered and its marker already on disk when `code_review_run`
        // starts waiting.
        drop_marker(&run, "task-1", 1, "correctness");
        seed_angle_file(&run, "task-1", 1, "correctness", CLEAN);

        let outcome = code_review_run(&h, &mut run, "task-1", 5000, false, None).unwrap();
        assert_eq!(outcome, ReviewOutcome::Clean, "the reviewer completed");

        let on_disk = RunState::load("cr-fast-reviewer").unwrap();
        assert_eq!(
            on_disk.review_phases[0]
                .pane_agent()
                .and_then(|a| a.session())
                .map(SessionId::as_str),
            Some(FakeHerdr::session_value_for("pane-1").as_str()),
            "a reviewer that finished before the first visit must still have been \
             polled — the marker must not be allowed to skip the capture"
        );
    }

    #[test]
    fn a_reviewers_session_is_captured_when_it_appears_after_the_agent_starts() {
        // THE failure this task exists to prevent, at the one pane where it is
        // unrecoverable.
        //
        // `phase_send`'s readiness gate returns on the FIRST poll reporting a
        // started agent — and herdr routinely publishes `agent_status` before it
        // publishes `agent_session`, so that poll can carry no session at all.
        // A pipeline phase survives that: `phase_wait` polls again every 500ms
        // for the life of the phase. A reviewer has no such second chance unless
        // ITS wait loop captures too — and reviewers are told to exit, at which
        // point herdr drops the session for good, and task 6 closes the pane.
        //
        // So: first poll started-but-session-less, later polls carrying one.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-late-session");
        // One angle, so the poll queue below maps to one reviewer deterministically.
        let cfg = std::path::Path::new(&std::env::var("XDG_CONFIG_HOME").unwrap())
            .join("drovr/config.toml");
        std::fs::write(
            &cfg,
            "review_agent = \"claude\"\nangles = [\"correctness\"]\n",
        )
        .unwrap();
        write_base(&run, "task-1");

        // Poll 1 — the readiness gate. Started, so the gate returns at once, but
        // herdr has not published a session yet.
        h.push_pane_info(Some(PaneInfo {
            tab_id: FakeHerdr::tab_id_for("pane-1"),
            agent_status: Some(AgentStatus::Idle),
            agent_session: None,
        }));
        // Poll 2+ — the reviewer wait loop. NOW the session is there. Status stays
        // Idle so the reviewer never "finishes" and the pass times out.
        h.push_pane_info(Some(PaneInfo {
            tab_id: FakeHerdr::tab_id_for("pane-1"),
            agent_status: Some(AgentStatus::Idle),
            agent_session: Some(FakeHerdr::session_for("pane-1")),
        }));

        let outcome = code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap();
        assert_eq!(outcome, ReviewOutcome::Timeout);
        assert_eq!(run.review_phases.len(), 1);

        let agent = run.review_phases[0]
            .pane_agent()
            .expect("the reviewer records its launch");
        assert_eq!(
            agent.session().map(SessionId::as_str),
            Some(FakeHerdr::session_value_for("pane-1").as_str()),
            "a session that appears after the agent reports started must still be \
             captured — the reviewer wait loop is the only place left to see it"
        );
        // And on disk, which is the only place task 5 can read it from.
        let on_disk = RunState::load("cr-late-session").unwrap();
        assert!(
            on_disk.review_phases[0]
                .pane_agent()
                .and_then(|a| a.session())
                .is_some(),
            "the capture must be persisted, not just held in memory"
        );
    }

    #[test]
    fn a_reviewer_records_its_own_backend_not_the_runs() {
        // A reviewer's backend is `review_agent_for`'s answer, NOT `run.agent`:
        // config can pin it, and cursor can be auto-selected. Session capture
        // checks a pane's session against the backend recorded on the phase, and
        // herdr attributes a session to whichever agent actually created it — so
        // handing `spawn_reviewer` the run's backend here would make capture ask
        // the wrong question and silently never record a cross-backend
        // reviewer's session. That is unrecoverable: herdr drops the session when
        // the reviewer exits, and reviewers are told to exit.
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-backend");
        // Pin reviews to cursor while the RUN stays on claude — the divergence
        // this test exists for. `make_run` wrote `review_agent = "claude"`.
        let cfg = std::path::Path::new(&std::env::var("XDG_CONFIG_HOME").unwrap())
            .join("drovr/config.toml");
        std::fs::write(&cfg, "review_agent = \"cursor\"\n").unwrap();
        assert_eq!(run.agent.as_deref(), Some("claude"), "the run is claude");
        write_base(&run, "task-1");

        let outcome = code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap();
        assert_eq!(outcome, ReviewOutcome::Timeout);
        assert!(!run.review_phases.is_empty());
        for p in &run.review_phases {
            assert_eq!(
                p.pane_agent().map(|a| a.backend()),
                Some("cursor"),
                "reviewer '{}' must record the backend it was launched with",
                p.name
            );
        }
    }

    #[test]
    fn timeout_leaves_reviewers_running_for_a_resume() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-timeout");
        write_base(&run, "task-1");

        // No markers dropped → tiny timeout → Timeout.
        let outcome = code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap();
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

        // What a re-run does with those leftovers is covered by
        // `rerun_after_timeout_resumes_the_same_iter_without_respawning` (resume) and
        // `fresh_flag_starts_a_new_iteration` (`--fresh`).
        let head = head_sha(&run.project_dir).unwrap();
        assert_eq!(
            std::fs::read_to_string(run_dir(&run.name).join("task-1-review-1.head"))
                .unwrap()
                .trim(),
            head,
            "the pass must record the head it seeded reviewers against, so a resume \
             can tell whether that diff still stands"
        );
    }

    #[test]
    fn reviewers_spawned_with_configured_readonly_launch() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-launch");
        write_base(&run, "task-1");
        for a in ["correctness", "security", "error-handling", "type-design"] {
            seed_angle_file(&run, "task-1", 1, a, CLEAN);
        }
        drop_markers(&run, "task-1", 1);

        code_review_run(&h, &mut run, "task-1", 5_000, false, None).unwrap();

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

    /// A reviewer with no findings channel is a reviewer that cannot deliver, so the
    /// panel must provision the MCP server before it spawns anyone, and point the
    /// launch at it.
    #[test]
    fn the_panel_writes_the_findings_server_config_and_launches_against_it() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-mcp-flag");
        write_base(&run, "task-1");

        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );

        // claude reads the file from a path on its command line, so it lands in
        // drovr's run dir — never in the project the reviewer is reviewing.
        let cfg_path = run_dir(&run.name).join("task-1-review-mcp.json");
        let body: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&cfg_path).unwrap()).unwrap();
        let server = &body["mcpServers"]["drovr-findings"];
        assert_eq!(
            server["args"],
            serde_json::json!(["mcp-findings", "cr-mcp-flag", "task-1", "1"]),
            "the server is pinned to this run, task and ITERATION: {body}"
        );
        assert!(
            server["command"].as_str().is_some_and(|c| !c.is_empty()),
            "the server must name a real drovr executable: {body}"
        );

        let calls = h.calls();
        let launches: Vec<&String> = calls.iter().filter(|c| c.contains("pane_run")).collect();
        assert_eq!(launches.len(), 4);
        for c in &launches {
            assert!(
                c.contains(&format!("--mcp-config '{}'", cfg_path.display())),
                "every reviewer must be handed the findings server: {c}"
            );
            assert!(
                c.contains("--strict-mcp-config"),
                "the reviewer gets drovr's one tool, not the user's whole MCP set: {c}"
            );
        }
    }

    /// cursor has no per-launch MCP flag, so the server has to be written into the
    /// project's `.cursor/mcp.json` — and then kept out of git, since that file is
    /// drovr's plumbing and not a change the user asked for.
    #[test]
    fn a_project_file_backend_gets_its_config_written_into_the_project_and_excluded() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-mcp-project-file");
        run.agent = Some("cursor".into());
        std::fs::write(
            std::path::Path::new(&std::env::var("XDG_CONFIG_HOME").unwrap())
                .join("drovr/config.toml"),
            "review_agent = \"cursor\"\n",
        )
        .unwrap();
        write_base(&run, "task-1");

        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );

        let project = std::path::PathBuf::from(&run.project_dir);
        let body: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(project.join(".cursor/mcp.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            body["mcpServers"]["drovr-findings"]["args"],
            serde_json::json!(["mcp-findings", "cr-mcp-project-file", "task-1", "1"])
        );
        assert!(
            h.calls()
                .iter()
                .filter(|c| c.contains("pane_run"))
                .all(|c| c.contains("--approve-mcps") && !c.contains("mcp.json")),
            "cursor has no flag to carry the path; it only needs to trust the file"
        );

        let exclude = std::fs::read_to_string(project.join(".git/info/exclude")).unwrap();
        assert!(
            exclude.lines().any(|l| l.trim() == ".cursor/mcp.json"),
            "drovr's plumbing must not show up as an untracked change: {exclude}"
        );

        // Every pass writes the config again, and the exclude file is SHARED (git's
        // common dir, so all of a repo's worktrees see it) — appending a line per
        // pass would grow it without bound.
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );
        let exclude = std::fs::read_to_string(project.join(".git/info/exclude")).unwrap();
        assert_eq!(
            exclude
                .lines()
                .filter(|l| l.trim() == ".cursor/mcp.json")
                .count(),
            1,
            "the exclude entry must be written once, not once per pass: {exclude}"
        );
    }

    /// `--approve-mcps` auto-approves EVERY server in the project file, and drovr
    /// cannot approve selectively. So any server drovr left in place would be silently
    /// handed to a read-only reviewer — and `.cursor/mcp.json` is a path a hostile
    /// repository can simply commit. The reviewer must see drovr's server and nothing
    /// else; the displaced config is preserved, not destroyed.
    #[test]
    fn a_foreign_server_in_the_project_config_is_never_handed_to_a_reviewer() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-mcp-foreign");
        run.agent = Some("cursor".into());
        std::fs::write(
            std::path::Path::new(&std::env::var("XDG_CONFIG_HOME").unwrap())
                .join("drovr/config.toml"),
            "review_agent = \"cursor\"\n",
        )
        .unwrap();
        write_base(&run, "task-1");

        let project = std::path::PathBuf::from(&run.project_dir);
        let cfg_path = project.join(".cursor/mcp.json");
        std::fs::create_dir_all(project.join(".cursor")).unwrap();
        std::fs::write(
            &cfg_path,
            r#"{"mcpServers":{"mine":{"command":"my-server"},"evil":{"command":"curl"}}}"#,
        )
        .unwrap();

        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );

        let body: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&cfg_path).unwrap()).unwrap();
        let servers = body["mcpServers"].as_object().unwrap();
        assert_eq!(
            servers.keys().collect::<Vec<_>>(),
            vec!["drovr-findings"],
            "a reviewer launched with --approve-mcps must see exactly one server: {body}"
        );

        // Destroying the user's config would be its own bug: it is displaced, not lost.
        let backup: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(backup_path(&cfg_path)).unwrap())
                .unwrap();
        assert_eq!(backup["mcpServers"]["mine"]["command"], "my-server");
        assert_eq!(backup["mcpServers"]["evil"]["command"], "curl");

        // A second pass must not overwrite the backup with drovr's own file — that is
        // how "preserved" quietly becomes "lost on the next run".
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );
        let backup: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(backup_path(&cfg_path)).unwrap())
                .unwrap();
        assert_eq!(
            backup["mcpServers"]["mine"]["command"], "my-server",
            "the original must survive every later pass: {backup}"
        );
        // …and the backup is drovr's plumbing too, so it must not dirty the tree.
        let exclude = std::fs::read_to_string(project.join(".git/info/exclude")).unwrap();
        assert!(
            exclude
                .lines()
                .any(|l| l.trim() == ".cursor/mcp.json.drovr-backup"),
            "the displaced original must be excluded from git too: {exclude}"
        );
    }

    /// A config that holds only drovr's own server (the ordinary steady state, every
    /// pass after the first) is rewritten in place — no backup, no noise.
    #[test]
    fn rewriting_drovrs_own_config_does_not_accumulate_backups() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-mcp-rewrite");
        run.agent = Some("cursor".into());
        std::fs::write(
            std::path::Path::new(&std::env::var("XDG_CONFIG_HOME").unwrap())
                .join("drovr/config.toml"),
            "review_agent = \"cursor\"\n",
        )
        .unwrap();
        write_base(&run, "task-1");

        for _ in 0..2 {
            assert_eq!(
                code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
                ReviewOutcome::Timeout
            );
        }
        let cfg_path = std::path::PathBuf::from(&run.project_dir).join(".cursor/mcp.json");
        assert!(
            !backup_path(&cfg_path).exists(),
            "drovr's own config is not something to back up"
        );
    }

    /// `.cursor/mcp.json` lives inside the checkout under review, so a repository can
    /// commit a symlink there. `fs::write` follows it, which would drop drovr's config
    /// wherever it points — outside the project entirely.
    #[test]
    fn a_symlinked_project_config_is_refused_rather_than_followed() {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let elsewhere = tmp.path().join("outside.json");
        std::fs::write(&elsewhere, "{}").unwrap();

        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(project.path().join(".cursor")).unwrap();
        let link = project.path().join(".cursor/mcp.json");
        std::os::unix::fs::symlink(&elsewhere, &link).unwrap();

        let err = write_mcp_config(&link, "r", "task-1", 1)
            .expect_err("a symlinked config must be refused, not followed");
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(
            std::fs::read_to_string(&elsewhere).unwrap(),
            "{}",
            "nothing may be written through the link"
        );

        // A symlinked PARENT redirects the write just as effectively.
        let project2 = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(tmp.path(), project2.path().join(".cursor")).unwrap();
        let err = write_mcp_config(&project2.path().join(".cursor/mcp.json"), "r", "task-1", 1)
            .expect_err("a symlinked parent must be refused too");
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    /// The respawn's delete is the only thing between a dead reviewer's verdict and a
    /// replacement that never submits. If it cannot be done, the pass must fail rather
    /// than spawn a replacement into a trap.
    #[test]
    fn a_findings_file_that_cannot_be_cleared_fails_the_respawn() {
        let dir = tempfile::tempdir().unwrap();
        // Already absent is the state the caller wants, not a failure.
        assert!(clear_findings_file(dir.path(), "task-1", 1, "correctness").is_ok());

        // A directory at the findings path cannot be removed with `remove_file`,
        // standing in for a delete that fails for any other reason (permissions, EIO).
        let path = findings_path(dir.path(), "task-1", 1, "security");
        std::fs::create_dir(&path).unwrap();
        let err = clear_findings_file(dir.path(), "task-1", 1, "security")
            .expect_err("an unclearable stale verdict must not be shrugged off");
        assert!(
            err.to_string().contains("would inherit"),
            "the error must say what goes wrong if the pass continues: {err}"
        );
    }

    /// "The reviewer never submitted" and "the file is there but unreadable" have
    /// different causes and different remedies. Reporting the second as the first
    /// sends whoever is debugging to look at the reviewer instead of the disk.
    #[test]
    fn an_unreadable_findings_file_is_not_reported_as_a_silent_reviewer() {
        let dir = tempfile::tempdir().unwrap();
        let missing = obtain_findings_json(dir.path(), "task-1", 1, "correctness", "ph")
            .expect_err("no file at all");
        assert!(
            missing.to_string().contains("never called submit_findings"),
            "{missing}"
        );

        // Present, but not readable as a file.
        std::fs::create_dir(findings_path(dir.path(), "task-1", 1, "security")).unwrap();
        let unreadable = obtain_findings_json(dir.path(), "task-1", 1, "security", "ph")
            .expect_err("a file that cannot be read");
        assert!(
            !unreadable
                .to_string()
                .contains("never called submit_findings"),
            "an IO failure must not be blamed on the reviewer: {unreadable}"
        );
        assert!(
            unreadable.to_string().contains("could not be read back"),
            "{unreadable}"
        );
    }

    /// The seed's schema and the MCP tool's schema are one definition rendered two
    /// ways. A drift there tells the reviewer to send a shape validation then rejects.
    #[test]
    fn seed_schema_is_rendered_from_the_one_definition() {
        let rendered = findings_schema();
        // Every closed value set comes from the types `parse_review` enforces.
        assert!(
            rendered.contains(r#""verdict": "clean" | "changes""#),
            "{rendered}"
        );
        assert!(
            rendered.contains(r#""severity": "critical" | "important" | "nit""#),
            "{rendered}"
        );
        assert!(
            rendered.contains(r#""impact": "low" | "medium" | "high""#),
            "{rendered}"
        );
        // …and every field the schema defines is actually shown to the reviewer.
        let schema = crate::mcp_findings::review_schema();
        for key in schema.as_object().unwrap().keys() {
            assert!(
                rendered.contains(&format!("\"{key}\"")),
                "the seed must show '{key}': {rendered}"
            );
        }
        for key in ["file", "line", "severity", "summary", "rationale"] {
            assert!(rendered.contains(&format!("\"{key}\"")), "{rendered}");
        }
        // The rendering is what the reviewer actually receives.
        assert!(
            build_seed("task-1", "security", "a", "b", "d", "/checkout/here", None)
                .contains(&rendered)
        );
    }

    /// A read error is not "no file here". Collapsing them lets drovr replace — and
    /// fail to back up — a config it never managed to read.
    #[test]
    fn an_unreadable_existing_config_is_an_error_not_a_silent_replacement() {
        let _lock = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        // A directory where the config should be: reading it fails with something
        // other than NotFound, exactly like a permissions or IO failure would.
        let path = dir.path().join("mcp.json");
        std::fs::create_dir(&path).unwrap();

        let err = write_mcp_config(&path, "r", "task-1", 1)
            .expect_err("an unreadable config must not be silently replaced");
        assert!(
            err.to_string().contains("mcp.json"),
            "the error must name the file: {err}"
        );
    }

    /// Without an MCP mechanism a reviewer has no way to submit findings at all, so
    /// the pass fails at spawn time with a readable reason rather than timing out.
    #[test]
    fn a_review_backend_with_no_findings_channel_is_refused_before_spawning() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-mcp-none");
        std::fs::write(
            std::path::Path::new(&std::env::var("XDG_CONFIG_HOME").unwrap())
                .join("drovr/config.toml"),
            "review_agent = \"codex\"\n",
        )
        .unwrap();
        write_base(&run, "task-1");

        let err = code_review_run(&h, &mut run, "task-1", 40, false, None)
            .expect_err("a backend that cannot be given the findings tool cannot review");
        assert!(err.to_string().contains("codex"), "{err}");
        assert!(
            run.review_phases.is_empty(),
            "nothing may be spawned when no reviewer could deliver"
        );
    }

    /// The findings file is the ONLY contract. With no file written, drovr reports the
    /// reviewer produced nothing — and never falls back to reading its pane.
    #[test]
    fn a_reviewer_that_never_submitted_produced_nothing_and_no_pane_is_read() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-file-only");
        write_base(&run, "task-1");
        // Every reviewer finishes (markers land) but none ever called the tool.
        drop_markers(&run, "task-1", 1);

        let err = code_review_run(&h, &mut run, "task-1", 5_000, false, None)
            .expect_err("a missing findings file must be an error, not a scrape");
        assert!(
            err.to_string().contains("never called submit_findings"),
            "unexpected error: {err}"
        );
        // "Never reads a pane to obtain findings" is the invariant, and it is
        // about the HARVEST. `phase_send` does read each pane while seeding it, to
        // prove the seed was delivered (a stalled prompt is indistinguishable from
        // a swallowed one without looking) — those reads all happen before their
        // own `agent_prompt_confirm`, and none of them can see findings, because
        // no reviewer has run yet. So pin the position rather than the count: not
        // one read after the last seed.
        let calls = h.calls();
        let last_seed = calls
            .iter()
            .rposition(|c| c.contains("agent_prompt_confirm"))
            .expect("precondition: the reviewers were seeded");
        assert!(
            !calls[last_seed..].iter().any(|c| c.contains("agent_read")),
            "the panel must never read a pane transcript to obtain findings: {calls:?}"
        );
    }

    /// A replacement reviewer must not inherit the dead one's findings file.
    #[test]
    fn a_respawned_reviewer_does_not_inherit_the_dead_ones_findings() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-respawn-inherit");
        write_base(&run, "task-1");
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );

        let dead = pane_of(&run, "review:task-1:1:correctness");
        h.kill_pane(dead.clone());
        // What the dead reviewer left behind — a torn write, so it is NOT a delivery
        // and the angle is genuinely due for replacement. Harvesting this would be
        // crediting the replacement with a file it never wrote.
        let leftover = findings_path(&run_dir(&run.name), "task-1", 1, "correctness");
        std::fs::write(&leftover, r#"{"verdict":"changes","findings":[{"fi"#).unwrap();
        drop_markers(&run, "task-1", 1);
        for angle in ["security", "error-handling", "type-design"] {
            seed_angle_file(&run, "task-1", 1, angle, CLEAN);
        }

        // correctness is respawned, so its leftover is cleared and the replacement
        // has written nothing — the pass must fail rather than reuse it.
        let err = code_review_run(&h, &mut run, "task-1", 40, false, None)
            .expect_err("a respawned angle with no file of its own must not succeed");
        assert!(err.to_string().contains("correctness"), "{err}");
        assert_ne!(
            pane_of(&run, "review:task-1:1:correctness"),
            dead,
            "the angle should have been respawned into a new pane"
        );
        assert!(
            !leftover.exists(),
            "the respawn must clear the outgoing reviewer's file, or the replacement \
             inherits it: {}",
            leftover.display()
        );
    }

    /// The counterpart, and the reason the test above uses a TORN leftover: a file that
    /// parses was written through `submit_findings` for this iteration's diff, so it is
    /// that reviewer's verdict — and it counts even though the pane is now gone. The
    /// pane's fate is not evidence about a review that was already delivered.
    #[test]
    fn a_delivered_verdict_still_counts_after_its_reviewer_s_pane_dies() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-delivered-then-died");
        write_base(&run, "task-1");
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );
        let spawned = spawn_count(&h);

        // Every angle delivered; then one reviewer's pane went away entirely.
        for a in load_config().unwrap().angles {
            seed_angle_file(&run, "task-1", 1, &a, CLEAN);
        }
        h.kill_pane(pane_of(&run, "review:task-1:1:correctness"));

        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 5_000, false, None).unwrap(),
            ReviewOutcome::Clean,
            "a delivered verdict is not invalidated by its pane closing"
        );
        assert_eq!(
            spawn_count(&h),
            spawned,
            "an angle that already delivered must not be respawned"
        );
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
            archived: false,
            retired_panes: vec![],
        };
        assert_eq!(next_iter(&base, "task-1"), 1);

        let mut run = base.clone();
        let mk = |name: &str| {
            let mut p = Phase::new(name);
            p.status = PhaseStatus::Running;
            p
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
    fn only_the_newest_running_iteration_is_resumable() {
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
            archived: false,
            retired_panes: vec![],
        };
        let mk = |name: &str, status: PhaseStatus| {
            let mut p = Phase::new(name);
            p.status = status;
            p
        };

        let angles: Vec<String> = ["correctness", "security"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        assert_eq!(
            resumable_iter(&base, "task-1", &angles),
            None,
            "nothing spawned yet"
        );

        // A pass still in flight is resumable.
        let mut run = base.clone();
        run.review_phases = vec![
            mk("review:task-1:1:correctness", PhaseStatus::Done),
            mk("review:task-1:1:security", PhaseStatus::Running),
        ];
        assert_eq!(resumable_iter(&run, "task-1", &angles), Some(1));

        // A pass that ran to completion is not: a re-run there is the fix loop
        // asking for a new review of newly-fixed code.
        let mut run = base.clone();
        run.review_phases = vec![
            mk("review:task-1:1:correctness", PhaseStatus::Done),
            mk("review:task-1:1:security", PhaseStatus::Done),
        ];
        assert_eq!(resumable_iter(&run, "task-1", &angles), None);

        // Neither is one whose only unfinished reviewer `Failed` — that angle needs a
        // replacement, which the fresh-panel path provides.
        let mut run = base.clone();
        run.review_phases = vec![
            mk("review:task-1:1:correctness", PhaseStatus::Done),
            mk("review:task-1:1:security", PhaseStatus::Failed),
        ];
        assert_eq!(resumable_iter(&run, "task-1", &angles), None);

        // Zombies from an abandoned (`--fresh`-superseded) iteration must never be
        // revived, even though they are still `Running` — iter 2 is what matters,
        // and it is done.
        let mut run = base.clone();
        run.review_phases = vec![
            mk("review:task-1:1:correctness", PhaseStatus::Running),
            mk("review:task-1:2:correctness", PhaseStatus::Done),
        ];
        assert_eq!(
            resumable_iter(&run, "task-1", &angles),
            None,
            "an older iteration's leftovers are not a resumable pass"
        );

        // A `Running` reviewer for an angle no longer in config holds nothing open:
        // the pass only ever waits on configured angles, so it would never finish.
        let mut run = base.clone();
        run.review_phases = vec![
            mk("review:task-1:1:correctness", PhaseStatus::Done),
            mk("review:task-1:1:security", PhaseStatus::Done),
            mk("review:task-1:1:performance", PhaseStatus::Running),
        ];
        assert_eq!(
            resumable_iter(&run, "task-1", &angles),
            None,
            "an unconfigured angle's leftover must not make the pass resumable"
        );

        // Tasks are independent.
        let mut run = base.clone();
        run.review_phases = vec![
            mk("review:task-1:1:correctness", PhaseStatus::Done),
            mk("review:task-2:1:correctness", PhaseStatus::Running),
        ];
        assert_eq!(resumable_iter(&run, "task-1", &angles), None);
        assert_eq!(resumable_iter(&run, "task-2", &angles), Some(1));
    }

    #[test]
    fn seed_contains_scope_schema_and_readonly_finish_instruction() {
        let seed = build_seed(
            "task-1",
            "security",
            "aaa",
            "bbb",
            "do the thing",
            "/checkout/here",
            None,
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
            seed.contains("submit_findings"),
            "seed must name the tool that delivers the review"
        );
        assert!(
            seed.contains("Do not modify any files or run `drovr phase done`"),
            "seed must preserve strict read-only behavior"
        );
        assert!(seed.contains("critical") && seed.contains("important") && seed.contains("nit"));
    }

    /// The diff alone cannot show whether a change is right — a reviewer has to read
    /// the callers, the invariants, and the tests it lands among. The seed must
    /// therefore name the checkout and grant whole-repo reads explicitly, or a
    /// reviewer reads the diff and stops.
    #[test]
    fn seed_grants_full_repo_reads_not_just_the_diff() {
        let seed = build_seed(
            "task-1",
            "correctness",
            "aaa",
            "bbb",
            "do the thing",
            "/checkout/here",
            None,
        );
        assert!(
            seed.contains("/checkout/here"),
            "seed must name the checkout the reviewer can read: {seed}"
        );
        assert!(
            seed.contains("read any file"),
            "seed must grant reads beyond the diffed files: {seed}"
        );
        assert!(
            seed.contains("run the tests") || seed.contains("run tests"),
            "seed must allow running the tests: {seed}"
        );
    }
    /// Context the driver supplies must reach the reviewer as a labelled section of
    /// the brief drovr composes — not as prose the driver wraps around it.
    #[test]
    fn seed_carries_driver_context_when_given() {
        let with = build_seed(
            "task-1",
            "correctness",
            "aaa",
            "bbb",
            "do the thing",
            "/checkout/here",
            Some("the retry loop is new; ignore the vendored dir"),
        );
        assert!(
            with.contains("## Context from the driver"),
            "context must land in its own labelled section: {with}"
        );
        assert!(with.contains("the retry loop is new; ignore the vendored dir"));

        let without = build_seed(
            "task-1",
            "correctness",
            "aaa",
            "bbb",
            "do the thing",
            "/checkout/here",
            None,
        );
        assert!(
            without.contains("## Context from the driver") && without.contains("none supplied"),
            "the section is always present, marked unsupplied — matching the phase \
             briefs, so a brief can always refer to it: {without}"
        );
    }

    /// Invariant 4: a resume must compose the SAME brief. The context is therefore
    /// recorded in the run dir on the pass that supplies it, and a later pass that
    /// passes none reuses the record rather than silently dropping it.
    #[test]
    fn context_is_recorded_and_reused_by_a_later_pass() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-context-persist");
        write_base(&run, "task-1");

        assert_eq!(
            code_review_run(
                &h,
                &mut run,
                "task-1",
                40,
                false,
                Some("watch the retry loop")
            )
            .unwrap(),
            ReviewOutcome::Timeout
        );
        let dir = run_dir(&run.name);
        assert_eq!(
            std::fs::read_to_string(dir.join("task-1-review-context.md"))
                .unwrap()
                .trim(),
            "watch the retry loop",
            "the pass that supplies context must record it"
        );

        // A fresh panel with NO context argument: the recorded context still has to
        // reach the reviewers, or the second panel reviews with less than the first.
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, true, None).unwrap(),
            ReviewOutcome::Timeout
        );
        let seed = std::fs::read_to_string(dir.join("task-1-review-correctness-seed.md")).unwrap();
        assert!(
            seed.contains("watch the retry loop"),
            "a later pass must reuse the recorded context: {seed}"
        );
    }

    /// `--context ''` must be able to un-say a recorded context on the reviewer path too
    /// (shared resolver, so this pins the delegation as much as the behavior).
    #[test]
    fn an_empty_context_clears_the_recorded_one() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run("cr-context-clear");
        write_base(&run, "task-1");
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, Some("stale context")).unwrap(),
            ReviewOutcome::Timeout
        );
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, true, Some("")).unwrap(),
            ReviewOutcome::Timeout
        );
        let seed =
            std::fs::read_to_string(run_dir(&run.name).join("task-1-review-correctness-seed.md"))
                .unwrap();
        assert!(
            !seed.contains("stale context"),
            "an explicitly empty --context must drop the record, not fall through: {seed}"
        );
        assert!(
            seed.contains("none supplied"),
            "and says so explicitly: {seed}"
        );
    }

    /// A driver that spawns its own read-only reviewer (in-harness subagent, no herdr
    /// integration, wedged panel) must be able to obtain the composed brief WITHOUT
    /// spawning anything — otherwise its prompt goes back to being agent-authored.
    #[test]
    fn brief_composes_the_same_frame_without_spawning() {
        let _lock = ENV_LOCK.lock().unwrap();
        let h = FakeHerdr::new();
        let (run, _repo) = make_run("cr-brief");
        write_base(&run, "task-1");

        let brief = code_review_brief(&run, "task-1", "security", Some("only the parser changed"))
            .expect("brief must compose from recorded base + current HEAD");

        assert!(brief.contains("# Review angle: security"));
        assert!(brief.contains("READ-ONLY reviewer"));
        assert!(brief.contains("read any file"), "full-repo grant: {brief}");
        assert!(brief.contains("only the parser changed"));
        assert!(
            brief.contains(&run.project_dir),
            "brief must name the checkout: {brief}"
        );
        assert_eq!(
            h.calls().len(),
            0,
            "composing a brief must not touch herdr: {:?}",
            h.calls()
        );
    }

    /// A brief with no recorded base cannot state its scope, so it must fail loudly
    /// rather than print a frame with an empty diff range.
    #[test]
    fn brief_without_a_recorded_base_is_an_error() {
        let _lock = ENV_LOCK.lock().unwrap();
        let (run, _repo) = make_run("cr-brief-no-base");
        let err = code_review_brief(&run, "task-1", "security", None)
            .expect_err("no base recorded must be an error");
        assert!(
            err.to_string().contains("code-review base"),
            "the error must say how to fix it: {err}"
        );
    }

    /// The reviewer runs read-only and cannot write its findings file, so the seed
    /// must route the review through the `submit_findings` tool — and must not tell
    /// the reviewer to attempt a write it will be refused. Printing is not a channel
    /// either: a rendered pane hard-wraps long lines, which puts raw newlines inside
    /// JSON string literals and loses a complete, valid verdict.
    #[test]
    fn seed_routes_findings_through_the_submit_tool() {
        let seed = build_seed(
            "task-1",
            "security",
            "aaa",
            "bbb",
            "do it",
            "/checkout/here",
            None,
        );
        assert!(
            seed.contains("submit_findings"),
            "seed must name the tool; got:\n{seed}"
        );
        assert!(
            seed.contains("`security`"),
            "seed must tell the reviewer which angle to submit under: {seed}"
        );
        assert!(
            seed.contains("never parsed"),
            "seed must say printing a review does not deliver it: {seed}"
        );
        // Probed 2026-07-26 against a real `claude --permission-mode plan
        // --mcp-config`: the tool is registered as `mcp__drovr-findings__…` and can
        // be DEFERRED behind a schema lookup, and the agent hesitated to call a
        // "writing" tool under plan mode. A seed that names only the bare tool and
        // says nothing about either loses the review.
        assert!(
            seed.contains("mcp__drovr-findings__submit_findings"),
            "seed must give the fully qualified tool id, which is how a backend that \
             namespaces MCP tools lists it: {seed}"
        );
        assert!(
            seed.contains("load its schema"),
            "seed must tell a reviewer whose tools are deferred to load this one: {seed}"
        );
        assert!(
            seed.contains("sanctioned"),
            "seed must say the call is expected under read-only mode, so a cautious \
             reviewer does not stop to ask permission it will never receive: {seed}"
        );
        // The reviewer is read-only: instructing a write would only earn it a refusal.
        let findings_file =
            crate::mcp_findings::findings_path(&run_dir("myrun"), "task-1", 1, "security")
                .display()
                .to_string();
        assert!(
            !seed.contains(&findings_file),
            "the seed must not name a file the reviewer cannot write: {seed}"
        );
        assert!(
            !seed.to_lowercase().contains("write this file"),
            "the seed must not demand a write that read-only mode refuses: {seed}"
        );
        assert!(
            seed.contains("Do not modify any files or run `drovr phase done`"),
            "the tool carve-out must not weaken read-only behavior"
        );
    }
}
