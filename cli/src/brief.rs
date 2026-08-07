//! Composition of PHASE briefs — the structural half of what a phase agent is told.
//!
//! Until this module, a phase brief was assembled by the driver agent: the templates in
//! `skills/pipeline/phase-prompts/` were prose the *skill* told it to substitute and
//! relay, and `drovr phase start` deliberately spawned a bare agent with no seed. That
//! made the frame — who you are, what you may write, how you signal done — the one part
//! of the contract most exposed to paraphrase, truncation under context pressure, and
//! quiet omission, with nothing able to detect any of it.
//!
//! Here the templates are embedded ([`include_str!`]) from the same files, drovr does the
//! substitution, and the driver contributes only `--context`. See
//! `~/.local/share/drovr/runs/structural-briefs/spec.md`.
//!
//! The templates stay on disk as the editable source (reviewer decision D) rather than
//! becoming Rust string literals, so they remain readable and reviewable as documents.

use std::io;
use std::io::Read;
use std::path::Path;

use crate::run::{RunState, run_dir};

const BRAINSTORM: &str = include_str!("../../skills/pipeline/phase-prompts/brainstorm.md");
const PLAN: &str = include_str!("../../skills/pipeline/phase-prompts/plan.md");
const IMPLEMENT_TASK: &str = include_str!("../../skills/pipeline/phase-prompts/implement-task.md");
const REVIEW: &str = include_str!("../../skills/pipeline/phase-prompts/review.md");
const HANDOFF_TEMPLATE: &str = include_str!("../../skills/handoff/HANDOFF-template.md");

/// The pipeline phases drovr has a template for. A phase name outside this set has no
/// composed brief (see [`compose_phase_brief`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhaseKind {
    Brainstorm,
    Plan,
    /// `implement-task-<N>`; carries `N` so the brief can say which task it is.
    ImplementTask(u32),
    Review,
}

/// Classify a phase name. `implement-task-<N>` must parse its number, since the template
/// refers to `<N>` throughout (including in the commands it tells the agent to run).
pub fn phase_kind(phase: &str) -> Option<PhaseKind> {
    match phase {
        "brainstorm" => Some(PhaseKind::Brainstorm),
        "plan" => Some(PhaseKind::Plan),
        "review" => Some(PhaseKind::Review),
        other => other
            .strip_prefix("implement-task-")
            .and_then(|n| n.parse::<u32>().ok())
            .map(PhaseKind::ImplementTask),
    }
}

fn template(kind: PhaseKind) -> &'static str {
    match kind {
        PhaseKind::Brainstorm => BRAINSTORM,
        PhaseKind::Plan => PLAN,
        PhaseKind::ImplementTask(_) => IMPLEMENT_TASK,
        PhaseKind::Review => REVIEW,
    }
}

/// Drop a leading `<!-- … -->` block. The templates open with a note to whoever
/// maintains them ("Injected as …; the driver substitutes …"); that is editorial, and
/// telling an agent how its own brief is assembled invites it to assemble one itself.
fn strip_editorial_comment(template: &str) -> &str {
    let t = template.trim_start();
    match t
        .strip_prefix("<!--")
        .and_then(|rest| rest.split_once("-->"))
    {
        Some((_, body)) => body.trim_start(),
        None => t,
    }
}

/// `<key>-context.md` — the driver's context for a brief, recorded so a later invocation
/// that omits the argument composes the SAME brief. Shared by the phase briefs here and
/// the reviewer briefs in `code_review`, so the two cannot drift apart.
pub fn context_record(dir: &Path, key: &str) -> std::path::PathBuf {
    dir.join(format!("{key}-context.md"))
}

/// Cap on any context drovr will put in a brief, whatever path it arrives by: `--context`,
/// `--context-file`, `phase send -` stdin, or a reused record. Round 4: the three input
/// paths were capped while the RECORD was not, so the limit was bypassable by writing the
/// record directly.
pub const MAX_CONTEXT: u64 = 1 << 20;

