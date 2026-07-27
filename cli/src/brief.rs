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

use crate::run::RunState;

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

    // Substitution is an ALLOWLIST. The templates contain prose that merely looks like a
    // placeholder — `answers[<id>]` in brainstorm.md describes a JSON shape — so
    // "replace anything in angle brackets" would corrupt the brief it is composing.
    let mut body = strip_editorial_comment(template(kind)).replace("<run>", &run.name);
    if let PhaseKind::ImplementTask(n) = kind {
        body = body.replace("<N>", &n.to_string());
    }

    let mut brief = body;
    if !brief.ends_with('\n') {
        brief.push('\n');
    }
    brief.push_str(&format!("\n## Task\n\n{}\n", run.task.trim()));
    // Same rule as the reviewer brief: no context renders no section, because an empty
    // heading reads as "the driver had nothing to say".
    if let Some(c) = context.map(str::trim).filter(|c| !c.is_empty()) {
        brief.push_str(&format!("\n## Context from the driver\n\n{c}\n"));
    }
    Ok(brief)
}

/// The empty handoff for a finishing agent to fill in: the fixed seven headings, each
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
         section from your own context, then run `drovr phase done`. Delete these\n\
         comments as you go. -->\n",
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
            "\n## {}\n\n<!-- {} -->\n\nTODO\n",
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

    /// `answers[<id>]` in brainstorm.md is prose about a JSON shape, not a placeholder.
    /// Substitution is therefore an allowlist, never "replace anything in angle
    /// brackets".
    #[test]
    fn composition_leaves_non_placeholder_angle_brackets_alone() {
        let brief = compose_phase_brief(&make_run(), "brainstorm", None).unwrap();
        assert!(
            brief.contains("answers[<id>]"),
            "prose that merely looks like a placeholder must survive: {brief}"
        );
    }

    #[test]
    fn composed_brief_carries_the_task_and_the_driver_context() {
        let run = make_run();
        let with =
            compose_phase_brief(&run, "plan", Some("the vendored dir is off limits")).unwrap();
        assert!(with.contains("make the widget reentrant"), "task: {with}");
        assert!(with.contains("## Context from the driver"));
        assert!(with.contains("the vendored dir is off limits"));

        let without = compose_phase_brief(&run, "plan", None).unwrap();
        assert!(without.contains("make the widget reentrant"));
        assert!(
            !without.contains("## Context from the driver"),
            "no context must mean no empty section: {without}"
        );
    }

    /// A phase drovr has no template for must fail loudly and point at the escape hatch
    /// the reviewer kept (decision A: `phase send` survives for free-form injection).
    #[test]
    fn an_unknown_phase_is_an_error_that_names_the_escape_hatch() {
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
