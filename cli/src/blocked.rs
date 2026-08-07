//! Read-only detection of agents parked on a prompt nobody is answering.
//!
//! `drovr phase wait` already learns that a phase is blocked — it polls herdr,
//! sees `agent_status: blocked`, and triages the prompt
//! ([`crate::phase::triage_blocked_phase`]). But that is a PULL: only a driver
//! actively waiting on that exact phase ever finds out. A human watching the
//! review UI, a driver waiting on a different phase, and `drovr list` all see a
//! run that looks perfectly healthy while its agent sits on a permission dialog.
//!
//! This module is the same fact made available to those watchers. It answers
//! "which of this run's agents are blocked, and on what" for anyone who asks.
//!
//! # Read-only, and that is load-bearing
//!
//! [`triage_blocked_phase`](crate::phase::triage_blocked_phase) AUTO-ANSWERS a
//! routine prompt: it sends the accept keystroke. That is right for a driver
//! that asked to wait and is committed to the phase. It would be very wrong
//! here — this scan runs off a browser poll and off `drovr list`, several times
//! a minute, from processes that are only LOOKING. So the scan never sends
//! anything, and a test asserts it (`a_routine_prompt_is_never_auto_answered`).
//!
//! The classification itself is not re-implemented: it goes through
//! [`classify_blocked_prompt`], the same pure function the triage path uses. One
//! authority for "what is this prompt", two policies for what to do about it.
//!
//! # What is worth waking someone for
//!
//! Every blocked pane is REPORTED; only some are worth a notification.
//! [`BlockedClass::needs_human`] draws that line at the classifier's own verdict:
//! destructive and unknown prompts need a human (drovr will never answer them
//! itself), routine ones do not (a waiting driver answers them, and a badge that
//! fires on every file-edit permission dialog is a badge nobody reads).

use crate::herdr::{AgentStatus, Herdr, PaneState};
use crate::phase::{classify_blocked_prompt, tail_snippet, BlockedClass};
use crate::run::RunState;

/// How many trailing pane lines an excerpt carries. Matches the triage
/// diagnostic's snippet, so the badge in the browser and the CLI's escalation
/// text quote the same thing.
const EXCERPT_LINES: usize = 6;

/// One of a run's agents, parked on a prompt, with what it is parked on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockedAgent {
    /// The phase (or `review:<task>:<iter>:<angle>` panel) holding the pane.
    pub phase: String,
    /// The herdr pane it is blocked in — what a human attaches to, and what the
    /// browser mirror types into.
    pub pane_id: String,
    /// What [`classify_blocked_prompt`] made of the prompt.
    pub class: BlockedClass,
    /// The tail of the pane, so a watcher can see the prompt without attaching.
    pub excerpt: String,
}

/// What one sweep of a run's panes found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunScan {
    /// Every pane herdr reported as `blocked`, classified.
    pub blocked: Vec<BlockedAgent>,
    /// Panes herdr answered for and that still carry an agent session — the
    /// panes something could still HAPPEN in. Zero of these (with none
    /// unreadable) is what makes a run finished as far as a watcher cares,
    /// whatever its phase statuses say.
    pub attached: usize,
    /// Panes herdr could not answer for at all. Deliberately counted apart from
    /// `attached`: an unreachable herdr means "we do not know", and a watcher
    /// that folded it into "no agents left" would announce a run finished
    /// because a socket blipped.
    pub unreadable: usize,
}

impl RunScan {
    /// Whether this sweep learned anything at all — i.e. whether an empty
    /// [`RunScan::blocked`] may be reported as "nothing is blocked".
    ///
    /// **The one definition of that question.** It was spelled out at three call
    /// sites (the server's cache, the list column, the wire) before this method
    /// existed, and three copies of a predicate about uncertainty is how two of
    /// them come to disagree about whether a run is fine.
    ///
    /// The line is drawn at "did ANY pane answer": a run where one pane answered
    /// is a run herdr is talking to. A partial failure — some panes answered,
    /// one did not — is therefore reported as conclusive, and that is a
    /// deliberate limit, not an oversight. `pane_info` returns the same `None`
    /// for a herdr that is down and for a pane id that no longer names anything,
    /// and a stale pane id is *permanent*: treating any unreadable pane as
    /// uncertainty would leave such a run flagged uncertain forever and defeat
    /// the cache, since an inconclusive sweep is deliberately never cached. See
    /// `forge.ko.ag/drovr/drovr/issues`, "A partially unreadable sweep is cached as a
    /// clean answer".
    pub fn inconclusive(&self) -> bool {
        self.attached == 0 && self.unreadable > 0
    }
}

