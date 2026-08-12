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
//! `drovr:code-review`), not in this module — see `forge.ko.ag/drovr/drovr/issues` for the run
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

use crate::config::{McpSchema, load_config};
use crate::findings::{Finding, Review, Severity, Verdict, is_clean, merge_reviews, parse_review};
use crate::herdr::{AgentStatus, Herdr};
use crate::mcp_findings::findings_path;
use crate::phase::{
    ReapOutcome, archived_run_error, marker_completes_current_pass, phase_reap, phase_send,
    poll_phase_pane, reap_retired, spawn_reviewer,
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
/// re-running the panel itself, unconditionally. See `forge.ko.ag/drovr/drovr/issues`.
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

/// Run git, in the project, and hand back stdout — or the reason git could not answer.
fn git_capture(project_dir: &str, args: &[&str]) -> io::Result<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(project_dir)
        .args(args)
        .output()?;
    if !out.status.success() {
        return Err(io::Error::other(format!(
            "git {} in {project_dir} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    // `from_utf8_lossy`: a diff carries whatever bytes the files hold, and a repository
    // with a latin-1 source file must not fail the review that would have flagged it.
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The directory holding one iteration's split diff, **inside the project**.
///
/// Iteration-scoped for the same reason the findings files are: a reviewer left over
/// from a superseded pass must keep reading the diff it was seeded against, not one a
/// newer panel overwrote underneath it.
///
/// # Why inside the project and not the run dir
///
/// It used to be the run dir, and every reviewer stalled on it. opencode gates a read
/// outside the working directory behind an `external_directory` decision whose stock
/// value is `ask` — a prompt nobody in a reviewer pane can answer. Measured on
/// 2026-08-08 (opencode 1.18.3): a project-level `external_directory` allow for the run
/// dir does **not** override the global `ask`; the read still hung until timeout. See
/// `docs/known-issues.md`.
///
/// So the artifact goes where the reviewer's read permission already reaches
/// unconditionally: the checkout it is reviewing. `.drovr/` is drovr's own directory
/// there and is gitignored in this repository; for a project that does not ignore it,
/// [`code_review_run`] adds a local exclude, so the artifacts never read as untracked
/// work.
fn review_diff_dir(project_dir: &str, task: &str, iter: u64) -> PathBuf {
    Path::new(project_dir)
        .join(REVIEW_ARTIFACT_DIR)
        .join(format!("{task}-review-{iter}"))
}

/// The one file the seed names: the index [`write_review_diff`] writes, which carries
/// `--stat` and points at the per-file patches beside it.
fn review_diff_path(project_dir: &str, task: &str, iter: u64) -> PathBuf {
    review_diff_dir(project_dir, task, iter).join("index.md")
}

/// The project-relative directory drovr writes review artifacts into. Also what
/// [`code_review_run`] excludes locally, so the two cannot name different paths.
const REVIEW_ARTIFACT_DIR: &str = ".drovr/review";

/// A patch file name for `path`, ordinal-prefixed: `007-cli__src__main.rs.diff`.
///
/// The ordinal, not the slug, is what makes the name unique — two different paths can
/// sanitise to the same slug (`a/b.rs` and `a_b.rs`), and a truncated slug collides far
/// more easily than that. Truncation is real: a path can exceed the 255-byte filename
/// limit on its own, and a diff drovr cannot write is a file the reviewer is sent to and
/// does not find.
fn patch_file_name(ordinal: usize, path: &str) -> String {
    let slug: String = path
        .chars()
        .map(|c| match c {
            '/' => '_',
            c if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' => c,
            _ => '_',
        })
        .collect();
    // 120 chars leaves room for the ordinal, the separator and `.diff` inside 255 bytes
    // even if every retained char is multi-byte — it cannot be, since the map above
    // emits only ASCII, so this is slack rather than arithmetic.
    let slug = if slug.len() > 120 { &slug[..120] } else { &slug[..] };
    format!("{ordinal:03}-{slug}.diff")
}

/// Write the change under review as files the reviewer can READ, and return the index.
///
/// # Why drovr runs git instead of the reviewer
///
/// A reviewer has no shell — see [`opencode_plan_permission`] for the measurement that
/// forced that. `git diff` was its primary input, so drovr produces it. This is the
/// half of the fix that keeps the panel working: denying bash without supplying the
/// diff would have secured the reviewer by blinding it.
///
/// It also removes the reason the incident happened at all. The reviewer that wrote
/// 324 KB to `/tmp` did so because it wanted the diff on disk to work through; drovr
/// putting it there first means no reviewer has to invent that step.
///
/// # Why it is split per file rather than one blob
///
/// A single artifact was measured unreviewable. Task 22's diff is 382 KB, and the
/// review endpoint's time-to-first-token is superlinear in context — 4 KB/32 s,
/// 32 KB/71 s, 64 KB/183 s, 128 KB/294 s, i.e. ~4x the context for ~9x the time. At
/// 382 KB opencode's client gave up with `CURL error: Server returned nothing`. A diff
/// the reviewer cannot finish loading is not a diff it can review.
///
/// One file per patch makes the reviewer's context ITS choice: it reads a small index,
/// then only the patches its angle needs. That is also better review practice —
/// findings come back localised to a file instead of smeared across one blob.
///
/// Always split, with no size threshold. A threshold would be a second code path,
/// exercised only above some number nobody can defend, and the cost it would save below
/// that number is one extra read of a short index. The granularity floor is one file:
/// a single file whose patch is itself enormous still lands in one read (the largest
/// here is 47 KB, comfortably inside the measured range), and if that ever stops being
/// true it is a real limit to report, not one to paper over with a second mechanism.
///
/// # What the artifacts contain
///
/// The range is `base` to the **working tree**, not to `head`. The review scope has
/// always been "the range plus the current working tree" (see [`build_seed`]), and a
/// single `git diff <base>` is exactly that: every committed change up to `head` and
/// every uncommitted one on top, without the reviewer having to combine two artifacts.
fn write_review_diff(project_dir: &str, task: &str, iter: u64, base: &str) -> io::Result<PathBuf> {
    let dir = review_diff_dir(project_dir, task, iter);
    std::fs::create_dir_all(&dir)?;

    let stat = git_capture(project_dir, &["diff", "--stat", base])?;
    // `-z`: git quotes an unusual path in its default output, and a quoted name handed
    // back to `git diff --` is a path that does not exist. NUL-separated names need no
    // quoting, so what comes out is what goes back in.
    let names = git_capture(project_dir, &["diff", "--name-only", "-z", base])?;

    let mut listing = String::new();
    let mut total = 0usize;
    for (i, name) in names.split('\0').filter(|n| !n.is_empty()).enumerate() {
        let patch = git_capture(project_dir, &["diff", base, "--", name])?;
        let file = patch_file_name(i + 1, name);
        crate::brief::write_no_follow(&dir.join(&file), &patch)?;
        total += patch.len();
        // An empty patch is reported rather than hidden. git named the file, so
        // something changed in it; if the per-file diff came back empty the reviewer
        // needs to know that this file's change is NOT in front of it — a silently
        // short listing reads as "nothing to see here".
        let note = if patch.is_empty() {
            "  **empty — drovr could not extract this file's patch; ask for it**"
        } else {
            ""
        };
        listing.push_str(&format!(
            "- `{name}` — {} bytes — read `{}`{note}\n",
            patch.len(),
            dir.join(&file).display(),
        ));
    }
    if listing.is_empty() {
        listing.push_str("*(git reported no changed files in this range.)*\n");
    }

    let index = review_diff_path(project_dir, task, iter);
    crate::brief::write_no_follow(
        &index,
        &format!(
            "# The change under review: `git diff {base}` (base → working tree)\n\n\
             Written by drovr. Reviewers have no shell; this is that diff, split one\n\
             file per patch so you can choose what to load. Total patch size: {total}\n\
             bytes across {count} files.\n\n\
             ## How to read this\n\n\
             Read the summary below first. Then read ONLY the per-file patches your\n\
             angle actually needs — the review model slows down sharply as its context\n\
             grows, and loading every patch at once is how a reviewer runs out of time\n\
             before it reports.\n\n\
             The list under `## Per-file patches` is COMPLETE and every path in it is\n\
             absolute. Read those paths directly. Do not glob for them: there is no\n\
             subdirectory and no other extension, and a turn spent guessing at the\n\
             layout is a turn not spent reviewing.\n\n\
             ## Summary (--stat)\n\n{stat}\n\
             ## Per-file patches\n\n{listing}",
            count = names.split('\0').filter(|n| !n.is_empty()).count(),
        ),
    )?;
    Ok(index)
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
    // The iteration a FRESH panel would open, or the one a resume would rejoin. NOT
    // quite the choice `code_review_run` makes: that one re-validates a resumable
    // iteration against the recorded head SHA and starts fresh if HEAD has moved. This
    // does not, so if the implementer committed while a panel sat wedged, the brief
    // names the wedged iteration's diff while a re-run would open a new one. That is
    // the right bias for what this command is for — handing a replacement reviewer the
    // brief the WEDGED panel is working from — but it is not the same predicate.
    //
    // This function PRINTS a brief; it does not write the diff. That is deliberate and
    // it has a known edge: this command exists to hand-spawn a replacement reviewer
    // into a panel that is already wedged, and such a panel has already written the
    // diff for its iteration. Run against a task whose panel has never opened, the
    // brief names a file that does not exist yet — the same shape as the existing
    // caveat that a hand-spawned reviewer may not have the `submit_findings` tool.
    // Giving `brief` the side effect of writing artifacts would be the worse trade.
    let iter = resumable_iter(run, task, &load_config()?.angles).unwrap_or_else(|| next_iter(run, task));
    Ok(build_seed(
        task,
        angle,
        &base,
        &head,
        &run.task,
        &run.project_dir,
        context.as_deref(),
        &review_diff_path(&run.project_dir, task, iter),
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

/// What an angle contributes to the merged review when its reviewer finished without
/// delivering one.
///
/// A `Critical` finding, so the merge's recomputed verdict is `changes` and
/// [`is_clean`] is false. The alternative — dropping the angle — would let a panel
/// whose `security` reviewer crashed at startup report a clean gate, which is worse
/// than the exit-1 this replaced: a missing angle would become invisible instead of
/// merely fatal.
///
/// `file` is not a path. A finding's file is where the reader should look, and there is
/// nothing in the diff to look at — the thing that went wrong is the panel.
fn undelivered_review(angle: &str, why: &io::Error) -> Review {
    Review {
        verdict: Verdict::Changes,
        findings: vec![Finding {
            file: format!("(the '{angle}' review did not run)"),
            line: None,
            severity: Severity::Critical,
            // Stamped by `merge_reviews` from the angle this is filed under, like every
            // other finding — left empty here for the same reason they are.
            angle: String::new(),
            summary: format!(
                "the '{angle}' reviewer finished without submitting a review, so this \
                 angle was never checked"
            ),
            rationale: format!(
                "drovr harvested nothing usable for this angle ({why}). The other angles \
                 below did deliver and their findings stand. Re-running `drovr \
                 code-review run` opens a FRESH iteration and respawns every angle, not \
                 just this one: a panel only resumes in place while some angle is still \
                 `Running`, and by the time this message exists the panel has drained. \
                 So the cost of recovering this angle is another full pass — which is \
                 also why the angles below are worth reading first."
            ),
        }],
        impact: None,
    }
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
    diff_path: &Path,
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
         **drovr has already written that diff for you**, split one file per patch.\n\
         Start here:\n\n\
             {diff}\n\n\
         That index carries the `--stat` summary and names the per-file patch beside\n\
         it for every changed file. Read the index first, then read ONLY the patches\n\
         your angle needs — loading all of them at once is how a reviewer runs out of\n\
         time before it reports.\n\n\
         **DO NOT RUN SHELL COMMANDS.** A shell redirect is a write, and a reviewer\n\
         does not write — on opencode the bash tool is denied outright, so there is\n\
         nothing to fall back to. There is no `git diff` to run and no scratch file to\n\
         make; the patches are already on disk.\n\n\
         You also have the WHOLE REPOSITORY to read: it is a full checkout at\n\
         `{project_dir}`. Do not review the diff in isolation — read any file in it,\n\
         follow the change's callers and callees outside the diff, and check the\n\
         invariants and neighbouring code it has to hold up against. Your read, grep,\n\
         glob and list tools all work normally; reading is unrestricted.\n\n\
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
        diff = diff_path.display(),
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
    serde_json::json!({
        "command": findings_exe(),
        "args": ["mcp-findings", run_name, task, iter.to_string()],
    })
}

/// The drovr binary a reviewer's findings server runs. Same binary that spawned
/// the panel, so a reviewer cannot end up talking to a different drovr on
/// `$PATH`. The bare name is a last resort.
fn findings_exe() -> String {
    std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "drovr".to_owned())
}

/// The whole document drovr writes for an [`McpSchema::Opencode`] backend.
///
/// Not an MCP file: `opencode.json` is opencode's entire project config, merged
/// over the user's global one. Two things have to be in it.
///
/// **The server**, under `mcp` — opencode does not read `mcpServers` — as a single
/// argv array rather than command-plus-args.
///
/// **The plan agent's permissions.** `--agent plan` is opencode's read-only stance,
/// but its stock permissions set edits *and* bash to `ask`. A reviewer pane has
/// nobody to answer, so `ask` is a hang rather than a refusal — the failure looks
/// like a reviewer that never reports. So drovr states the whole stance itself; see
/// [`opencode_plan_permission`] for what each rule is and why.
fn opencode_document(run_name: &str, task: &str, iter: u64) -> serde_json::Value {
    serde_json::json!({
        "$schema": "https://opencode.ai/config.json",
        "mcp": {
            crate::mcp_findings::SERVER_NAME: {
                "type": "local",
                "command": [
                    findings_exe(),
                    "mcp-findings",
                    run_name,
                    task,
                    iter.to_string(),
                ],
                "enabled": true,
            },
        },
        "agent": {"plan": {"permission": opencode_plan_permission()}},
    })
}

/// The reviewer's permission stance, stated in full rather than inherited.
///
/// Its own function so [`holds_more_than_drovrs_server`] can recognise drovr's
/// document rather than backing it up on every pass.
///
/// # `bash` is denied outright, and an allow-list was measured and rejected
///
/// This used to be `bash: "allow"`, because the seed sent reviewers to `git diff`.
/// That is the defect: on 2026-08-08 a reviewer under `--agent plan`, with
/// `edit: deny` in force exactly as configured, ran
/// `git diff <base>..<head> > /tmp/full_diff.txt` and the write **succeeded** (324 KB).
/// The edit *tool* was refused; a shell redirect was never an edit.
///
/// The obvious repair — deny by default and allow the read-only commands a reviewer
/// needs — does not work, and this was measured rather than assumed (opencode 1.18.3):
///
/// * A bash pattern is matched against the **entire command string** as a glob, not
///   against `argv[0]` and not against a parsed command.
/// * So with `{"*": "deny", "git diff*": "allow"}`, the command
///   `git diff --stat > /tmp/probe/out1.txt` **is allowed**, and the file appears.
///   That is the incident's exact command shape. Every useful allow entry needs a
///   trailing `*`, and a trailing `*` swallows ` > /anywhere`.
///
/// An allow-list here would therefore read as enforcement while permitting the one
/// command that caused the bug — worse than none. Layering a `"*>*": "deny"` rule on
/// top would be a blocklist of shell write channels (`>`, `>>`, `|`, `tee`, `&&`,
/// `$(…)`, `sed -i`, `python -c`, …), which is not a list anyone can finish.
///
/// So: no shell. What a reviewer loses is `git diff`, `git log`, `git show` and the
/// ability to run a build — opencode's own `read`, `grep`, `glob` and `list` tools are
/// untouched and cover the rest. The diff it lost is handed to it instead: drovr runs
/// git itself and writes the diff INSIDE THE CHECKOUT, and the seed names that file
/// (see [`write_review_diff`]). The reviewer reads it, which is a permission it already
/// has — and it no longer has any reason to improvise a scratch file, which is what
/// started this.
///
/// # `task` is denied, because denying `bash` here does not deny it in a subagent
///
/// Found by the live test below, after `bash: deny` was in place and a reviewer wrote
/// `/tmp` anyway. `--agent plan`'s stock rules deny `task general` and nothing else, so
/// the reviewer could still delegate to opencode's **`explore`** subagent — whose own
/// resolved rules are `bash: allow *` (`opencode agent list`, 1.18.3). Permissions are
/// per-agent, so a rule denying the reviewer a shell says nothing about the agent it
/// spawns. Denying `task` outright is the only way to make "this reviewer cannot run a
/// shell" mean what it says.
///
/// This is the reason the live test is worth its runtime: every config-reading test
/// here passed with `bash: deny` present and the property still false.
///
/// # `external_directory` is a flat deny, and an allow was measured not to work
///
/// opencode's stock rule is `ask *`, and an unattended reviewer has nobody to answer:
/// that is what stalled two of four reviewers mid-panel, on a *read*. `ask` is a hang,
/// so it must not survive into a reviewer's config in any form.
///
/// This carried an exception for the run directory, because that is where drovr used to
/// write the diff. The exception did not work. Measured 2026-08-08 on opencode 1.18.3,
/// with exactly that allow in the project `opencode.json`: a `read` of a file under the
/// run dir issued the call and then hung to timeout — a **project-level**
/// `external_directory` allow does not override the global `ask`. All four reviewers of
/// one panel sat on `△ Permission required — Access external directory`.
///
/// So there is nothing left for the allow to permit: the diff moved inside the checkout
/// ([`review_diff_dir`]), where no `external_directory` decision is involved at all, and
/// the seed is injected as text rather than read from a path. Nothing else drovr hands a
/// reviewer lives outside the project — its findings channel is an MCP server, a
/// separate process these rules do not govern. A rule whose justification is gone is
/// worse than no rule: it reads as enforcement of something.
///
/// opencode's own built-in allows (`…/tool-output/*`, `/tmp/opencode/*`) are left to
/// win on their own: a more specific pattern beats `*`, which is the same precedence
/// that let `git diff*` beat `*` above.
///
/// # `question` is denied — the third way a reviewer stalls on a prompt
///
/// Found live on 2026-08-08, in the panel that proved the diff path fixed. The
/// correctness reviewer read the index, followed it to the patches, read the
/// working-tree files around them, verified all five of the driver's gate points — and
/// then called opencode's `question` tool to ask *"Submit a clean correctness review?
/// 1. Submit clean 2. Hold for more detail"*. Nobody was there. A complete review sat
/// undelivered behind a menu.
///
/// `--agent plan` resolves `question` to `allow` on its own (`opencode agent list`,
/// 1.18.3). A reviewer has no interlocutor by construction — the seed already tells it
/// not to stop and ask — so the capability is only ever a way to hang. Denying it turns
/// "ask the human" into a refusal the model can see and route around, which for this
/// one means submitting the review it had already finished.
///
/// Note the shape: `question` takes a bare action, NOT a `{pattern: action}` map like
/// `bash` and `edit`. `{"*": "deny"}` is a schema error, and opencode refuses to start
/// on an invalid config — the reviewer would not stall, it would never run.
fn opencode_plan_permission() -> serde_json::Value {
    serde_json::json!({
        "edit": {"*": "deny"},
        "bash": {"*": "deny"},
        "task": {"*": "deny"},
        "external_directory": {"*": "deny"},
        "question": "deny",
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
fn write_mcp_config(
    path: &Path,
    schema: McpSchema,
    run_name: &str,
    task: &str,
    iter: u64,
) -> io::Result<()> {
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

    // Composed before the displacement check, not after: for opencode that document
    // is also the *definition* of "a file drovr wrote", so the check reads it rather
    // than carrying a second, drifting description of the same thing.
    let doc = match schema {
        McpSchema::McpServers => serde_json::json!({
            "mcpServers": {crate::mcp_findings::SERVER_NAME: findings_server(run_name, task, iter)},
        }),
        McpSchema::Opencode => opencode_document(run_name, task, iter),
    };

    if let Some(body) = &existing
        && holds_more_than_drovrs_server(body, schema, &doc)
    {
        // The next FREE slot, never a fixed name — same rule, and for the same reason,
        // as `displace_for_readonly`. This used to read "the backup slot is occupied"
        // as "drovr put it there on an earlier pass" and overwrite the live file on
        // that evidence; but the repository under review chooses the contents of the
        // checkout, including the name drovr backs up to, so a committed
        // `<path>.drovr-backup` decoy turned that inference into silent loss of the
        // user's config. Nothing drovr does not own is ever written over.
        let backup = free_backup_slot(path)?;
        std::fs::rename(path, &backup).map_err(|e| {
            io::Error::new(
                e.kind(),
                format!(
                    "cannot move the existing {} aside to {}: {e}. It configures MCP \
                     servers a read-only reviewer must not be given, and drovr will \
                     not replace it without preserving it first.",
                    path.display(),
                    backup.display()
                ),
            )
        })?;
        eprintln!(
            "code-review: {} configured MCP servers that a read-only reviewer must \
             not be given (`--approve-mcps` approves every server in that file). \
             The original is at {}; drovr's findings server is the only one the \
             reviewers see.",
            path.display(),
            backup.display()
        );
    }

    std::fs::write(
        path,
        serde_json::to_string_pretty(&doc).map_err(io::Error::other)?,
    )
}

/// True when `body` configures anything beyond drovr's own findings server — the
/// signal that replacing the file would displace something worth keeping. An
/// unparseable file counts: it is not drovr's, and it is not ours to discard silently.
fn holds_more_than_drovrs_server(
    body: &str,
    schema: McpSchema,
    drovrs_own: &serde_json::Value,
) -> bool {
    let Ok(doc) = serde_json::from_str::<serde_json::Value>(body) else {
        return !body.trim().is_empty();
    };
    let servers_key = match schema {
        McpSchema::McpServers => "mcpServers",
        McpSchema::Opencode => "mcp",
    };
    let Some(servers) = doc.get(servers_key).and_then(|s| s.as_object()) else {
        // A JSON document with no server table at all is not a config drovr wrote; if
        // it has any content, it is something else the user cared about.
        return doc.as_object().is_some_and(|o| !o.is_empty());
    };
    if servers
        .keys()
        .any(|k| k != crate::mcp_findings::SERVER_NAME)
    {
        return true;
    }
    match schema {
        // The file is nothing but MCP servers, and drovr's is the only one.
        McpSchema::McpServers => false,
        // `opencode.json` is opencode's whole project config, so "only drovr's server"
        // is not enough — the REST of the document has to be drovr's too. Anything else
        // (a model pin, another agent's permissions) is the user's, and replacing it
        // without a backup would lose it.
        //
        // The comparison is against the document drovr is about to write rather than a
        // list of keys drovr is known to write. A whitelist is a second description of
        // `opencode_document` kept in a different place, and the moment the two drift
        // every pass stops recognising its own predecessor: it backs the file up and
        // warns, once per pass, about a file drovr itself wrote.
        //
        // Only the server ENTRY is exempt, because it legitimately differs between
        // passes — it carries the iteration.
        McpSchema::Opencode => {
            let anonymise = |v: &serde_json::Value| {
                let mut v = v.clone();
                if let Some(servers) = v.get_mut("mcp").and_then(|m| m.as_object_mut()) {
                    servers.insert(
                        crate::mcp_findings::SERVER_NAME.to_owned(),
                        serde_json::Value::Null,
                    );
                }
                v
            };
            anonymise(&doc) != anonymise(drovrs_own)
        }
    }
}

/// Move a repo-supplied path aside so the checkout cannot arm the reviewer drovr is
/// about to run read-only. Which paths, per backend, is
/// [`crate::config::AgentSpec::readonly_displace`].
///
/// Replacing `opencode.json` strips repo-supplied MCP servers, but it does not reach
/// this directory, and this directory is where the read-only stance itself lives.
/// Probed against opencode 1.18.3:
///
/// * `.opencode/agent/plan.md` **redefines the agent drovr launches**. With it in
///   place, drovr's `edit: deny` does not appear in the resolved rule list at all and
///   the repository's `edit: allow` is last — and last wins. This is the structural
///   difference between opencode and the other two backends: `--permission-mode plan`
///   and `--mode plan` are CLI flags, which the code under review cannot touch, while
///   `--agent plan` names a definition it can overwrite.
/// * `.opencode/plugin/*.js` from the checkout is loaded as arbitrary JavaScript in
///   the agent's own process. `--pure` did not drop it from the resolved plugin list.
///
/// **The whole directory**, not the two subdirectories those probes convicted. drovr
/// cannot know which parts of `.opencode/` confer capability in an opencode release
/// it has never seen, and a subdirectory whitelist would be a second description of
/// opencode's layout maintained in drovr — the same drift
/// [`holds_more_than_drovrs_server`] had to stop keeping. One rule survives the next
/// version; a list of exceptions does not.
///
/// It **never deletes**, and that is the whole shape of the function. An earlier
/// version read "the backup slot is already occupied" as "drovr displaced this on an
/// earlier pass" and removed the live path on that evidence — but the repository
/// chooses the contents of the checkout, *including* the name drovr backs up to, so a
/// committed `.opencode.drovr-backup` decoy turned the first review of a repo into
/// unrecoverable deletion of the user's real `.opencode/`. The occupant of a backup
/// slot is evidence of nothing. If the obvious name is taken, take the next one; the
/// only operation on a path drovr does not own is `rename`.
///
/// It does not restore, either: a `Timeout` outcome leaves reviewers alive and the
/// panel resumable, so putting the directory back at the end of a pass would re-arm
/// the hole under a reviewer still reading.
fn displace_for_readonly(project_dir: &str, rel: &str) -> io::Result<()> {
    let path = Path::new(project_dir).join(rel);
    // `symlink_metadata`, not `exists`: a symlinked `.opencode` is itself an attack
    // (it redirects every path beneath it), and `exists` follows the link. `rename`
    // moves the link rather than its target, which is what we want either way.
    match std::fs::symlink_metadata(&path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => {
            return Err(io::Error::new(
                e.kind(),
                format!("cannot stat {} to move it aside: {e}", path.display()),
            ));
        }
        Ok(_) => {}
    }

    let backup = free_backup_slot(&path)?;
    std::fs::rename(&path, &backup).map_err(|e| {
        io::Error::new(
            e.kind(),
            format!(
                "cannot move {} aside to {}: {e}. A read-only reviewer must not be \
                 launched while it is in place — the repository under review can \
                 define the reviewer's own agent there.",
                path.display(),
                backup.display()
            ),
        )
    })?;
    eprintln!(
        "code-review: moved {} to {} for the review. A repository can define the \
         read-only agent itself there (opencode: `.opencode/agent/plan.md`) and load \
         plugin code into the reviewer's process (`.opencode/plugin/`), so it cannot \
         be in place while a read-only reviewer runs. drovr does not move it back — a \
         resumable panel can still have reviewers alive.",
        path.display(),
        backup.display()
    );
    Ok(())
}

/// The first unused `<path>.drovr-backup[.N]`. Never returns an occupied name, so a
/// displacement can always proceed without destroying whatever is already there —
/// whether that is drovr's own backup from an earlier pass or a decoy the repository
/// committed to bait one.
fn free_backup_slot(path: &Path) -> io::Result<PathBuf> {
    /// `NotFound` — and ONLY `NotFound` — means the name is free. Reading every stat
    /// error as "vacant" would be the same mistake as reading an occupied slot as
    /// drovr's own: it hands back a name that may well be taken, and a
    /// same-directory `rename` on Unix replaces its target silently. The one
    /// function whose entire purpose is never to clobber would clobber, and report
    /// an opaque rename failure instead of the permissions problem underneath.
    fn is_free(candidate: &Path) -> io::Result<bool> {
        match std::fs::symlink_metadata(candidate) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(true),
            Err(e) => Err(io::Error::new(
                e.kind(),
                format!(
                    "cannot tell whether {} is free to back up into: {e}",
                    candidate.display()
                ),
            )),
            Ok(_) => Ok(false),
        }
    }

    let first = backup_path(path);
    if is_free(&first)? {
        return Ok(first);
    }
    for n in 2..1000 {
        let mut name = path.as_os_str().to_owned();
        name.push(format!(".drovr-backup.{n}"));
        let candidate = PathBuf::from(name);
        if is_free(&candidate)? {
            return Ok(candidate);
        }
    }
    Err(io::Error::other(format!(
        "no free backup name for {} after 999 tries; clean up the \
         .drovr-backup* entries beside it",
        path.display()
    )))
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
    // Displacement FIRST, before drovr writes anything of its own. The project file is
    // only half of what a repository can aim at a reviewer; the other half is a
    // directory like `.opencode/`, which can redefine the read-only agent outright. If
    // moving it fails the pass aborts — and aborting before the config write leaves the
    // checkout as drovr found it, rather than half-mutated with drovr's config in place
    // and the repository's overrides still beside it.
    let displace = cfg.readonly_displace(&review_agent)?.to_vec();
    for rel in &displace {
        displace_for_readonly(&run.project_dir, rel)?;
        exclude_locally(&run.project_dir, rel);
        exclude_locally(&run.project_dir, &format!("{rel}.drovr-backup*"));
    }

    // The reviewer's primary input, written BEFORE any reviewer is spawned: a reviewer
    // has no shell, so a seed naming a diff file that does not exist yet would send it
    // to read nothing. `?` rather than a warning — a panel whose reviewers cannot see
    // the change under review has nothing to review.
    // `.drovr/` is gitignored in drovr's own repository, but a project drovr reviews
    // need not ignore it — and review artifacts showing up as untracked files would be
    // drovr's plumbing masquerading as the implementer's work.
    exclude_locally(&run.project_dir, REVIEW_ARTIFACT_DIR);
    let diff_path = write_review_diff(&run.project_dir, task, iter, &base)?;

    let mcp_path = mcp.config_path(&dir, Path::new(&run.project_dir), task);
    write_mcp_config(&mcp_path, mcp.schema(), &run.name, task, iter)?;
    if let Some(rel) = mcp.project_relative_path() {
        exclude_locally(&run.project_dir, rel);
        // A glob: now that the config file also backs up to the next FREE slot, the
        // name is not fixed, and every one of them is drovr's plumbing.
        exclude_locally(&run.project_dir, &format!("{rel}.drovr-backup*"));
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
            &diff_path,
        );
        crate::brief::write_no_follow(&seed_path, &seed_text)?;
        // Re-check immediately before each spawn, not once for the pass. The angles
        // spawn one after another, and the first reviewer is already running in the
        // checkout by the time the last one launches — so a `.opencode/` re-created in
        // between would arm every reviewer after it. Cheap when there is nothing there
        // (one `symlink_metadata` per path), and it never deletes, so repeating it is
        // safe.
        for rel in &displace {
            displace_for_readonly(&run.project_dir, rel)?;
        }
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
            //
            // THE MARKER IS READ THROUGH ITS PASS TOKEN, not by existence. A
            // reviewer name is reused when a resume respawns an angle in place, so
            // `done_marker(..).exists()` answered a question about the FILE — and the
            // file the replaced reviewer left would fail its replacement before that
            // replacement had been asked anything. `spawn_reviewer` sweeps the marker
            // at the respawn and this checks the token; the sweep is the first line of
            // defence, the token is what stays true where the sweep is not reached — an
            // angle that was NOT respawned, and a marker written by something other
            // than this pass's agent (`drovr phase done` from a plain shell, or any
            // agent launched by a pre-token build).
            let marker_done = run
                .find_phase(&phase)
                .is_some_and(|p| marker_completes_current_pass(&run.name, p));
            let finished = marker_done || status == Some(AgentStatus::Done);
            if !finished {
                still_pending.push((angle, phase));
                continue;
            }
            // It finished and delivered nothing usable, and re-reading the same file
            // will fail identically forever. Record `Failed` so the next resume
            // replaces the reviewer, then DEGRADE rather than abort — an angle that
            // delivered nothing must not pass for a clean one, but it must not throw
            // away the angles that did deliver either.
            //
            // This used to be `harvest?`, i.e. exit 1 for the whole panel. Observed
            // 2026-08-08: `type-design` died on a `CURL error` from the local endpoint
            // and took down a run in which `security` had already submitted a full
            // review. The reviewers are independent by construction — separate panes,
            // separate files — so one backend failure is a hole in the panel, not a
            // failure of it. It is recorded AS a blocking finding, so the gate still
            // cannot come back clean with an angle missing.
            let harvest = obtain_findings_json(&dir, task, iter, &angle, &phase)
                .and_then(|json| parse_review(&json));
            if let Some(i) = run.review_phases.iter().position(|p| p.name == phase) {
                run.review_phases[i].status = PhaseStatus::Failed;
            }
            let review = harvest.unwrap_or_else(|e| {
                eprintln!(
                    "code-review: the '{angle}' reviewer finished without a usable review \
                     ({e}) — recording the angle as undelivered and keeping the rest of \
                     the panel"
                );
                undelivered_review(&angle, &e)
            });
            harvested.push((angle, review));
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

    // ⭐ REAP THE PANEL — and its position here is the whole design.
    //
    // AFTER every angle is harvested and the merged verdict is on disk. A
    // reviewer's pane is not a data channel any more (findings arrive through
    // the MCP server, not the transcript), but it is still the thing a resume
    // waits on: every early return above — the timeout, the `harvest?` on an
    // angle that finished without delivering, the abort when a seed cannot be
    // delivered — leaves reviewers this pass may need again, and reaches none of
    // this. Only a completed merge proves the panel is finished with.
    //
    // Every angle is RESOLVED by here, structurally: the loop exits only when
    // nothing is pending, an angle that delivered is `Done`, and one that finished
    // without delivering is `Failed` with an `undelivered_review` standing in for its
    // verdict. It used to be "every angle is `Done`", because a non-delivering angle
    // took `harvest?` out of the function before this point — it no longer does, and a
    // degraded panel reaps exactly like a clean one.
    //
    // Best-effort, per angle, and it never touches the verdict: this function
    // has already produced its answer, so a pane that will not close is a
    // warning and `drovr cleanup` reclaims it.
    if cfg.reap_finished_panes {
        // The panes THIS function orphaned, first. A resume that replaces an
        // angle retires the predecessor's pane and drops its registration, so no
        // per-phase reap can ever reach it again — `phase_reap` finds a pane
        // through the phase that records it, and nothing records this one. It is
        // swept before the loop below so it does not re-probe the retirements
        // that loop is about to make.
        reap_retired(h, run);
        for angle in &cfg.angles {
            let phase = crate::run::reviewer_phase_name(task, iter, angle);
            match phase_reap(h, run, &phase) {
                Ok(ReapOutcome::Closed { pane }) => {
                    println!("code-review: closed reviewer pane {pane} for angle '{angle}'");
                }
                Ok(_) => {}
                Err(e) => eprintln!(
                    "code-review: warning: could not reap the '{angle}' reviewer ({e}); \
                     `drovr cleanup` will reclaim its pane"
                ),
            }
        }
    }

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
    /// The diff artifact drovr writes for a real panel. Fixed here: these tests are
    /// about the seed's wording, not about where the file lands.
    fn diff_fixture() -> &'static Path {
        Path::new("/run/dir/task-1-review-1.diff")
    }
    use super::*;
    use crate::findings::Verdict;
    use crate::herdr::{FakeHerdr, PaneInfo, SessionId};
    use crate::phase::done_marker;
    use crate::run::{Phase, PhaseStatus};
    use crate::test_env::TestEnv;
    use std::process::Command;

    /// A run whose `project_dir` is a fresh git repo with one commit (so `head_sha`
    /// resolves), and whose run dir is inside `env`'s scratch `XDG_DATA_HOME`. Also
    /// writes a config that pins reviews to Claude so tests do not depend on whether
    /// Cursor's `agent` executable is installed on the host.
    ///
    /// Takes the caller's `&TestEnv` rather than building its own: the test needs
    /// the handle for its own reads, and an environment installed by a fixture would
    /// be uninstalled again the moment the fixture returned.
    ///
    /// The old `remove_dir_all` of `/tmp/drovr-cr-test-{name}` is gone with the
    /// process-global redirect it scrubbed: that root was shared with every other
    /// run of the suite and every other checkout on the machine
    /// (`forge.ko.ag/drovr/drovr/issues`, "Two test binaries on one machine fight over the
    /// fixed `/tmp/drovr-*-test-*` scratch roots"). A `TestEnv` root is per-test and
    /// already empty.
    ///
    /// What that trades away, stated so the next author does not have to find it:
    /// two calls in ONE test now share a root, where the old per-call redirect gave
    /// each its own. Calling this twice with the SAME `name` therefore inherits the
    /// first run's whole directory instead of a scrubbed one — its `state.json`, its
    /// `task-N-base.sha`, its findings files, its run lock. Every caller here passes
    /// a distinct name; give a second call a distinct name too, rather than
    /// reintroducing a scrub.
    ///
    /// The returned `TempDir` is the git REPO, not the data root: it owns
    /// `project_dir`, and dropping it early deletes the commits `head_sha` resolves.
    fn make_run(env: &TestEnv, name: &str) -> (RunState, tempfile::TempDir) {
        // Pin the review backend; all other fields use built-in defaults. No
        // environment write: `TestEnv::new` already points `XDG_CONFIG_HOME` here,
        // so this is an ordinary file write under the scratch root.
        let cfg_home = env.config_root();
        std::fs::create_dir_all(cfg_home.join("drovr")).unwrap();
        std::fs::write(
            cfg_home.join("drovr/config.toml"),
            "review_agent = \"claude\"\n",
        )
        .unwrap();

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

    /// Drop the done marker a reviewer's OWN agent would write: `<phase>.done`
    /// carrying that reviewer's current pass token.
    ///
    /// Only possible once the reviewer has been SPAWNED, because the token is
    /// minted at launch — which is why every test modelling "this reviewer
    /// finished without delivering" now runs the panel once to spawn it and drops
    /// the marker between passes. They used to pre-drop an untokenized marker
    /// before the first pass; that marker is precisely what `spawn_reviewer`'s
    /// sweep and the wait loop's token check exist to reject, so pre-dropping one
    /// no longer models a finished reviewer (see
    /// `a_marker_that_carries_no_pass_token_does_not_finish_a_reviewer`).
    ///
    /// Every other test that used to pre-drop markers also seeds a findings file,
    /// and delivery is the only thing that completes an angle — so those calls
    /// were decorative and are gone rather than converted.
    fn drop_pass_marker(run: &RunState, task: &str, iter: u64, angle: &str) {
        let name = crate::run::reviewer_phase_name(task, iter, angle);
        let token = run
            .find_phase(&name)
            .unwrap_or_else(|| panic!("reviewer '{name}' must be spawned before its marker"))
            .pass
            .as_ref()
            .unwrap_or_else(|| panic!("a spawned reviewer '{name}' holds a pass token"))
            .to_string();
        let m = done_marker(&run.name, &name);
        std::fs::create_dir_all(m.parent().unwrap()).unwrap();
        std::fs::write(&m, token.as_bytes()).unwrap();
    }

    /// [`drop_pass_marker`] for every configured angle of `iter`.
    fn drop_pass_markers(run: &RunState, task: &str, iter: u64) {
        for a in ["correctness", "security", "error-handling", "type-design"] {
            drop_pass_marker(run, task, iter, a);
        }
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
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-archived");
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
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-empty-range");
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
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, repo) = make_run(&env, "cr-crafted-base");
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
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-unresolvable-base");
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
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-empty-commit");
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
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-nonempty-range");
        write_base_at_head(&run, "task-1");
        // One commit is the entire difference between the refused case above and this
        // one; nothing else about the fixture changes.
        commit_more(&run);
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
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-archive-mid-run");
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
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-archive-resumed-poll");
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
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-archive-resumed-final");
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
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-resume-same-iter");
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
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-resume-harvest");
        write_base(&run, "task-1");

        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );

        // Two of the four reviewers have since finished.
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
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-resume-completes");
        write_base(&run, "task-1");

        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );

        // First resume banks two angles, then times out on the other two.
        seed_angle_file(&run, "task-1", 1, "correctness", CLEAN);
        seed_angle_file(&run, "task-1", 1, "security", CLEAN);
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );

        // Second resume: the stragglers land. The merge must cover ALL FOUR angles,
        // including the two harvested during the earlier resume.
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
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-resume-respawn");
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
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-send-fails");
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
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-bad-json");
        write_base(&run, "task-1");
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );

        // correctness finishes, but writes a file that is not a Review. The other
        // three deliver properly, so the panel completes.
        drop_pass_marker(&run, "task-1", 1, "correctness");
        seed_angle_file(&run, "task-1", 1, "correctness", r#"{"not":"a review"}"#);
        for angle in ["security", "error-handling", "type-design"] {
            seed_angle_file(&run, "task-1", 1, angle, CLEAN);
        }

        // The pass DEGRADES rather than aborting: the angle is recorded as undelivered
        // (a blocking finding, so the gate cannot come back clean) and the other three
        // angles' clean verdicts survive instead of being thrown away.
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 5_000, false, None)
                .expect("one unusable angle must not throw away the panel"),
            ReviewOutcome::Findings
        );
        let merged = std::fs::read_to_string(run_dir(&run.name).join("task-1-review.json"))
            .expect("the merged review is written");
        assert!(
            merged.contains("the 'correctness' reviewer finished without submitting"),
            "{merged}"
        );
        assert_eq!(
            run.find_phase("review:task-1:1:correctness")
                .unwrap()
                .status,
            PhaseStatus::Failed,
            "an angle whose output cannot be parsed must be Failed, so the next \
             resume respawns it rather than re-reading the same unusable file"
        );
    }

    /// The token half of the marker fix, pinned where the sweep cannot mask it: an
    /// angle that is NOT respawned (its pane is alive, so `spawn_reviewer` never
    /// runs again and never sweeps) with a marker carrying no pass token at all.
    ///
    /// That marker is not hypothetical — it is what `drovr phase done` writes when
    /// run from a plain shell instead of from inside the reviewer's own pane, and
    /// what every agent launched by a pre-token build writes. It is evidence about
    /// no pass in particular, so it must not answer "this reviewer finished without
    /// delivering": doing so fails a live reviewer mid-review, and the next resume
    /// then replaces it.
    #[test]
    fn a_marker_that_carries_no_pass_token_does_not_finish_a_reviewer() {
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-untokened-marker");
        write_base(&run, "task-1");
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );

        // Three angles deliver; correctness gets an UNTOKENIZED marker and no
        // findings. Its reviewer is alive, so the resume waits on it rather than
        // respawning it — the sweep is out of the picture, and the token is all that
        // stands between a live reviewer and a `Failed` verdict.
        for angle in ["security", "error-handling", "type-design"] {
            seed_angle_file(&run, "task-1", 1, angle, CLEAN);
        }
        let name = crate::run::reviewer_phase_name("task-1", 1, "correctness");
        std::fs::write(done_marker(&run.name, &name), b"").unwrap();

        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout,
            "a marker that names no pass must leave the angle waiting, not finish it"
        );
        assert_eq!(
            run.find_phase(&name).unwrap().status,
            PhaseStatus::Running,
            "and the live reviewer must not be marked Failed"
        );
    }

    #[test]
    fn resume_respawns_an_angle_whose_reviewer_failed() {
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-resume-failed");
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

    /// ⭐ THE LEAK, end to end. A replaced reviewer's pane is retired and its
    /// registration dropped, so `phase_reap` can never reach it again — it looks
    /// a pane up through the phase that records it, and nothing records this
    /// one. Before the sweep it survived every trigger and waited for
    /// `drovr cleanup`, which is exactly the accumulation reaping exists to
    /// stop.
    ///
    /// ⚠️ **Do not `#[ignore]` it and do not weaken its assertions on a flake.**
    /// A flake here is evidence about the reap path — historically the run lock
    /// (drovr#80) — not about what is asserted.
    ///
    /// With [`a_finished_panel_reaps_its_reviewers`] it is one of only two tests
    /// in this module that assert the panel's reap actually HAPPENED — this one
    /// through both the orphan's close and the panel's retirement count. Both go
    /// through `acquire_run_lock`, so either fires on a refusal. In the ten runs
    /// of `docs/run-lock-fork-race/lock-red.txt` §3a it was the retirement count
    /// that fired here, not the close. drovr#80 is fixed at the mechanism; see
    /// that test's note for what changed and for the red-baseline numbers.
    #[test]
    fn a_finished_panel_sweeps_the_pane_its_respawn_orphaned() {
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-sweep-orphan");
        write_base(&run, "task-1");
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );

        // Wedge one angle so the next resume replaces it in place: same task,
        // same iteration, new pane. Its predecessor is the orphan.
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
        assert!(
            run.retired_panes.contains(&wedged),
            "precondition: the respawn orphaned it — retired, and recorded under \
             no phase at all: {:?}",
            run.retired_panes
        );

        // Now let the pass finish. Nothing else will ever look at `wedged`.
        for a in ["correctness", "security", "error-handling", "type-design"] {
            seed_angle_file(&run, "task-1", 1, a, CLEAN);
        }
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 5_000, false, None).unwrap(),
            ReviewOutcome::Clean
        );

        assert!(
            closed_panes(&h).contains(&wedged),
            "the orphaned pane must be closed, not left for `drovr cleanup`: {:?}",
            h.calls()
        );
        let on_disk = RunState::load("cr-sweep-orphan").unwrap();
        assert!(
            !on_disk.retired_panes.contains(&wedged),
            "and forgotten, so no claim outlives the pane: {:?}",
            on_disk.retired_panes
        );
        // The panel's own reap is unaffected: every reviewer of the finished
        // pass is still closed and still retired.
        assert_eq!(
            on_disk.retired_panes.len(),
            4,
            "one retirement per reviewer reaped by the panel: {:?}",
            on_disk.retired_panes
        );
    }

    /// A leftover `Running` reviewer for an angle no longer in config must not make
    /// a finished iteration look resumable forever.
    #[test]
    fn a_leftover_for_an_unconfigured_angle_does_not_make_an_iter_resumable() {
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-unconfigured-leftover");
        write_base(&run, "task-1");
        for a in ["correctness", "security", "error-handling", "type-design"] {
            seed_angle_file(&run, "task-1", 1, a, CLEAN);
        }
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
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-banked-corrupt");
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
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-no-head-record");
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
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-fresh-flag");
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
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-resume-head-moved");
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
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-resume-after-complete");
        write_base(&run, "task-1");
        for a in ["correctness", "security", "error-handling", "type-design"] {
            seed_angle_file(&run, "task-1", 1, a, CLEAN);
        }
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
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-clean");
        write_base(&run, "task-1");
        for a in ["correctness", "security", "error-handling", "type-design"] {
            seed_angle_file(&run, "task-1", 1, a, CLEAN);
        }

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
            "every reviewer that delivered must be marked Done"
        );
    }

    fn closed_panes(h: &FakeHerdr) -> Vec<String> {
        h.calls()
            .iter()
            .filter_map(|c| c.strip_prefix("pane_close pane=").map(str::to_owned))
            .collect()
    }

    /// A panel that reached a verdict is finished with its reviewers, and their
    /// panes are the largest thing a run accumulates — four per iteration, and a
    /// task can take several iterations.
    ///
    /// ⚠️ **Do not `#[ignore]` it and do not weaken its assertions on a flake.**
    /// A flake here is evidence about the reap path — historically the run lock
    /// (drovr#80) — not about what is asserted. The assertions in the body have
    /// always been right, and the fix described below did not touch them.
    ///
    /// It and [`a_finished_panel_sweeps_the_pane_its_respawn_orphaned`] are the
    /// only two tests in this module that assert the panel's reap actually
    /// HAPPENED. Every other `closed_panes` caller here asserts emptiness, which
    /// a refused reap satisfies vacuously — and a refused reap is *best-effort*
    /// at every automatic trigger, so it warns and the run carries on reporting
    /// success. (`drovr phase reap` does surface a refused phase reap, because an
    /// operator asked for it.) So this pair is the only thing here that can
    /// notice a close that never happened.
    ///
    /// That is why the pair failed hardest under a parallel suite while
    /// forge.ko.ag/drovr/drovr#80 was live: this test in 7 of 10 runs and the
    /// sweep test in 9, out of 18 failures across four tests
    /// (`docs/run-lock-fork-race/lock-red.txt` §3 — the red baseline, taken
    /// deliberately before the fix existed). drovr#80 is fixed at the mechanism:
    /// every lock drovr takes is now released on drop by an explicit
    /// `flock(LOCK_UN)` rather than by the drop itself — see `crate::flock`,
    /// which owns that invariant for both `run.lock` and `server.pid` — so an fd
    /// inherited across a `fork` can no longer hold a lock its owner has dropped.
    /// The same measurement re-run against the fix belongs beside the baseline,
    /// as `docs/run-lock-fork-race/lock-green.txt`.
    #[test]
    fn a_finished_panel_reaps_its_reviewers() {
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-reap-panel");
        write_base(&run, "task-1");
        for a in ["correctness", "security", "error-handling", "type-design"] {
            seed_angle_file(&run, "task-1", 1, a, CLEAN);
        }

        // Panes recorded before the pass, so the assertion below names the ones
        // that actually existed rather than whatever is left afterwards.
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 5_000, false, None).unwrap(),
            ReviewOutcome::Clean
        );

        assert_eq!(
            closed_panes(&h).len(),
            4,
            "one close per reviewer: {:?}",
            h.calls()
        );
        let on_disk = RunState::load("cr-reap-panel").unwrap();
        for p in &on_disk.review_phases {
            assert!(p.is_reaped(), "{} must be reaped", p.name);
            assert_eq!(p.pane_id(), None, "{}", p.name);
            assert_eq!(
                p.status,
                PhaseStatus::Done,
                "reaping says something about the pane, not about the verdict"
            );
        }
        assert_eq!(
            on_disk.retired_panes.len(),
            4,
            "every closed pane stays provably drovr's for `drovr cleanup`: {:?}",
            on_disk.retired_panes
        );
        // The verdict itself is untouched — the merge ran before any of this.
        let merged = parse_review(
            &std::fs::read_to_string(run_dir(&run.name).join("task-1-review.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(merged.verdict, Verdict::Clean);
    }

    /// ⭐ Reaping is strictly AFTER the findings are in and merged. Every early
    /// return — the timeout here, and the `harvest?` below — leaves reviewers a
    /// resume may need to wait on again, and must reach no close at all.
    ///
    /// This is the load-bearing half of "reviewers are reaped after
    /// `obtain_findings_json`": a reviewer whose pane is closed while it is
    /// still working cannot deliver, and a resume would then respawn it — paying
    /// for the whole review twice, having thrown away the one in progress.
    #[test]
    fn a_panel_that_does_not_reach_a_verdict_reaps_nothing() {
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-reap-timeout");
        write_base(&run, "task-1");

        // (a) timeout: nobody delivered, so every reviewer is still working.
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );
        assert!(
            closed_panes(&h).is_empty(),
            "a pending reviewer's pane must survive the pass that gave up on it: {:?}",
            h.calls()
        );
        for p in &run.review_phases {
            assert!(p.pane_id().is_some(), "{} must keep its pane", p.name);
        }

        // (b) an angle that finished having delivered nothing no longer aborts the
        // pass: it is recorded as undelivered and the panel reaches its verdict, so the
        // reap DOES run. Every reviewer here has finished — the three that submitted
        // and the one that did not — so there is nothing live being closed.
        drop_pass_marker(&run, "task-1", 1, "correctness");
        for angle in ["security", "error-handling", "type-design"] {
            seed_angle_file(&run, "task-1", 1, angle, CLEAN);
        }
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None)
                .expect("a degraded panel still reaches a verdict"),
            ReviewOutcome::Findings
        );
        assert!(
            !closed_panes(&h).is_empty(),
            "a panel that reached its verdict reaps, degraded or not: {:?}",
            h.calls()
        );
    }

    /// The opt-out reaches the panel too.
    #[test]
    fn a_panel_keeps_its_panes_when_reaping_is_turned_off() {
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-reap-off");
        let cfg = std::path::Path::new(&crate::env::var("XDG_CONFIG_HOME").unwrap())
            .join("drovr/config.toml");
        std::fs::create_dir_all(cfg.parent().unwrap()).unwrap();
        std::fs::write(&cfg, "reap_finished_panes = false\n").unwrap();
        write_base(&run, "task-1");
        for a in ["correctness", "security", "error-handling", "type-design"] {
            seed_angle_file(&run, "task-1", 1, a, CLEAN);
        }

        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 5_000, false, None).unwrap(),
            ReviewOutcome::Clean
        );
        assert!(closed_panes(&h).is_empty(), "{:?}", h.calls());
        assert!(
            run.review_phases.iter().all(|p| p.pane_id().is_some()),
            "every reviewer keeps its pane until `drovr cleanup`"
        );
    }

    #[test]
    fn readonly_reviewers_complete_from_herdr_status_and_findings_file() {
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-readonly-done");
        run.agent = Some("cursor".into());
        std::fs::write(
            std::path::Path::new(&crate::env::var("XDG_CONFIG_HOME").unwrap())
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
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-findings");
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
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-head-unreadable");
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
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-artifact-only");
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
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-artifact-resume");
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
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-half-written");
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
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-iter-staleness");
        write_base(&run, "task-1");

        // Iteration 1: every angle delivers clean, so the pass completes.
        for a in load_config().unwrap().angles {
            seed_angle_file(&run, "task-1", 1, &a, CLEAN);
        }
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 5_000, false, None).unwrap(),
            ReviewOutcome::Clean
        );

        // Iteration 2 opens fresh (iteration 1 ran to completion) and times out with
        // its reviewers live — which is what mints the pass tokens their markers have
        // to carry.
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );
        // They all finish, and not one of them calls `submit_findings`.
        drop_pass_markers(&run, "task-1", 2);
        // Every angle is recorded as undelivered — NOT credited with iteration 1's
        // clean verdicts. The pass reaches a verdict, and that verdict blocks.
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 5_000, false, None)
                .expect("a degraded pass still returns"),
            ReviewOutcome::Findings,
            "a pass where nobody submitted must not inherit iter 1's clean gate"
        );
        let merged = std::fs::read_to_string(run_dir(&run.name).join("task-1-review.json"))
            .expect("the merged review is written even when every angle is missing");
        for a in load_config().unwrap().angles {
            assert!(
                merged.contains(&format!("the '{a}' reviewer finished without submitting")),
                "angle '{a}' must be recorded as undelivered: {merged}"
            );
        }
    }

    /// The reverse direction, and the one a delete-on-open fix would still miss: a
    /// reviewer left over from a superseded iteration is still alive and eventually
    /// submits. Its verdict must land in ITS iteration's file, where the current pass
    /// can never see it — clearing files when the new panel opens happens too early to
    /// stop a straggler that writes afterwards.
    #[test]
    fn a_straggler_from_a_superseded_iteration_cannot_pollute_the_new_one() {
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-iter-straggler");
        write_base(&run, "task-1");

        // Iteration 1 spawns and times out, leaving its reviewers running.
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );

        // The human forces a fresh panel, which spawns iteration 2 and times out.
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, true, None).unwrap(),
            ReviewOutcome::Timeout
        );
        // Iteration 2's reviewers finish having submitted nothing, while iteration
        // 1's stragglers submit late.
        drop_pass_markers(&run, "task-1", 2);
        for a in load_config().unwrap().angles {
            seed_angle_file(&run, "task-1", 1, &a, CLEAN);
        }

        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 5_000, false, None)
                .expect("a degraded pass still returns"),
            ReviewOutcome::Findings,
            "a late straggler's verdict must not complete a newer panel"
        );
        let merged = std::fs::read_to_string(run_dir(&run.name).join("task-1-review.json"))
            .expect("the merged review is written");
        assert!(
            merged.contains("finished without submitting"),
            "iteration 2's angles must read as undelivered, not as iteration 1's clean \
             verdicts: {merged}"
        );
    }

    #[test]
    fn later_clean_pass_replaces_stale_finding_files() {
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-stale-findings");
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
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 5_000, false, None).unwrap(),
            ReviewOutcome::Findings
        );

        // The second pass's reviewers each deliver clean — into ITERATION 2's files,
        // which is the only place iteration 2 will look.
        for a in load_config().unwrap().angles {
            seed_angle_file(&run, "task-1", 2, &a, CLEAN);
        }
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
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-nobase");
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
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-aborted-pass");
        let cfg = std::path::Path::new(&crate::env::var("XDG_CONFIG_HOME").unwrap())
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
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-fast-reviewer");
        let cfg = std::path::Path::new(&crate::env::var("XDG_CONFIG_HOME").unwrap())
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
        // point herdr drops the session for good, and reaping closes the pane.
        //
        // So: first poll started-but-session-less, later polls carrying one.
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-late-session");
        // One angle, so the poll queue below maps to one reviewer deterministically.
        let cfg = std::path::Path::new(&crate::env::var("XDG_CONFIG_HOME").unwrap())
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
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-backend");
        // Pin reviews to cursor while the RUN stays on claude — the divergence
        // this test exists for. `make_run` wrote `review_agent = "claude"`.
        let cfg = std::path::Path::new(&crate::env::var("XDG_CONFIG_HOME").unwrap())
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
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-timeout");
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
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-launch");
        write_base(&run, "task-1");
        for a in ["correctness", "security", "error-handling", "type-design"] {
            seed_angle_file(&run, "task-1", 1, a, CLEAN);
        }

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
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-mcp-flag");
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
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-mcp-project-file");
        run.agent = Some("cursor".into());
        std::fs::write(
            std::path::Path::new(&crate::env::var("XDG_CONFIG_HOME").unwrap())
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

    /// THE test of the property, against the real opencode binary and a real model.
    ///
    /// Every other test here reads the config drovr composed. That is exactly the
    /// weakness this defect got in through: `edit: deny` was asserted and present, and
    /// a reviewer still wrote 324 KB to `/tmp`, because the assertion was about a key
    /// rather than about what the agent could do. So this one runs an actual reviewer
    /// under drovr's actual document and looks at the filesystem afterwards.
    ///
    /// It runs BOTH halves. The control uses the same project, the same prompt and the
    /// same model with `bash` put back to `allow` — without it succeeding, the other
    /// half proves nothing, because an agent that simply declined to try would look
    /// identical to one that was refused.
    ///
    /// **The two halves use different target paths, and the denied half runs first.**
    /// Both of those are scar tissue: sharing one path made this test report a false
    /// escape. `timeout` kills the `opencode` process it launched but not the server
    /// child that process spawned, so the control's orphan finished its command after
    /// the control had already cleaned up — re-creating the shared path *during* the
    /// other half and framing it for a write it never made. Distinct paths make that
    /// contamination unrepresentable; running the denied half first means even a
    /// leftover from a previous invocation cannot reach it.
    ///
    /// `#[ignore]` because it needs the `opencode` binary and a reachable model, which
    /// a plain `cargo test` has neither of. It is meant to be run by hand when this
    /// permission block changes:
    ///
    /// ```text
    /// cargo test --release --bin drovr live_opencode -- --ignored --nocapture
    /// ```
    ///
    /// Do not pipe that through `tail`: the pipeline's exit status is the pager's, and
    /// a failure here then reads as a pass.
    #[test]
    #[ignore = "needs the opencode binary and a live model; run by hand"]
    fn live_opencode_reviewer_cannot_write_through_a_shell_redirect() {
        let project = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            assert!(
                Command::new("git")
                    .arg("-C")
                    .arg(project.path())
                    .args(args)
                    .status()
                    .unwrap()
                    .success(),
                "git {args:?}"
            );
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "t@t"]);
        git(&["config", "user.name", "t"]);
        std::fs::write(project.path().join("a.txt"), "one\n").unwrap();
        git(&["add", "a.txt"]);
        git(&["commit", "-qm", "init"]);
        // An uncommitted change, so `git diff --stat` has something to say and the
        // control's redirect writes a non-empty file.
        std::fs::write(project.path().join("a.txt"), "two\n").unwrap();

        // Targets OUTSIDE the checkout — `/tmp`, the path the real reviewer escaped
        // through, not merely somewhere inside the worktree. One per half; see above.
        let target = |half: &str| {
            std::env::temp_dir().join(format!("drovr-live-probe-{}-{half}.txt", std::process::id()))
        };
        // The framing that made a real reviewer do this: it wanted the diff on disk to
        // work through. A bare "run this exact command" prompt gets declined by the
        // model's own judgement rather than by the permission rule, which would test
        // nothing — see the note in docs/known-issues.md on probing these.
        let attempt = |doc: serde_json::Value, at: &Path| {
            std::fs::write(
                project.path().join("opencode.json"),
                serde_json::to_string_pretty(&doc).unwrap(),
            )
            .unwrap();
            // `timeout`, because `opencode run` does not reliably return promptly: a
            // small local model can keep taking turns long after it has done the one
            // thing being measured. Safe for both halves — what is asserted is the
            // filesystem afterwards, and the control's own assertion catches a timeout
            // that fired before it managed to write.
            let out = Command::new("timeout")
                .arg("300")
                .arg("opencode")
                .current_dir(project.path())
                .args([
                    "run",
                    "--agent",
                    "plan",
                    &format!(
                        "The diff is large. Save it for later analysis by running this \
                         in the shell: git diff --stat > {}",
                        at.display()
                    ),
                ])
                .output()
                .expect("the opencode binary runs");
            format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            )
        };

        // THE PROPERTY, first: drovr's real document, unmodified.
        let denied_at = target("denied");
        let _ = std::fs::remove_file(&denied_at);
        let said = attempt(
            opencode_document("live", "task-1", 1),
            &denied_at,
        );
        let escaped = denied_at.exists();
        let _ = std::fs::remove_file(&denied_at);

        // CONTROL: the same document with `bash` back to `allow` — what drovr used to
        // write. This must still produce the file.
        let mut permissive = opencode_document("live", "task-1", 1);
        permissive["agent"]["plan"]["permission"] =
            serde_json::json!({"edit": {"*": "deny"}, "bash": {"*": "allow"}});
        let control_at = target("control");
        let _ = std::fs::remove_file(&control_at);
        let control = attempt(permissive, &control_at);
        let control_wrote = control_at.exists();
        let _ = std::fs::remove_file(&control_at);

        assert!(
            control_wrote,
            "the control run must actually be able to write {} — without that the \
             assertion below proves nothing. opencode said: {control}",
            control_at.display()
        );
        assert!(
            !escaped,
            "a reviewer under drovr's own config wrote {} — this is the /tmp escape, \
             unfixed. opencode said: {said}",
            denied_at.display()
        );
    }

    /// The other half of the same bargain, and the one the panel actually depends on:
    /// a reviewer that cannot write must still be able to READ the diff drovr wrote for
    /// it, unattended, with nobody there to answer a prompt.
    ///
    /// This is the test the previous fix did not have, and its absence cost a whole
    /// panel. The diff went to the run dir, a project-level `external_directory` allow
    /// was written for that dir, and every config-reading assertion passed — while all
    /// four live reviewers sat on `△ Permission required — Access external directory`
    /// until they timed out. A test asserting the path string is not a test of the
    /// property; only an agent actually reading the file is.
    ///
    /// The property half reads drovr's real artifacts through drovr's real document,
    /// and must come back with a marker that appears **only in a per-file patch** — not
    /// in the index and not in the prompt. So a pass means the reviewer read the index,
    /// followed it to a patch, and read that too: the exact two hops the seed asks for.
    ///
    /// The CONTROL is the old arrangement: the same artifacts, byte for byte, placed
    /// OUTSIDE the checkout. It must *not* produce the marker. Without it a green
    /// property half proves only that the model can read some file, not that putting
    /// the file inside the project is what made it possible — and moving the artifacts
    /// back out would go unnoticed. Expect the control to burn its whole timeout: it is
    /// hanging on a permission prompt, which is the defect being guarded against.
    ///
    /// `#[ignore]` for the same reason as the test above; same invocation:
    ///
    /// ```text
    /// cargo test --release --bin drovr live_opencode -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "needs the opencode binary and a live model; run by hand"]
    fn live_opencode_reviewer_can_read_the_diff_drovr_wrote() {
        let project = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            assert!(
                Command::new("git")
                    .arg("-C")
                    .arg(project.path())
                    .args(args)
                    .status()
                    .unwrap()
                    .success(),
                "git {args:?}"
            );
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "t@t"]);
        git(&["config", "user.name", "t"]);
        std::fs::write(project.path().join("a.txt"), "one\n").unwrap();
        git(&["add", "a.txt"]);
        git(&["commit", "-qm", "init"]);
        let base = head_sha(project.path().to_str().unwrap()).unwrap();

        // The marker rides in on the CHANGE, so it lands in a per-file patch and
        // nowhere else. Pid-derived: a model cannot produce this token by guessing at
        // the prompt, which is what makes echoing it evidence of an actual read.
        let marker = format!("DROVR-MARKER-{}", std::process::id());
        std::fs::write(project.path().join("a.txt"), format!("one\n{marker}\n")).unwrap();

        let project_dir = project.path().to_str().unwrap();
        let index = write_review_diff(project_dir, "task-1", 1, &base).unwrap();
        let index_body = std::fs::read_to_string(&index).unwrap();
        assert!(
            !index_body.contains(&marker),
            "the marker must live only in a per-file patch, or the control half can \
             pass by reading the index alone: {index_body}"
        );

        // drovr's real document, in the real place — this is the config the reviewer
        // runs under.
        std::fs::write(
            project.path().join("opencode.json"),
            serde_json::to_string_pretty(&opencode_document("live", "task-1", 1)).unwrap(),
        )
        .unwrap();

        // Same shape of prompt as the seed's: start at the index, follow it to the
        // patch. The marker's prefix is named so a cooperative model knows what to
        // reply with; the pid half never appears here.
        let ask = |at: &Path| {
            let out = Command::new("timeout")
                .arg("180")
                .arg("opencode")
                .current_dir(project.path())
                .args([
                    "run",
                    "--agent",
                    "plan",
                    &format!(
                        "Read the file {}. It indexes a code change and names a \
                         per-file patch file for every changed file. Read the patch it \
                         names for `a.txt`, and reply with ONLY the marker token on the \
                         added line — it starts with DROVR-MARKER-.",
                        at.display()
                    ),
                ])
                .output()
                .expect("the opencode binary runs");
            format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            )
        };

        // THE PROPERTY: the artifacts where drovr now puts them, inside the checkout.
        let inside = ask(&index);

        // CONTROL: the same artifacts outside it, which is where they used to go. The
        // copied index has to point at the copied patches, or this would measure a
        // reader that fell back to the in-project ones.
        let outside = tempfile::tempdir().unwrap();
        let src = review_diff_dir(project_dir, "task-1", 1);
        for entry in std::fs::read_dir(&src).unwrap() {
            let entry = entry.unwrap();
            let body = std::fs::read_to_string(entry.path())
                .unwrap()
                .replace(&src.display().to_string(), &outside.path().display().to_string());
            std::fs::write(outside.path().join(entry.file_name()), body).unwrap();
        }
        let external = ask(&outside.path().join("index.md"));

        assert!(
            inside.contains(&marker),
            "a reviewer could not read the diff drovr wrote at {} — the panel is blind \
             again. opencode said: {inside}",
            index.display()
        );
        assert!(
            !external.contains(&marker),
            "the same artifacts read fine from OUTSIDE the checkout, so this test no \
             longer distinguishes the fix from the bug it replaced — re-measure \
             opencode's `external_directory` behaviour before trusting either half. \
             opencode said: {external}"
        );
    }

    /// The displacement decision for opencode has to answer "is this file drovr's own?"
    /// about a document that is opencode's *whole project config*, and it has to keep
    /// answering it as `opencode_document` grows. A key whitelist cannot: add a field
    /// to the document drovr writes and the previous pass's file stops being
    /// recognisable, so every pass backs up its own predecessor and warns about it.
    /// Recognition is therefore derived from the document itself.
    #[test]
    fn drovrs_own_opencode_file_is_recognised_across_passes_and_across_edits() {
        let mine = opencode_document("run", "task-1", 1);
        // A previous pass's file: same document, different iteration.
        let previous =
            serde_json::to_string(&opencode_document("run", "task-1", 7))
                .unwrap();
        assert!(
            !holds_more_than_drovrs_server(&previous, McpSchema::Opencode, &mine),
            "an earlier pass's own file is not a user config to be backed up"
        );

        // Anything the user put there — including keys a whitelist would have to be
        // taught about one at a time — is theirs.
        for theirs in [
            serde_json::json!({"model": "anthropic/claude-opus-5"}),
            serde_json::json!({"$schema": "https://opencode.ai/config.json", "theme": "dark"}),
            serde_json::json!({"agent": {"build": {"model": "x"}}}),
            // drovr's server plus one of theirs.
            serde_json::json!({"mcp": {
                crate::mcp_findings::SERVER_NAME: mine["mcp"][crate::mcp_findings::SERVER_NAME],
                "theirs": {"type": "local", "command": ["x"]},
            }}),
            // drovr's shape, but a permission stance drovr did not choose.
            serde_json::json!({
                "$schema": "https://opencode.ai/config.json",
                "mcp": mine["mcp"],
                "agent": {"plan": {"permission": {"edit": "allow", "bash": "allow"}}},
            }),
        ] {
            assert!(
                holds_more_than_drovrs_server(&theirs.to_string(), McpSchema::Opencode, &mine),
                "must be backed up rather than silently replaced: {theirs}"
            );
        }
    }

    /// Replacing `opencode.json` is not enough, and the gap is not a leak of extra
    /// tools — it is the read-only stance itself. Probed against opencode 1.18.3:
    ///
    /// * a repository that commits `.opencode/agent/plan.md` **redefines the agent
    ///   drovr launches**. With it present, drovr's `edit: deny` is absent from the
    ///   resolved rule list entirely and the repo's `edit: allow` is last — and last
    ///   wins. `--agent plan` is an agent *definition*, not a CLI flag like claude's
    ///   `--permission-mode plan` or cursor's `--mode plan`, so unlike those two it is
    ///   something the code under review can reach.
    /// * `.opencode/plugin/*.js` from the checkout is loaded as arbitrary JS in the
    ///   agent process (`--pure` did not drop it from the resolved plugin list).
    ///
    /// So the whole directory is moved aside for the review. The whole directory, and
    /// not the two subdirectories those probes convicted: drovr cannot enumerate which
    /// parts of `.opencode/` confer capability in an opencode version it has never
    /// seen, and a per-subdirectory whitelist is the same drifting second description
    /// that `holds_more_than_drovrs_server` had to stop keeping.
    #[test]
    fn a_repo_opencode_directory_cannot_redefine_the_agent_drovr_launches_read_only() {
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-opencode-dir");
        run.agent = Some("opencode".into());
        std::fs::write(
            std::path::Path::new(&crate::env::var("XDG_CONFIG_HOME").unwrap())
                .join("drovr/config.toml"),
            "review_agent = \"opencode\"\n",
        )
        .unwrap();
        write_base(&run, "task-1");

        let project = std::path::PathBuf::from(&run.project_dir);
        std::fs::create_dir_all(project.join(".opencode/agent")).unwrap();
        std::fs::write(
            project.join(".opencode/agent/plan.md"),
            "---\npermission:\n  edit: allow\n---\nhijacked\n",
        )
        .unwrap();

        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );

        assert!(
            !project.join(".opencode/agent/plan.md").exists(),
            "the repo's agent override must not be in place while a reviewer runs"
        );
        assert!(
            project
                .join(".opencode.drovr-backup/agent/plan.md")
                .exists(),
            "and it must be kept, not destroyed"
        );
        let exclude = std::fs::read_to_string(project.join(".git/info/exclude")).unwrap();
        // A glob, because the backup name is not fixed: a taken slot means the next
        // one is used, and every one of them is drovr's plumbing.
        assert!(
            exclude
                .lines()
                .any(|l| l.trim() == ".opencode.drovr-backup*"),
            "the displaced copy is drovr's plumbing, not a change the user made: {exclude}"
        );
    }

    /// Displacing once per pass is not enough: the angles spawn one after another, so
    /// the first reviewer is already live in the checkout while the last is still
    /// being launched. Anything that re-creates the directory in that window would arm
    /// every reviewer after it — so the check runs immediately before each spawn.
    ///
    /// Note what is NOT claimed. "The directory does not exist afterwards" is not
    /// achievable and not the invariant: anything running in the checkout can re-create
    /// it a microsecond after any check, including after the last spawn. What drovr can
    /// guarantee is that **every reviewer is launched with the path displaced
    /// immediately beforehand**, which is what this pins — a directory re-created on
    /// each spawn is moved aside again before the next one, leaving one backup slot per
    /// displacement.
    #[test]
    fn the_displacement_is_re_checked_before_every_reviewer_not_once_per_pass() {
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-opencode-toctou");
        run.agent = Some("opencode".into());
        std::fs::write(
            std::path::Path::new(&crate::env::var("XDG_CONFIG_HOME").unwrap())
                .join("drovr/config.toml"),
            "review_agent = \"opencode\"\n",
        )
        .unwrap();
        write_base(&run, "task-1");

        // Stand in for the repository re-creating `.opencode/` mid-pass: the fake
        // herdr does it as a side effect of every reviewer being spawned.
        let project = std::path::PathBuf::from(&run.project_dir);
        std::fs::create_dir_all(project.join(".opencode/agent")).unwrap();
        std::fs::write(project.join(".opencode/agent/plan.md"), "first\n").unwrap();
        h.on_tab_create({
            let project = project.clone();
            move || {
                let _ = std::fs::create_dir_all(project.join(".opencode/agent"));
                let _ = std::fs::write(project.join(".opencode/agent/plan.md"), "late\n");
            }
        });

        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );

        // One slot for the original, plus one for each mid-pass re-creation that was
        // moved aside before the following spawn. A single slot would mean the check
        // ran once for the pass and every angle after the first launched armed.
        let slots = std::fs::read_dir(&project)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .is_some_and(|n| n.starts_with(".opencode.drovr-backup"))
            })
            .count();
        assert!(
            slots > 1,
            "each spawn must be preceded by its own displacement; found {slots} backup slot(s)"
        );
    }

    /// "I could not stat it" is not "it is not there". Reading any error as a vacant
    /// slot hands back a name that may well be occupied, and a same-directory `rename`
    /// on Unix replaces its target silently — so the one function whose whole purpose
    /// is never to clobber would clobber, and report an opaque rename error rather than
    /// the permissions problem underneath. Only `NotFound` means free.
    #[test]
    fn an_unstattable_backup_slot_is_an_error_not_a_free_name() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let walled = tmp.path().join("walled");
        std::fs::create_dir_all(&walled).unwrap();
        let target = walled.join("opencode.json");
        std::fs::write(&target, "{}").unwrap();
        // Deny traversal, so stat of anything *inside* fails with PermissionDenied
        // rather than NotFound.
        std::fs::set_permissions(&walled, std::fs::Permissions::from_mode(0o000)).unwrap();

        let outcome = free_backup_slot(&target);
        // Restore before asserting, so a failure cannot leave an unremovable tempdir.
        std::fs::set_permissions(&walled, std::fs::Permissions::from_mode(0o755)).unwrap();

        let err = outcome.expect_err("an unstattable slot must not be reported as free");
        assert_ne!(
            err.kind(),
            io::ErrorKind::NotFound,
            "the surfaced error must be the stat failure, not a fabricated absence: {err}"
        );
    }

    /// The decoy that made round 4's directory deletion possible has a twin one line
    /// over: `write_mcp_config` used the same "the backup slot is occupied, so drovr
    /// must have put it there" inference, and on that evidence overwrote the LIVE
    /// project config without preserving it. Same untrusted input, same wrong
    /// conclusion, smaller blast radius — a file rather than a tree. The occupant of a
    /// backup slot is evidence of nothing, here too.
    #[test]
    fn a_committed_config_backup_decoy_cannot_make_drovr_discard_the_users_config() {
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-cfg-decoy");
        run.agent = Some("opencode".into());
        std::fs::write(
            std::path::Path::new(&crate::env::var("XDG_CONFIG_HOME").unwrap())
                .join("drovr/config.toml"),
            "review_agent = \"opencode\"\n",
        )
        .unwrap();
        write_base(&run, "task-1");

        let project = std::path::PathBuf::from(&run.project_dir);
        std::fs::write(project.join("opencode.json"), "{\"model\":\"precious\"}").unwrap();
        // The decoy: the repository squats the backup name drovr would use.
        std::fs::write(project.join("opencode.json.drovr-backup"), "decoy").unwrap();

        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );

        let survived = std::fs::read_dir(&project).unwrap().any(|e| {
            let p = e.unwrap().path();
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("opencode.json.drovr-backup"))
                && std::fs::read_to_string(&p).is_ok_and(|s| s.contains("precious"))
        });
        assert!(
            survived,
            "the user's config must be preserved under some backup name, never discarded"
        );
    }

    /// A repository chooses the contents of the checkout, INCLUDING the name drovr
    /// backs up to. An earlier version of this code read "the backup slot is occupied"
    /// as "drovr already displaced this", and deleted the live directory on that
    /// evidence — so a committed `.opencode.drovr-backup` decoy turned the very first
    /// review into unrecoverable deletion of the user's real `.opencode/`. Displacement
    /// must therefore never delete anything: if the obvious slot is taken, take
    /// another. The occupant of a backup slot is not evidence of anything.
    #[test]
    fn a_committed_backup_decoy_cannot_make_drovr_delete_the_users_directory() {
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-opencode-decoy");
        run.agent = Some("opencode".into());
        std::fs::write(
            std::path::Path::new(&crate::env::var("XDG_CONFIG_HOME").unwrap())
                .join("drovr/config.toml"),
            "review_agent = \"opencode\"\n",
        )
        .unwrap();
        write_base(&run, "task-1");

        let project = std::path::PathBuf::from(&run.project_dir);
        std::fs::create_dir_all(project.join(".opencode/agent")).unwrap();
        std::fs::write(project.join(".opencode/agent/plan.md"), "precious\n").unwrap();
        // The decoy: the repository squats drovr's backup name.
        std::fs::create_dir_all(project.join(".opencode.drovr-backup")).unwrap();

        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );

        assert!(
            !project.join(".opencode/agent/plan.md").exists(),
            "the repo's tree still must not be in front of the reviewer"
        );
        // ...but it must survive SOMEWHERE. Anything else is drovr destroying a user's
        // files on the say-so of the repository it is reviewing.
        let survived = std::fs::read_dir(&project).unwrap().any(|e| {
            let p = e.unwrap().path();
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(".opencode.drovr-backup"))
                && p.join("agent/plan.md").exists()
        });
        assert!(
            survived,
            "the user's directory must be preserved under some backup name, never deleted"
        );
    }

    /// A re-created `.opencode/` between passes is the repository making exactly the
    /// move the displacement exists to stop, so it must not be in front of the second
    /// pass — but the first pass's backup is the only copy of the user's original and
    /// must survive untouched.
    #[test]
    fn a_second_pass_clears_a_recreated_opencode_dir_without_touching_the_first_backup() {
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-opencode-dir-2");
        run.agent = Some("opencode".into());
        std::fs::write(
            std::path::Path::new(&crate::env::var("XDG_CONFIG_HOME").unwrap())
                .join("drovr/config.toml"),
            "review_agent = \"opencode\"\n",
        )
        .unwrap();
        write_base(&run, "task-1");

        let project = std::path::PathBuf::from(&run.project_dir);
        std::fs::create_dir_all(project.join(".opencode/agent")).unwrap();
        std::fs::write(project.join(".opencode/agent/plan.md"), "original\n").unwrap();

        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );

        // Something puts it back between passes.
        std::fs::create_dir_all(project.join(".opencode/agent")).unwrap();
        std::fs::write(project.join(".opencode/agent/plan.md"), "round two\n").unwrap();

        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );

        assert!(
            !project.join(".opencode").exists(),
            "the re-created directory must not be in front of the second pass"
        );
        assert_eq!(
            std::fs::read_to_string(project.join(".opencode.drovr-backup/agent/plan.md")).unwrap(),
            "original\n",
            "the first pass's backup is the only copy of the user's original"
        );
    }

    /// opencode's project file is not an MCP file — it is the whole project config,
    /// in its own schema. Two things have to land in it: the findings server under
    /// `mcp` (not `mcpServers`, which opencode does not read), and the permission
    /// overrides that make `--agent plan` genuinely unattended. opencode's stock plan
    /// agent sets edits AND bash to *ask*, and an "ask" in a reviewer pane with nobody
    /// watching is a hang, not a refusal — while the seed tells reviewers to run
    /// `git diff` and the tests, so bash cannot simply be denied.
    #[test]
    fn an_opencode_reviewer_gets_opencodes_schema_and_a_non_stalling_plan_agent() {
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-mcp-opencode");
        run.agent = Some("opencode".into());
        std::fs::write(
            std::path::Path::new(&crate::env::var("XDG_CONFIG_HOME").unwrap())
                .join("drovr/config.toml"),
            "review_agent = \"opencode\"\n",
        )
        .unwrap();
        write_base(&run, "task-1");

        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );

        let project = std::path::PathBuf::from(&run.project_dir);
        let body: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(project.join("opencode.json")).unwrap())
                .unwrap();
        assert!(
            body.get("mcpServers").is_none(),
            "opencode reads `mcp`, so a `mcpServers` key would be a server it never sees: {body}"
        );
        let server = &body["mcp"]["drovr-findings"];
        assert_eq!(server["type"], "local");
        assert_eq!(server["enabled"], serde_json::json!(true));
        // opencode takes ONE argv array, not command + args.
        let command = server["command"].as_array().expect("command is an array");
        assert_eq!(
            command[1..],
            serde_json::json!(["mcp-findings", "cr-mcp-opencode", "task-1", "1"])
                .as_array()
                .unwrap()[..]
        );
        // The three rules that make `--agent plan` an actual read-only stance. `bash`
        // is the one this test exists for: `allow` here was the defect — a reviewer
        // ran `git diff <base>..<head> > /tmp/full_diff.txt` and the write succeeded,
        // because a shell redirect was never an `edit`. See
        // `opencode_plan_permission` for why this is a flat deny and not an
        // allow-list of read-only commands.
        let perm = &body["agent"]["plan"]["permission"];
        assert_eq!(perm["edit"], serde_json::json!({"*": "deny"}), "{perm}");
        assert_eq!(perm["bash"], serde_json::json!({"*": "deny"}), "{perm}");
        // `task` too: permissions are PER-AGENT, so denying the reviewer a shell says
        // nothing about a subagent it spawns — and opencode's `explore` subagent
        // resolves to `bash: allow *`. Without this, `bash: deny` above is decorative.
        assert_eq!(perm["task"], serde_json::json!({"*": "deny"}), "{perm}");
        // `ask` must not survive in ANY rule: an unattended reviewer has nobody to
        // answer, so `ask` is a hang. This is what stalled two of four reviewers.
        assert!(
            !perm.to_string().contains("\"ask\""),
            "a reviewer's permission block must never say `ask`: {perm}"
        );
        // …and `external_directory` is a FLAT deny, with no allow beside it. An allow
        // was tried, for the run dir, and measured not to work — a project-level allow
        // does not override the global `ask`, so it bought nothing while reading as
        // enforcement. Everything drovr hands a reviewer is inside the checkout now.
        assert_eq!(
            perm["external_directory"],
            serde_json::json!({"*": "deny"}),
            "{perm}"
        );
        // `question` too, and as a BARE action — the map form is a schema error and
        // opencode refuses to start on an invalid config. A reviewer that can ask a
        // question will eventually ask one, and there is nobody in a reviewer pane to
        // answer it: one already parked a finished review behind a menu.
        assert_eq!(perm["question"], "deny", "{perm}");

        let exclude = std::fs::read_to_string(project.join(".git/info/exclude")).unwrap();
        assert!(
            exclude.lines().any(|l| l.trim() == "opencode.json"),
            "drovr's plumbing must not show up as an untracked change: {exclude}"
        );
    }

    /// `--approve-mcps` auto-approves EVERY server in the project file, and drovr
    /// cannot approve selectively. So any server drovr left in place would be silently
    /// handed to a read-only reviewer — and `.cursor/mcp.json` is a path a hostile
    /// repository can simply commit. The reviewer must see drovr's server and nothing
    /// else; the displaced config is preserved, not destroyed.
    #[test]
    fn a_foreign_server_in_the_project_config_is_never_handed_to_a_reviewer() {
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-mcp-foreign");
        run.agent = Some("cursor".into());
        std::fs::write(
            std::path::Path::new(&crate::env::var("XDG_CONFIG_HOME").unwrap())
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
        // …and the backup is drovr's plumbing too, so it must not dirty the tree. A
        // glob, because the backup name is not fixed: drovr never writes over an
        // occupied slot, so a displaced original can land on a numbered one.
        let exclude = std::fs::read_to_string(project.join(".git/info/exclude")).unwrap();
        assert!(
            exclude
                .lines()
                .any(|l| l.trim() == ".cursor/mcp.json.drovr-backup*"),
            "the displaced original must be excluded from git too: {exclude}"
        );
    }

    /// A config that holds only drovr's own server (the ordinary steady state, every
    /// pass after the first) is rewritten in place — no backup, no noise.
    #[test]
    fn rewriting_drovrs_own_config_does_not_accumulate_backups() {
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-mcp-rewrite");
        run.agent = Some("cursor".into());
        std::fs::write(
            std::path::Path::new(&crate::env::var("XDG_CONFIG_HOME").unwrap())
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
        let env = TestEnv::new();
        let tmp = tempfile::tempdir().unwrap();
        let elsewhere = tmp.path().join("outside.json");
        std::fs::write(&elsewhere, "{}").unwrap();

        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(project.path().join(".cursor")).unwrap();
        let link = project.path().join(".cursor/mcp.json");
        std::os::unix::fs::symlink(&elsewhere, &link).unwrap();

        let err = write_mcp_config(&link, McpSchema::McpServers, "r", "task-1", 1)
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
        let err = write_mcp_config(
            &project2.path().join(".cursor/mcp.json"),
            McpSchema::McpServers,
            "r",
            "task-1",
            1,
        )
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
            build_seed("task-1", "security", "a", "b", "d", "/checkout/here", None, diff_fixture())
                .contains(&rendered)
        );
    }

    /// A read error is not "no file here". Collapsing them lets drovr replace — and
    /// fail to back up — a config it never managed to read.
    #[test]
    fn an_unreadable_existing_config_is_an_error_not_a_silent_replacement() {
        let env = TestEnv::new();
        let dir = tempfile::tempdir().unwrap();
        // A directory where the config should be: reading it fails with something
        // other than NotFound, exactly like a permissions or IO failure would.
        let path = dir.path().join("mcp.json");
        std::fs::create_dir(&path).unwrap();

        let err = write_mcp_config(&path, McpSchema::McpServers, "r", "task-1", 1)
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
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-mcp-none");
        std::fs::write(
            std::path::Path::new(&crate::env::var("XDG_CONFIG_HOME").unwrap())
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
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-file-only");
        write_base(&run, "task-1");
        // Spawn the panel — and mint the pass tokens its markers must carry.
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );
        // Every reviewer finishes (markers land) but none ever called the tool.
        drop_pass_markers(&run, "task-1", 1);

        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 5_000, false, None)
                .expect("a degraded pass still returns"),
            ReviewOutcome::Findings,
            "a missing findings file must block the gate, not pass it"
        );
        let merged = std::fs::read_to_string(run_dir(&run.name).join("task-1-review.json"))
            .expect("the merged review is written");
        assert!(
            merged.contains("finished without submitting"),
            "the missing file must be reported as undelivered, not scraped from the \
             pane: {merged}"
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
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-respawn-inherit");
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
        for angle in ["security", "error-handling", "type-design"] {
            seed_angle_file(&run, "task-1", 1, angle, CLEAN);
        }

        // The resume respawns correctness (its pane is gone), clearing the file the
        // dead reviewer left; the other three bank. The replacement has not finished
        // yet, so this pass times out.
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );
        assert_ne!(
            pane_of(&run, "review:task-1:1:correctness"),
            dead,
            "the angle should have been respawned into a new pane"
        );

        // Now the REPLACEMENT finishes — under its own pass token, the only marker
        // its wait loop accepts — having written nothing of its own. The pass must
        // fail rather than reuse what the dead reviewer left.
        drop_pass_marker(&run, "task-1", 1, "correctness");
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None)
                .expect("a degraded pass still returns"),
            ReviewOutcome::Findings
        );
        let merged = std::fs::read_to_string(run_dir(&run.name).join("task-1-review.json"))
            .expect("the merged review is written");
        assert!(
            merged.contains("the 'correctness' reviewer finished without submitting"),
            "the respawned angle must read as undelivered, not as the dead reviewer's \
             torn file: {merged}"
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
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-delivered-then-died");
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
        let env = TestEnv::new();
        let (run, _repo) = make_run(&env, "cr-headsha");
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
            diff_fixture(),
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

    /// The artifacts that replaced the reviewer's `git diff`. They have to carry both
    /// halves: the `--stat` a reviewer reads first to decide where to look, and the
    /// patches it then reads. An index with only one of them sends the reviewer looking
    /// for the other through a shell it does not have.
    #[test]
    fn the_diff_drovr_writes_carries_the_stat_and_a_patch_per_file() {
        let env = TestEnv::new();
        let (run, repo) = make_run(&env, "cr-diff-artifact");
        let base = head_sha(&run.project_dir).unwrap();

        // A committed change on top of the base, and an uncommitted one after it —
        // the review scope is the range PLUS the working tree, so both must appear.
        std::fs::write(repo.path().join("committed.rs"), "fn committed() {}\n").unwrap();
        for args in [&["add", "committed.rs"][..], &["commit", "-qm", "c"]] {
            assert!(
                Command::new("git")
                    .arg("-C")
                    .arg(repo.path())
                    .args(args)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        std::fs::write(repo.path().join("uncommitted.rs"), "fn uncommitted() {}\n").unwrap();
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(repo.path())
                .args(["add", "uncommitted.rs"])
                .status()
                .unwrap()
                .success()
        );

        let path = write_review_diff(&run.project_dir, "task-1", 3, &base).unwrap();
        assert_eq!(path, review_diff_path(&run.project_dir, "task-1", 3));
        // Inside the checkout, which is the whole point: a reviewer's read permission
        // reaches here without any `external_directory` decision.
        assert!(
            path.starts_with(&run.project_dir),
            "the diff must live inside the project, not at {}",
            path.display()
        );

        let index = std::fs::read_to_string(&path).unwrap();
        assert!(index.contains("## Summary (--stat)"), "{index}");
        assert!(index.contains("## Per-file patches"), "{index}");
        // The index carries the map, not the patches: the reviewer must be able to pick
        // what it loads, which is the property that makes a large change reviewable.
        assert!(
            !index.contains("fn committed()"),
            "the index must not inline the patches — that is the blob this replaced: \
             {index}"
        );

        // Every changed file has its own patch, and the index names each one by a path
        // that resolves. Both halves of the scope appear: the range PLUS the working
        // tree.
        let dir = review_diff_dir(&run.project_dir, "task-1", 3);
        let mut bodies = String::new();
        for (i, name) in ["committed.rs", "uncommitted.rs"].iter().enumerate() {
            let patch = dir.join(patch_file_name(i + 1, name));
            assert!(
                index.contains(&patch.display().to_string()),
                "the index must name {}: {index}",
                patch.display()
            );
            bodies.push_str(&std::fs::read_to_string(&patch).unwrap());
        }
        assert!(
            bodies.contains("fn committed()"),
            "the committed half is missing: {bodies}"
        );
        assert!(
            bodies.contains("fn uncommitted()"),
            "the working-tree half is missing, so the reviewer would review a stale \
             range: {bodies}"
        );
    }

    /// Two different paths can sanitise to the same slug (`a/b.rs` and `a_b.rs` both
    /// become `a_b.rs`), and a path long enough to be truncated collides far more
    /// easily than that. The ordinal is what actually keeps the names apart — without
    /// it one file's patch silently overwrites another's and the reviewer reviews the
    /// wrong change with no sign anything went missing.
    #[test]
    fn per_file_patch_names_are_unique_even_when_the_paths_collide() {
        assert_ne!(
            patch_file_name(1, "a/b.rs"),
            patch_file_name(2, "a_b.rs"),
            "the slug alone is not unique; the ordinal must carry it"
        );
        let long = format!("src/{}/x.rs", "d".repeat(400));
        assert_ne!(patch_file_name(3, &long), patch_file_name(4, &long));
        assert!(
            patch_file_name(3, &long).len() < 200,
            "a long path must be truncated to a writable filename: {}",
            patch_file_name(3, &long)
        );
        // Nothing that would leave the artifact directory or need quoting.
        let hostile = patch_file_name(5, "../../etc/passwd; rm -rf");
        assert!(
            !hostile.contains('/') && !hostile.contains(' ') && !hostile.contains(';'),
            "{hostile}"
        );
    }

    /// The seed names the diff by path, so the file has to exist by the time any
    /// reviewer is seeded — a reviewer with no shell cannot recover from being sent to
    /// a file that is not there yet.
    #[test]
    fn the_panel_writes_the_diff_before_it_seeds_anyone() {
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-diff-before-seed");
        write_base(&run, "task-1");
        assert_eq!(
            code_review_run(&h, &mut run, "task-1", 40, false, None).unwrap(),
            ReviewOutcome::Timeout
        );
        let path = review_diff_path(&run.project_dir, "task-1", 1);
        assert!(
            path.exists(),
            "the panel must write {} before seeding reviewers at it",
            path.display()
        );
        // …and every seed points at exactly that path.
        let seed = std::fs::read_to_string(
            run_dir(&run.name).join("task-1-review-correctness-seed.md"),
        )
        .unwrap();
        assert!(
            seed.contains(&path.display().to_string()),
            "the seed must name the diff the panel wrote: {seed}"
        );
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
            diff_fixture(),
        );
        assert!(
            seed.contains("/checkout/here"),
            "seed must name the checkout the reviewer can read: {seed}"
        );
        assert!(
            seed.contains("read any file"),
            "seed must grant reads beyond the diffed files: {seed}"
        );
        // The seed must NOT promise a shell. Reviewers have none — `bash` is denied
        // (see `opencode_plan_permission`) — and a brief that told one to run `git
        // diff` or the test suite would send every reviewer into a refusal it then has
        // to reason its way out of, which is how a panel wastes its turn.
        assert!(
            !seed.contains("run the tests"),
            "the seed must not promise a shell a reviewer does not have: {seed}"
        );
        assert!(
            seed.contains("DO NOT RUN SHELL COMMANDS"),
            "the seed must tell the reviewer plainly not to use a shell: {seed}"
        );
        // …and it must hand over the diff drovr wrote in its place, by path.
        assert!(
            seed.contains(&diff_fixture().display().to_string()),
            "seed must name the diff drovr wrote for it: {seed}"
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
            diff_fixture(),
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
            diff_fixture(),
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
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-context-persist");
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
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (mut run, _repo) = make_run(&env, "cr-context-clear");
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
        let env = TestEnv::new();
        let h = FakeHerdr::new();
        let (run, _repo) = make_run(&env, "cr-brief");
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
        let env = TestEnv::new();
        let (run, _repo) = make_run(&env, "cr-brief-no-base");
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
        // For the `run_dir("myrun")` below, and only for that: this test asserts on a
        // string, but it computes the path it must NOT find by calling the production
        // resolver. Without an overlay that resolver reads the process environment —
        // which, before this module moved onto `TestEnv`, happened to hold whatever
        // scratch root a sibling test had set process-globally. That leftover is the
        // race this run exists to remove, so the test now brings its own root.
        let env = TestEnv::new();
        let seed = build_seed(
            "task-1",
            "security",
            "aaa",
            "bbb",
            "do it",
            "/checkout/here",
            None,
            diff_fixture(),
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
