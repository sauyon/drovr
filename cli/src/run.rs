use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::{fs, io};

use crate::herdr::SessionId;

#[derive(Serialize, Deserialize, PartialEq, Clone, Debug, Default)]
pub enum PhaseStatus {
    #[default]
    Pending,
    Running,
    Done,
    Failed,
}

/// Identifies ONE launch of a phase's agent ("pass"). Minted by `phase_start`,
/// carried to the agent in `$DROVR_PASS`, stamped into `<phase>.done`, and
/// compared for equality by `phase_wait` — never parsed, ordered, or displayed as
/// anything but itself.
///
/// A newtype rather than a bare `String` because every other string in this area
/// (phase names, pane ids, marker paths, env var names) is also a `String`, and
/// mixing them up would be a silent equality check that is always false — i.e. a
/// phase that never completes. This makes that class of mistake a type error.
///
/// An empty token is NOT representable — neither through [`PassToken::new`] nor
/// through `Deserialize`. "This phase has no token" is `Option::None` and means
/// something specific (a run created before pass tokens, which completes on an
/// UNTOKENIZED marker); a `PassToken("")` would be `Some` while matching no
/// marker at all, i.e. a phase that can never complete under either rule. The
/// two must not be the same value.
///
/// Serializes transparently as a JSON string, so `state.json` is unchanged.
#[derive(Serialize, Deserialize, PartialEq, Eq, Clone, Debug)]
#[serde(try_from = "String")]
pub struct PassToken(String);

impl PassToken {
    /// `None` for a value no marker could ever match. The only in-tree caller
    /// (`phase::new_pass_token`) builds from `format!`ed pid/nanos/counter and so
    /// can never hit it, but `new` is `pub` and this is the invariant the type
    /// exists to carry.
    pub fn new(value: String) -> Option<PassToken> {
        if value.trim().is_empty() {
            return None;
        }
        Some(PassToken(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    /// Whether a `<phase>.done` marker holding `token` was written by the agent
    /// this pass launched. Empty never matches: an empty marker means the writer
    /// had no `$DROVR_PASS` at all, which is either a pre-token build or a
    /// `phase done` run from outside the pane — neither is evidence about *this*
    /// pass.
    pub fn matches_marker(&self, token: &str) -> bool {
        !token.is_empty() && token == self.0
    }
}

/// The second constructor: `state.json` is a file, and anything that can write it
/// can propose a token. It is held to exactly the rule [`PassToken::new`]
/// enforces, so a deserialized token is as trustworthy as a minted one. A run
/// whose state really does carry `"pass": ""` fails to load LOUDLY rather than
/// running on with a phase that no `phase wait` could ever complete.
impl TryFrom<String> for PassToken {
    type Error = String;
    fn try_from(value: String) -> Result<PassToken, String> {
        PassToken::new(value).ok_or_else(|| "pass token must not be empty".to_string())
    }
}

impl std::fmt::Display for PassToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// One phase of a run. Construct via [`Phase::new`] or
/// `{ let mut p = Phase::new(&…); p }` — never by listing every field, so
/// adding a field stays a one-line diff instead of touching ~25 literal sites.
///
/// The cost of that convenience, for whoever adds the next field:
/// * The compiler no longer flags construction sites that ought to populate it.
///   Grep the `..Default::default()` sites and decide each one deliberately.
/// * The first five fields (`name`, `status`, `handoff_doc`, `herdr_session`,
///   `pane_id`) are NOT `#[serde(default)]`, so deserialization requires them;
///   every field added since (`pass`, `tab_id`, `pane_agent`, `reaped`) carries its own
///   `#[serde(default, skip_serializing_if = …)]`. There is no struct-level
///   default: a new field without that attribute makes every existing
///   `state.json` fail to load → `load_run` exits 1 → the run STOPs. Mirror what
///   the four newer fields do, and add a back-compat test for it (see
///   `missing_phase_pass_defaults_to_none` /
///   `missing_phase_capture_fields_default_to_absent`).
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Phase {
    pub name: String,
    pub status: PhaseStatus,
    pub handoff_doc: Option<String>,
    /// DEAD. Always written as `None` (`phase_start`), asserted `None` by a
    /// test, and pinned `null` by four back-compat fixtures below. It predates
    /// pane-id-based cleanup, which made it unnecessary.
    ///
    /// Kept rather than repurposed into [`PhaseAgent::session`]. Repurposing
    /// would have meant a field whose meaning on disk changes between drovr
    /// builds: every `state.json` already carries `"herdr_session": null`, and a
    /// build that started reading it as a resumable session id would be reading
    /// a key written under different rules by a different version. A dead field
    /// that is obviously dead is safer than a live one that used to mean
    /// something else. Removing it outright is a separate, mechanical change
    /// (four fixtures and an assertion) and is not this task's.
    pub herdr_session: Option<String>,
    /// The live herdr pane this phase occupies, if any. **Private — see
    /// [`Phase::set_pane`] and [`Phase::mark_reaped`].**
    ///
    /// It is half of a lifecycle pair with [`Phase::reaped`], and as two public
    /// fields the contradictory combination was one assignment away.
    pane_id: Option<String>,
    /// Token identifying the CURRENT pass over this phase, minted by each
    /// `phase_start` and exported into the agent's environment as `DROVR_PASS`.
    /// `drovr phase done` stamps it into `<phase>.done`, and `phase_wait` accepts
    /// a marker only if its token matches this one — which is what stops the
    /// previous pass's still-live agent from completing the current pass by
    /// recreating the marker. `None` only for runs created before pass tokens
    /// existed — `phase_start` persists a token before it launches anything, so no
    /// phase this build has started is ever `None`. Such a legacy phase completes
    /// on an UNTOKENIZED marker (its agent has no `$DROVR_PASS` to stamp either);
    /// a tokened marker against it is an inconsistency and is rejected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pass: Option<PassToken>,
    /// The herdr tab holding [`Phase::pane_id`], captured opportunistically by
    /// the poll loops in `phase.rs`.
    ///
    /// **Diagnostic, not an operand.** A tab id read minutes ago may name a tab
    /// that is gone or reused, and `Herdr::tab_close` deliberately takes a
    /// `herdr::TabId` that only a live `pane_info` read can mint — so anything
    /// about to close a tab resolves a fresh one first, and this field exists so
    /// a human (or a `state.json` reader) can see which tab a phase occupied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<String>,
    /// What is (or last was) running in this phase's pane — see [`PhaseAgent`].
    /// `None` for a phase this build never launched.
    ///
    /// **Named `pane_agent`, not `agent`, to keep it distinct from
    /// `RunState::agent`.** They sat one level apart holding different types and
    /// meaning different things — this phase's captured agent record versus the
    /// run's configured default backend — which is a standing invitation to
    /// reach for the wrong one. A reviewer's backend legitimately differs from
    /// the run's (`Config::review_agent_for` picks it), so the two really are
    /// independent facts.
    ///
    /// Private, like the rest of the lifecycle: read it with
    /// [`Phase::pane_agent`], write it with [`Phase::record_launch`] /
    /// [`Phase::record_session`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pane_agent: Option<PhaseAgent>,
    /// Whether drovr closed this phase's pane — see [`Reaped`], which cannot be
    /// set to "yes" from outside this module.
    ///
    /// Written by [`Phase::mark_reaped`] alone, on the reap paths in
    /// `phase.rs`. It was declared one task before anything wrote it, so the
    /// whole capture record landed in one `state.json` shape rather than
    /// migrating twice, and so the lifecycle rule was in place before the code
    /// that depends on it.
    #[serde(default, skip_serializing_if = "Reaped::is_no")]
    reaped: Reaped,
}

/// Whether drovr has closed a phase's pane.
///
/// A newtype over `bool` whose inner value is PRIVATE, so "yes" can only be
/// produced inside this module — in practice by [`Phase::mark_reaped`], which
/// drops the pane id in the same statement.
///
/// The point is that "reaped" and "has a live pane" are contradictory claims
/// about one phase, and as a bare `pub bool` beside `pane_id` the contradiction
/// was one assignment away. A phase marked reaped while `pane_id` still names a
/// pane makes `drovr attach` offer a pane that is gone, and makes cleanup and
/// reaping disagree about whose pane it is. The rule landed one task before
/// reaping did, deliberately: it is much harder to impose once reaping depends
/// on the shape.
///
/// Serializes transparently as a bare `true`/`false`, so `state.json` is
/// unchanged by the newtype.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(transparent)]
pub struct Reaped(bool);

impl Reaped {
    /// Whether the pane has been reaped.
    pub fn yes(self) -> bool {
        self.0
    }
    /// For `skip_serializing_if`: an un-reaped phase stays absent from
    /// `state.json`, so a run written by this build still loads on an older one.
    pub fn is_no(&self) -> bool {
        !self.0
    }
}

impl Phase {
    /// A `Pending` phase named `name`, with every other field at its default.
    pub fn new(name: &str) -> Phase {
        Phase {
            name: name.to_owned(),
            ..Default::default()
        }
    }

    /// The live pane this phase occupies, if any.
    pub fn pane_id(&self) -> Option<&str> {
        self.pane_id.as_deref()
    }

    /// Give this phase a live pane.
    ///
    /// Clears `reaped` in the same statement, which is not a convenience but the
    /// definition: a phase holding a live pane is not a reaped one. That closes
    /// the contradictory state from the only direction [`Phase::mark_reaped`]
    /// leaves open — assigning a pane to an already-reaped phase — and it is
    /// exactly the transition rehydrate performs when it brings a reaped phase
    /// back on a fresh pane.
    pub fn set_pane(&mut self, pane_id: impl Into<String>) {
        self.pane_id = Some(pane_id.into());
        self.reaped = Reaped(false);
    }

    /// Drop a pane id that no longer names anything, WITHOUT claiming drovr
    /// reaped it.
    ///
    /// The one caller is `ensure_workspace`'s repair path: the run's whole herdr
    /// workspace is gone, so every recorded pane id in it is dangling — and a
    /// stale id is exactly what `phase_send` would aim at and what `cleanup`
    /// would try to close. That is a different fact from [`Phase::mark_reaped`],
    /// which says drovr deliberately closed a pane it still wants `cleanup` to
    /// account for. Here nothing was closed and there is nothing to retire; the
    /// pane died with its workspace. Conflating the two would advertise a
    /// rehydrate for a phase whose pane drovr never took.
    pub fn forget_dangling_pane(&mut self) {
        self.pane_id = None;
    }

    /// Test-only: drop the agent record, to model a phase launched by a build
    /// that never recorded one. There is deliberately no production way to do
    /// this — a launch always records, and a relaunch REPLACES.
    #[cfg(test)]
    pub fn clear_pane_agent_for_test(&mut self) {
        self.pane_agent = None;
    }

    /// Test-only builder: the same `set_pane` transition, chainable, so a
    /// fixture can still be written as one expression now that `pane_id` is not
    /// nameable in a struct literal.
    #[cfg(test)]
    pub fn with_pane(mut self, pane_id: impl Into<String>) -> Phase {
        self.set_pane(pane_id);
        self
    }

    /// What is (or last was) running in this phase's pane.
    pub fn pane_agent(&self) -> Option<&PhaseAgent> {
        self.pane_agent.as_ref()
    }

    /// Everything a resume needs for this phase, or `None` — the whole bundle
    /// or nothing. See [`ResumeTarget`]; this is the accessor task 5 uses.
    pub fn resume_target(&self) -> Option<ResumeTarget<'_>> {
        self.pane_agent.as_ref().and_then(PhaseAgent::resume)
    }

    /// Record a launch into this phase's pane: a NEW agent record, with no
    /// session yet.
    ///
    /// Replaces any previous record wholesale, which is the point — a launch
    /// starts a new agent process, so whatever session the old record named
    /// belongs to a conversation that is no longer this phase's. There is
    /// deliberately no way to clear a session on its own (see
    /// [`PhaseAgent::record_session`]); replacing the record is the only way a
    /// session is ever discarded, and it happens exactly where a new process
    /// starts.
    pub fn record_launch(&mut self, backend: impl Into<String>, profile: Option<String>) {
        self.pane_agent = Some(PhaseAgent::launched(backend, profile));
    }

    /// Seed this phase's agent record from one read off disk, **only if it has
    /// none**. Returns whether it landed.
    ///
    /// Used when reconciling with what is already persisted: another writer may
    /// have recorded a profile or a backend this process never saw, so the
    /// record is taken whole rather than rebuilt.
    ///
    /// ⚠️ **It REFUSES an occupied slot, and that refusal is the point.** This
    /// was `adopt_pane_agent`, which assigned wholesale with no guard — so the
    /// fill-never-replace rule that [`Phase::record_session`] enforces could be
    /// walked straight around, session and all, without going through
    /// [`Phase::record_launch`]. Production was safe only because its one
    /// caller happened to test `pane_agent().is_none()` first, which is the
    /// invariant living in a call-site `match` instead of in the API — the
    /// same drift this module has now closed for reviewer identity and for
    /// resume evidence. A future caller cannot skip a guard that is no longer
    /// theirs to skip.
    ///
    /// `record_launch` remains the one sanctioned way an existing record — and
    /// so a held session — is ever replaced, because that is where a new agent
    /// process starts.
    pub fn seed_pane_agent(&mut self, agent: PhaseAgent) -> bool {
        if self.pane_agent.is_some() {
            return false;
        }
        self.pane_agent = Some(agent);
        true
    }

    /// Record a session captured from a live poll. Returns whether it landed.
    ///
    /// `false` means one of the two things this refuses, and neither is a
    /// failure to paper over:
    ///
    /// * there is no `PhaseAgent` — a session is only meaningful beside the
    ///   backend that created it, so it must not be stored without one. The
    ///   caller establishes the launch record first (`record_launch`);
    /// * a session is ALREADY held — see [`Phase::accepts_captured_session`].
    pub fn record_session(&mut self, session: SessionId) -> bool {
        match self.pane_agent.as_mut() {
            Some(agent) => agent.record_session(session),
            None => false,
        }
    }

    /// ⭐ **Whether a captured session may be written to this phase: only into
    /// an EMPTY slot.** The rule, in one place, asked by the predicates that
    /// decide whether a capture has work to do and enforced by
    /// [`PhaseAgent::record_session`], which is the only thing that can write
    /// one.
    ///
    /// # The bug this closes
    ///
    /// A rehydrate that RESUMES deliberately does not `record_launch` — the
    /// conversation is meant to be the same one, so the agent record must
    /// survive the relaunch. Both of its waits poll the NEW pane through a
    /// capturing poll, and when the resume did not land that pane's agent
    /// reports a *different* session. Capture wrote it over the recorded id and
    /// saved it, so an honest exit-2 "your conversation did not come back" left
    /// the state worse than before the attempt: the resume token a retry needs
    /// was gone, and every later rehydrate would compose `--resume` for a
    /// stranger's conversation instead of reseeding from the handoff.
    ///
    /// It was first closed one layer up, in the capture logic. That left the
    /// rule in a caller rather than on the API that mutates, so any future call
    /// to `record_session` with a session already held reintroduced it with no
    /// type error. It lives here now, and the predicate and the mutator are the
    /// same fact so they cannot disagree about whether there is work to do.
    ///
    /// # Why "fill, never replace" is right in general
    ///
    /// A session is meaningful only beside the agent process that created it,
    /// and a new process means a new record: [`Phase::record_launch`] REPLACES
    /// the whole `PhaseAgent`, clearing the session, and a capture then fills
    /// the empty slot. That is the only sanctioned way a session ever changes.
    /// So a poll reporting a session *different* from the recorded one is never
    /// evidence of a legitimate change — it says the pane is running an agent
    /// the phase's record does not describe, which is a thing to refuse to
    /// write down, not a thing to overwrite with.
    ///
    /// `true` for a phase with no agent record at all: the capture path creates
    /// one (carrying the backend it resolved) and then fills it, so there is
    /// genuinely something to add.
    ///
    /// # What it deliberately gives up
    ///
    /// A backend that minted a NEW session id on resume would never have its
    /// record updated. That is known not to be claude's behaviour — verified
    /// live, `claude --resume <id>` reports `<id>` back byte-identical — and
    /// drovr could not tell such a backend apart from "we landed in a
    /// stranger's conversation" anyway. Both deserve the same answer, and it is
    /// an unconfirmed resume, not a silent overwrite.
    pub fn accepts_captured_session(&self) -> bool {
        self.pane_agent
            .as_ref()
            .is_none_or(|a| a.session().is_none())
    }

    /// The half of the rehydrate precondition that is about the PHASE.
    ///
    /// Private, and it must stay private: [`RunState::rehydratable`] is the one
    /// predicate every caller asks, and this exists only as the part of it that
    /// needs nothing but the phase. Exposed, it is a second, weaker gate — which
    /// is exactly the defect that made the ⟳ render on phases the CLI refused.
    ///
    /// It is deliberately STRICTER than [`Phase::has_run`], which answers a
    /// different question ("is this a real phase or a `drovr new` placeholder",
    /// for display). Reusing the weaker one as the gate re-opens the door the
    /// `NoAgentEverRan` arm exists to close.
    fn phase_level_rehydratable(&self) -> Result<(), NotRehydratable> {
        if let Some(pane) = self.pane_id() {
            return Err(NotRehydratable::HoldsPane(pane.to_owned()));
        }
        if !self.has_run() {
            return Err(NotRehydratable::NeverStarted);
        }
        // `has_run()` is true for a phase whose launch FAILED: `phase_start`
        // persists `Running` before it launches, and only records the agent on
        // success. Such a phase never had an agent in it — no backend, no
        // profile, no session, and no seed — so bringing it "back" would be
        // `phase start` under a name that promises recovery. A phase reaped by
        // a build older than the agent record is exempt: reaping only ever
        // touched a phase that held a pane, so it demonstrably ran.
        if !self.is_reaped() && self.pane_agent().is_none() {
            return Err(NotRehydratable::NoAgentEverRan);
        }
        Ok(())
    }

    /// Whether an agent has ever been launched into this phase.
    ///
    /// **The one answer to "is this a real phase or a placeholder".** `drovr
    /// new` pre-seeds every run with `Pending` phases that have never held an
    /// agent, and `phase_start` appends any name it is handed — so "the phase
    /// exists in `state.json`" is not evidence that anything ran in it.
    ///
    /// Two consumers, deliberately sharing one predicate: the review UI's agent
    /// tree omits a phase that has not run (a placeholder is not an agent), and
    /// `phase_rehydrate` refuses one (there is nothing to bring back — that is
    /// `drovr phase start`). Split into two predicates, the tree could offer a
    /// ⟳ on a node the CLI would then refuse.
    ///
    /// `is_reaped()` is checked first and separately from the status because
    /// reaping does not change a phase's status: a reaped `Done` phase must
    /// still answer `true` here, and so must a reaped phase of any status.
    pub fn has_run(&self) -> bool {
        self.is_reaped() || self.status != PhaseStatus::Pending
    }

    /// Whether drovr has closed this phase's pane. Read by [`Phase::has_run`],
    /// by `phase_rehydrate` (a reaped phase is the thing it brings back), and by
    /// `main::rehydrate_hint`. Written only by [`Phase::mark_reaped`].
    pub fn is_reaped(&self) -> bool {
        self.reaped.yes()
    }

    /// Record that drovr closed this phase's pane, dropping the pane id in the
    /// same step.
    ///
    /// **This is the only way to set `reaped`, and that is the point.** "Reaped"
    /// and "has a live pane" are contradictory claims about one phase, and as
    /// two independent public fields the contradiction was one assignment away:
    /// `reaped = true` while `pane_id` still names a pane makes `drovr attach`
    /// offer a pane that is gone, and makes cleanup and reaping disagree about
    /// whose pane it is. Setting one requires clearing the other, here, in one
    /// statement no caller can half-perform.
    ///
    /// Returns the pane id it dropped, which is what a caller needs to retire
    /// (`RunState::retire_pane`) so cleanup still knows the pane was drovr's.
    ///
    /// Two callers, and they arrive from opposite directions:
    /// `phase::surrender_misattributed_pane` (error recovery on a half-completed
    /// rehydrate) and `phase::phase_reap` (supersession). Both go through
    /// `phase::release_phase_from_pane`, so the retirement and this transition
    /// land in one save. The transition is the same either way, and deliberately
    /// so: "drovr closed this phase's pane" is one fact with one way to record
    /// it.
    pub fn mark_reaped(&mut self) -> Option<String> {
        self.reaped = Reaped(true);
        self.pane_id.take()
    }
}

/// The prefix every review-panel agent's phase name carries.
///
/// Spelled once so [`reviewer_phase_name`] and [`is_reviewer_phase_name`]
/// cannot disagree about the shape, and so the per-task scans in
/// `code_review.rs` build their prefixes from the same constant.
pub const REVIEWER_PREFIX: &str = "review:";

/// The name a review-panel agent runs under. **The only place this shape is
/// constructed.**
pub fn reviewer_phase_name(task: &str, iter: u64, angle: &str) -> String {
    format!("{REVIEWER_PREFIX}{task}:{iter}:{angle}")
}

/// Whether `name` identifies a review-panel agent.
///
/// ⭐ **Identity, not list membership, and that distinction was a real bug.**
/// The rehydrate refusal used to ask `review_phases.iter().any(|p| p.name ==
/// name)` — but a reviewer-shaped name is a perfectly legal `phase_start` name,
/// so `drovr phase start <run> review:t:1:security` registered one in `phases`,
/// where that scan could not see it. The impostor then passed
/// [`RunState::rehydratable`], rendered a ⟳, and was relaunched with
/// `readonly = false` and no findings MCP — the exact two things
/// [`NotRehydratable::Reviewer`] exists to prevent.
///
/// So there is ONE predicate and both gates ask it: the creation gate in
/// `phase_start` (which now refuses to mint such a name at all — the panel
/// mints them) and the rehydrate gate here. A name cannot drift from a list it
/// is not consulted against.
pub fn is_reviewer_phase_name(name: &str) -> bool {
    name.starts_with(REVIEWER_PREFIX)
}

/// Why a phase cannot be rehydrated — see [`RunState::rehydratable`].
///
/// An enum rather than a bool because each arm needs a *different* thing said
/// to the user (attach to the pane / start the phase / start it WITH its seed),
/// and because the HTTP layer has to map them to status codes without
/// re-deriving the reasons.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotRehydratable {
    /// No phase of this run answers to that name. The one arm the HTTP layer
    /// maps to 404 rather than 409; every other arm is a real phase drovr is
    /// refusing to act on.
    NoSuchPhase,
    /// A review-panel agent.
    ///
    /// A reviewer's only job is to deliver findings, and it delivers them
    /// through drovr's findings MCP server — written per (task, iteration) and
    /// handed to the agent on its command line at launch. `resume_launch`
    /// passes no `mcp_config`, so a resumed reviewer would come up with no
    /// `submit_findings` tool and `delivered_review` would wait on a file that
    /// can never appear. Threading the server through would mean rewriting the
    /// per-task config for an OLD iteration, which is the file a currently
    /// running panel's reviewers read. So drovr does not offer the button:
    /// a panel is re-run (`drovr code-review run`), not rehydrated.
    Reviewer,
    /// Still holds a pane, named here. Attach to it instead.
    HoldsPane(String),
    /// A `Pending` placeholder — `drovr new` seeds four of them. Nothing ran.
    NeverStarted,
    /// It looks started, but no agent was ever recorded: its last
    /// `phase start` persisted `Running` and then failed to launch.
    NoAgentEverRan,
    /// The RUN records no `project_dir`, so there is no directory to launch the
    /// agent in — and no cwd for its session to resolve under.
    NoProjectDir,
    /// The RUN has no herdr workspace, so there is nowhere to open the tab.
    NoWorkspace,
}

/// Why a phase's pane cannot be reaped — see [`RunState::reapable`].
///
/// An enum rather than an `Option<&str>` for the same reason
/// [`NotRehydratable`] is one: each arm needs a different thing said, and the
/// two callers do different things with them. `NoPane` is the ordinary,
/// successful "nothing to do"; `RootShell` is a refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotReapable {
    /// No phase of this run answers to that name.
    NoSuchPhase,
    /// It holds no pane: never launched, already reaped, or its pane died with
    /// its workspace (`Phase::forget_dangling_pane`). Nothing to close and
    /// nothing to clear — which is what makes a second reap of one phase a
    /// no-op rather than an error.
    NoPane,
    /// Its recorded pane IS the run's root shell, named here.
    ///
    /// The root pane anchors the workspace for the run's whole lifetime, and
    /// herdr destroys a workspace when its last pane closes — so reaping it
    /// takes the workspace and every other phase in it. No phase this build
    /// launches can reach this state (`phase_start` gives every phase its own
    /// tab), but a `state.json` written by a build where the FIRST phase
    /// claimed the root pane can, and such a run still loads and still works.
    RootShell(String),
}