/// Sweep a run's panes: which are blocked, and whether anything is still live.
///
/// Walks the run's phases AND its review panels — a reviewer hitting a
/// permission prompt strands the panel exactly as a phase agent strands the
/// pipeline. A phase with no `pane_id` is skipped, which covers both the ones
/// that never started and the reaped ones ([`crate::run::Phase::mark_reaped`]
/// drops the id in the same statement it sets the flag).
///
/// # Cost
///
/// One `pane_info` per live pane, and a pane READ only for the panes that came
/// back `blocked` — which is almost never more than one. That is what makes this
/// cheap enough to sit behind a polling browser; the caller adds a TTL on top
/// (see `review::Ctx::blocked_of`) so the poll rate and the scan rate are
/// separate numbers.
pub fn scan_run<H: Herdr>(h: &H, run: &RunState) -> RunScan {
    let mut scan = RunScan::default();
    for phase in run.phases.iter().chain(run.review_phases.iter()) {
        let Some(pane_id) = phase.pane_id() else {
            continue;
        };
        let info = h.pane_info(pane_id);
        // Through `PaneState` rather than re-deriving the three cases from
        // `Option` + `has_agent_session`: it exists precisely so a caller cannot
        // conflate "could not read the pane" with "the agent has exited", and
        // this scan turns on exactly that distinction.
        match PaneState::from_poll(info.as_ref()) {
            PaneState::Unreadable => {
                scan.unreadable += 1;
                continue;
            }
            PaneState::AgentAttached => scan.attached += 1,
            PaneState::NoAgentSession => {}
        }
        // Narrowing `pane_info` to the status at the site that wants it, as the
        // trait's own docs require. A pane with no status at all is not evidence
        // of a block — herdr answering short must not paint a run as stuck.
        if info.and_then(|i| i.agent_status) != Some(AgentStatus::Blocked) {
            continue;
        }
        scan.blocked.push(classify_pane(h, &phase.name, pane_id));
    }
    scan
}

/// Classify one pane already known to be `blocked`.
///
/// A pane drovr cannot READ is reported as [`BlockedClass::Unknown`], i.e. as
/// needing a human. It matches the triage path's fail-safe and it is the honest
/// answer: herdr says an agent is waiting on something, and we cannot see what.
/// Silently dropping it would hide exactly the case where the human is most
/// needed.
///
/// That does put an IO failure and an unrecognised prompt in one variant, which
/// is a real (if small) conflation — a reviewer flagged it. It stays that way
/// deliberately: `BlockedClass` is what `phase wait`'s triage decides POLICY on,
/// the policy for both is identical ("do not guess, ask a person"), and a
/// transport variant added here would have to be handled by every caller of a
/// prompt classifier that has nothing to do with transport. The excerpt says
/// which it was, in the one place that difference is actionable — in front of
/// the human being asked to look.
fn classify_pane<H: Herdr>(h: &H, phase: &str, pane_id: &str) -> BlockedAgent {
    let (class, excerpt) = match h.agent_read(pane_id) {
        Ok(pane) => (
            classify_blocked_prompt(&pane),
            tail_snippet(&pane, EXCERPT_LINES),
        ),
        Err(e) => (
            BlockedClass::Unknown,
            format!("(pane {pane_id} is blocked, but its contents could not be read: {e})"),
        ),
    };
    BlockedAgent {
        phase: phase.to_owned(),
        pane_id: pane_id.to_owned(),
        class,
        excerpt,
    }
}

/// Whether this run's herdr workspace is one herdr currently has — i.e. whether
/// sweeping its panes can tell us anything.
///
/// `live` is `workspace_list`'s answer, and `None` (herdr could not be asked)
/// means EVERY run is worth sweeping: "unknown" must not read as "gone", or a
/// herdr blip would quietly stop watching everything.
///
/// One definition, because two callers need the same answer for opposite
/// reasons — `drovr list` uses it to decide what to sweep, `watch_tick` to
/// decide what a failed sweep means.
fn workspace_is_live(run: &RunState, live: Option<&[String]>) -> bool {
    let Some(live) = live else {
        return true;
    };
    run.workspace
        .as_deref()
        .is_some_and(|ws| live.iter().any(|id| id == ws))
}