/// Write `contents` to `path` without ever following a symlink at `path` or at the temp
/// file, and without leaving a partial file behind.
///
/// `fs::write` follows a symlink at its destination, which turns "record this" into
/// "clobber whatever that link points at". Round 3 caught it at the context record; round 4
/// caught the same thing at the review-JSON paths, because the fix had been applied to one
/// site and not its siblings. This helper is that fix, in one place, for all of them.
///
/// `create_new` refuses to open anything that already exists — symlink included. The pid
/// keeps concurrent writers apart, and a stale temp from a crashed process is removed and
/// retried once rather than blocking every future write with `AlreadyExists`.
pub fn write_no_follow(path: &Path, contents: &str) -> io::Result<()> {
    let dir = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} has no parent directory", path.display()),
        )
    })?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "record".into());
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));

    let open = |tmp: &Path| {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(tmp)
    };
    let mut file = match open(&tmp) {
        Ok(f) => f,
        // A temp left by a crashed process (or a reused pid) must not block writing
        // forever. It is drovr's own name in drovr's own dir, so removing it is safe.
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            std::fs::remove_file(&tmp).map_err(|rm| {
                io::Error::new(
                    rm.kind(),
                    format!(
                        "stale temp {} could not be removed ({rm}); the original error was {e}",
                        tmp.display()
                    ),
                )
            })?;
            open(&tmp)?
        }
        Err(e) => return Err(e),
    };

    let write = std::io::Write::write_all(&mut file, contents.as_bytes());
    drop(file);
    if let Err(e) = write {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

/// Read a record drovr wrote, refusing to follow a symlink and refusing anything over
/// [`MAX_CONTEXT`].
///
/// Round 4, security: hardening the WRITE path left the READ path following symlinks, so a
/// link planted at the record path made drovr read an arbitrary file and inject it into a
/// brief. There is no `O_NOFOLLOW` in std, so this is a `symlink_metadata` check — racy in
/// principle, decisive against a planted link in practice, and the run dir is not a
/// contested directory (see docs/known-issues.md on what its permissions do and do not
/// buy).
fn read_record_no_follow(path: &Path) -> io::Result<Option<String>> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    if meta.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{} is a symlink; refusing to read a brief's context through it",
                path.display()
            ),
        ));
    }
    // Must be a REGULAR file, checked BEFORE opening. "Not a symlink" was not enough: a
    // FIFO passes that check and then `read_to_string` blocks forever, hanging the driver
    // on every brief it composes. Devices and directories are equally not records.
    if !meta.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{} is not a regular file; refusing to read a brief's context from it",
                path.display()
            ),
        ));
    }
    // Recording appends a trailing newline, so a context of exactly MAX_CONTEXT bytes is
    // MAX_CONTEXT + 1 on disk. Allowing that byte keeps an at-limit `--context` from being
    // accepted on the way in and then refused on reuse.
    let limit = MAX_CONTEXT + 1;
    if meta.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{} is {} bytes, over the {MAX_CONTEXT}-byte context limit",
                path.display(),
                meta.len()
            ),
        ));
    }
    // Bound the READ, not just the metadata check: the file can grow between the two, so a
    // metadata-only cap is a TOCTOU that lets an oversized record through.
    let file = std::fs::File::open(path)?;
    let mut text = String::new();
    std::io::Read::take(file, limit + 1).read_to_string(&mut text)?;
    if text.len() as u64 > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{} grew past the {MAX_CONTEXT}-byte context limit while being read",
                path.display()
            ),
        ));
    }
    Ok(Some(text))
}

/// Resolve the context for a brief. Three cases, deliberately distinct:
///
/// * `Some(text)` — record it and use it. A later invocation that passes nothing reuses it.
/// * `Some("")` — the flag was given EMPTY, which is a request for *no* context. Clears the
///   record. Previously this fell through to "absent" and silently resurrected stale
///   context, so there was no way to un-say something.
/// * `None` — the flag was absent: reuse whatever is recorded, and say so on stderr. Silence
///   was the real defect here; a driver has to be able to see which context is in effect.
///   (stderr, never stdout — `phase brief`'s stdout is the brief itself, often piped.)
pub fn resolve_context(
    dir: &Path,
    key: &str,
    supplied: Option<&str>,
) -> io::Result<Option<String>> {
    let path = context_record(dir, key);
    match supplied {
        Some(text) if !text.trim().is_empty() => {
            // Enforced here as well as in the CLI: this is the function every caller goes
            // through, so the limit belongs at this boundary and not only at the one in
            // front of it today.
            if text.trim().len() as u64 > MAX_CONTEXT {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "context is {} bytes, over the {MAX_CONTEXT}-byte limit",
                        text.trim().len()
                    ),
                ));
            }
            std::fs::create_dir_all(dir)?;
            write_no_follow(&path, &format!("{}\n", text.trim()))?;
            Ok(Some(text.trim().to_owned()))
        }
        Some(_) => {
            // Explicitly empty: un-say it. Remove unconditionally and tolerate NotFound,
            // rather than exists()-then-remove, which races with anything else in the run
            // dir and turns a benign "already gone" into an error.
            // `--context ''` is what the unreadable-record error tells the driver to run,
            // so it has to work in the states that produce that error — including a
            // DIRECTORY sitting at the record path, where `remove_file` fails outright.
            // A non-empty directory is not cleared automatically: recursive deletion of
            // something drovr did not create is not a clear-the-context operation.
            match std::fs::remove_file(&path) {
                Ok(()) => eprintln!("drovr: cleared the recorded context for '{key}'"),
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(_) if path.is_dir() => match std::fs::remove_dir(&path) {
                    Ok(()) => eprintln!("drovr: cleared the recorded context for '{key}'"),
                    Err(e) => {
                        return Err(io::Error::new(
                            e.kind(),
                            format!(
                                "cannot clear the recorded context: {} is a non-empty \
                                 directory ({e}) — remove it yourself; drovr will not delete \
                                 a tree it did not create",
                                path.display()
                            ),
                        ));
                    }
                },
                Err(e) => return Err(e),
            }
            Ok(None)
        }
        None => {
            // A MISSING record is the normal case (no context was ever given). Any other
            // read error is not: proceeding contextless while the driver believes its
            // recorded context is in effect is the silent failure this mechanism exists
            // to prevent, so say so loudly and carry on.
            // A MISSING record is the normal case (no context was ever given). Any other
            // read error FAILS the composition: a brief that silently goes out without
            // context the driver believes is in it is precisely the failure this
            // mechanism exists to prevent, and a warning on stderr is too easy to miss in
            // a pipeline. Four review angles independently called warn-and-proceed wrong.
            let recorded = read_record_no_follow(&path)
                .map_err(|e| {
                    io::Error::new(
                        e.kind(),
                        format!(
                            "cannot use the recorded context {} ({e}) — refusing to compose a \
                             brief without it; fix the file, or pass --context '' to drop it",
                            path.display()
                        ),
                    )
                })?
                .map(|c| c.trim().to_owned());
            // A record that exists but holds only whitespace is a broken state, not "no
            // context": say so rather than composing as if nothing was ever recorded.
            let recorded = match recorded {
                Some(c) if c.is_empty() => {
                    eprintln!(
                        "drovr: the recorded context for '{key}' ({}) is empty — composing \
                         without it",
                        path.display()
                    );
                    None
                }
                other => other,
            };
            if recorded.is_some() {
                eprintln!(
                    "drovr: reusing the recorded context for '{key}' ({}) — pass --context to \
                     replace it, or --context '' to drop it",
                    path.display()
                );
            }
            Ok(recorded)
        }
    }
}

