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
/// * The first five fields (`name` … `herdr_session`) are NOT
///   `#[serde(default)]`, so deserialization requires them; every field added
///   since (`pass`, `tab_id`, `pane_agent`, `reaped`) carries its own
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
    /// Written by nothing yet; reaping is a later step. It is declared now so
    /// the whole capture record lands in one `state.json` shape rather than
    /// migrating twice, and so the lifecycle rule is in place before the code
    /// that depends on it is written.
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
/// reaping disagree about whose pane it is. Task 6 is the code that will write
/// this; the rule is here first, deliberately, because it is much harder to
/// impose once reaping depends on the shape.
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
    #[allow(dead_code)]
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

    /// Adopt an agent record wholesale — used when reconciling with what is
    /// already on disk, where the persisted record may carry a profile or
    /// backend this process never saw.
    pub fn adopt_pane_agent(&mut self, agent: PhaseAgent) {
        self.pane_agent = Some(agent);
    }

    /// Record a session captured from a live poll, if this phase has a launch
    /// record to attach it to. Returns whether it landed.
    ///
    /// `false` means there is no `PhaseAgent`, and that is not a failure to
    /// paper over: a session is only meaningful beside the backend that created
    /// it, so it must not be stored without one. The caller establishes the
    /// launch record first (`record_launch`) and retries.
    pub fn record_session(&mut self, session: SessionId) -> bool {
        match self.pane_agent.as_mut() {
            Some(agent) => {
                agent.record_session(session);
                true
            }
            None => false,
        }
    }

    /// Whether drovr has closed this phase's pane. Nothing reads it yet —
    /// task 6 (reaping) and task 5 (rehydrate) do.
    #[allow(dead_code)]
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
    /// Nothing calls this yet — task 6 does.
    #[allow(dead_code)]
    pub fn mark_reaped(&mut self) -> Option<String> {
        self.reaped = Reaped(true);
        self.pane_id.take()
    }
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
    #[allow(dead_code)]
    pub fn session(&self) -> &'a SessionId {
        self.session
    }
    /// The agent that created it — the only one it means anything to.
    #[allow(dead_code)]
    pub fn backend(&self) -> &'a str {
        self.backend
    }
    /// The `CLAUDE_CONFIG_DIR` to resume UNDER; `None` = the default profile.
    #[allow(dead_code)]
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
    pub fn record_session(&mut self, session: SessionId) {
        self.session = Some(session);
    }

    /// Everything a resume needs, or `None`. All three or nothing — that is the
    /// whole reason this is one type.
    ///
    /// Nothing resumes yet, so this is exercised by tests alone until task 5
    /// composes `--resume`; it exists now because it is the accessor that makes
    /// the bundle impossible to take apart.
    #[allow(dead_code)]
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
    /// The workspace's auto-created root shell pane id (set by `drovr new`). The
    /// first phase runs `claude` *inside* this pane instead of splitting a new
    /// pane beside it, so no empty shell is left dangling. `phase_start` takes it
    /// (leaving `None`) so later phases each get their own tab. `None` for pre-fix
    /// runs → the first phase falls back to a fresh tab.
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
pub fn data_dir() -> PathBuf {
    let base = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap()).join(".local/share"));
    base.join("drovr")
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

        let dir = run_dir(&self.name);
        fs::create_dir_all(&dir)?;
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
    pub fn retire_pane(&mut self, pane_id: impl Into<String>) {
        let id = pane_id.into();
        if !self.retired_panes.contains(&id) {
            self.retired_panes.push(id);
        }
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