/// The three things a resume needs, handed over together or not at all.
///
/// Assembled only by [`PhaseAgent::resume`]. A session id alone is not enough to
/// resume anything:
/// * `backend` — the id is only meaningful to the agent that created it
///   (`AgentSession::resumable_for` is built on exactly that rule);
/// * `profile` — claude resolves a session under
///   `$CLAUDE_CONFIG_DIR/projects/<escaped-cwd>/`, so resuming under a different
///   profile silently finds NOTHING. `None` means the default profile, which is
///   what the launch itself inlined.
///
/// Bundling all three is the difference between "atomic" and "sufficient": an
/// earlier version of this type paired session with backend only, which is
/// atomic and still not enough to find the conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResumeTarget<'a> {
    session: &'a SessionId,
    backend: &'a str,
    profile: Option<&'a str>,
}

impl<'a> ResumeTarget<'a> {
    /// The session id to resume.
    pub fn session(&self) -> &'a SessionId {
        self.session
    }
    /// The agent that created it — the only one it means anything to.
    pub fn backend(&self) -> &'a str {
        self.backend
    }
    /// The `CLAUDE_CONFIG_DIR` to resume UNDER; `None` = the default profile.
    pub fn profile(&self) -> Option<&'a str> {
        self.profile
    }
}

/// The agent behind a phase's pane: what it was launched as, and — once a poll
/// caught it while it was alive — the session a resume can bring back.
///
/// **The point of this type is that `backend` is REQUIRED.** A session id is
/// only meaningful to the agent that created it (`AgentSession::resumable_for`
/// is built on exactly that), so "a session with no backend" must not be
/// representable at the `state.json` boundary any more than it is in `herdr.rs`.
/// As two independent `Option` fields it was: they could disagree in both
/// directions, and re-checking the pairing would have fallen to every reader.
/// Here the pairing is structural — [`PhaseAgent::resume`] hands back the whole
/// bundle or nothing.
///
/// A `backend` with no `session` is the normal pre-capture state and is fine;
/// that is the asymmetry the type encodes.
///
/// **Fields are private.** `Deserialize` is the one other constructor, and it is
/// held to the same shape (`backend` required). Everything else goes through
/// [`PhaseAgent::launched`] and [`PhaseAgent::record_session`], so there is no
/// way to assemble a half-built record in memory and persist it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PhaseAgent {
    /// The backend the pane was launched with (`claude`, `cursor`, …).
    backend: String,
    /// The `CLAUDE_CONFIG_DIR` in effect at launch; `None` = the default
    /// profile. Recorded because claude resolves a session under
    /// `$CLAUDE_CONFIG_DIR/projects/<escaped-cwd>/`, so resuming from a process
    /// holding a *different* profile silently finds nothing. The launch is the
    /// only moment it is knowable — a later command may run from a plain shell.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    profile: Option<String>,
    /// The session id, captured while the agent was ALIVE because herdr drops
    /// `agent_session` the moment the process exits (verified against 0.7.5).
    ///
    /// A [`SessionId`], not a `String`: `herdr.rs` makes "a `kind:"id"` session,
    /// attributed to this backend, in an alphabet a `--resume` can carry" a
    /// property of the type, and a bare string would drop that proof here.
    /// `Deserialize` re-checks the alphabet, so a loaded id is as trustworthy as
    /// a parsed one.
    ///
    /// Once set it is only ever REPLACED, never cleared by a poll — an absent
    /// session in a later poll is herdr forgetting, not the agent disowning it.
    /// `phase_start` is the one exception: it launches a NEW process in the
    /// pane, so the id it clears names a conversation that is no longer this
    /// phase's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    session: Option<SessionId>,
}

impl PhaseAgent {
    /// A freshly launched agent: backend and profile known, no session yet.
    pub fn launched(backend: impl Into<String>, profile: Option<String>) -> PhaseAgent {
        PhaseAgent {
            backend: backend.into(),
            profile,
            session: None,
        }
    }

    /// The backend this pane runs.
    pub fn backend(&self) -> &str {
        &self.backend
    }

    /// The `CLAUDE_CONFIG_DIR` the launch used, if any.
    pub fn profile(&self) -> Option<&str> {
        self.profile.as_deref()
    }

    /// The captured session id, if a poll has caught one.
    pub fn session(&self) -> Option<&SessionId> {
        self.session.as_ref()
    }

    /// Record a session id captured from a live poll.
    ///
    /// Only ever SETS. There is deliberately no way to clear a session through
    /// this type: herdr dropping `agent_session` means the agent exited, not
    /// that the conversation stopped existing. `phase_start` clears by replacing
    /// the whole `PhaseAgent`, which is the one case where the old id really
    /// does name someone else's conversation.
    /// Fill this record's session. Returns whether it landed.
    ///
    /// **Refuses to REPLACE a session already held** — see
    /// [`Phase::accepts_captured_session`] for why that rule lives on the
    /// mutator and not in a caller. [`PhaseAgent::launched`] (via
    /// `Phase::record_launch`) is the only way a held session is ever
    /// discarded, and it discards it by replacing the whole record, which is
    /// exactly where a new agent process starts.
    pub fn record_session(&mut self, session: SessionId) -> bool {
        if self.session.is_some() {
            return false;
        }
        self.session = Some(session);
        true
    }