/// Compose the full brief for `phase`: the embedded template with drovr's substitutions,
/// the run's task, and the driver's `context` as its own section.
pub fn compose_phase_brief(
    run: &RunState,
    phase: &str,
    context: Option<&str>,
) -> io::Result<String> {
    let kind = phase_kind(phase).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "no brief template for phase '{phase}' (drovr composes briefs for \
                 brainstorm, plan, implement-task-<N> and review). For a one-off phase, \
                 brief it yourself with `drovr phase send {} {phase} \"<text>\"`.",
                run.name
            ),
        )
    })?;

    // Substitution is an ALLOWLIST. The templates contain angle-bracket text that merely
    // looks like a placeholder — `"<one line: what changed since last version>"` in
    // brainstorm.md is what the agent is told to write for itself, and the ask directive's
    // `--question "<what you need decided>"` likewise — so "replace anything in angle
    // brackets" would corrupt the brief it is composing.
    let mut body = strip_editorial_comment(template(kind)).replace("<run>", &run.name);
    if let PhaseKind::ImplementTask(n) = kind {
        body = body.replace("<N>", &n.to_string());
    }

    // Recorded, so the UNBRIEFED remediation (`phase brief | phase send -`) reproduces
    // the SAME brief without the driver having to re-supply context it already gave.
    let context = resolve_context(&run_dir(&run.name), phase, context)?;

    // `## Task` alone was ambiguous for implement-task: this is the RUN's task, while
    // that phase's actual scope is the per-task brief in the context section. An agent
    // that read the run-level statement as its scope would implement the whole change.
    let mut sections = format!("## The run's task\n\n{}\n", run.task.trim());
    if matches!(kind, PhaseKind::ImplementTask(_)) {
        // Point at the context section only when there IS one. Unconditionally telling an
        // agent its scope is "the context section below" when no context was supplied
        // sends it looking for a section that does not exist.
        sections.push_str(if context.is_some() {
            "\nThat is the whole run, for orientation only. **Your scope is the task brief in \
             the context section below**, not this run-level statement.\n"
        } else {
            "\nThat is the whole run, for orientation only. **Your scope is only this task's \
             brief in `plan.md`** — no context was supplied with this brief, so read it there \
             and do not widen your scope to the run.\n"
        });
    }
    // The section is ALWAYS emitted, even empty. Suppressing it looked tidier, but the
    // templates refer to "the context section below" and those references then pointed at
    // nothing — a critical finding in round 4, and the third round in a row to trip over
    // an absent section. An explicit "none supplied" makes every reference true and tells
    // the agent that the absence is deliberate rather than a delivery failure.
    sections.push_str("\n## Context from the driver\n\n");
    match context.as_deref() {
        Some(c) => {
            sections.push_str(c);
            sections.push('\n');
        }
        None => sections.push_str(
            "*(none supplied — the driver passed no `--context`. Do not wait for it; work from \
             the task above and the artifacts named in this brief.)*\n",
        ),
    }

    // Insert BEFORE the template's closing `## Done when`. Appending put the task and
    // context after the completion criteria, orphaning them: the agent read "Done when
    // …" and then hit new material, and the criteria no longer ended the brief.
    //
    // The split is a line-anchored `rfind`, which assumes no template contains that exact
    // heading text inside a code block or in prose before its real closing section. All
    // four do end with `## Done when`, and `embedded_templates_match_the_files_on_disk`
    // plus these tests would catch a template edited into a shape this mishandles. The
    // fallback branch (append) exists for a template without the heading at all, e.g. a
    // future phase kind.
    let mut brief = match body.rfind("\n## Done when") {
        Some(i) => {
            let (before, done_when) = body.split_at(i);
            format!(
                "{}\n{}\n{}",
                before.trim_end(),
                sections,
                done_when.trim_start_matches('\n')
            )
        }
        None => format!("{}\n\n{}", body.trim_end(), sections),
    };
    if !brief.ends_with('\n') {
        brief.push('\n');
    }
    Ok(brief)
}

/// The placeholder `handoff_scaffold` writes as each section's body.
///
/// `phase_done`'s gate refuses a handoff that still contains this line at column 0 — the
/// placeholder sitting where content belongs. Matched UNTRIMMED, so an indented copy is
/// quoted text rather than a leftover. Shared so the writer and that checker cannot drift
/// apart about what an unfilled section looks like.
pub const SCAFFOLD_PLACEHOLDER: &str = "TODO";