/// Drop the runs whose herdr workspace is gone, in ONE herdr call.
///
/// This is not an optimisation, it is what makes a sweep over EVERY run usable.
/// A finished run keeps its recorded `pane_id`s forever — nothing clears them —
/// so scanning it asks herdr about panes that died weeks ago, and each of those
/// `pane.get` failures prints herdr's "agent status polling is degraded"
/// diagnostic. On a data dir with thirty old runs, `drovr list` drowned in it.
///
/// **For deciding what to SWEEP only.** A run filtered out here is not a run
/// that can be declared over: `watch_tick` applies the same predicate itself,
/// precisely so a run whose workspace is absent still counts as watchable while
/// its phases are outstanding.
pub fn with_live_workspace<H: Herdr>(h: &H, runs: Vec<RunState>) -> Vec<RunState> {
    let live = h.workspace_list();
    runs.into_iter()
        .filter(|run| workspace_is_live(run, live.as_deref()))
        .collect()
}

/// A blocked agent together with the run it belongs to — what a watcher
/// spanning several runs has to report.
///
/// Named apart from [`crate::findings::Finding`], which is a code-review result:
/// two unrelated things called `Finding` in one crate is an import alias waiting
/// to be got wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchFinding {
    pub run: String,
    pub agent: BlockedAgent,
}

/// What one poll of `drovr watch` concluded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchTick {
    /// At least one agent needs a human. The watch ends here.
    Alarm(Vec<WatchFinding>),
    /// Something could still block: an agent is attached, herdr could not be
    /// reached (which is not the same as "gone"), or a run has phases left to
    /// run and so can still be handed an agent. Keep watching.
    Watching,
    /// No scoped run can produce a blocked agent any more: every pane herdr
    /// answered for has lost its agent, AND every run has finished its phases.
    /// Reporting this rather than waiting out the timeout is the difference
    /// between "your run is over" and "something might still be coming".
    NothingToWatch,
}