    /// Everything a resume needs, or `None`. All three or nothing — that is the
    /// whole reason this is one type.
    ///
    /// Nothing resumes yet, so this is exercised by tests alone until task 5
    /// composes `--resume`; it exists now because it is the accessor that makes
    /// the bundle impossible to take apart.
    pub fn resume(&self) -> Option<ResumeTarget<'_>> {
        self.session.as_ref().map(|session| ResumeTarget {
            session,
            backend: &self.backend,
            profile: self.profile.as_deref(),
        })
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RunState {
    pub name: String,
    pub task: String,
    pub phases: Vec<Phase>,
    /// Agent backend captured when the run was created. Older runs fall back to
    /// Claude, which was the only backend before this field existed.
    #[serde(default = "legacy_agent", skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// Reviewer phases (`review:<task>:<iter>:<angle>`), kept OUT of `phases` so
    /// they never pollute pipeline progress: `first_incomplete` and
    /// `format_progress` (main.rs) walk `phases` only, and that omission IS the
    /// isolation. Only `find_phase` (and the marker/pane-id lookups that delegate
    /// to it) consult this list. `#[serde(default)]` so pre-existing state.json
    /// files (written before this field) load with an empty list.
    #[serde(default)]
    pub review_phases: Vec<Phase>,
    pub gate: String,
    pub cursor: usize,
    /// The herdr workspace id created for this run (set by `drovr new`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    /// The workspace's auto-created root shell pane id (set by `drovr new`,
    /// which also labels it).
    ///
    /// **No agent ever runs here.** Every phase and every reviewer gets its own
    /// tab, so this stays an idle shell that anchors the workspace for the run's
    /// lifetime — which is what makes a phase's tab closeable without taking the
    /// workspace, and every other phase, with it. Once set it is never cleared;
    /// `drovr cleanup` reclaims it like any other pane drovr opened
    /// (`drovr_pane_ids` lists it first).
    ///
    /// `None` for a run whose workspace creation failed at `drovr new`, and for
    /// runs created before this field existed. A `state.json` written by an
    /// older build may instead have `None` here with the *first phase* carrying
    /// the root pane id, because that build let the first phase claim it; such a
    /// run keeps working — the id is simply an ordinary phase pane to every
    /// caller.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_pane: Option<String>,
    /// The project directory phases should run in (trusted by claude).
    /// Captured at `drovr new` time; defaults to empty string for old runs.
    #[serde(default)]
    pub project_dir: String,
    /// Absolute path of the git worktree created for this run (`.drovr/wt/<run>`),
    /// set by `drovr new --worktree`. When `Some`, `project_dir` points *into*
    /// this worktree and `cmd_cleanup` prunes it. `None` for in-place runs and any
    /// run created before worktree support existed → identical to today's behavior.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,
    /// The branch (`drovr/<run>`) the worktree checks out. Kept on cleanup so the
    /// human can merge it; deleted only under `--purge`. `None` when no worktree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_branch: Option<String>,
    /// Set by `cmd_cleanup`: the human is done with this run and its panes are
    /// gone (the workspace too, unless the human's own panes kept it alive —
    /// `close_run_panes`). Needed because nothing else reconciles a
    /// torn-down run — phase statuses are frozen at their last write and the
    /// review gate keeps whatever verdict slot it was parked in, so a cleaned-up
    /// run that never finished its phases would otherwise display as live
    /// forever. `#[serde(default)]` + skip-if-false keeps pre-existing
    /// `state.json` files loading (and re-serializing) unchanged.
    ///
    /// # `state.json` IS THE AUTHORITY FOR THIS FIELD
    ///
    /// Unique among `RunState`'s fields: `archived` is set by a *different*
    /// process from the one holding this struct — `drovr cleanup`, or the review
    /// server's Archive/Restore button, both of which re-read and then
    /// [`save`](RunState::save) — while every other field is owned by whoever
    /// loaded the run. So an in-memory `archived` is a CACHE, and it can be stale
    /// in either direction: `false` when the human has just archived, `true` when
    /// they have just restored.
    ///
    /// The rule, and it has no exceptions:
    ///
    /// * **Consulting it means refreshing it** — call
    ///   [`refresh_archived`](RunState::refresh_archived), which re-reads
    ///   `state.json`, adopts what it finds, and returns it. Reading the field
    ///   directly answers a question about your own copy, not about the run.
    /// * **Writing it means owning it** — a caller that has *decided* to archive
    ///   or restore sets the field and uses [`save`](RunState::save) /
    ///   [`save_in`](RunState::save_in), which write it verbatim. Everyone else
    ///   uses [`save_preserving_archived`](RunState::save_preserving_archived),
    ///   which takes disk's value over its own.
    ///
    /// This was not always one rule, and the gap had teeth: a guard that read disk
    /// while the save beside it merged with `|=` could refuse to repair a restored
    /// run, or silently re-archive one. Two sources of truth for one bit is enough
    /// to invert a human's decision.
    ///
    /// ## The rule is a CONVENTION — the type does not enforce it
    ///
    /// This field is `pub bool`, so nothing stops a new call site reading it
    /// directly and acting on a stale value. Said plainly rather than left to be
    /// inferred from the paragraphs above, which describe a discipline the
    /// compiler knows nothing about.
    ///
    /// Not enforced because the obvious enforcement is worse than the gap. A
    /// private field needs a constructor and rewrites at 17 struct literals across
    /// 6 files (`RunState` has no `Default`), and an accessor that reads the
    /// authority would put a `state.json` read behind every consultation —
    /// including [`is_complete`](RunState::is_complete), which the review server
    /// calls per row on a 2s poll. That trades a documented convention for a hot
    /// disk read and a wide mechanical change.
    ///
    /// **Kept by these tests**, which fail if a site stops obeying it:
    /// * `run::tests::refresh_archived_adopts_disk_in_both_directions`
    /// * `run::tests::refresh_archived_fails_loudly_rather_than_picking_an_authority`
    /// * `run::tests::a_save_never_re_archives_a_run_the_human_restored`
    /// * `run::tests::a_stale_save_never_resurrects_an_archived_run`
    /// * `run::tests::restore_can_still_clear_the_archived_flag`
    /// * `phase::tests::repairing_a_restored_run_leaves_it_restored_on_disk`
    /// * `phase::tests::a_restore_on_disk_beats_a_stale_archive_held_in_memory`
    /// * `phase::tests::an_unreadable_state_json_refuses_the_repair_rather_than_guessing`
    /// * `code_review::tests::archiving_mid_run_survives_every_save_the_review_makes`
    ///
    /// A reader added to that list is a reader that must go through
    /// [`refresh_archived`](RunState::refresh_archived) first.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub archived: bool,
    /// Panes drovr opened that no longer belong to any phase, yet are still
    /// drovr's to reap: a reviewer that was replaced in place (`code_review`'s
    /// resume drops the stale registration so the harvest cannot read the old
    /// pane's transcript) leaves its pane running.
    ///
    /// Load-bearing for cleanup, not bookkeeping trivia. `drovr cleanup` closes
    /// exactly the panes this file records and leaves everything else alone —
    /// panes in the run's workspace belong to the human unless drovr can prove
    /// otherwise. A pane dropped from `review_phases` without landing here would
    /// therefore be both immortal and mistaken for the human's, keeping the
    /// workspace open forever. `#[serde(default)]` + skip-if-empty keeps
    /// pre-existing `state.json` files loading unchanged.
    ///
    /// **The list SHRINKS as well as grows**, and both directions matter.
    /// `phase::reap_retired` closes these panes at the same triggers a phase's
    /// is reaped at, and forgets an entry once the pane behind it is provably
    /// gone: an entry that outlives its pane proves nothing, and herdr reissues
    /// pane ids. See [`RunState::reapable_retired`] for which entries a close
    /// may reach and [`RunState::forget_retired_panes`] for what authorises
    /// dropping one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub retired_panes: Vec<String>,
}

fn legacy_agent() -> Option<String> {
    Some("claude".into())
}

/// The drovr data root (`$XDG_DATA_HOME/drovr` or `~/.local/share/drovr`).
///
/// Home of the global always-on-server discovery files (`server.addr`,
/// `server.pid`) and the `runs/` directory. [`run_dir`] resolves under it.
///
/// This is the ONLY place the data root is resolved. `drovr list` used to carry
/// its own inline copy of the `XDG_DATA_HOME`-or-`$HOME` expression; two copies
/// of this is how one of them gets a guard and the other keeps deleting things.
///
/// # Fail-closed under `cfg(test)`
///
/// Under test this refuses to resolve anywhere inside `$HOME` and panics
/// instead — see [`refuse_home_data_root`]. Behaviour outside `cfg(test)` is
/// unchanged: the real CLI resolves the real data root, which is the point of
/// it.
pub fn data_dir() -> PathBuf {
    let base = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap()).join(".local/share"));
    #[cfg(test)]
    refuse_home_data_root(&base);
    base.join("drovr")
}

/// Panic if a test just resolved the data root inside the user's home
/// directory.
///
/// `cargo test` destroyed the real `~/.local/share/drovr` twice, taking ~65
/// runs across four agents with it. The mechanism was not one bad test: tests
/// redirect `XDG_DATA_HOME` by mutating it PROCESS-GLOBALLY under `ENV_LOCK`,
/// and the only thing tying a test to that lock was a doc comment. A test that
/// read the variable outside the lock — or simply ran before any test had set
/// it, with the developer's own `XDG_DATA_HOME=$HOME/.local/share` still in the
/// environment — resolved the LIVE root and *silently succeeded*. The silent
/// success is the bug. This makes that case stop.
///
/// The rule is one comparison, deliberately: **the resolved base must not lie
/// inside `$HOME`**. Not "does this look like a real path" — there is no
/// guessing here, and there is no second heuristic backstop next to it. A test
/// data root belongs in a temp dir; anything under the user's home is a test
/// that got it wrong, whether it arrived via the `$HOME/.local/share` fallback
/// or an `XDG_DATA_HOME` aimed straight at the live root.
///
/// Both sides are canonicalised where they exist, so a symlinked `$HOME` (or a
/// `..`-laden `XDG_DATA_HOME`) does not walk around the check.
///
/// # Where this guard is weaker than it looks
///
/// * It is `cfg(test)` only, so it covers the unit tests compiled into the
///   `drovr` bin. The integration tests under `cli/tests/` drive the *built
///   binary*, which is compiled WITHOUT `cfg(test)` and therefore unguarded;
///   they must keep pinning `XDG_DATA_HOME` on the child themselves.
/// * A path that reaches `$HOME` only through a symlink whose own parent does
///   not exist yet cannot be canonicalised, so it is compared literally.
/// * `$HOME` unset leaves nothing to protect and the check passes.
#[cfg(test)]
fn refuse_home_data_root(base: &std::path::Path) {
    let home = match std::env::var("HOME") {
        Ok(h) if !h.is_empty() => PathBuf::from(h),
        _ => return,
    };
    let real_home = fs::canonicalize(&home).unwrap_or(home);
    let real_base = fs::canonicalize(base).unwrap_or_else(|_| base.to_path_buf());
    if real_base.starts_with(&real_home) {
        panic!(
            "drovr test guard: data_dir() resolved to {}/drovr, inside the real home \
             directory {} — this is the LIVE drovr data root, and a test writing there \
             is how ~/.local/share/drovr got destroyed. XDG_DATA_HOME was {:?}. Set it \
             to a temp dir (holding ENV_LOCK) before anything that calls \
             data_dir()/run_dir()/runs_dir(), or pass an explicit root to the *_in() \
             variant.",
            real_base.display(),
            real_home.display(),
            std::env::var("XDG_DATA_HOME").ok(),
        );
    }
}

pub fn run_dir(name: &str) -> PathBuf {
    data_dir().join("runs").join(name)
}

/// The directory holding every run (`<data_dir>/runs`). May not exist yet.
pub fn runs_dir() -> PathBuf {
    data_dir().join("runs")
}

/// Enumerate run names: the immediate subdirectories of `root` that hold a
/// `state.json`. Returned unsorted; callers sort as they see fit. A missing
/// `root` yields an empty list (not an error) — a fresh install has no runs.
/// The always-on server passes its configured runs root (injectable in tests);
/// the global convenience is `list_runs_in(&runs_dir())`. Entries whose name is
/// not valid UTF-8 are skipped.
pub fn list_runs_in(root: &std::path::Path) -> Vec<String> {
    let mut out = Vec::new();
    let entries = match fs::read_dir(root) {
        Ok(e) => e,
        Err(_) => return out,
    };
    for entry in entries.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let name = match entry.file_name().into_string() {
            Ok(n) => n,
            Err(_) => continue,
        };
        if entry.path().join("state.json").is_file() {
            out.push(name);
        }
    }
    out
}

impl RunState {
    pub fn load(name: &str) -> io::Result<RunState> {
        let p = run_dir(name).join("state.json");
        let state: RunState =
            serde_json::from_str(&fs::read_to_string(p)?).map_err(io::Error::other)?;
        state.check_pane_lifecycle()?;
        Ok(state)
    }