/// The empty handoff for a finishing agent to fill in:/// The empty handoff for a finishing agent to fill in: the fixed seven headings, each
/// with the template's own guidance as an HTML comment, and nothing else.
///
/// Deliberately contains NO derived content. An earlier draft had drovr fill in the git
/// pointers (branch, base, HEAD, changed files) it can compute; the reviewer rejected
/// that — drovr guessing which commits and files belong to this session would be wrong
/// exactly when it matters, and the agent knows. So the structure is drovr's and every
/// word of substance is the agent's.
pub fn handoff_scaffold() -> String {
    let mut out = String::from(
        "<!-- Scaffolded by `drovr handoff-scaffold`. Structure is fixed; fill in every\n\
         section from your own context, then run `drovr phase done` — which REFUSES while\n\
         any section is still `TODO`, and names the ones left. Delete these comments as\n\
         you go. -->\n",
    );
    // Leading newline so the FIRST heading splits like every other one (the stripped
    // body starts directly with `## Objective`).
    let body = format!("\n{}", strip_editorial_comment(HANDOFF_TEMPLATE));
    // Sections only: the template's trailing "## Authoring rules" is instruction for the
    // author, not a heading the handoff itself carries.
    for section in body.split("\n## ").skip(1) {
        let (heading, guidance) = section.split_once('\n').unwrap_or((section, ""));
        if heading.trim() == "Authoring rules" {
            break;
        }
        out.push_str(&format!(
            "\n## {}\n\n<!-- {} -->\n\n{SCAFFOLD_PLACEHOLDER}\n",
            heading.trim(),
            guidance.trim().replace("\n", " ")
        ));
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::RunState;
    use crate::test_util::ENV_LOCK;

    /// Byte offset of the LINE that is exactly `heading`. The templates now mention
    /// headings in prose (e.g. "the task brief in the `## Context from the driver`
    /// section"), so `str::find` on the heading text matches the mention, not the section.
    fn heading_pos(brief: &str, heading: &str) -> Option<usize> {
        let mut at = 0usize;
        for line in brief.split('\n') {
            if line.trim_end() == heading {
                return Some(at);
            }
            at += line.len() + 1;
        }
        None
    }

    /// Composition now records/reads `<phase>-context.md`, so every test that composes
    /// needs its own data home. Caller holds ENV_LOCK.
    fn isolate(name: &str) -> std::path::PathBuf {
        let data = std::path::PathBuf::from(format!("/tmp/drovr-brief-test-{name}"));
        let _ = std::fs::remove_dir_all(&data);
        unsafe {
            std::env::set_var("XDG_DATA_HOME", &data);
        }
        data
    }

    fn make_run() -> RunState {
        RunState {
            name: "myrun".into(),
            task: "make the widget reentrant".into(),
            agent: Some("claude".into()),
            phases: vec![],
            review_phases: vec![],
            gate: "spec".into(),
            cursor: 0,
            workspace: None,
            root_pane: None,
            project_dir: "/checkout/here".into(),
            worktree_path: None,
            worktree_branch: None,
            archived: false,
            retired_panes: vec![],
        }
    }

    #[test]
    fn phase_kind_classifies_the_pipeline_phases() {
        assert_eq!(phase_kind("brainstorm"), Some(PhaseKind::Brainstorm));
        assert_eq!(phase_kind("plan"), Some(PhaseKind::Plan));
        assert_eq!(phase_kind("review"), Some(PhaseKind::Review));
        assert_eq!(
            phase_kind("implement-task-3"),
            Some(PhaseKind::ImplementTask(3))
        );
        assert_eq!(
            phase_kind("implement-task-12"),
            Some(PhaseKind::ImplementTask(12))
        );
        // Not pipeline phases: a custom phase has no template, and must be reported as
        // such rather than silently briefed with the wrong frame.
        assert_eq!(phase_kind("verify-land"), None);
        assert_eq!(phase_kind("implement-task-"), None);
        assert_eq!(phase_kind("implement-task-x"), None);
        assert_eq!(phase_kind("review:task-1:1:security"), None);
    }

    #[test]
    fn composed_brief_substitutes_run_and_task_number() {
        let _lock = ENV_LOCK.lock().unwrap();
        isolate("composed_brief_substitutes_run_and_task_number");
        let run = make_run();
        let brief = compose_phase_brief(&run, "implement-task-3", None).unwrap();
        assert!(
            brief.contains("implement **task 3**"),
            "the task number must be substituted: {brief}"
        );
        assert!(
            brief.contains("drovr code-review base myrun task-3"),
            "commands the agent is told to run must be literal, not placeholders: {brief}"
        );
        assert!(
            !brief.contains("<run>"),
            "no `<run>` may survive composition: {brief}"
        );
        assert!(
            !brief.contains("<N>"),
            "no `<N>` may survive composition: {brief}"
        );
    }

    /// The templates carry a leading HTML comment addressed to whoever maintains them
    /// ("Injected as … the driver substitutes …"). That is editorial, not part of the
    /// contract, and telling an agent how its own brief gets assembled invites it to
    /// assemble one itself.
    #[test]
    fn composed_brief_drops_the_editorial_comment() {
        let _lock = ENV_LOCK.lock().unwrap();
        isolate("composed_brief_drops_the_editorial_comment");
        let brief = compose_phase_brief(&make_run(), "brainstorm", None).unwrap();
        assert!(
            !brief.contains("<!--"),
            "the maintainer comment must not reach the agent: {brief}"
        );
        assert!(
            brief
                .trim_start()
                .starts_with("You are the **brainstorm** phase")
        );
    }

    /// Every `<...>` in `body`, in order, duplicates included.
    ///
    /// Crude on purpose: the templates are prose, and the only thing this has to
    /// separate is "angle-bracket token" from "not one".
    ///
    /// Two rules, and both exist because a stray `<` — a less-than in prose, a
    /// `Vec<T`, a shell redirect — otherwise **swallows real tokens silently**,
    /// leaving the caller's derived corpus quietly smaller than it looks. A guard
    /// that checks fewer things without failing is the exact defect this whole
    /// area keeps producing.
    ///
    /// 1. **Per line.** A token never spans a newline, while a stray `<` does
    ///    meet a Markdown block-quote `>` two lines down.
    /// 2. **Innermost pairing.** A `<` inside the candidate means the outer one
    ///    was the stray, so re-anchor on the last one and check the real token
    ///    instead of a garbled span containing it. Without this,
    ///    `<a> x < y <b>` yields `<a>` and `< y <b>` — and `<b>` is never checked.
    ///
    /// Slicing is at `<` and `>`, both ASCII, so the byte offsets `find`/`rfind`
    /// return are always char boundaries however much non-ASCII prose surrounds
    /// them. Every branch advances `rest` by at least one byte, so it terminates.
    fn angle_tokens(body: &str) -> Vec<&str> {
        let mut out = Vec::new();
        for line in body.lines() {
            let mut rest = line;
            while let Some(open) = rest.find('<') {
                let Some(len) = rest[open..].find('>') else { break };
                let end = open + len + 1;
                let token = &rest[open..end];
                match token[1..].rfind('<') {
                    Some(inner) => rest = &rest[open + 1 + inner..],
                    None => {
                        out.push(token);
                        rest = &rest[end..];
                    }
                }
            }
        }
        out
    }

    /// [`angle_tokens`] against the two shapes that make it lie rather than fail:
    /// a stray `<` before a real token, and one that never closes.
    ///
    /// Direct, because the guard that uses it cannot see this. A swallowed token
    /// leaves the derived corpus non-empty and every surviving token still
    /// present, so `composition_leaves_non_placeholder_angle_brackets_alone`
    /// stays green while checking less than it claims.
    #[test]
    fn angle_tokens_are_not_swallowed_by_a_stray_bracket() {
        assert_eq!(
            angle_tokens("<first> ok. Then a stray x < y with no close. <second> lost?"),
            vec!["<first>", "<second>"],
            "a stray `<` paired with a later token's `>` and ate the token between them"
        );
        assert_eq!(
            angle_tokens("count < limit, and <kept> after it"),
            vec!["<kept>"]
        );
        // Unclosed on its own line stops THAT line, not the scan.
        assert_eq!(
            angle_tokens("a < b\n<still-seen>\n"),
            vec!["<still-seen>"],
            "an unmatched `<` must not abandon the rest of the document"
        );
        // A quote marker two lines down is not a closer.
        assert_eq!(angle_tokens("x < y\n> quoted\n"), Vec::<&str>::new());
    }

    /// Substitution is an allowlist, never "replace anything in angle brackets".
    ///
    /// **Derived from the template**, so it pins the rule and not a sentence: the
    /// non-placeholders are whatever `brainstorm.md` currently carries besides
    /// `<run>` and `<N>`, and every one of them must survive composition
    /// verbatim. A reworded placeholder is then just a different token that still
    /// has to survive — which is the point, since this file has no business
    /// failing over the prompt's copy-editing.
    ///
    /// **Counted exactly, not merely present.** Several tokens repeat (`<value>`
    /// four times, `<text>`/`<path>`/`<label>` twice each), so a presence check
    /// would accept a rule that ate three of four `<value>`s and left one
    /// standing — green, while asserting something weaker than the sentence
    /// above. Not reachable through today's two literal `str::replace` calls,
    /// which are all-or-nothing per pattern; asserted anyway, because the whole
    /// point of deriving the corpus is that the rule outlives the implementation
    /// that happens to satisfy it.
    ///
    /// **And counted against the template's own text**, not the assembled brief.
    /// [`compose_phase_brief`] splices the run's task and the driver's context
    /// into the middle of the template, and both are free text. A `>=` count over
    /// the whole brief is therefore paddable: a reviewer demonstrated a real
    /// dropped `<value>` going undetected because the run's task happened to
    /// quote `--option <value>=<label>` — not a contrived input in a repo whose
    /// tasks are often *about* the ask directive. So the fixture is asserted
    /// disjoint from the corpus first, and the counts are then `==`.
    ///
    /// Both halves, because half of an allowlist is not one. `<run>` must be
    /// **gone** from the composed brief — the ask directive tells the agent to run
    /// `drovr ask <run> …` verbatim, and an unsubstituted one there is a command
    /// that errors — while `<what you need decided>` beside it must remain.
    ///
    /// (This previously pinned `answers[<id>]`, prose about the retired
    /// `questions.json` answer map. That text is gone with the channel; the guard
    /// is not.)
    #[test]
    fn composition_leaves_non_placeholder_angle_brackets_alone() {
        let _lock = ENV_LOCK.lock().unwrap();
        isolate("composition_leaves_non_placeholder_angle_brackets_alone");
        let run = make_run();
        let brief = compose_phase_brief(&run, "brainstorm", None).unwrap();

        // The editorial comment is stripped before substitution, so tokens inside
        // it are absent from the brief for an unrelated reason and would report a
        // failure this test is not about.
        let mut prose: std::collections::BTreeMap<&str, usize> = Default::default();
        for token in angle_tokens(strip_editorial_comment(BRAINSTORM)) {
            if token != "<run>" && token != "<N>" {
                *prose.entry(token).or_default() += 1;
            }
        }
        assert!(
            !prose.is_empty(),
            "brainstorm.md carries no angle-bracket prose outside the substituted \
             placeholders, so the survival assertion below would pass having checked \
             nothing"
        );

        for (token, wanted) in &prose {
            // The spliced-in sections must not be able to pay for a loss in the
            // template. Checked per token rather than assumed of the fixture, so
            // a later edit to `make_run()` fails here instead of quietly turning
            // the count below into an inequality.
            assert!(
                !run.task.contains(token),
                "the fixture's run task quotes `{token}`, which pads the count below and \
                 can mask a real loss — give make_run() a task with no angle-bracket prose"
            );
            let seen = brief.matches(token).count();
            assert_eq!(
                seen, *wanted,
                "`{token}` is prose, not a placeholder: the template carries it {wanted} \
                 time(s) and the composed brief {seen}. Substitution must stay an \
                 allowlist, and must leave every occurrence alone: {brief}"
            );
        }
        assert!(
            !brief.contains("<run>"),
            "an unsubstituted `<run>` reached the agent, inside commands it is told \
             to run verbatim: {brief}"
        );
    }

    #[test]
    fn composed_brief_carries_the_task_and_the_driver_context() {
        let _lock = ENV_LOCK.lock().unwrap();
        isolate("carries-context");
        let run = make_run();
        let with =
            compose_phase_brief(&run, "plan", Some("the vendored dir is off limits")).unwrap();
        assert!(with.contains("make the widget reentrant"), "task: {with}");
        assert!(with.contains("## Context from the driver"));
        assert!(with.contains("the vendored dir is off limits"));

        // Absent argument REUSES the record: the UNBRIEFED remediation
        // (`phase brief | phase send -`) must reproduce the same brief, not a thinner
        // one, without the driver re-supplying context it already gave.
        let again = compose_phase_brief(&run, "plan", None).unwrap();
        assert_eq!(with, again, "a re-brief must be byte-identical");

        // An explicitly EMPTY --context is a request for NO context, and must be able to
        // un-say what was recorded. The SECTION still appears — always emitted, so the
        // templates' references to it are never false — but marked as unsupplied.
        let cleared = compose_phase_brief(&run, "plan", Some("   ")).unwrap();
        assert!(
            !cleared.contains("the vendored dir is off limits"),
            "--context '' must drop the recorded context, not fall through to it: {cleared}"
        );
        assert!(
            heading_pos(&cleared, "## Context from the driver").is_some()
                && cleared.contains("none supplied"),
            "the section stays, marked unsupplied: {cleared}"
        );
        assert!(cleared.contains("make the widget reentrant"));
    }

    /// Round 2, four angles: composing a brief WITHOUT context the driver believes is
    /// recorded is the silent failure this mechanism exists to prevent. An unreadable
    /// record must fail the composition, not warn and ship a thinner brief.
    #[test]
    fn an_unreadable_context_record_fails_the_composition() {
        let _lock = ENV_LOCK.lock().unwrap();
        isolate("unreadable-record");
        let run = make_run();
        // A DIRECTORY where the record belongs: readable path, unreadable as a file, and
        // deterministic across platforms and privilege levels (unlike chmod 000).
        let dir = run_dir(&run.name);
        std::fs::create_dir_all(context_record(&dir, "plan")).unwrap();

        let err = compose_phase_brief(&run, "plan", None)
            .expect_err("an unreadable record must not compose silently");
        let msg = err.to_string();
        assert!(msg.contains("recorded context"), "says what failed: {msg}");
        assert!(
            msg.contains("--context ''"),
            "says how to get unstuck: {msg}"
        );
    }

    /// `fs::write` follows a symlink at the destination, so a link planted in the run dir
    /// would make recording context clobber whatever it points at. Write-then-rename
    /// replaces the link itself.
    #[test]
    fn recording_context_replaces_a_symlink_instead_of_following_it() {
        let _lock = ENV_LOCK.lock().unwrap();
        isolate("symlink-record");
        let run = make_run();
        let dir = run_dir(&run.name);
        std::fs::create_dir_all(&dir).unwrap();
        let victim = dir.join("victim.txt");
        std::fs::write(&victim, "precious\n").unwrap();
        std::os::unix::fs::symlink(&victim, context_record(&dir, "plan")).unwrap();

        compose_phase_brief(&run, "plan", Some("new context")).unwrap();

        assert_eq!(
            std::fs::read_to_string(&victim).unwrap(),
            "precious\n",
            "the symlink target must be untouched"
        );
        assert!(
            std::fs::symlink_metadata(context_record(&dir, "plan"))
                .unwrap()
                .file_type()
                .is_file(),
            "the link itself must have been replaced by a real file"
        );
    }

    /// Round 3, security: the first version of the symlink fix protected the DESTINATION
    /// and then wrote the temp file with `fs::write` at a fixed path — which follows a
    /// symlink planted there just as readily. The hole moved rather than closed, which is
    /// worse than not fixing it, because it reads as solved.
    #[test]
    fn recording_context_does_not_follow_a_symlink_at_the_temp_path() {
        let _lock = ENV_LOCK.lock().unwrap();
        isolate("symlink-tmp");
        let run = make_run();
        let dir = run_dir(&run.name);
        std::fs::create_dir_all(&dir).unwrap();
        let victim = dir.join("victim.txt");
        std::fs::write(&victim, "precious\n").unwrap();
        // Plant a link at the temp path this process will use.
        let tmp = dir.join(format!(".plan-context.{}.tmp", std::process::id()));
        std::os::unix::fs::symlink(&victim, &tmp).unwrap();

        // Either outcome is acceptable — refuse, or write elsewhere — but the victim must
        // survive either way.
        let _ = compose_phase_brief(&run, "plan", Some("new context"));
        assert_eq!(
            std::fs::read_to_string(&victim).unwrap(),
            "precious\n",
            "a symlink at the TEMP path must not be followed either"
        );
    }

    /// The unreadable-record error tells the driver to run `--context ''`, so that must
    /// work in the state that produces the error — including a directory at the path.
    #[test]
    fn clearing_context_works_when_the_record_is_a_directory() {
        let _lock = ENV_LOCK.lock().unwrap();
        isolate("clear-a-directory");
        let run = make_run();
        let dir = run_dir(&run.name);
        std::fs::create_dir_all(context_record(&dir, "plan")).unwrap();

        // Precondition: this is exactly the state that fails to compose.
        assert!(compose_phase_brief(&run, "plan", None).is_err());

        // The advertised remedy must actually clear it.
        let brief = compose_phase_brief(&run, "plan", Some(""))
            .expect("--context '' must clear a directory record, as the error says it will");
        assert!(
            brief.contains("none supplied"),
            "no context, said explicitly"
        );
        assert!(
            !context_record(&dir, "plan").exists(),
            "the bogus record must be gone"
        );
    }

    /// Round 4, security: the WRITE path was hardened and the READ path left following
    /// symlinks, so a link planted at the record made drovr read an arbitrary file and
    /// inject it into a brief.
    #[test]
    fn reading_a_symlinked_context_record_is_refused() {
        let _lock = ENV_LOCK.lock().unwrap();
        isolate("symlink-read");
        let run = make_run();
        let dir = run_dir(&run.name);
        std::fs::create_dir_all(&dir).unwrap();
        let secret = dir.join("elsewhere.txt");
        std::fs::write(&secret, "attacker-controlled text\n").unwrap();
        std::os::unix::fs::symlink(&secret, context_record(&dir, "plan")).unwrap();

        let err = compose_phase_brief(&run, "plan", None)
            .expect_err("a symlinked record must not be read through");
        assert!(err.to_string().contains("symlink"), "says why: {err}");

        // And the contents must not have reached a brief by any path.
        let cleared = compose_phase_brief(&run, "plan", Some("")).unwrap();
        assert!(!cleared.contains("attacker-controlled"));
    }

    /// Round 4: a temp left behind by a crashed process (or a reused pid) must not block
    /// recording forever with an opaque `AlreadyExists`.
    #[test]
    fn a_stale_temp_file_does_not_block_recording() {
        let _lock = ENV_LOCK.lock().unwrap();
        isolate("stale-temp");
        let run = make_run();
        let dir = run_dir(&run.name);
        std::fs::create_dir_all(&dir).unwrap();
        let stale = dir.join(format!(".plan-context.md.{}.tmp", std::process::id()));
        std::fs::write(&stale, "leftover from a crash\n").unwrap();

        let brief = compose_phase_brief(&run, "plan", Some("fresh context"))
            .expect("a stale temp must be cleared, not fatal");
        assert!(brief.contains("fresh context"));
        assert!(!brief.contains("leftover from a crash"));
        assert!(!stale.exists(), "the temp must not survive the write");
    }

    /// A record over the cap must be refused, or the limit applied to `--context`,
    /// `--context-file` and stdin is bypassable by writing the record directly.
    #[test]
    fn an_oversized_context_record_is_refused() {
        let _lock = ENV_LOCK.lock().unwrap();
        isolate("oversized-record");
        let run = make_run();
        let dir = run_dir(&run.name);
        std::fs::create_dir_all(&dir).unwrap();
        // Clearly over: MAX + 2. (MAX + 1 is legal — recording appends a newline, so an
        // at-limit context is MAX + 1 on disk; see the round-trip assertion below.)
        let big = "x".repeat(MAX_CONTEXT as usize + 2);
        std::fs::write(context_record(&dir, "plan"), &big).unwrap();

        let err = compose_phase_brief(&run, "plan", None)
            .expect_err("an oversized record must be refused");
        assert!(err.to_string().contains("context limit"), "says why: {err}");
    }

    /// Round 5: a context of exactly `MAX_CONTEXT` was accepted on the way in and then
    /// REFUSED on reuse, because recording appends a newline and the read cap did not
    /// account for it. An at-limit context must survive the round trip.
    #[test]
    fn an_at_limit_context_survives_being_recorded_and_reused() {
        let _lock = ENV_LOCK.lock().unwrap();
        isolate("at-limit-roundtrip");
        let run = make_run();
        let at_limit = "y".repeat(MAX_CONTEXT as usize);

        let first = compose_phase_brief(&run, "plan", Some(&at_limit))
            .expect("an at-limit context must be accepted");
        let reused =
            compose_phase_brief(&run, "plan", None).expect("and must still be readable on reuse");
        assert_eq!(first, reused, "the round trip must be lossless");

        // One byte more must not be accepted at all.
        let over = "y".repeat(MAX_CONTEXT as usize + 1);
        assert!(
            compose_phase_brief(&run, "plan", Some(&over)).is_err(),
            "over-limit context must be refused at the boundary that records it"
        );
    }

    /// The implement-task scope pointer must not send an agent to a section that is not
    /// there: with no context, its scope is the task's entry in `plan.md`.
    #[test]
    fn the_implement_task_scope_pointer_matches_what_the_brief_contains() {
        let _lock = ENV_LOCK.lock().unwrap();
        isolate("scope-pointer");
        let run = make_run();

        // Assert on the pointer DROVR emits, not on the template's own prose (which
        // legitimately describes both cases).
        let ctx_pointer = "**Your scope is the task brief in the context section below**";
        let plan_pointer = "**Your scope is only this task's brief in `plan.md`**";

        let without = compose_phase_brief(&run, "implement-task-2", None).unwrap();
        assert!(
            !without.contains(ctx_pointer),
            "must not point at an absent section: {without}"
        );
        assert!(
            without.contains(plan_pointer),
            "must redirect to plan.md: {without}"
        );

        let with = compose_phase_brief(&run, "implement-task-2", Some("task 2 brief")).unwrap();
        assert!(with.contains(ctx_pointer));
        assert!(!with.contains(plan_pointer));
    }

    /// The templates used to end with a paste-target footer (`--- BRAINSTORM HANDOFF:`,
    /// `--- TASK BRIEF + ACCUMULATED INTERFACES:`) that the driver filled in by hand. drovr
    /// appends a context section now, so those were empty duplicates of the same channel
    /// sitting after the completion criteria. `## Done when` must be the LAST heading of
    /// every composed brief, with or without context.
    #[test]
    fn no_composed_brief_ends_with_a_stale_paste_footer() {
        let _lock = ENV_LOCK.lock().unwrap();
        isolate("no-stale-footer");
        let run = make_run();
        for phase in ["brainstorm", "plan", "review", "implement-task-1"] {
            for context in [None, Some("some context")] {
                let brief = compose_phase_brief(&run, phase, context).unwrap();
                let last = brief
                    .lines()
                    .rfind(|l| l.starts_with("## "))
                    .unwrap_or_else(|| panic!("{phase} has no headings"));
                assert_eq!(
                    last,
                    "## Done when",
                    "{phase} (context: {}) must end on its completion criteria",
                    context.is_some()
                );
                for stale in ["HANDOFF:", "TASK BRIEF +", "IMPLEMENT REPORTS", "\nTASK:"] {
                    assert!(
                        !brief.contains(stale),
                        "{phase}: stale paste footer {stale:?} is back:\n{brief}"
                    );
                }
            }
        }
    }

    /// Round 4 (critical): the templates refer to "the context section below", and
    /// suppressing that section when no `--context` was passed made those references point
    /// at nothing. The section is therefore unconditional.
    #[test]
    fn every_composed_brief_has_a_context_section() {
        let _lock = ENV_LOCK.lock().unwrap();
        isolate("always-context-section");
        let run = make_run();
        for phase in ["brainstorm", "plan", "review", "implement-task-1"] {
            for context in [None, Some("real context")] {
                let brief = compose_phase_brief(&run, phase, context).unwrap();
                assert!(
                    heading_pos(&brief, "## Context from the driver").is_some(),
                    "{phase} (context: {}) must carry the section: {brief}",
                    context.is_some()
                );
                if context.is_none() {
                    assert!(
                        brief.contains("none supplied"),
                        "{phase}: absence must be stated, not implied: {brief}"
                    );
                }
            }
        }
    }

    /// Context is per phase, never shared between them.
    #[test]
    fn recorded_context_does_not_leak_across_phases() {
        let _lock = ENV_LOCK.lock().unwrap();
        isolate("context-per-phase");
        let run = make_run();
        compose_phase_brief(&run, "plan", Some("plan-only context")).unwrap();
        let other = compose_phase_brief(&run, "review", None).unwrap();
        assert!(
            !other.contains("plan-only context"),
            "one phase's context must not reach another: {other}"
        );
        assert!(
            other.contains("none supplied"),
            "and says so explicitly: {other}"
        );
    }

    /// The template's closing `## Done when` states the completion criteria. Appending
    /// the task and context after it orphaned those criteria mid-brief.
    #[test]
    fn task_and_context_land_before_the_completion_criteria() {
        let _lock = ENV_LOCK.lock().unwrap();
        isolate("section-order");
        let run = make_run();
        let brief = compose_phase_brief(&run, "plan", Some("ctx here")).unwrap();
        let task = heading_pos(&brief, "## The run's task").expect("task section");
        let ctx = heading_pos(&brief, "## Context from the driver").expect("context section");
        let done = heading_pos(&brief, "## Done when").expect("templates end with Done when");
        assert!(
            task < done && ctx < done,
            "criteria must stay last:\n{brief}"
        );
        assert!(task < ctx);
    }

    /// A phase drovr has no template for must fail loudly and point at the escape hatch
    /// the reviewer kept (decision A: `phase send` survives for free-form injection).
    #[test]
    fn an_unknown_phase_is_an_error_that_names_the_escape_hatch() {
        let _lock = ENV_LOCK.lock().unwrap();
        isolate("an_unknown_phase_is_an_error_that_names_the_escape_hatch");
        let err = compose_phase_brief(&make_run(), "verify-land", None)
            .expect_err("no template must not mean an improvised brief");
        let msg = err.to_string();
        assert!(msg.contains("verify-land"), "names the phase: {msg}");
        assert!(msg.contains("phase send"), "names the escape hatch: {msg}");
    }

    /// The scaffold is structure only: seven headings, the template's guidance, and a
    /// TODO per section. Anything drovr *derived* here would be a guess about which work
    /// belongs to the session — the reviewer's call was that the agent knows better.
    #[test]
    fn handoff_scaffold_is_structure_only() {
        let s = handoff_scaffold();
        for heading in [
            "## Objective",
            "## State",
            "## Decisions + rationale",
            "## Interfaces / contracts",
            "## Open questions",
            "## Next step",
            "## Artifact pointers",
        ] {
            assert!(s.contains(heading), "missing {heading}: {s}");
        }
        assert_eq!(
            s.matches("\nTODO\n").count(),
            7,
            "every section must be left for the agent to fill: {s}"
        );
        assert!(
            !s.contains("## Authoring rules"),
            "the authoring rules are guidance for the author, not a handoff section: {s}"
        );
        assert!(
            !s.contains("Scaffolded by `drovr handoff scaffold`.\n"),
            "the scaffold note must be a single-line comment, not a stray heading"
        );
    }

    /// Invariant 5: the embedded template and the file on disk must agree. This fails
    /// when a template is edited without rebuilding — which is exactly the drift where
    /// the docs say one thing and the binary briefs another.
    #[test]
    fn embedded_templates_match_the_files_on_disk() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("skills/pipeline/phase-prompts");
        for (name, embedded) in [
            ("brainstorm.md", BRAINSTORM),
            ("plan.md", PLAN),
            ("implement-task.md", IMPLEMENT_TASK),
            ("review.md", REVIEW),
        ] {
            let on_disk = std::fs::read_to_string(root.join(name))
                .unwrap_or_else(|e| panic!("cannot read {name}: {e}"));
            assert_eq!(
                on_disk, embedded,
                "{name} on disk differs from the embedded copy — rebuild, or the brief \
                 drovr injects is not the template you are reading"
            );
        }
    }
}