/// Decide what one sweep across `runs` means for a watcher.
///
/// Split from the poll loop deliberately: the loop is a sleep and a re-read, and
/// the interesting part — an alarm beats liveness, and "unreachable" is not
/// "finished" — is all here, where a test can reach it without a clock.
pub fn watch_tick<H: Herdr>(h: &H, runs: &[RunState]) -> WatchTick {
    let mut findings = Vec::new();
    let mut anything_live = false;
    // Liveness gates the SWEEP, never the verdict, and that separation is the
    // whole reason it is applied here rather than by the caller filtering the
    // list. A run whose workspace herdr does not list has no panes worth asking
    // about — but it is still a run, and if its phases are outstanding it can be
    // handed an agent at any moment. Filtering it away before the tick made the
    // no-name `drovr watch` exit 0 with "none has phases outstanding" without
    // ever having read a single run's phases.
    let live = h.workspace_list();
    for run in runs {
        let scan = if workspace_is_live(run, live.as_deref()) {
            scan_run(h, run)
        } else {
            RunScan::default()
        };
        // `!is_complete()` is the third term, and it exists because of a race a
        // driver hits constantly: the documented use is to background this watch
        // ALONGSIDE `drovr phase start`, and between `drovr new` and the pane
        // appearing, every phase has `pane_id: None`. Judged on attached panes
        // alone the first poll would announce "nothing left to watch" and exit 0
        // while the phase was still launching, so the block it later hits would
        // wake nobody.
        //
        // A run with phases outstanding can always be handed an agent — by the
        // driver, or by the browser's ⟳ — so it is watchable whether or not one
        // is attached this instant. The cost is that a genuinely abandoned run
        // keeps the watch alive to its timeout (exit 2, "re-run to keep
        // watching") rather than exiting 0, which is the right way round: exit 0
        // asserts the run is OVER, and a harness acts on that.
        anything_live |= scan.attached > 0 || scan.unreadable > 0 || !run.is_complete();
        findings.extend(
            scan.blocked
                .into_iter()
                .filter(|a| a.class.needs_human())
                .map(|agent| WatchFinding {
                    run: run.name.clone(),
                    agent,
                }),
        );
    }
    if !findings.is_empty() {
        // An alarm wins over liveness: an agent parked on a destructive prompt
        // holds a session, so it is "live" by every other measure, and it is
        // precisely the state the watch exists to report.
        return WatchTick::Alarm(findings);
    }
    if anything_live {
        WatchTick::Watching
    } else {
        WatchTick::NothingToWatch
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::herdr::FakeHerdr;
    use crate::run::{Phase, PhaseStatus, RunState};

    /// A run with two phase panes and one review panel pane, all live.
    /// A fake herdr that reports [`run_with_panes`]'s workspaces as open, which
    /// is what makes `watch_tick` sweep them at all. The default fake reports NO
    /// live workspace, and a watch deliberately does not sweep a run herdr has
    /// no workspace for.
    fn fake_watching() -> FakeHerdr {
        let h = FakeHerdr::new();
        h.set_live_workspaces(Some(vec!["w".into(), "x".into()]));
        h
    }

    fn run_with_panes() -> RunState {
        let phase = |name: &str, pane: &str| {
            let mut p = Phase::new(name);
            p.status = PhaseStatus::Running;
            p.set_pane(pane);
            p
        };
        RunState {
            name: "r".into(),
            task: "t".into(),
            agent: Some("claude".into()),
            phases: vec![phase("brainstorm", "w:p1"), phase("implement", "w:p2")],
            review_phases: vec![phase("review:task-1:1:security", "w:p3")],
            gate: "spec".into(),
            cursor: 0,
            workspace: Some("w".into()),
            root_pane: Some("w:root".into()),
            project_dir: "/tmp/p".into(),
            worktree_path: None,
            worktree_branch: None,
            archived: false,
            retired_panes: vec![],
        }
    }

    #[test]
    fn a_destructive_prompt_is_reported_and_needs_a_human() {
        let h = FakeHerdr::new();
        h.push_status_for("w:p2", Some("blocked"));
        h.push_read_for(
            "w:p2",
            "Dangerous rm operation detected\n  rm -rf build/\n1. Yes\n2. No",
        );
        let found = scan_run(&h, &run_with_panes()).blocked;
        assert_eq!(found.len(), 1, "only the blocked pane is reported");
        assert_eq!(found[0].phase, "implement");
        assert_eq!(found[0].pane_id, "w:p2");
        assert_eq!(found[0].class, BlockedClass::Destructive);
        assert!(found[0].class.needs_human());
        assert!(
            found[0].excerpt.contains("rm -rf build/"),
            "the excerpt quotes the prompt: {}",
            found[0].excerpt
        );
    }

    #[test]
    fn a_routine_prompt_is_reported_but_raises_no_alarm() {
        let h = FakeHerdr::new();
        h.push_status_for("w:p2", Some("blocked"));
        h.push_read_for("w:p2", "Do you want to make this edit to main.rs?\n1. Yes");
        let found = scan_run(&h, &run_with_panes()).blocked;
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].class, BlockedClass::Routine);
        assert!(
            !found[0].class.needs_human(),
            "a driver's wait answers routine prompts; a watcher is not woken for them"
        );
    }

    /// The safety property this module exists to preserve. `triage_blocked_phase`
    /// answers routine prompts; the scan runs from a browser poll and must not.
    #[test]
    fn a_routine_prompt_is_never_auto_answered() {
        let h = FakeHerdr::new();
        h.push_status_for("w:p2", Some("blocked"));
        h.push_read_for("w:p2", "Do you want to create tests/new.rs?\n1. Yes");
        scan_run(&h, &run_with_panes());
        let sends: Vec<String> = h
            .calls()
            .into_iter()
            .filter(|c| c.starts_with("agent_send") || c.starts_with("agent_send_keys"))
            .collect();
        assert!(
            sends.is_empty(),
            "the scan must be read-only, but it sent: {sends:?}"
        );
    }

    #[test]
    fn a_pane_that_is_not_blocked_is_never_even_read() {
        let h = FakeHerdr::new();
        h.push_status_for("w:p1", Some("working"));
        h.push_status_for("w:p2", Some("idle"));
        h.push_status_for("w:p3", Some("done"));
        assert!(scan_run(&h, &run_with_panes()).blocked.is_empty());
        let reads: Vec<String> = h
            .calls()
            .into_iter()
            .filter(|c| c.starts_with("agent_read"))
            .collect();
        assert!(
            reads.is_empty(),
            "a pane read is only earned by a `blocked` status, but read: {reads:?}"
        );
    }

    #[test]
    fn an_unreadable_pane_leaves_the_status_unknown_not_blocked() {
        let h = FakeHerdr::new();
        h.fail_pane_info();
        let scan = scan_run(&h, &run_with_panes());
        assert!(
            scan.blocked.is_empty(),
            "herdr being unreachable is not evidence that an agent is stuck"
        );
        assert_eq!(scan.attached, 0);
        assert_eq!(
            scan.unreadable, 3,
            "and it must be counted as UNKNOWN, not as three panes with no agent"
        );
    }

    #[test]
    fn a_blocked_pane_drovr_cannot_read_escalates_as_unknown() {
        let h = FakeHerdr::new();
        h.push_status_for("w:p2", Some("blocked"));
        h.fail_agent_read();
        let found = scan_run(&h, &run_with_panes()).blocked;
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].class, BlockedClass::Unknown);
        assert!(found[0].class.needs_human());
        assert!(
            found[0].excerpt.contains("could not be read"),
            "the excerpt says why it is empty: {}",
            found[0].excerpt
        );
    }

    #[test]
    fn a_blocked_review_panel_is_reported_like_a_phase() {
        let h = FakeHerdr::new();
        h.push_status_for("w:p3", Some("blocked"));
        h.push_read_for("w:p3", "Bash command\n  git push --force\n1. Yes\n2. No");
        let found = scan_run(&h, &run_with_panes()).blocked;
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].phase, "review:task-1:1:security");
        assert_eq!(found[0].class, BlockedClass::Destructive);
    }

    #[test]
    fn a_reaped_phase_is_not_scanned() {
        let h = FakeHerdr::new();
        h.push_status_for("w:p2", Some("blocked"));
        h.push_read_for("w:p2", "Dangerous rm operation\n1. Yes");
        let mut run = run_with_panes();
        run.phases[1].mark_reaped();
        assert!(
            scan_run(&h, &run).blocked.is_empty(),
            "a reaped phase holds no pane, so there is nothing to be blocked in"
        );
    }

    #[test]
    fn a_pane_whose_agent_exited_is_neither_attached_nor_unknown() {
        let h = FakeHerdr::new();
        // herdr's own `unknown` is what it reports for a pane whose agent has
        // gone: it ANSWERED, and the answer is "nothing is running here".
        h.push_status_for("w:p1", Some("unknown"));
        h.push_status_for("w:p2", Some("unknown"));
        h.push_status_for("w:p3", Some("working"));
        let scan = scan_run(&h, &run_with_panes());
        assert_eq!(scan.attached, 1, "only the pane still holding an agent");
        assert_eq!(scan.unreadable, 0, "herdr answered for all three");
    }

    /// The reason this filter exists: a run keeps its `pane_id`s forever, so
    /// sweeping a run whose workspace is gone costs a failed `pane.get` — and a
    /// "polling is degraded" line on stderr — per pane, per sweep. On a data dir
    /// of thirty old runs that buried `drovr list`'s own output.
    #[test]
    fn a_run_whose_workspace_herdr_no_longer_has_is_not_swept() {
        let h = FakeHerdr::new();
        h.set_live_workspaces(Some(vec!["w".into()]));
        let mut gone = run_with_panes();
        gone.name = "gone".into();
        gone.workspace = Some("w-old".into());
        let kept = with_live_workspace(&h, vec![run_with_panes(), gone]);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].name, "r");
    }

    #[test]
    fn a_run_that_never_had_a_workspace_is_not_swept_either() {
        let h = FakeHerdr::new();
        h.set_live_workspaces(Some(vec!["w".into()]));
        let mut orphan = run_with_panes();
        orphan.workspace = None;
        assert!(with_live_workspace(&h, vec![orphan]).is_empty());
    }

    /// Unknown is not gone. A herdr blip must not quietly stop a watch from
    /// looking at anything.
    #[test]
    fn an_unreachable_herdr_keeps_every_run_in_the_sweep() {
        let h = FakeHerdr::new();
        h.set_live_workspaces(None);
        let mut gone = run_with_panes();
        gone.workspace = Some("w-old".into());
        assert_eq!(with_live_workspace(&h, vec![run_with_panes(), gone]).len(), 2);
    }

    #[test]
    fn a_watch_alarms_on_the_first_agent_that_needs_a_human() {
        let h = fake_watching();
        h.push_status_for("w:p2", Some("blocked"));
        h.push_read_for("w:p2", "Dangerous rm operation\n1. Yes\n2. No");
        let WatchTick::Alarm(found) = watch_tick(&h, &[run_with_panes()]) else {
            panic!("a destructive prompt must raise the alarm");
        };
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].run, "r");
        assert_eq!(found[0].agent.phase, "implement");
    }

    #[test]
    fn a_watch_keeps_watching_through_a_routine_prompt() {
        let h = fake_watching();
        h.push_status_for("w:p2", Some("blocked"));
        h.push_read_for("w:p2", "Do you want to read Cargo.toml?\n1. Yes");
        assert_eq!(
            watch_tick(&h, &[run_with_panes()]),
            WatchTick::Watching,
            "a driver's wait answers this one; waking a human for it is noise"
        );
    }

    #[test]
    fn a_watch_ends_when_every_agent_has_exited_and_the_run_is_finished() {
        let h = fake_watching();
        for pane in ["w:p1", "w:p2", "w:p3"] {
            h.push_status_for(pane, Some("unknown"));
        }
        let mut run = run_with_panes();
        for p in run.phases.iter_mut() {
            p.status = PhaseStatus::Done;
        }
        assert_eq!(
            watch_tick(&h, &[run]),
            WatchTick::NothingToWatch,
            "nothing can block in a finished run with no agents left"
        );
    }

    /// The race the third term of `anything_live` exists for: a driver
    /// backgrounds the watch alongside `drovr phase start`, and for the moment
    /// before the pane appears the run holds no pane at all. Exiting 0 there
    /// tells the driver the run is over while its first phase is still
    /// launching, and the block it later hits wakes nobody.
    #[test]
    fn a_watch_does_not_end_on_a_run_whose_phases_have_not_started() {
        let h = FakeHerdr::new();
        let mut run = run_with_panes();
        run.review_phases.clear();
        for p in run.phases.iter_mut() {
            p.status = PhaseStatus::Pending;
            p.forget_dangling_pane();
        }
        assert_eq!(
            watch_tick(&h, &[run]),
            WatchTick::Watching,
            "a run with phases outstanding can still be handed an agent"
        );
        assert!(
            h.calls().iter().all(|c| !c.contains("agent_status")),
            "and it costs no herdr poll to say so — there are no panes to poll"
        );
    }

    /// Same rule between phases, which is where a long run spends most of its
    /// life: brainstorm done and reaped, plan not started, no pane anywhere.
    #[test]
    fn a_watch_does_not_end_between_two_phases() {
        let h = FakeHerdr::new();
        let mut run = run_with_panes();
        run.review_phases.clear();
        run.phases[0].status = PhaseStatus::Done;
        run.phases[0].mark_reaped();
        run.phases[1].status = PhaseStatus::Pending;
        run.phases[1].forget_dangling_pane();
        assert_eq!(watch_tick(&h, &[run]), WatchTick::Watching);
    }

    /// The failure this distinction exists to prevent: herdr blips, every pane
    /// reads as unreadable, and a watcher announces the run finished.
    #[test]
    fn an_unreachable_herdr_is_not_a_finished_run() {
        let h = FakeHerdr::new();
        h.fail_pane_info();
        assert_eq!(
            watch_tick(&h, &[run_with_panes()]),
            WatchTick::Watching,
            "not knowing is not the same as knowing there is nothing there"
        );
    }

    #[test]
    fn a_watch_spans_every_run_it_was_given() {
        let h = fake_watching();
        let mut other = run_with_panes();
        other.name = "second".into();
        other.phases[0].set_pane("x:p1");
        other.phases[1].set_pane("x:p2");
        other.review_phases[0].set_pane("x:p3");
        h.push_status_for("x:p2", Some("blocked"));
        h.push_read_for("x:p2", "Some prompt drovr has never seen\n1. Yes");
        let WatchTick::Alarm(found) = watch_tick(&h, &[run_with_panes(), other]) else {
            panic!("the block is in the second run, and must still be found");
        };
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].run, "second");
        assert_eq!(found[0].agent.class, BlockedClass::Unknown);
    }

    #[test]
    fn every_blocked_pane_in_a_run_is_reported() {
        let h = FakeHerdr::new();
        h.push_status_for("w:p1", Some("blocked"));
        h.push_read_for("w:p1", "Delete the branch?\n1. Yes");
        h.push_status_for("w:p3", Some("blocked"));
        h.push_read_for("w:p3", "Some prompt drovr has never seen\n1. Yes");
        let found = scan_run(&h, &run_with_panes()).blocked;
        assert_eq!(found.len(), 2, "the scan does not stop at the first block");
        assert_eq!(found[0].phase, "brainstorm");
        assert_eq!(found[1].phase, "review:task-1:1:security");
    }
}