    /// Refuse a `state.json` that claims a phase is both reaped and holding a
    /// live pane.
    ///
    /// [`Phase::mark_reaped`] makes that state unreachable through the API, and
    /// [`Reaped`]'s private inner makes it unconstructable outside this module —
    /// but `Deserialize` is a third constructor, reachable by anyone who can
    /// write the file. This closes it, the same way [`PassToken`] and
    /// [`crate::herdr::SessionId`] close theirs.
    ///
    /// **Scope, precisely: this guards `RunState::load`, not every deserialize.**
    /// `cmd_list` reads run states directly with `serde_json::from_str(..).ok()`
    /// to render `drovr list`, and deliberately keeps doing so — it only
    /// displays, never acts, and a run whose state is inconsistent is one a
    /// human most wants to SEE listed rather than have silently vanish. Every
    /// path that acts on a run goes through here.
    ///
    /// Failing the load is the right severity: it exits 1 and STOPs the run,
    /// which is loud — and the alternative is a phase whose pane `drovr attach`
    /// offers but reaping has already closed. Nothing drovr writes can produce
    /// this, so the only way to see it is a corrupted or hand-edited file, where
    /// stopping is exactly what a human wants.
    fn check_pane_lifecycle(&self) -> io::Result<()> {
        for p in self.phases.iter().chain(self.review_phases.iter()) {
            if p.is_reaped() && p.pane_id().is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "run '{}': phase '{}' is recorded as reaped but still holds pane {:?}. \
                         drovr never writes that — reaping drops the pane id in the same step \
                         it sets the flag — so this state.json has been corrupted or edited by \
                         hand. Refusing to load rather than offer a pane that is gone.",
                        self.name,
                        p.name,
                        p.pane_id().unwrap_or_default()
                    ),
                ));
            }
        }
        Ok(())
    }
    /// Write `state.json` ATOMICALLY: serialize into a temp file in the same
    /// directory, then `fs::rename` it over the target. A bare `fs::write`
    /// truncates in place, so any concurrent READER (the always-on review server
    /// reading run state, a second CLI invocation, a `phase wait` loop) can parse
    /// a half-written file — and a failed `load_run` exits 1, which per the
    /// pipeline skill STOPs the whole run. A same-directory rename is atomic on
    /// POSIX: a reader sees either the old file or the new one, never a splice.
    ///
    /// The temp name carries pid + a process-local counter rather than being a
    /// fixed `state.json.tmp`: two concurrent savers sharing one temp path would
    /// interleave their writes into the same inode and rename that corruption
    /// into place, which is precisely the failure this is meant to remove.
    ///
    /// Scope, precisely — this fixes torn READS and nothing else:
    /// * NOT durability. The temp file is not `fsync`ed and neither is the
    ///   directory, so a power loss can still leave a stale (or, on some
    ///   filesystems, empty) `state.json`. Deliberate: `save` runs in poll loops
    ///   and an fsync per save is not worth it for a workflow whose herdr
    ///   workspace does not survive a crash either.
    /// * NOT serialized updates. Every writer still does load→mutate→save on its
    ///   own copy, so two concurrent CLI invocations cleanly clobber each other
    ///   whole-file. Losing an update is the pre-existing behavior; this only
    ///   guarantees the loser's file is never *corrupt*.
    /// * A SIGKILL between the write and the rename orphans a
    ///   `.state.json.tmp.<pid>.<n>`; nothing sweeps it. Cosmetic — every
    ///   `read_dir` over a run root gates on a `state.json` child, and nothing
    ///   enumerates inside a run dir.
    pub fn save(&self) -> io::Result<()> {
        self.save_in(&run_dir(&self.name))
    }
    /// Save into an explicitly given run directory.
    ///
    /// The always-on server is parameterised on a `runs_root` (a temp dir under
    /// test), so its writers must not go through [`RunState::save`]: that
    /// resolves `run_dir()` from the ambient `XDG_DATA_HOME` and would write to
    /// the developer's real data dir instead of the root the server was handed.
    ///
    /// The atomicity described on `save` lives HERE, so it applies to the
    /// server's writes too — they are the ones a reader is most likely to race,
    /// being a button a human can hit mid-phase.
    pub fn save_in(&self, dir: &std::path::Path) -> io::Result<()> {
        use std::sync::atomic::{AtomicU64, Ordering};

        // Refuse to WRITE what `load` would refuse to READ.
        //
        // Asymmetric validation is worse than none: a `save` that happily wrote
        // a lifecycle contradiction would strand the run outright, because every
        // later `load_run` exits 1 and the pipeline STOPs — with no way back
        // except hand-editing the file drovr itself produced. Checking the same
        // invariant here means the failure surfaces at the write, on the caller
        // that caused it, while the last good `state.json` is still on disk.
        self.check_pane_lifecycle()?;
        static SEQ: AtomicU64 = AtomicU64::new(0);

        fs::create_dir_all(dir)?;
        let body = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        let tmp = dir.join(format!(
            ".state.json.tmp.{}.{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        // Best-effort cleanup of the temp file on any failure, so a transient
        // ENOSPC/EACCES can't litter the run dir with orphaned partials.
        if let Err(e) = fs::write(&tmp, &body) {
            let _ = fs::remove_file(&tmp);
            return Err(e);
        }
        if let Err(e) = fs::rename(&tmp, dir.join("state.json")) {
            let _ = fs::remove_file(&tmp);
            return Err(e);
        }
        Ok(())
    }
    /// Re-read `archived` from `state.json`, adopt it, and return it.
    ///
    /// THE way to consult [`archived`](RunState::archived) — see that field for
    /// why disk is the authority. Adopting rather than merely returning is the
    /// load-bearing half: it leaves this copy agreeing with disk, so the next
    /// [`save_preserving_archived`] cannot write a stale value back and quietly
    /// invert the human's decision.
    ///
    /// A read failure is an `Err`, NOT a fallback to the copy in hand. Callers
    /// gate destructive or infrastructure-creating work on this, and a torn read
    /// or a permissions problem must fail closed rather than silently decide which
    /// authority applies. The one exception is a `state.json` that is not there:
    /// nothing has ever been recorded about the run, so there is no decision to
    /// contradict, and the value in hand stands.
    ///
    /// [`save_preserving_archived`]: RunState::save_preserving_archived
    pub fn refresh_archived(&mut self) -> io::Result<bool> {
        match RunState::load(&self.name) {
            Ok(disk) => self.archived = disk.archived,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        Ok(self.archived)
    }

    /// Save every field from this copy EXCEPT [`archived`](RunState::archived),
    /// which is taken from disk — the authority rule that field documents, applied
    /// at the moment of writing.
    ///
    /// Long-running commands hold a `RunState` loaded minutes ago — `code-review
    /// run` blocks for up to its timeout (30 min by default) before writing back —
    /// and the human can archive OR restore from the web UI inside that window.
    /// A plain [`save`] would write whichever value this copy happens to hold over
    /// their decision, in whichever direction: reviving a run they filed away, or
    /// re-filing one they just restored.
    ///
    /// Adopts disk's value (`=`) rather than merging it (`|=`). The merge was a
    /// half-rule — it rescued an Archive and lost a Restore — and a half-rule is
    /// how one bit ends up with two sources of truth. There is no writer this
    /// costs: a caller that has *decided* to archive or restore owns the field and
    /// uses [`save`]/[`save_in`], which write it verbatim.
    ///
    /// This narrows the race; it does not close it. A concurrent write landing
    /// between the re-read and the write is still lost (see docs/known-issues.md
    /// — `state.json` has no locking or compare-and-swap). An unreadable
    /// `state.json` leaves the value in hand and proceeds, because this is a WRITE
    /// path: refusing to persist a phase's progress over an unrelated read failure
    /// would lose more than it protects. Callers that must not act on a stale flag
    /// gate on [`refresh_archived`](RunState::refresh_archived) first, which does
    /// fail closed.
    ///
    /// [`save`]: RunState::save
    /// [`save_in`]: RunState::save_in
    pub fn save_preserving_archived(&mut self) -> io::Result<()> {
        if let Ok(disk) = RunState::load(&self.name) {
            self.archived = disk.archived;
        }
        self.save()
    }
    pub fn first_incomplete(&self) -> Option<usize> {
        self.phases
            .iter()
            .position(|p| p.status != PhaseStatus::Done)
    }
    /// Whether this run is finished and can be filed away: either every phase
    /// reached `Done`, or the human archived it via `drovr cleanup`.
    ///
    /// The `phases` emptiness guard is load-bearing, not defensive noise. Callers
    /// that recover from an unreadable `state.json` with a default `RunState` hold
    /// zero phases, and `first_incomplete()` over zero phases is vacuously `None`
    /// — so without this check the runs whose state we *failed to read* would be
    /// the ones reported complete and hidden from view.
    pub fn is_complete(&self) -> bool {
        self.archived || (!self.phases.is_empty() && self.first_incomplete().is_none())
    }
    /// `(phases done, total phases)` — pipeline progress for display. Counts
    /// `phases` only, never `review_phases` (see that field's note).
    pub fn progress(&self) -> (usize, usize) {
        let done = self
            .phases
            .iter()
            .filter(|p| p.status == PhaseStatus::Done)
            .count();
        (done, self.phases.len())
    }
    /// Remember `pane_id` as drovr's even though no phase points at it any more —
    /// see [`RunState::retired_panes`]. Idempotent, so a caller may retire the
    /// same pane twice without growing the list.
    /// The pane that represents this run right now: `(phase name, pane id)` for
    /// the phase the run is currently on, if it holds one.
    ///
    /// **The single answer for every caller that asks "where is this run?"** —
    /// `drovr attach` and the review UI's live mirror both call this. They used
    /// to each walk `phases` themselves, with *different* predicates, so the two
    /// could point at different panes for the same run; and two copies of a rule
    /// this subtle drift apart.
    ///
    /// **There is no fallback to an earlier phase, deliberately.** A run whose
    /// current phase holds no pane is honestly empty, and saying so beats
    /// offering a pane from a phase the run has already moved past: that pane
    /// makes a stalled run look alive at the wrong place. It gets worse once
    /// finished phases are reaped, because earlier phases are exactly the ones
    /// reaping closes — the fallback would hand back a pane id that is dead, or
    /// worse, recycled by herdr for something else entirely.
    ///
    /// **The rule is "the first NOT-`Done` phase that holds a pane".** Two
    /// halves, and each is doing work:
    ///
    /// * *not `Done`* — never `status == Running`. A phase that failed still
    ///   holds the pane worth looking at, and that is precisely when a human
    ///   attaches. It is also what makes the no-fallback rule safe against
    ///   reaping: reaping only ever closes `Done` phases, so a pane this returns
    ///   is by construction not one that reaping took.
    /// * *that holds a pane* — the search skips FORWARD over phases with no
    ///   pane, so a run whose earlier phases were never started still finds the
    ///   agent actually running later in the list. It never walks BACKWARD.
    ///
    /// The forward skip is not hypothetical: `phase_start` appends any phase
    /// name it is given, so a driver that starts `implement-task-1` without
    /// starting `plan` leaves a `Pending` phase sitting in front of a live one.
    /// Stopping at the first incomplete phase would report that run as empty
    /// while its agent is right there.
    pub fn live_agent_pane(&self) -> Option<(&str, &str)> {
        self.phases
            .iter()
            .filter(|p| p.status != PhaseStatus::Done)
            .find_map(|p| Some((p.name.as_str(), p.pane_id()?)))
    }

    pub fn retire_pane(&mut self, pane_id: impl Into<String>) {
        let id = pane_id.into();
        if !self.retired_panes.contains(&id) {
            self.retired_panes.push(id);
        }
    }

    /// Drop `panes` from [`RunState::retired_panes`] — the inverse of
    /// [`RunState::retire_pane`], for panes that no longer EXIST.
    ///
    /// A retirement is a claim: "this pane is drovr's, close it at `drovr
    /// cleanup`". Once the pane is provably gone the claim has no subject left,
    /// and keeping it is not merely tidy-vs-untidy — herdr hands pane ids back
    /// out, so a permanent entry naming a dead pane is a standing claim on
    /// whatever pane wears that id next. Forgetting it is the same reasoning
    /// that makes [`Phase::mark_reaped`] clear `pane_id` rather than leave a
    /// registration pointing at nothing.
    ///
    /// Establishing that a pane is gone is not something `RunState` can do —
    /// only herdr can, and only the sweep in `phase::reap_retired` asks it. This
    /// is the write-down of that answer, not the judgement.
    pub fn forget_retired_panes(&mut self, panes: &[String]) {
        self.retired_panes.retain(|p| !panes.contains(p));
    }

    /// ⭐ **THE sweep precondition: which retired panes a reap may close.**
    ///
    /// The counterpart of [`RunState::reapable`] for the panes no phase holds
    /// any more. Membership in [`RunState::retired_panes`] is already most of
    /// the permission — that list exists precisely to record "drovr made this
    /// pane" after the phase that held it let go, and it is what `drovr cleanup`
    /// acts on under main's `8173f03` (never close what you cannot prove is
    /// yours). So closing one can never take a human's pane.
    ///
    /// Two entries are excluded, and they are the same two exclusions the
    /// phase-level predicate makes, for the same reasons:
    ///
    /// * **the workspace's root shell.** herdr destroys a workspace when its
    ///   last pane closes, so this one takes the run and every other phase in it.
    ///   A retirement really can name it: a `state.json` from the build where the
    ///   first phase claimed the root pane is retired by any release or surrender
    ///   of that phase, which is the same provenance [`RunState::reapable`]'s
    ///   [`NotReapable::RootShell`] arm documents.
    /// * **a pane some phase still records.** The retirement and the
    ///   registration disagree, and the registration is the one with a live
    ///   phase behind it: closing that pane would leave the phase holding a
    ///   pane that is gone, which is exactly the stuck `HoldsPane` this branch
    ///   spent a task learning to repair. It is also the guard against herdr
    ///   reissuing a closed pane's id — a stale retirement must not authorise
    ///   closing a pane that now belongs to something else.
    ///
    /// Returns owned ids, not borrows: every caller closes panes and then writes
    /// the run back, and a borrow of `self` would outlive the decision.
    pub fn reapable_retired(&self) -> Vec<String> {
        let held: Vec<&str> = self
            .root_pane
            .iter()
            .map(String::as_str)
            .chain(self.phases.iter().filter_map(|p| p.pane_id()))
            .chain(self.review_phases.iter().filter_map(|p| p.pane_id()))
            .collect();
        self.retired_panes
            .iter()
            .filter(|p| !held.contains(&p.as_str()))
            .cloned()
            .collect()
    }

    /// ⭐ **THE rehydrate precondition, in ONE place.**
    ///
    /// Three callers ask this exact question and they must not answer it
    /// differently: `phase_rehydrate` (which refuses), the `POST /rehydrate`
    /// handler (which must refuse the same things, or the button a human clicks
    /// is more permissive than the command it shells out to), and the agent tree
    /// (whose ⟳ must appear only where a click will work).
    ///
    /// It lives on `RunState`, not on `Phase`, because half of what a rehydrate
    /// needs is not a property of the phase at all: the run has to record a
    /// `project_dir` to launch in and a herdr workspace to open a tab in, and a
    /// reviewer is a reviewer only by virtue of living in `review_phases`. Those
    /// three were enforced inside `phase_rehydrate` and nowhere else, so the ⟳
    /// rendered on phases that refused the moment they were clicked — the same
    /// stronger-operation-than-predicate defect that had already been fixed one
    /// level down.
    ///
    /// Order: identity, then category, then the phase, then the run. The most
    /// specific true statement wins, because the arm IS the message.
    pub fn rehydratable(&self, name: &str) -> Result<(), NotRehydratable> {
        let Some(phase) = self.find_phase(name) else {
            return Err(NotRehydratable::NoSuchPhase);
        };
        // A reviewer can be brought back, but not brought back USABLE — see
        // `NotRehydratable::Reviewer`. Categorical, so it is answered before the
        // per-phase state: "attach to its pane instead" would be advice toward a
        // recovery that does not exist.
        //
        // Asked of the NAME, not of `review_phases` membership: a
        // reviewer-shaped name in `phases` is still a reviewer, and asking the
        // list let exactly that impostor through — see
        // [`is_reviewer_phase_name`].
        if is_reviewer_phase_name(name) {
            return Err(NotRehydratable::Reviewer);
        }
        phase.phase_level_rehydratable()?;
        if self.project_dir.is_empty() {
            return Err(NotRehydratable::NoProjectDir);
        }
        if self.workspace.is_none() {
            return Err(NotRehydratable::NoWorkspace);
        }
        Ok(())
    }
    /// ⭐ **THE reap precondition, in ONE place** — and the pane a reap would
    /// close, so a caller cannot ask the question and then act on a different
    /// answer.
    ///
    /// Two callers, deliberately sharing it: `phase::phase_reap` (which does it)
    /// and [`RunState::superseded_by`] (which decides which phases get it done
    /// to them). Written as separate checks, the automatic trigger would be free
    /// to hand `phase_reap` a phase it refuses — and the root-shell arm is not a
    /// refusal anyone wants discovered at the herdr call.
    ///
    /// It is deliberately NOT the mirror of [`RunState::rehydratable`]. That one
    /// is a precondition for CREATING an agent, so it is strict about whether
    /// there is anything to bring back; this one is a precondition for
    /// DESTROYING a pane, and the only thing that makes a pane un-closeable is
    /// that closing it would take the run's workspace with it. In particular a
    /// reviewer is reapable (a delivered verdict is a file, not a pane) and a
    /// phase of any status is reapable — `drovr phase reap` is also the
    /// supported way to clear a registration whose pane herdr has lost, and that
    /// phase may be `Running`. WHICH phases the automatic trigger picks is
    /// `superseded_by`'s rule, not this one's.
    pub fn reapable(&self, name: &str) -> Result<&str, NotReapable> {
        let phase = self.find_phase(name).ok_or(NotReapable::NoSuchPhase)?;
        let pane = phase.pane_id().ok_or(NotReapable::NoPane)?;
        // Identity, not "is it in the same tab". Reaping closes the PANE it is
        // given (see `phase::phase_reap` for why pane granularity rather than
        // tab), so the only way to hurt the root shell is to be handed its id.
        if self.root_pane.as_deref() == Some(pane) {
            return Err(NotReapable::RootShell(pane.to_owned()));
        }
        Ok(pane)
    }

    /// ⭐ **The phases a launch of `starting` supersedes**: every OTHER phase
    /// that is `Done` and still holds a reapable pane, in `phases` order.
    ///
    /// This is the whole rule for the automatic trigger, stated once where it
    /// can be tested — rather than as a filter written inline above a
    /// `phase_reap` call with a comment explaining why the conditions are there.
    /// Each of the three is load-bearing:
    ///
    /// * **`Done`** — and never `Failed` or `Running`. A `Failed` phase's pane
    ///   is exactly what a human attaches to in order to find out what went
    ///   wrong, and a `Running` one has an agent in it. It is also what makes
    ///   `RunState::live_agent_pane`'s no-fallback rule safe: that search skips
    ///   `Done` phases, so a pane it returns is by construction not one reaping
    ///   took.
    /// * **OTHER than `starting`** — a phase re-entered by `phase_start` is
    ///   `Done` on disk at the moment the new pass is persisted, and reaping it
    ///   would close the pane the launch is about to use (or has just used).
    /// * **holds a pane** — via [`RunState::reapable`], so the root shell and
    ///   the already-reaped are both excluded by the same predicate the reap
    ///   itself asks.
    ///
    /// **Only `phases`, never `review_phases`**, and that is a scope decision
    /// rather than an oversight: a reviewer's pane belongs to a panel that may
    /// still be in flight (a timed-out `code-review run` is resumed, and its
    /// reviewers are waited on again), and `code_review_run` is the only thing
    /// that knows whether that is so. It reaps its own panel, after the merge.
    pub fn superseded_by(&self, starting: &str) -> Vec<String> {
        self.phases
            .iter()
            .filter(|p| p.name != starting && p.status == PhaseStatus::Done)
            .filter(|p| self.reapable(&p.name).is_ok())
            .map(|p| p.name.clone())
            .collect()
    }

    /// Look up a phase by name across BOTH `phases` and `review_phases`. Reviewer
    /// lookups (marker-drop, seed injection) need to resolve names living in
    /// `review_phases`; pipeline progress deliberately does NOT use this (it stays
    /// bound to `phases` only — see `first_incomplete`). Searches `phases` first,
    /// then `review_phases`.
    pub fn find_phase(&self, name: &str) -> Option<&Phase> {
        self.phases
            .iter()
            .chain(self.review_phases.iter())
            .find(|p| p.name == name)
    }
    /// [`RunState::find_phase`], mutably — same lists, same order, same caveat.
    /// Session capture needs it because a reviewer's pane is polled through the
    /// same readiness gate as a pipeline phase's, and a reviewer lives only in
    /// `review_phases`.
    ///
    /// `phases` is searched first, and `phase::require_name_unclaimed` refuses a
    /// name the other list already answers to, so this and `find_phase` cannot
    /// resolve to different entries.
    pub fn find_phase_mut(&mut self, name: &str) -> Option<&mut Phase> {
        self.phases
            .iter_mut()
            .chain(self.review_phases.iter_mut())
            .find(|p| p.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::ENV_LOCK;

    // A RunState with the given phases; other fields inert. `archived` defaults
    // off so each test opts in explicitly.
    fn completion_run(phases: Vec<Phase>) -> RunState {
        RunState {
            name: "r".into(),
            task: "t".into(),
            agent: None,
            phases,
            review_phases: vec![],
            gate: "spec".into(),
            cursor: 0,
            workspace: None,
            root_pane: None,
            project_dir: "/tmp/p".into(),
            worktree_path: None,
            worktree_branch: None,
            archived: false,
            retired_panes: vec![],
        }
    }

    fn done(name: &str) -> Phase {
        Phase {
            name: name.into(),
            status: PhaseStatus::Done,
            ..Default::default()
        }
    }

    fn running(name: &str) -> Phase {
        let mut p = Phase::new(name);
        p.status = PhaseStatus::Running;
        p.set_pane("w:p1");
        p
    }

    /// A phase in exactly the state reaping leaves behind: it ran, an agent was
    /// recorded, its pane is gone.
    fn reaped(name: &str) -> Phase {
        let mut p = Phase::new(name);
        p.status = PhaseStatus::Done;
        p.set_pane("w:p9");
        p.record_launch("claude", None);
        p.mark_reaped();
        p
    }

    /// `completion_run`, plus the two run-level things a rehydrate needs.
    fn rehydrate_run(phases: Vec<Phase>, review_phases: Vec<Phase>) -> RunState {
        let mut r = completion_run(phases);
        r.review_phases = review_phases;
        r.workspace = Some("ws".into());
        r
    }

    #[test]
    fn the_rehydrate_gate_asks_the_run_and_not_only_the_phase() {
        // The predicate the UI uses to SHOW the ⟳ was weaker than the one
        // `phase_rehydrate` enforces: the phase-level checks lived on `Phase`,
        // while "the run records a project_dir" and "the run has a herdr
        // workspace" were enforced inside the operation only. So the ⟳ rendered
        // on phases that would refuse the moment they were clicked. One
        // predicate, asked by all three callers.
        let ok = rehydrate_run(vec![reaped("plan")], vec![]);
        assert_eq!(ok.rehydratable("plan"), Ok(()));

        let mut no_dir = rehydrate_run(vec![reaped("plan")], vec![]);
        no_dir.project_dir = String::new();
        assert_eq!(
            no_dir.rehydratable("plan"),
            Err(NotRehydratable::NoProjectDir)
        );

        let mut no_ws = rehydrate_run(vec![reaped("plan")], vec![]);
        no_ws.workspace = None;
        assert_eq!(no_ws.rehydratable("plan"), Err(NotRehydratable::NoWorkspace));

        // And a name this run does not answer to is its own refusal, so the
        // HTTP layer can map it to 404 without a second lookup.
        assert_eq!(
            ok.rehydratable("nope"),
            Err(NotRehydratable::NoSuchPhase)
        );

        // The phase-level refusals still hold, through the same door.
        let live = rehydrate_run(vec![running("plan")], vec![]);
        assert_eq!(
            live.rehydratable("plan"),
            Err(NotRehydratable::HoldsPane("w:p1".into()))
        );
        let placeholder = rehydrate_run(vec![Phase::new("plan")], vec![]);
        assert_eq!(
            placeholder.rehydratable("plan"),
            Err(NotRehydratable::NeverStarted)
        );
    }

    /// The reap precondition answers with the PANE, so a caller cannot ask the
    /// question and then close something else — and its refusals are the two
    /// that are not "no".
    #[test]
    fn the_reap_gate_hands_back_the_pane_it_cleared() {
        let mut run = completion_run(vec![running("plan")]);
        assert_eq!(run.reapable("plan"), Ok("w:p1"));

        // Already reaped, never launched, or its pane died with its workspace:
        // one answer, and it is the ordinary "nothing to do" that makes a
        // second reap of one phase a no-op rather than an error.
        run.phases.push(reaped("done-already"));
        run.phases.push(Phase::new("placeholder"));
        assert_eq!(run.reapable("done-already"), Err(NotReapable::NoPane));
        assert_eq!(run.reapable("placeholder"), Err(NotReapable::NoPane));
        assert_eq!(run.reapable("nope"), Err(NotReapable::NoSuchPhase));

        // ⚠️ The one real refusal. herdr destroys a workspace when its last pane
        // closes, so reaping the root shell takes the workspace and every other
        // phase in it. No phase this build launches can hold that id; a
        // `state.json` from the build where the first phase claimed it can.
        let mut legacy = completion_run(vec![running("plan")]);
        legacy.root_pane = Some("w:p1".into());
        assert_eq!(
            legacy.reapable("plan"),
            Err(NotReapable::RootShell("w:p1".into())),
            "the pane that anchors the workspace is never reapable"
        );

        // A reviewer IS reapable — its verdict is a file, not a pane. That is
        // the deliberate asymmetry with `rehydratable`, which refuses one.
        let rev = rehydrate_run(vec![], vec![running("review:t:1:correctness")]);
        assert_eq!(rev.reapable("review:t:1:correctness"), Ok("w:p1"));
    }

    /// The automatic trigger's whole rule, in the one place it is written.
    #[test]
    fn a_launch_supersedes_every_other_finished_phase_that_still_holds_a_pane() {
        let mut run = completion_run(vec![]);
        let with_pane = |name: &str, status: PhaseStatus, pane: &str| {
            let mut p = Phase::new(name);
            p.status = status;
            p.set_pane(pane);
            p
        };
        run.phases.push(with_pane("brainstorm", PhaseStatus::Done, "w:p1"));
        // Failed keeps its pane: that pane is exactly what a human attaches to
        // in order to find out what went wrong.
        run.phases.push(with_pane("plan", PhaseStatus::Failed, "w:p2"));
        // Running keeps its pane for the obvious reason.
        run.phases.push(with_pane("implement", PhaseStatus::Running, "w:p3"));
        // Done but already reaped — nothing left to take.
        run.phases.push(reaped("review"));
        // Done with a pane, and it is the one being re-entered.
        run.phases.push(with_pane("verify", PhaseStatus::Done, "w:p5"));

        assert_eq!(
            run.superseded_by("verify"),
            vec!["brainstorm".to_string()],
            "only OTHER Done phases that still hold a reapable pane"
        );
        assert_eq!(
            run.superseded_by("implement"),
            vec!["brainstorm".to_string(), "verify".to_string()],
            "starting a third phase supersedes both finished ones"
        );

        // A phase re-entered by `phase_start` is `Done` on disk at the moment
        // its new pass is persisted — excluding it is what stops the launch
        // closing the pane it is about to use.
        assert!(
            !run.superseded_by("brainstorm").contains(&"brainstorm".to_string()),
            "a launch never supersedes itself"
        );

        // And the root shell is excluded through the same predicate the reap
        // itself asks, rather than by a second rule written here.
        run.root_pane = Some("w:p1".into());
        assert!(
            run.superseded_by("implement").is_empty()
                || !run.superseded_by("implement").contains(&"brainstorm".to_string()),
            "a phase holding the root pane is never superseded into a close"
        );

        // Reviewers are `code_review_run`'s to reap, not a launch's.
        let mut with_reviewer = completion_run(vec![with_pane("plan", PhaseStatus::Running, "w:p9")]);
        with_reviewer
            .review_phases
            .push(with_pane("review:t:1:correctness", PhaseStatus::Done, "w:r1"));
        assert!(
            with_reviewer.superseded_by("plan").is_empty(),
            "a pipeline launch never reaps a panel that may still be in flight"
        );
    }

    /// The sweep's whole rule, in the one place it is written — the retirement
    /// list minus the two entries a close would hurt.
    #[test]
    fn the_sweep_gate_offers_every_retirement_nothing_else_points_at() {
        let with_pane = |name: &str, status: PhaseStatus, pane: &str| {
            let mut p = Phase::new(name);
            p.status = status;
            p.set_pane(pane);
            p
        };
        let mut run = completion_run(vec![]);
        assert!(
            run.reapable_retired().is_empty(),
            "nothing retired, nothing to sweep"
        );

        // The ordinary case: a pane drovr made, that no phase points at any
        // more. `retired_panes` IS the proof it is drovr's, so membership is
        // most of the permission.
        run.retire_pane("w:p9");
        assert_eq!(run.reapable_retired(), vec!["w:p9".to_string()]);

        // ⚠️ Never the root shell. herdr destroys a workspace when its last
        // pane closes; only a `state.json` from the build where the first phase
        // claimed the root pane can retire this one.
        run.retire_pane("w:root");
        run.root_pane = Some("w:root".into());
        assert_eq!(
            run.reapable_retired(),
            vec!["w:p9".to_string()],
            "the pane that anchors the workspace is never swept"
        );

        // ⚠️ Never a pane a phase still records. The two disagree and the
        // registration wins: closing it would leave that phase holding a pane
        // that is gone. It is also what stops a stale retirement authorising
        // the close of a pane herdr has since reissued.
        run.retire_pane("w:p1");
        run.phases.push(with_pane("plan", PhaseStatus::Running, "w:p1"));
        run.retire_pane("w:r1");
        run.review_phases
            .push(with_pane("review:t:1:security", PhaseStatus::Done, "w:r1"));
        assert_eq!(
            run.reapable_retired(),
            vec!["w:p9".to_string()],
            "a registration outranks a retirement, in `phases` and `review_phases` alike"
        );

        // And forgetting is the write-down of an answer, not a judgement: it
        // removes exactly what it is given.
        run.forget_retired_panes(&["w:p9".to_string(), "w:nonexistent".to_string()]);
        assert_eq!(
            run.retired_panes,
            vec!["w:root".to_string(), "w:p1".to_string(), "w:r1".to_string()]
        );
    }

    #[test]
    fn a_reviewer_is_not_rehydratable_because_its_findings_channel_cannot_come_back() {
        // A reviewer delivers through drovr's findings MCP server, which is
        // written per (task, iteration) and handed over on the command line at
        // launch. `Config::resume_launch` passes no `mcp_config`, so a resumed
        // reviewer would have no `submit_findings` tool at all — and
        // `delivered_review` would then wait on a file that can never appear.
        // Rather than a ⟳ that brings back an agent which cannot do its one
        // job, the type refuses.
        let run = rehydrate_run(vec![reaped("implement-task-1")], vec![reaped(
            "review:task-1:1:security",
        )]);
        assert_eq!(run.rehydratable("implement-task-1"), Ok(()));
        assert_eq!(
            run.rehydratable("review:task-1:1:security"),
            Err(NotRehydratable::Reviewer)
        );
    }

    #[test]
    fn is_complete_only_when_every_phase_is_done() {
        let s = completion_run(vec![done("brainstorm"), done("plan")]);
        assert!(s.is_complete(), "all phases Done → complete");

        let s = completion_run(vec![done("brainstorm"), running("plan")]);
        assert!(!s.is_complete(), "a Running phase means still in flight");
    }

    #[test]
    fn is_complete_is_false_for_a_run_with_no_phases() {
        // Guard against the `unwrap_or_default()` trap on the server's list path:
        // a missing or garbled state.json yields an empty RunState, and
        // `first_incomplete()` on zero phases is vacuously None. Reporting that as
        // "complete" would hide precisely the runs whose state we failed to read.
        let s = completion_run(vec![]);
        assert!(!s.is_complete(), "no phases is unknown, not complete");
    }

    #[test]
    fn archived_forces_complete_even_mid_flight() {
        // `drovr cleanup` tore the run's panes down; the phase statuses are frozen
        // mid-run and no longer reflect anything live (see cmd_cleanup).
        let mut s = completion_run(vec![done("brainstorm"), running("plan")]);
        assert!(!s.is_complete());
        s.archived = true;
        assert!(s.is_complete(), "an archived run is done regardless of phases");
    }

    #[test]
    fn archived_defaults_false_when_absent_from_state_json() {
        // Every run written before this field existed must keep showing as active.
        let json = r#"{
            "name": "legacy", "task": "t", "phases": [], "gate": "spec",
            "cursor": 0, "project_dir": "/tmp/p"
        }"#;
        let s: RunState = serde_json::from_str(json).expect("legacy state.json must load");
        assert!(!s.archived, "legacy runs default to not-archived");
    }

    #[test]
    fn archived_survives_a_save_load_round_trip() {
        let mut s = completion_run(vec![done("brainstorm")]);
        s.archived = true;
        let round: RunState =
            serde_json::from_str(&serde_json::to_string(&s).unwrap()).expect("round trip");
        assert!(round.archived);
    }

    #[test]
    fn a_stale_save_never_resurrects_an_archived_run() {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        unsafe {
            std::env::set_var("XDG_DATA_HOME", tmp.path().to_str().unwrap());
        }

        // A long-running command (`code-review run` blocks for up to its timeout)
        // loads the run while it is still active...
        let mut stale = completion_run(vec![running("implement")]);
        stale.save().unwrap();

        // ...the human archives it from the web UI meanwhile, which closes the
        // workspace and destroys every pane...
        let mut archiver = RunState::load("r").unwrap();
        archiver.archived = true;
        archiver.save().unwrap();

        // ...and only then does the long-running command write its copy back.
        stale.phases[0].status = PhaseStatus::Done;
        stale.save_preserving_archived().unwrap();

        let on_disk = RunState::load("r").unwrap();
        assert!(
            on_disk.archived,
            "a save carrying a stale `archived: false` must not un-archive a run \
             whose workspace has already been destroyed"
        );
        assert_eq!(
            on_disk.phases[0].status,
            PhaseStatus::Done,
            "the writer's own progress must still land — only `archived` is rescued"
        );
    }

    /// The authority rule, at the type that owns it: consulting `archived` means
    /// re-reading it, and what is on disk is what the run is.
    #[test]
    fn refresh_archived_adopts_disk_in_both_directions() {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        unsafe {
            std::env::set_var("XDG_DATA_HOME", tmp.path().to_str().unwrap());
        }
        let mut s = completion_run(vec![running("implement")]);
        s.save().unwrap();

        // The human archives from the web UI while we hold our copy.
        let mut ui = RunState::load("r").unwrap();
        ui.archived = true;
        ui.save().unwrap();
        assert!(s.refresh_archived().unwrap(), "must report disk's true");
        assert!(s.archived, "and adopt it, so a later save cannot revert it");

        // ...then restores. The stale `true` we just adopted must not survive it:
        // this is the direction a one-way `|=` merge gets wrong.
        ui.archived = false;
        ui.save().unwrap();
        assert!(!s.refresh_archived().unwrap(), "must report disk's false");
        assert!(!s.archived, "a Restore is as authoritative as an Archive");
    }

    #[test]
    fn refresh_archived_fails_loudly_rather_than_picking_an_authority() {
        // A guard that refuses archived runs must FAIL CLOSED on an unreadable
        // state.json. Folding a read error into "trust my own copy" would let a
        // torn read or a permissions problem silently decide which authority is
        // in force — the caller cannot even tell it happened.
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        unsafe {
            std::env::set_var("XDG_DATA_HOME", tmp.path().to_str().unwrap());
        }
        let mut s = completion_run(vec![running("implement")]);
        s.save().unwrap();
        fs::write(run_dir("r").join("state.json"), b"{ this is not json").unwrap();

        assert!(
            s.refresh_archived().is_err(),
            "an unreadable state.json is not evidence about the archive flag"
        );

        // A run with NO state.json is different in kind: nothing has ever been
        // recorded about it, so there is no decision to contradict the copy in
        // hand. That must not be an error — `ensure_workspace` runs before a
        // brand-new run's first save in tests, and on a run mid-creation.
        fs::remove_file(run_dir("r").join("state.json")).unwrap();
        let mut fresh = completion_run(vec![running("implement")]);
        fresh.archived = true;
        assert!(
            fresh.refresh_archived().expect("absent is not unreadable"),
            "with nothing on disk, the copy in hand is all there is"
        );
    }

    #[test]
    fn a_save_never_re_archives_a_run_the_human_restored() {
        // The mirror of `a_stale_save_never_resurrects_an_archived_run`, and the
        // half that a one-way `|=` merge got wrong: a long-running command whose
        // copy latched `archived: true` (from an Archive it observed) must not
        // write that back over a Restore that has since landed.
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        unsafe {
            std::env::set_var("XDG_DATA_HOME", tmp.path().to_str().unwrap());
        }
        let mut stale = completion_run(vec![running("implement")]);
        stale.archived = true;
        stale.save().unwrap();

        // The human restores it.
        let mut restore = RunState::load("r").unwrap();
        restore.archived = false;
        restore.save().unwrap();

        // The long-running command writes its progress back, still holding `true`.
        stale.phases[0].status = PhaseStatus::Done;
        stale.save_preserving_archived().unwrap();

        let on_disk = RunState::load("r").unwrap();
        assert!(
            !on_disk.archived,
            "a save carrying a stale `archived: true` must not undo a Restore"
        );
        assert_eq!(
            on_disk.phases[0].status,
            PhaseStatus::Done,
            "the writer's own progress must still land"
        );
        assert!(
            !stale.archived,
            "and the writer's copy must agree with what it just wrote"
        );
    }

    #[test]
    fn restore_can_still_clear_the_archived_flag() {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        unsafe {
            std::env::set_var("XDG_DATA_HOME", tmp.path().to_str().unwrap());
        }
        let mut s = completion_run(vec![done("implement")]);
        s.archived = true;
        s.save().unwrap();

        // Restore deliberately clears the flag and uses a plain `save`, which must
        // NOT rescue the on-disk `true` — otherwise archiving would be one-way.
        let mut restore = RunState::load("r").unwrap();
        restore.archived = false;
        restore.save().unwrap();

        assert!(
            !RunState::load("r").unwrap().archived,
            "Restore must still be able to un-archive a run"
        );
    }

    #[test]
    fn retired_panes_defaults_empty_and_round_trips() {
        // A state.json written before the field existed must still load: cleanup
        // reads this list to decide which panes are drovr's, and a hard parse error
        // here would wedge every pre-existing run.
        let json = r#"{
            "name": "legacy", "task": "t", "phases": [], "gate": "spec",
            "cursor": 0, "project_dir": "/tmp/p"
        }"#;
        let s: RunState = serde_json::from_str(json).expect("legacy state.json must load");
        assert!(s.retired_panes.is_empty());

        // Empty stays invisible on disk (skip-if-empty), so writing a run that never
        // retired a pane leaves its state.json shape unchanged.
        assert!(
            !serde_json::to_string(&s).unwrap().contains("retired_panes"),
            "an empty list must not be serialized"
        );

        let mut s = completion_run(vec![done("brainstorm")]);
        s.retire_pane("w:p7");
        s.retire_pane("w:p7");
        assert_eq!(s.retired_panes, vec!["w:p7"], "retire_pane is idempotent");
        let round: RunState =
            serde_json::from_str(&serde_json::to_string(&s).unwrap()).expect("round trip");
        assert_eq!(round.retired_panes, vec!["w:p7"]);
    }

    #[test]
    fn run_dir_uses_xdg() {
        let _lock = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("XDG_DATA_HOME", "/tmp/drovr-xdg-test");
        }
        assert_eq!(
            run_dir("demo"),
            PathBuf::from("/tmp/drovr-xdg-test/drovr/runs/demo")
        );
        assert_eq!(data_dir(), PathBuf::from("/tmp/drovr-xdg-test/drovr"));
    }

    /// `data_dir()` must never resolve inside the real home directory while the
    /// test suite is running.
    ///
    /// It did, twice, and took the run directories of ~65 runs with it. The
    /// failure mode was SILENT SUCCESS: a test that lost the process-global
    /// `XDG_DATA_HOME` race resolved the LIVE data root and carried on writing
    /// — and deleting — there, passing all the while. Nothing about the old
    /// code could tell the two apart, so the guard has to be the thing that
    /// stops, not a convention.
    ///
    /// Both ways in are covered, because only one of them is the documented
    /// one:
    ///   1. `XDG_DATA_HOME` unset → the `$HOME/.local/share` fallback.
    ///   2. `XDG_DATA_HOME` SET to the live root — which is exactly what a
    ///      developer's shell exports, so this is the case that actually fired.
    /// A scratch root outside `$HOME` must still resolve normally; a guard that
    /// refused everything would just be a broken suite.
    #[test]
    fn data_dir_refuses_to_resolve_inside_the_real_home() {
        let _lock = ENV_LOCK.lock().unwrap();
        let home = PathBuf::from(std::env::var("HOME").expect("HOME must be set"));
        let prev = std::env::var("XDG_DATA_HOME").ok();

        // (1) Unset: the `$HOME/.local/share` fallback.
        unsafe {
            std::env::remove_var("XDG_DATA_HOME");
        }
        let fallback = std::panic::catch_unwind(data_dir);

        // (2) Set, but aimed straight at the live root.
        unsafe {
            std::env::set_var("XDG_DATA_HOME", home.join(".local/share"));
        }
        let live = std::panic::catch_unwind(data_dir);

        // (3) A scratch root outside `$HOME` still resolves.
        let tmp = tempfile::tempdir().unwrap();
        unsafe {
            std::env::set_var("XDG_DATA_HOME", tmp.path());
        }
        let scratch = std::panic::catch_unwind(data_dir);

        // Restore before asserting: a failed assertion must not leak a
        // home-pointing `XDG_DATA_HOME` into whatever runs next.
        unsafe {
            match prev {
                Some(v) => std::env::set_var("XDG_DATA_HOME", v),
                None => std::env::remove_var("XDG_DATA_HOME"),
            }
        }

        assert!(
            fallback.is_err(),
            "data_dir() silently resolved the LIVE data root through the \
             $HOME/.local/share fallback: {fallback:?}"
        );
        assert!(
            live.is_err(),
            "data_dir() silently resolved the LIVE data root from an \
             XDG_DATA_HOME pointing at it: {live:?}"
        );
        assert_eq!(
            scratch.expect("a scratch root outside $HOME must still resolve"),
            tmp.path().join("drovr")
        );
    }

    /// Two threads, each pointing `XDG_DATA_HOME` at its own root, each
    /// re-reading `data_dir()` N times. Against the process-global `set_var`
    /// implementation one thread observes the other's root. Un-ignored and
    /// rewritten in `TestEnv` terms at T13, where it must pass.
    ///
    /// This is the race itself, in miniature and on purpose. The two threads
    /// stand in for two tests running concurrently under `cargo test`: both
    /// redirect the data root the only way today's code allows — by mutating
    /// the variable for the WHOLE PROCESS — and `ENV_LOCK` is the only thing
    /// keeping them apart. Nothing in the type system makes holding it
    /// mandatory, so "hold ENV_LOCK" is a convention, and a convention is
    /// exactly what this test declines to follow.
    ///
    /// It is `#[ignore]`d because it is *expected to fail* for the whole of
    /// this branch: it documents the defect while the fix is built, and the
    /// suite must stay green meanwhile. Its recorded red output is spec §7's
    /// first artifact. At T13 the body is rewritten against the scoped
    /// `TestEnv` handle and the `#[ignore]` comes off — the same two threads,
    /// no longer able to see each other's root.
    ///
    /// The observation is deliberately one-way: a thread counts only the
    /// iterations where `data_dir()` did NOT start with the root that same
    /// thread had just set. Zero means no thread ever saw another's root; any
    /// non-zero count is the race, caught in the act. Both threads' counts are
    /// reported, because "which side lost" varies run to run and neither is
    /// the interesting fact.
    ///
    /// # Nothing panics while `ENV_LOCK` is held
    ///
    /// This test is *supposed* to fail, so its failure path is a path it takes
    /// every time — not an edge case. Both of its panics are therefore pushed
    /// past the end of the critical section: the threads are joined into
    /// `Result`s rather than unwrapped in place, `XDG_DATA_HOME` is restored
    /// the way [`data_dir_refuses_to_resolve_inside_the_real_home`] restores
    /// it, and the guard is dropped explicitly — and only *then* is anything
    /// asserted or unwrapped.
    ///
    /// The order matters both ways. A panic under the guard poisons `ENV_LOCK`
    /// for every other test in the process, turning one honest failure into a
    /// cascade of `PoisonError`s that say nothing (`code_review.rs` reaches for
    /// `into_inner` to survive exactly that; not poisoning it in the first
    /// place is better). And a panic before the restore leaves a foreign
    /// `XDG_DATA_HOME` behind for whatever runs next. A worker can panic too —
    /// `data_dir` calls [`refuse_home_data_root`], which is a `panic!` — so
    /// `join`'s `Err` has to survive to the far side of the cleanup rather
    /// than short-circuit it.
    #[test]
    #[ignore = "demonstrates the race this branch removes; un-ignored at T13"]
    fn data_root_is_not_shared_between_threads() {
        /// Reads per thread. Tuned upward until the race lost reliably; see
        /// `docs/test-isolation/race-red.txt` for the observed failure ratio.
        const ITERATIONS: usize = 20_000;

        let lock = ENV_LOCK.lock().unwrap();
        let prev = std::env::var("XDG_DATA_HOME").ok();

        // Held to the end of the test: dropping a `TempDir` deletes it, and a
        // thread still resolving under a deleted root proves nothing.
        let dirs: Vec<tempfile::TempDir> = (0..2)
            .map(|_| tempfile::tempdir().expect("scratch data root"))
            .collect();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(dirs.len()));

        let handles: Vec<_> = dirs
            .iter()
            .map(|dir| {
                let root = dir.path().to_path_buf();
                let barrier = std::sync::Arc::clone(&barrier);
                std::thread::spawn(move || {
                    // Line the threads up so the writes actually interleave;
                    // staggered, each could finish before the other starts.
                    barrier.wait();
                    // `data_dir()` is `<XDG_DATA_HOME>/drovr` exactly, so the
                    // expected value is exact too. A prefix test would also
                    // accept a root that merely nests under ours.
                    let mine = root.join("drovr");
                    let mut stolen = 0usize;
                    for _ in 0..ITERATIONS {
                        unsafe {
                            std::env::set_var("XDG_DATA_HOME", &root);
                        }
                        if data_dir() != mine {
                            stolen += 1;
                        }
                    }
                    stolen
                })
            })
            .collect();

        // Collected, not unwrapped: a worker panic must not short-circuit the
        // cleanup below. See the doc comment.
        let joined: Vec<std::thread::Result<usize>> =
            handles.into_iter().map(|h| h.join()).collect();

        unsafe {
            match prev {
                Some(v) => std::env::set_var("XDG_DATA_HOME", v),
                None => std::env::remove_var("XDG_DATA_HOME"),
            }
        }
        drop(lock);

        // Past this line the critical section is over and panicking is safe.
        let observed: Vec<usize> = joined
            .into_iter()
            .map(|r| r.expect("neither thread may panic"))
            .collect();

        assert_eq!(
            observed.iter().sum::<usize>(),
            0,
            "data_dir() resolved another thread's data root: {observed:?} of \
             {ITERATIONS} reads per thread saw a root the reading thread had \
             not set. XDG_DATA_HOME is process-global, so two tests \
             redirecting it concurrently share one slot and the loser writes \
             into the winner's directory — silently, which is why this went \
             unnoticed until it deleted the live data root."
        );
    }

    #[test]
    fn list_runs_finds_dirs_with_state_json() {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        unsafe {
            std::env::set_var("XDG_DATA_HOME", tmp.path().to_str().unwrap());
        }
        // Missing runs/ dir → empty, not an error.
        assert!(list_runs_in(&runs_dir()).is_empty());

        let runs = runs_dir();
        // A real run: has state.json.
        fs::create_dir_all(runs.join("alpha")).unwrap();
        fs::write(runs.join("alpha").join("state.json"), b"{}").unwrap();
        // A dir without state.json → skipped (e.g. a stray/half-created dir).
        fs::create_dir_all(runs.join("bogus")).unwrap();
        // A file (not a dir) at the top level → skipped.
        fs::write(runs.join("afile"), b"x").unwrap();

        let mut got = list_runs_in(&runs_dir());
        got.sort();
        assert_eq!(got, vec!["alpha".to_string()]);
    }
    /// The ONE answer to "which pane represents this run", shared by
    /// `drovr attach` and the review UI's mirror.
    ///
    /// They had two copies with two different predicates, which is a bug on its
    /// own — the two can point at different panes for the same run — and two
    /// copies of this would keep drifting. There is no fallback to an EARLIER
    /// phase: a run whose current phase has no pane is honestly empty. Under
    /// reaping, earlier phases are precisely the ones that get closed, so such a
    /// fallback would surface a dead or recycled pane as the run's current state.
    #[test]
    fn live_agent_pane_is_the_current_phases_pane_or_nothing() {
        let mk = |name: &str, status: PhaseStatus, pane: Option<&str>| {
            let mut p = Phase::new(name);
            p.status = status;
            if let Some(pane) = pane {
                p.set_pane(pane);
            }
            p
        };
        let run = |phases: Vec<Phase>, root: Option<&str>| RunState {
            name: "r".into(),
            task: "t".into(),
            agent: None,
            phases,
            review_phases: vec![],
            gate: "spec".into(),
            cursor: 0,
            workspace: Some("w".into()),
            root_pane: root.map(str::to_owned),
            project_dir: String::new(),
            worktree_path: None,
            worktree_branch: None,
            archived: false,
            retired_panes: vec![],
        };

        // The phase the run is ON — not the first pane it can find.
        let s = run(
            vec![
                mk("brainstorm", PhaseStatus::Done, Some("w:p1")),
                mk("plan", PhaseStatus::Running, Some("w:p2")),
            ],
            Some("w:root"),
        );
        assert_eq!(s.live_agent_pane(), Some(("plan", "w:p2")));

        // Current phase has NO pane (never started, or reaped) → None. It must
        // NOT walk BACK to brainstorm's pane, even though that pane is right
        // there and looks alive: a Done phase is exactly what reaping closes.
        let s = run(
            vec![
                mk("brainstorm", PhaseStatus::Done, Some("w:p1")),
                mk("plan", PhaseStatus::Pending, None),
            ],
            Some("w:root"),
        );
        assert_eq!(
            s.live_agent_pane(),
            None,
            "an earlier phase's pane is not this run's current state"
        );

        // But it MUST skip FORWARD over a phase that was never started. A
        // driver can `phase start implement-task-1` without ever starting
        // `plan`, which leaves a Pending phase in front of a live one — and
        // reporting that run as empty while its agent sits right there would be
        // its own kind of lie.
        let s = run(
            vec![
                mk("brainstorm", PhaseStatus::Done, Some("w:p1")),
                mk("plan", PhaseStatus::Pending, None),
                mk("implement-task-1", PhaseStatus::Running, Some("w:p7")),
            ],
            Some("w:root"),
        );
        assert_eq!(
            s.live_agent_pane(),
            Some(("implement-task-1", "w:p7")),
            "a live later phase must not be hidden by an unstarted earlier one"
        );

        // Every phase Done → the run is finished; there is no current phase.
        let s = run(
            vec![mk("brainstorm", PhaseStatus::Done, Some("w:p1"))],
            Some("w:root"),
        );
        assert_eq!(s.live_agent_pane(), None);

        // A phase that FAILED still holds the pane worth looking at — the
        // predicate is "the phase the run is on", not "status == Running".
        let s = run(
            vec![mk("implement", PhaseStatus::Failed, Some("w:p9"))],
            None,
        );
        assert_eq!(s.live_agent_pane(), Some(("implement", "w:p9")));

        // The idle root shell is never it, and neither is an empty run.
        assert_eq!(run(vec![], Some("w:root")).live_agent_pane(), None);
    }

    #[test]
    fn state_roundtrips_and_finds_first_incomplete() {
        let _lock = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("XDG_DATA_HOME", "/tmp/drovr-xdg-test2");
        }
        let s = RunState {
            name: "demo".into(),
            task: "t".into(),
            agent: Some("claude".into()),
            phases: vec![
                {
                    let mut p = Phase::new("brainstorm");
                    p.status = PhaseStatus::Done;
                    p
                },
                Phase::new("plan"),
            ],
            // A populated review_phases list must NOT influence pipeline progress:
            // it round-trips but `first_incomplete` (and `format_progress`) ignore it.
            review_phases: vec![{
                let mut p = Phase::new("review:task-1:1:correctness");
                p.status = PhaseStatus::Running;
                p.set_pane("p1");
                p
            }],
            gate: "spec".into(),
            cursor: 1,
            workspace: None,
            root_pane: None,
            project_dir: "/tmp/proj".into(),
            worktree_path: None,
            worktree_branch: None,
            archived: false,
            retired_panes: vec![],
        };
        s.save().unwrap();
        let loaded = RunState::load("demo").unwrap();
        assert_eq!(loaded.phases.len(), 2);
        assert_eq!(
            loaded.review_phases.len(),
            1,
            "review_phases must round-trip"
        );
        // first_incomplete walks `phases` only — the pending "plan" at index 1 wins,
        // and the Running review phase is invisible to it.
        assert_eq!(loaded.first_incomplete(), Some(1));
    }

    #[test]
    fn missing_review_phases_defaults_to_empty() {
        // A pre-existing state.json written before `review_phases` existed has no
        // such key; serde's #[serde(default)] must yield an empty vec, not an error.
        let json = r#"{
            "name":"old","task":"t",
            "phases":[{"name":"plan","status":"Pending","handoff_doc":null,"herdr_session":null,"pane_id":null}],
            "gate":"spec","cursor":0,"project_dir":"/tmp/proj"
        }"#;
        let loaded: RunState = serde_json::from_str(json).unwrap();
        assert_eq!(loaded.agent.as_deref(), Some("claude"));
        assert!(
            loaded.review_phases.is_empty(),
            "absent review_phases must default to []"
        );
    }

    #[test]
    fn missing_worktree_fields_default_to_none() {
        // A state.json written before worktree support has no worktree_path /
        // worktree_branch keys; #[serde(default)] must yield None, not an error —
        // that None is exactly what makes old (in-place) runs behave as today.
        let json = r#"{
            "name":"old","task":"t",
            "phases":[{"name":"plan","status":"Pending","handoff_doc":null,"herdr_session":null,"pane_id":null}],
            "gate":"spec","cursor":0,"project_dir":"/tmp/proj"
        }"#;
        let loaded: RunState = serde_json::from_str(json).unwrap();
        assert!(
            loaded.worktree_path.is_none(),
            "absent worktree_path → None"
        );
        assert!(
            loaded.worktree_branch.is_none(),
            "absent worktree_branch → None"
        );
    }

    #[test]
    fn worktree_fields_roundtrip() {
        let json = r#"{
            "name":"wt","task":"t",
            "phases":[],"gate":"spec","cursor":0,"project_dir":"/repo/.drovr/wt/wt",
            "worktree_path":"/repo/.drovr/wt/wt","worktree_branch":"drovr/wt"
        }"#;
        let loaded: RunState = serde_json::from_str(json).unwrap();
        assert_eq!(loaded.worktree_path.as_deref(), Some("/repo/.drovr/wt/wt"));
        assert_eq!(loaded.worktree_branch.as_deref(), Some("drovr/wt"));
        // Re-serialize and reload: the fields survive a full round-trip.
        let reloaded: RunState =
            serde_json::from_str(&serde_json::to_string(&loaded).unwrap()).unwrap();
        assert_eq!(reloaded.worktree_branch.as_deref(), Some("drovr/wt"));
    }

    #[test]
    fn save_leaves_no_temp_file_behind() {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        unsafe {
            std::env::set_var("XDG_DATA_HOME", tmp.path().to_str().unwrap());
        }
        let mut s = fat_run("tmpclean", 3);
        s.save().unwrap();
        s.cursor = 1;
        s.save().unwrap();

        // The real invariant is "no temp file survives a save"; asserting the dir
        // holds ONLY state.json would start failing for the wrong reason the day
        // anything else legitimately lands in a run dir.
        let leftovers: Vec<String> = fs::read_dir(run_dir("tmpclean"))
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n != "state.json")
            .collect();
        assert!(
            leftovers.is_empty(),
            "save must rename its temp file away, leaving only state.json; found {leftovers:?}"
        );
        assert_eq!(RunState::load("tmpclean").unwrap().cursor, 1);
    }

    #[test]
    fn concurrent_saves_never_expose_a_partial_state_json() {
        // A bare `fs::write` truncates in place, so a concurrent `load` parses a
        // half-written file, `load_run` exits 1, and per the pipeline skill the
        // whole run STOPs.
        //
        // Tuned for DETECTION, verified by mutation (revert `save` to a bare
        // `fs::write` and this must fail, repeatedly):
        //  * MANY saves, not one huge one. The vulnerable window is between
        //    `File::create`'s O_TRUNC and the write completing, and it is roughly
        //    fixed per save — so detection scales with the NUMBER of saves, not
        //    the size of each. 4 writers x 400 saves = 1600 windows.
        //  * a moderate phases vec: big enough to keep the window open, small
        //    enough that the reader's parse is cheap and it samples often. Both
        //    a much fatter and a much thinner vec detect worse.
        //  * each writer builds its `RunState` ONCE, outside the loop, so its
        //    time goes into `fs::write` rather than into `format!`.
        //
        // Structured so NOTHING panics inside a spawned thread: a panic there
        // would surface at a join on the main thread, which is holding ENV_LOCK,
        // poisoning it and cascade-failing every other test in the binary. Threads
        // return Results; the main thread asserts. `thread::scope` guarantees all
        // threads are joined even if the body unwinds, so no thread can outlive
        // the lock guard or the TempDir and race the next test's `set_var`.
        const PHASES: usize = 200;
        const WRITERS: usize = 4;
        const SAVES: usize = 400;

        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        unsafe {
            std::env::set_var("XDG_DATA_HOME", tmp.path().to_str().unwrap());
        }
        // Seed the file so the reader always has something to open.
        fat_run("race", PHASES).save().unwrap();

        let finished = std::sync::atomic::AtomicUsize::new(0);
        let finished = &finished;
        // Without this handshake the writers can burn through every save before
        // the reader thread is first scheduled; the reader then does one clean
        // read of a quiescent file and the test passes even against a
        // deliberately non-atomic `save`. Verified by mutation: reverting `save`
        // to a bare `fs::write` must fail this test.
        let reading = std::sync::atomic::AtomicBool::new(false);
        let reading = &reading;
        let (write_errs, read_result) = std::thread::scope(|s| {
            let writers: Vec<_> = (0..WRITERS)
                .map(|w| {
                    s.spawn(move || {
                        let mut st = fat_run("race", PHASES);
                        while !reading.load(std::sync::atomic::Ordering::SeqCst) {
                            std::thread::yield_now();
                        }
                        let mut errs = Vec::new();
                        for i in 0..SAVES {
                            st.cursor = w * 1000 + i;
                            if let Err(e) = st.save() {
                                errs.push(e.to_string());
                            }
                        }
                        finished.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        errs
                    })
                })
                .collect();
            let reader = s.spawn(move || {
                // Deadline backstop: if a writer dies without incrementing
                // `finished`, this must fail the test rather than spin forever
                // (`cargo test` has no per-test timeout, so a hang is worse than
                // a failure).
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
                let mut reads = 0usize;
                loop {
                    // Released only once the reader is actually in its loop, so
                    // every save below overlaps a live read.
                    reading.store(true, std::sync::atomic::Ordering::SeqCst);
                    match RunState::load("race") {
                        Ok(l) if l.phases.len() == PHASES => reads += 1,
                        Ok(l) => {
                            return Err(format!(
                                "torn read: state.json parsed but held {} phases, not {PHASES}",
                                l.phases.len()
                            ));
                        }
                        Err(e) => return Err(format!("torn read: state.json did not parse: {e}")),
                    }
                    // Checked AFTER a read, so `reads` is non-zero even if every
                    // writer finishes before this thread is first scheduled.
                    if finished.load(std::sync::atomic::Ordering::SeqCst) == WRITERS {
                        return Ok(reads);
                    }
                    if std::time::Instant::now() >= deadline {
                        return Err("writers never finished within 60s".to_string());
                    }
                }
            });
            let errs: Vec<String> = writers.into_iter().flat_map(|w| w.join().unwrap()).collect();
            (errs, reader.join().unwrap())
        });

        assert!(write_errs.is_empty(), "save() failed: {write_errs:?}");
        let reads = read_result.expect("load must never observe a partially-written state.json");
        assert!(reads > 0, "the reader thread must have observed some loads");
    }

    /// A `RunState` with `n` phases, fat enough that a non-atomic `save` has a
    /// wide window in which a concurrent `load` sees a truncated file.
    fn fat_run(name: &str, n: usize) -> RunState {
        RunState {
            name: name.into(),
            task: "t".repeat(200),
            agent: Some("claude".into()),
            phases: (0..n)
                .map(|i| {
                    let mut p = Phase::new(&format!("phase-{i}-{}", "x".repeat(64)));
                    p.handoff_doc = Some("h".repeat(128));
                    p
                })
                .collect(),
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
        }
    }

    #[test]
    fn missing_phase_pass_defaults_to_none() {
        // The Phase-level back-compat guard. `Phase`'s fields are NOT
        // `#[serde(default)]` as a group, so any field added without its own
        // `#[serde(default)]` makes EVERY existing state.json fail to load →
        // `load_run` exits 1 → the run STOPs. This pins that `pass` (and, by
        // example, whatever task 3 adds next) is absent-tolerant.
        let json = r#"{
            "name":"old","task":"t",
            "phases":[{"name":"plan","status":"Running","handoff_doc":null,"herdr_session":null,"pane_id":"w:p1"}],
            "gate":"spec","cursor":0,"project_dir":"/tmp/proj"
        }"#;
        let loaded: RunState = serde_json::from_str(json).unwrap();
        assert!(
            loaded.phases[0].pass.is_none(),
            "a Phase written before pass tokens must load with pass: None"
        );
        // And a phase with no pass must not start emitting one on re-save.
        let out = serde_json::to_string(&loaded).unwrap();
        assert!(
            !out.contains("\"pass\""),
            "pass: None must be skipped on serialize: {out}"
        );
    }

    #[test]
    fn missing_phase_capture_fields_default_to_absent() {
        // The Phase-level back-compat guard for the four fields session capture
        // adds. Same stakes as `missing_phase_pass_defaults_to_none`: `Phase`'s
        // fields are not `#[serde(default)]` as a group, so one of these landing
        // without its own default makes EVERY existing state.json fail to load →
        // `load_run` exits 1 → the run STOPs. This is the state.json shape drovr
        // wrote before session capture existed.
        let json = r#"{
            "name":"old","task":"t",
            "phases":[{"name":"plan","status":"Running","handoff_doc":null,"herdr_session":null,"pane_id":"w:p1","pass":"abc-1"}],
            "gate":"spec","cursor":0,"project_dir":"/tmp/proj"
        }"#;
        let loaded: RunState = serde_json::from_str(json).unwrap();
        let p = &loaded.phases[0];
        assert!(p.tab_id.is_none(), "absent tab_id → None");
        assert!(
            p.pane_agent().is_none(),
            "absent agent → None (backend, profile, session)"
        );
        assert!(
            !p.is_reaped(),
            "absent reaped → false (the phase still has its pane)"
        );

        // And a phase that captured nothing must not start emitting the keys:
        // a run written by this build stays loadable by an older one.
        //
        // Serialize the PHASE, not the whole run — `RunState` has an `agent` key
        // of its own, and matching against the run's JSON would pass for the
        // wrong reason (or fail for it, as this test did when `Phase::agent`
        // arrived).
        let out = serde_json::to_string(p).unwrap();
        for key in ["tab_id", "pane_agent", "reaped"] {
            assert!(!out.contains(key), "empty {key} must be skipped: {out}");
        }
    }

    #[test]
    fn a_persisted_session_id_is_revalidated_on_load() {
        // `state.json` is a file. A session id that reached it by any other route
        // than a capture must clear the same bar, because task 5 interpolates it
        // into `--resume '<id>'` — so a hand-edited one that could break out of
        // those quotes fails the LOAD, loudly, rather than the composition.
        let phase = |session: &str| {
            format!(
                r#"{{"name":"plan","status":"Running","handoff_doc":null,
                    "herdr_session":null,"pane_id":null,
                    "pane_agent":{{"backend":"claude","session":{session}}}}}"#
            )
        };
        let good: Phase = serde_json::from_str(&phase("\"cca92f5b-3a8c\"")).unwrap();
        assert_eq!(
            good.pane_agent().and_then(|a| a.resume()),
            Some(ResumeTarget {
                session: &SessionId::new("cca92f5b-3a8c".into()).unwrap(),
                backend: "claude",
                profile: None,
            }),
            "a loaded session comes back paired with its backend"
        );
        // Round-trips as a bare string, so state.json stays human-readable.
        let out = serde_json::to_string(&good).unwrap();
        assert!(out.contains(r#""session":"cca92f5b-3a8c""#), "{out}");

        for bad in ["\"\"", "\"a b\"", "\"a'b; rm -rf /\""] {
            assert!(
                serde_json::from_str::<Phase>(&phase(bad)).is_err(),
                "{bad} must not load as a session id"
            );
        }
    }

    #[test]
    fn keys_a_phase_does_not_recognise_must_never_fail_the_load() {
        // `Phase` must stay tolerant of keys it does not know. Two independent
        // reasons, and the second is the one that bites:
        //
        //  * A newer drovr writes a field an older one has never heard of; the
        //    older binary must still load the run rather than exit 1 and STOP it.
        //  * Adding `#[serde(deny_unknown_fields)]` here — an innocuous-looking
        //    tightening — would do exactly that to every run in flight.
        //
        // The literal below is this branch's own intermediate shape (`agent_session`
        // / `agent_backend` / `agent_profile` as three loose fields, before they
        // became one `PhaseAgent`). No released or installed build ever wrote it,
        // so nothing in the wild carries those keys and no migration is owed — but
        // it is a free, honest example of "keys this build does not know".
        let json = r#"{
            "name":"plan","status":"Running","handoff_doc":null,
            "herdr_session":null,"pane_id":"w:p1",
            "agent_session":"cca92f5b-3a8c","agent_backend":"cursor",
            "agent_profile":"/cfg","some_future_field":{"nested":true}
        }"#;
        let p: Phase = serde_json::from_str(json).expect("unknown keys must not fail the load");
        assert_eq!(p.pane_id(), Some("w:p1"));
        // They are IGNORED, not migrated — say so out loud, so nobody reads a
        // passing test as a promise that the old values were carried over.
        assert!(
            p.pane_agent().is_none(),
            "unknown keys are dropped, not adopted; the next poll re-captures"
        );
    }

    #[test]
    fn only_a_relaunch_may_replace_a_held_session() {
        // ⭐ THE RULE, ON THE API THAT MUTATES — not in one caller.
        //
        // Round 3 fixed a critical (an unconfirmed resume overwriting the very
        // session id it was trying to confirm) by gating `Capture::apply`. But
        // `record_session` still assigned unconditionally, so the rule lived in
        // one caller-specific layer: any future call with a session already
        // held reintroduced the overwrite with no type error, and the bug that
        // cost two rounds would be back.
        //
        // A session is meaningful only beside the agent process that made it,
        // so the ONE sanctioned way it changes is a new process —
        // `record_launch`, which replaces the whole record and clears the slot.
        // Capture fills the empty slot. Nothing replaces.
        let first = SessionId::new("sess-first".into()).unwrap();
        let second = SessionId::new("sess-second".into()).unwrap();

        let mut p = Phase::new("plan");
        assert!(
            !p.record_session(first.clone()),
            "no agent record → nowhere to put a session"
        );

        p.record_launch("claude", None);
        assert!(p.record_session(first.clone()), "fills an empty slot");
        assert!(
            !p.record_session(second.clone()),
            "and REFUSES to replace a held one — the pane would be running an \
             agent the record does not describe"
        );
        assert_eq!(p.resume_target().map(|t| t.session()), Some(&first));

        // The sanctioned path: a new process, so a new record.
        p.record_launch("claude", None);
        assert!(
            p.resume_target().is_none(),
            "a relaunch clears the session — that IS how one is discarded"
        );
        assert!(p.record_session(second.clone()));
        assert_eq!(p.resume_target().map(|t| t.session()), Some(&second));

        // And the predicate `Capture` asks is the same one the mutator enforces,
        // so a caller cannot decide there is work to do that the API refuses.
        assert!(!p.accepts_captured_session());
        p.record_launch("claude", None);
        assert!(p.accepts_captured_session());
        assert!(
            Phase::new("plan").accepts_captured_session(),
            "a phase with no agent record yet still accepts one — `record_capture` \
             creates the record first"
        );
    }

    #[test]
    fn seeding_a_record_may_not_replace_one_that_is_already_there() {
        // ⚠️ THE LAST DOOR ROUND 4 LEFT OPEN. `record_session` was made to
        // refuse replacing a held session, but `adopt_pane_agent` assigned the
        // whole `PhaseAgent` wholesale with no guard — so the rule could be
        // walked straight around, session and all, without going through
        // `record_launch`. Production was safe only because its one caller
        // happened to check `pane_agent().is_none()` first, which puts the
        // invariant in a call-site `match` rather than in the API.
        //
        // Same drift as reviewer identity (list vs name) and resume evidence
        // (`Option` vs `ResumeEvidence`), and fixed the same way: the API
        // refuses, so a future caller cannot skip a guard that no longer exists
        // to be skipped.
        let held = SessionId::new("sess-held".into()).unwrap();
        let other = SessionId::new("sess-other".into()).unwrap();

        // An empty slot is what seeding is FOR: a phase launched by a build
        // that recorded no agent, reconciled against what is on disk.
        let mut empty = Phase::new("plan");
        let mut from_disk = PhaseAgent::launched("cursor", Some("/prof".into()));
        assert!(from_disk.record_session(other.clone()));
        assert!(
            empty.seed_pane_agent(from_disk.clone()),
            "an empty slot takes the persisted record whole — backend, profile and all"
        );
        assert_eq!(empty.pane_agent().map(|a| a.backend()), Some("cursor"));
        assert_eq!(empty.resume_target().map(|t| t.session()), Some(&other));

        // An occupied one refuses, and the held session is what it protects.
        let mut occupied = Phase::new("plan");
        occupied.record_launch("claude", None);
        assert!(occupied.record_session(held.clone()));
        assert!(
            !occupied.seed_pane_agent(from_disk),
            "a record already there is not something to overwrite"
        );
        assert_eq!(
            occupied.resume_target().map(|t| t.session()),
            Some(&held),
            "the session survives, and so does the backend it belongs to"
        );
        assert_eq!(occupied.pane_agent().map(|a| a.backend()), Some("claude"));

        // …and `record_launch` is still the one sanctioned way it changes.
        occupied.record_launch("claude", None);
        assert!(occupied.resume_target().is_none());
    }

    #[test]
    fn the_resume_bundle_carries_the_profile_too_or_it_finds_nothing() {
        // Atomic is not the same as sufficient. An earlier version of `resume()`
        // handed back (session, backend) and dropped the profile — a bundle that
        // cannot be taken apart, and still not enough to find the conversation:
        // claude resolves a session under
        // `$CLAUDE_CONFIG_DIR/projects/<escaped-cwd>/`, so resuming under the
        // wrong profile silently finds NOTHING and falls back to a fresh agent.
        let id = SessionId::new("cca92f5b-3a8c".into()).unwrap();
        let mut agent = PhaseAgent::launched("cursor", Some("/home/u/.config/claude-work".into()));
        assert!(agent.resume().is_none(), "no session yet → no bundle");

        agent.record_session(id.clone());
        assert_eq!(
            agent.resume(),
            Some(ResumeTarget {
                session: &id,
                backend: "cursor",
                profile: Some("/home/u/.config/claude-work"),
            }),
            "all three travel together"
        );

        // `None` profile is meaningful — "the default profile", which is exactly
        // what the launch inlined — not "unknown".
        let mut default_profile = PhaseAgent::launched("claude", None);
        default_profile.record_session(id.clone());
        assert_eq!(default_profile.resume().unwrap().profile, None);

        // And it survives the disk round trip, which is the only path task 5 has.
        let round: PhaseAgent =
            serde_json::from_str(&serde_json::to_string(&agent).unwrap()).unwrap();
        assert_eq!(round.resume(), agent.resume());
    }

    #[test]
    fn save_refuses_exactly_what_load_refuses() {
        // Asymmetric validation is worse than none. If `save` wrote a lifecycle
        // contradiction that `load` rejects, drovr would produce a `state.json`
        // it then refuses to read — every later `load_run` exits 1, the pipeline
        // STOPs, and there is no way back except hand-editing the file drovr
        // itself wrote. Checking the same invariant on both sides means the
        // failure lands on the caller that caused it, while the last good file
        // is still on disk.
        //
        // Reaching the illegal state needs this module's own privacy (that is
        // the point — `set_pane` clears `reaped`, so the public API cannot build
        // it), which is exactly why the write-side check is defence in depth
        // rather than dead code.
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        unsafe {
            std::env::set_var("XDG_DATA_HOME", tmp.path().to_str().unwrap());
        }
        let mut s = completion_run(vec![done("plan")]);
        s.name = "asym".into();
        s.save().expect("a consistent state saves");

        s.phases[0].mark_reaped();
        s.phases[0].pane_id = Some("w:p1".into()); // module-private; no caller can do this
        let err = s
            .save()
            .expect_err("save must refuse what load would reject");
        assert!(
            err.to_string().contains("reaped but still holds pane"),
            "same message as the read side: {err}"
        );

        // And the last good file is untouched — the refusal did not corrupt it.
        assert!(
            RunState::load("asym").is_ok(),
            "a refused save must leave the previous state.json loadable"
        );
    }

    #[test]
    fn a_phase_cannot_be_reaped_while_it_still_holds_a_pane() {
        // "Reaped" and "has a live pane" are contradictory claims about one
        // phase. As a bare `pub bool` beside `pane_id` the contradiction was one
        // assignment away, and it is the kind that reads fine and behaves badly:
        // `drovr attach` would offer a pane that is gone, and cleanup and reaping
        // would disagree about whose pane it is.
        //
        // Three doors, all shut: `Reaped`'s inner bool is private, so only this
        // module can say "yes"; `mark_reaped` is the only thing that does, and it
        // drops the pane in the same statement; and `RunState::load` refuses a
        // file that claims otherwise.
        let mut p = Phase::new("plan");
        p.set_pane("w:p1");
        assert!(!p.is_reaped());

        assert_eq!(
            p.mark_reaped(),
            Some("w:p1".to_string()),
            "hands back the pane to retire"
        );
        assert!(p.is_reaped());
        assert!(
            p.pane_id().is_none(),
            "setting one clears the other, in one step"
        );
        assert_eq!(p.mark_reaped(), None, "idempotent; nothing left to retire");

        // The third door: a hand-written state.json.
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        unsafe {
            std::env::set_var("XDG_DATA_HOME", tmp.path().to_str().unwrap());
        }
        let dir = run_dir("illegal");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("state.json"),
            r#"{"name":"illegal","task":"t","gate":"spec","cursor":0,"project_dir":"/tmp/p",
                "phases":[{"name":"plan","status":"Done","handoff_doc":null,
                "herdr_session":null,"pane_id":"w:p1","reaped":true}]}"#,
        )
        .unwrap();
        let err = RunState::load("illegal").expect_err("must refuse the contradiction");
        assert!(
            err.to_string().contains("reaped but still holds pane"),
            "the refusal must say what is wrong: {err}"
        );

        // The same file WITHOUT the live pane is fine — that is a real reaped phase.
        fs::write(
            dir.join("state.json"),
            r#"{"name":"illegal","task":"t","gate":"spec","cursor":0,"project_dir":"/tmp/p",
                "phases":[{"name":"plan","status":"Done","handoff_doc":null,
                "herdr_session":null,"pane_id":null,"reaped":true}]}"#,
        )
        .unwrap();
        assert!(RunState::load("illegal").unwrap().phases[0].is_reaped());
    }

    #[test]
    fn a_session_cannot_be_persisted_without_the_backend_it_belongs_to() {
        // The invariant `PhaseAgent` exists for. `resumable_for(backend)` makes a
        // session id meaningless without the agent that created it, and two loose
        // `Option` fields let `state.json` say otherwise in both directions. Here
        // `backend` is REQUIRED, so a session with no backend does not parse at
        // all — and `resume()` hands back both or neither.
        let bare = r#"{"name":"plan","status":"Running","handoff_doc":null,
            "herdr_session":null,"pane_id":null,
            "pane_agent":{"session":"cca92f5b-3a8c"}}"#;
        assert!(
            serde_json::from_str::<Phase>(bare).is_err(),
            "a session with no backend must not be representable on disk"
        );

        // A backend with NO session is the normal pre-capture state, and stays legal.
        let pre = r#"{"name":"plan","status":"Running","handoff_doc":null,
            "herdr_session":null,"pane_id":null,
            "pane_agent":{"backend":"cursor","profile":"/cfg"}}"#;
        let p: Phase = serde_json::from_str(pre).unwrap();
        let agent = p.pane_agent().unwrap();
        assert_eq!(agent.backend(), "cursor");
        assert_eq!(agent.profile(), Some("/cfg"));
        assert!(
            agent.resume().is_none(),
            "no session yet → nothing to resume, and no half-pair to misuse"
        );
    }

    #[test]
    fn an_empty_pass_token_is_not_representable() {
        // "no token" (a phase from a pre-token build, which completes on an EMPTY
        // marker) and "a token that happens to be empty" are opposite statements
        // about a phase, and `Option<PassToken>` already carries the first one.
        // A `PassToken("")` would satisfy `pass.is_some()` while matching no
        // marker at all — a phase that can never complete, by either rule.
        assert!(PassToken::new(String::new()).is_none());
        assert!(PassToken::new("   ".into()).is_none());
        assert!(PassToken::new("abc-1".into()).is_some());

        // `Deserialize` is a second constructor, reachable by anyone who can write
        // state.json. It must enforce the same rule.
        let r: Result<Phase, _> = serde_json::from_str(
            r#"{"name":"plan","status":"Running","handoff_doc":null,
                "herdr_session":null,"pane_id":null,"pass":""}"#,
        );
        assert!(
            r.is_err(),
            "an empty pass token on disk must not deserialize into Some(PassToken)"
        );
    }

    #[test]
    fn pass_token_only_matches_a_non_empty_equal_marker() {
        let t = PassToken::new("abc-1".into()).unwrap();
        assert!(t.matches_marker("abc-1"));
        assert!(!t.matches_marker("abc-2"));
        assert!(
            !t.matches_marker(""),
            "an empty marker is not evidence about any pass"
        );
        // Serializes transparently: state.json shape is unchanged by the newtype.
        assert_eq!(serde_json::to_string(&t).unwrap(), "\"abc-1\"");
        let p: Phase = serde_json::from_str(
            r#"{"name":"plan","status":"Running","handoff_doc":null,
                "herdr_session":null,"pane_id":null,"pass":"xyz-9"}"#,
        )
        .unwrap();
        assert_eq!(p.pass.unwrap().as_str(), "xyz-9");
    }

    #[test]
    fn phase_default_is_pending_with_empty_fields() {
        let p = Phase::new("plan");
        assert_eq!(p.name, "plan");
        assert_eq!(p.status, PhaseStatus::Pending);
        assert!(p.handoff_doc.is_none());
        assert!(p.herdr_session.is_none());
        assert!(p.pane_id().is_none());
        assert_eq!(Phase::default().name, "");
        assert_eq!(PhaseStatus::default(), PhaseStatus::Pending);
    }

    #[test]
    fn find_phase_searches_both_lists() {
        let mk = |name: &str| {
            let mut p = Phase::new(name);
            p.status = PhaseStatus::Running;
            p
        };
        let s = RunState {
            name: "r".into(),
            task: "t".into(),
            agent: None,
            phases: vec![mk("plan")],
            review_phases: vec![mk("review:task-1:1:correctness")],
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
        assert_eq!(s.find_phase("plan").map(|p| p.name.as_str()), Some("plan"));
        assert_eq!(
            s.find_phase("review:task-1:1:correctness")
                .map(|p| p.name.as_str()),
            Some("review:task-1:1:correctness"),
            "find_phase must also search review_phases"
        );
        assert!(s.find_phase("nonexistent").is_none());
    }
}
